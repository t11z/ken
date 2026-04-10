//! Windows Update observer per ADR-0020.
//!
//! Collects pending update counts from the Windows Update Agent COM API
//! on a background-refresh schedule with a one-hour cache TTL. The
//! heartbeat-tick path never blocks on a WUA call.
//!
//! On non-Windows platforms, all fields are `Unobserved`.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use ken_protocol::status::{Observation, WindowsUpdateStatus};
use time::OffsetDateTime;
use tokio::sync::watch;

use super::trait_def::Observer;

/// Cache TTL for WUA results. Per ADR-0020, hardcoded at one hour.
const WUA_CACHE_TTL: Duration = Duration::from_secs(3600);

/// Internal time guard for the WUA COM call. Per ADR-0020, this is
/// separate from the per-observer heartbeat budget (ADR-0018) and
/// protects the background task from genuinely hung COM calls.
const WUA_INTERNAL_TIMEOUT: Duration = Duration::from_secs(120);

/// Result of a successful WUA search: total pending and critical pending counts.
#[derive(Debug, Clone, Copy)]
struct WuaCounts {
    total: u32,
    critical: u32,
    observed_at: OffsetDateTime,
}

/// Windows Update observer struct per ADR-0018 and ADR-0020.
///
/// On Windows, spawns a long-lived background task at construction that
/// periodically queries WUA and writes results into a shared cache. The
/// `observe` method on the heartbeat path reads the cache and never
/// touches WUA directly.
///
/// On non-Windows, all fields are permanently `Unobserved`.
pub struct WindowsUpdateObserver {
    /// Receiver for the shared cache written by the background task.
    cache_rx: watch::Receiver<Option<WuaCounts>>,

    /// Shutdown signal for the background task.
    _shutdown: Arc<AtomicBool>,
}

impl WindowsUpdateObserver {
    /// Create a new Windows Update observer.
    ///
    /// On Windows, spawns the background WUA refresh task using the
    /// provided Tokio handle. On non-Windows, no background work occurs.
    #[must_use]
    pub fn new(shutdown: Arc<AtomicBool>) -> Self {
        let cache_rx = Self::init_background(shutdown.clone());
        Self {
            cache_rx,
            _shutdown: shutdown,
        }
    }

    /// On Windows, spawn the WUA background refresh task and return the
    /// receiver end of the cache channel. On non-Windows, return a
    /// receiver whose sender has been dropped (permanently `None`).
    #[cfg(windows)]
    fn init_background(shutdown: Arc<AtomicBool>) -> watch::Receiver<Option<WuaCounts>> {
        let (cache_tx, cache_rx) = watch::channel(None);
        tokio::spawn(async move {
            wua_background_loop(cache_tx, shutdown).await;
        });
        cache_rx
    }

    /// On non-Windows, no background task — return an inert channel.
    #[cfg(not(windows))]
    fn init_background(_shutdown: Arc<AtomicBool>) -> watch::Receiver<Option<WuaCounts>> {
        let (_cache_tx, cache_rx) = watch::channel(None);
        cache_rx
    }

    /// Create an observer for testing without spawning a background task.
    /// The provided receiver controls what values the observer sees.
    #[cfg(test)]
    fn new_with_receiver(
        cache_rx: watch::Receiver<Option<WuaCounts>>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        Self {
            cache_rx,
            _shutdown: shutdown,
        }
    }
}

impl Observer for WindowsUpdateObserver {
    type Output = WindowsUpdateStatus;

    fn name(&self) -> &'static str {
        "windows_update"
    }

    fn observe(&mut self) -> WindowsUpdateStatus {
        let cached = *self.cache_rx.borrow();

        // The registry-based fields (last_search_time, last_install_time) are
        // cheap reads not covered by ADR-0020. They remain Unobserved until
        // a separate cheap-observer implementation lands.
        let (pending_update_count, pending_critical_update_count) = match cached {
            Some(counts) => {
                let age = OffsetDateTime::now_utc() - counts.observed_at;
                if age < time::Duration::seconds(1) {
                    // Observed within the current tick window.
                    (
                        Observation::Fresh {
                            value: counts.total,
                            observed_at: counts.observed_at,
                        },
                        Observation::Fresh {
                            value: counts.critical,
                            observed_at: counts.observed_at,
                        },
                    )
                } else {
                    (
                        Observation::Cached {
                            value: counts.total,
                            observed_at: counts.observed_at,
                        },
                        Observation::Cached {
                            value: counts.critical,
                            observed_at: counts.observed_at,
                        },
                    )
                }
            }
            None => (Observation::Unobserved, Observation::Unobserved),
        };

        WindowsUpdateStatus {
            last_search_time: Observation::Unobserved,
            last_install_time: Observation::Unobserved,
            pending_update_count,
            pending_critical_update_count,
        }
    }
}

/// Background refresh loop for the WUA COM API (Windows only).
///
/// Per ADR-0020:
/// 1. Wait until cache is empty or older than TTL.
/// 2. Call `spawn_blocking` on the WUA search routine with the internal
///    time guard.
/// 3. On success, write the new counts to the shared cache.
///    On failure, write nothing (prior values remain).
/// 4. Loop.
#[cfg(windows)]
async fn wua_background_loop(
    cache_tx: watch::Sender<Option<WuaCounts>>,
    shutdown: Arc<AtomicBool>,
) {
    use std::sync::atomic::Ordering;

    loop {
        if shutdown.load(Ordering::SeqCst) {
            tracing::debug!("wua background task: shutdown signalled, exiting");
            break;
        }

        // Determine whether a refresh is needed.
        let needs_refresh = match *cache_tx.borrow() {
            None => true,
            Some(counts) => {
                let age = OffsetDateTime::now_utc() - counts.observed_at;
                age >= time::Duration::try_from(WUA_CACHE_TTL).unwrap_or(time::Duration::HOUR)
            }
        };

        if needs_refresh {
            tracing::debug!("wua background task: initiating refresh");

            // Apply the internal time guard (120s) around the blocking WUA call.
            let result = tokio::time::timeout(
                WUA_INTERNAL_TIMEOUT,
                tokio::task::spawn_blocking(wua_search),
            )
            .await;

            match result {
                Ok(Ok(Ok(counts))) => {
                    tracing::info!(
                        total = counts.total,
                        critical = counts.critical,
                        "wua refresh succeeded"
                    );
                    let _ = cache_tx.send(Some(counts));
                }
                Ok(Ok(Err(e))) => {
                    // WUA call failed — per ADR-0020, leave cache unchanged.
                    tracing::warn!(error = %e, "wua search failed");
                }
                Ok(Err(join_err)) => {
                    // Panic in the blocking task.
                    tracing::warn!(error = %join_err, "wua search panicked");
                }
                Err(_timeout) => {
                    // Internal time guard exceeded (120s).
                    tracing::warn!(
                        timeout_secs = WUA_INTERNAL_TIMEOUT.as_secs(),
                        "wua search exceeded internal time guard"
                    );
                }
            }
        }

        // Sleep before checking again. Check shutdown frequently.
        for _ in 0..60 {
            if shutdown.load(Ordering::SeqCst) {
                tracing::debug!("wua background task: shutdown signalled during sleep");
                return;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

/// Perform the actual WUA COM search (Windows only).
///
/// This function runs inside `spawn_blocking`. It initializes COM,
/// creates an `IUpdateSession`, searches for `IsInstalled=0`, and
/// counts total and critical updates.
///
/// Per ADR-0020, "critical" means `MsrcSeverity` == `"Critical"` on the
/// `IUpdate` interface. All failure modes return an error; the caller
/// maps every error to `Observation::Unobserved`.
#[cfg(windows)]
fn wua_search() -> Result<WuaCounts, WuaError> {
    use windows::core::BSTR;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::UpdateAgent::{IUpdateSearcher, IUpdateSession, UpdateSession};

    // Guard that calls CoUninitialize on drop, ensuring COM cleanup
    // even if the function returns early via `?`.
    struct ComGuard;
    impl Drop for ComGuard {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }

    // Initialize COM in the spawned thread with STA apartment model
    // as required by WUA. CoInitializeEx returns HRESULT directly;
    // .ok() converts it to Result.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| WuaError::ComInit(format!("{e}")))?;
    }
    let _guard = ComGuard;

    // Create IUpdateSession.
    let session: IUpdateSession = unsafe {
        CoCreateInstance(&UpdateSession, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| WuaError::SessionCreate(format!("{e}")))?
    };

    // Create IUpdateSearcher.
    let searcher: IUpdateSearcher = unsafe {
        session
            .CreateUpdateSearcher()
            .map_err(|e| WuaError::SearcherCreate(format!("{e}")))?
    };

    // Search for updates that are not installed.
    let criteria = BSTR::from("IsInstalled=0");
    let search_result = unsafe {
        searcher
            .Search(&criteria)
            .map_err(|e| WuaError::Search(format!("{e}")))?
    };

    // Get the list of updates.
    let updates = unsafe {
        search_result
            .Updates()
            .map_err(|e| WuaError::ResultIteration(format!("{e}")))?
    };

    let count = unsafe {
        updates
            .Count()
            .map_err(|e| WuaError::ResultIteration(format!("{e}")))?
    };

    let total = u32::try_from(count).unwrap_or(u32::MAX);
    let mut critical = 0u32;

    // Count updates where MsrcSeverity == "Critical" per ADR-0020.
    for i in 0..count {
        let update = unsafe {
            updates
                .get_Item(i)
                .map_err(|e| WuaError::ResultIteration(format!("get_Item({i}): {e}")))?
        };

        let severity = unsafe {
            update
                .MsrcSeverity()
                .map_err(|e| WuaError::ResultIteration(format!("MsrcSeverity({i}): {e}")))?
        };

        if severity == "Critical" {
            critical = critical.saturating_add(1);
        }
    }

    Ok(WuaCounts {
        total,
        critical,
        observed_at: OffsetDateTime::now_utc(),
    })
}

/// Error type for WUA search failures.
///
/// Per ADR-0020, all variants map to `Observation::Unobserved`.
/// The diagnostic detail is logged locally, not transmitted on the wire.
#[derive(Debug)]
#[cfg(windows)]
enum WuaError {
    ComInit(String),
    SessionCreate(String),
    SearcherCreate(String),
    Search(String),
    ResultIteration(String),
}

#[cfg(windows)]
impl std::fmt::Display for WuaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ComInit(e) => write!(f, "COM initialization failed: {e}"),
            Self::SessionCreate(e) => write!(f, "IUpdateSession creation failed: {e}"),
            Self::SearcherCreate(e) => write!(f, "IUpdateSearcher creation failed: {e}"),
            Self::Search(e) => write!(f, "WUA search failed: {e}"),
            Self::ResultIteration(e) => write!(f, "result iteration failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_returns_all_unobserved_without_background_task() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let (_tx, rx) = watch::channel(None);
        let mut obs = WindowsUpdateObserver::new_with_receiver(rx, shutdown);
        let status = obs.observe();
        assert_eq!(status.pending_update_count, Observation::Unobserved);
        assert_eq!(
            status.pending_critical_update_count,
            Observation::Unobserved
        );
    }

    #[test]
    fn observe_returns_cached_when_cache_is_old() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let old_time = OffsetDateTime::now_utc() - time::Duration::hours(2);
        let counts = WuaCounts {
            total: 5,
            critical: 2,
            observed_at: old_time,
        };
        let (_tx, rx) = watch::channel(Some(counts));
        let mut obs = WindowsUpdateObserver::new_with_receiver(rx, shutdown);
        let status = obs.observe();

        // Values older than 1 second are Cached.
        match &status.pending_update_count {
            Observation::Cached { value, observed_at } => {
                assert_eq!(*value, 5);
                assert_eq!(*observed_at, old_time);
            }
            other => panic!("expected Cached, got {other:?}"),
        }
        match &status.pending_critical_update_count {
            Observation::Cached { value, observed_at } => {
                assert_eq!(*value, 2);
                assert_eq!(*observed_at, old_time);
            }
            other => panic!("expected Cached, got {other:?}"),
        }
    }

    #[test]
    fn observe_returns_fresh_when_cache_is_recent() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let now = OffsetDateTime::now_utc();
        let counts = WuaCounts {
            total: 3,
            critical: 1,
            observed_at: now,
        };
        let (_tx, rx) = watch::channel(Some(counts));
        let mut obs = WindowsUpdateObserver::new_with_receiver(rx, shutdown);
        let status = obs.observe();

        // Values within 1 second are Fresh.
        match &status.pending_update_count {
            Observation::Fresh { value, .. } => {
                assert_eq!(*value, 3);
            }
            other => panic!("expected Fresh, got {other:?}"),
        }
        match &status.pending_critical_update_count {
            Observation::Fresh { value, .. } => {
                assert_eq!(*value, 1);
            }
            other => panic!("expected Fresh, got {other:?}"),
        }
    }

    #[test]
    fn zero_pending_updates_is_fresh_not_unobserved() {
        // Per ADR-0020: "A successful search that returns zero updates
        // is not a failure. It maps to Observation::Fresh { value: 0, ... }"
        let shutdown = Arc::new(AtomicBool::new(false));
        let now = OffsetDateTime::now_utc();
        let counts = WuaCounts {
            total: 0,
            critical: 0,
            observed_at: now,
        };
        let (_tx, rx) = watch::channel(Some(counts));
        let mut obs = WindowsUpdateObserver::new_with_receiver(rx, shutdown);
        let status = obs.observe();

        match &status.pending_update_count {
            Observation::Fresh { value, .. } => assert_eq!(*value, 0),
            other => panic!("expected Fresh with value 0, got {other:?}"),
        }
    }

    #[test]
    fn failure_produces_unobserved() {
        // When the background task has never succeeded (cache is None),
        // the observer reports Unobserved for both fields.
        let shutdown = Arc::new(AtomicBool::new(false));
        let (_tx, rx) = watch::channel(None);
        let mut obs = WindowsUpdateObserver::new_with_receiver(rx, shutdown);
        let status = obs.observe();
        assert_eq!(status.pending_update_count, Observation::Unobserved);
        assert_eq!(
            status.pending_critical_update_count,
            Observation::Unobserved
        );
    }

    #[test]
    fn both_counts_always_paired() {
        // Per ADR-0020: the two counts are always emitted as a pair.
        let shutdown = Arc::new(AtomicBool::new(false));
        let now = OffsetDateTime::now_utc();
        let counts = WuaCounts {
            total: 7,
            critical: 3,
            observed_at: now,
        };
        let (_tx, rx) = watch::channel(Some(counts));
        let mut obs = WindowsUpdateObserver::new_with_receiver(rx, shutdown);
        let status = obs.observe();

        // Both should be the same variant (Fresh here).
        let total_is_fresh = matches!(status.pending_update_count, Observation::Fresh { .. });
        let critical_is_fresh = matches!(
            status.pending_critical_update_count,
            Observation::Fresh { .. }
        );
        assert_eq!(total_is_fresh, critical_is_fresh);
    }

    #[tokio::test]
    async fn cache_ttl_constants_are_sane() {
        // Verify the TTL and timeout constants have expected values.
        assert_eq!(WUA_CACHE_TTL, Duration::from_secs(3600));
        assert_eq!(WUA_INTERNAL_TIMEOUT, Duration::from_secs(120));
        // The internal timeout must differ from the per-observer budget (500ms default).
        assert_ne!(WUA_INTERNAL_TIMEOUT, Duration::from_millis(500));
    }
}
