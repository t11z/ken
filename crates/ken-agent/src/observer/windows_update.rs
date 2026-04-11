//! Windows Update observer per ADR-0020 and ADR-0021.
//!
//! Collects pending update counts from the Windows Update Agent COM API
//! on a background-refresh schedule with a one-hour cache TTL. The
//! heartbeat-tick path never blocks on a WUA call; it reads from the
//! internal cache and tags values via [`TickBoundary`] per ADR-0021.
//!
//! On non-Windows platforms, all fields are `Unobserved`.

use std::time::Duration;

use ken_protocol::status::{Observation, WindowsUpdateStatus};
use time::OffsetDateTime;
use tokio::sync::watch;

use super::lifecycle::ObserverLifecycle;
use super::tick::TickBoundary;
use super::trait_def::{Observer, ObserverKind};

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

/// Windows Update observer struct per ADR-0018, ADR-0020, and ADR-0021.
///
/// On Windows, the [`start`](Observer::start) lifecycle hook spawns a
/// long-lived background task that periodically queries WUA and writes
/// results into a shared cache. The [`observe`](Observer::observe)
/// method on the heartbeat path reads the cache and tags values via
/// [`TickBoundary::tag`], never touching WUA directly.
///
/// On non-Windows, all fields are permanently `Unobserved`.
pub struct WindowsUpdateObserver {
    /// Receiver for the shared cache written by the background task.
    cache_rx: watch::Receiver<Option<WuaCounts>>,

    /// Sender half, held until [`start`](Observer::start) passes it
    /// to the background task. `None` after `start` is called or in
    /// test-only construction.
    cache_tx: Option<watch::Sender<Option<WuaCounts>>>,
}

impl WindowsUpdateObserver {
    /// Create a new Windows Update observer.
    ///
    /// The background task is **not** spawned here. It is spawned in
    /// [`start`](Observer::start) via the [`ObserverLifecycle`] hook,
    /// per ADR-0021.
    #[must_use]
    pub fn new() -> Self {
        let (cache_tx, cache_rx) = watch::channel(None);
        Self {
            cache_rx,
            cache_tx: Some(cache_tx),
        }
    }

    /// Create an observer for testing without spawning a background task.
    /// The provided receiver controls what values the observer sees.
    #[cfg(test)]
    fn new_with_receiver(cache_rx: watch::Receiver<Option<WuaCounts>>) -> Self {
        Self {
            cache_rx,
            cache_tx: None,
        }
    }

    /// Create an observer pre-loaded with cached values for testing.
    #[cfg(test)]
    pub(crate) fn new_with_cache(total: u32, critical: u32, observed_at: OffsetDateTime) -> Self {
        let counts = WuaCounts {
            total,
            critical,
            observed_at,
        };
        let (_tx, rx) = watch::channel(Some(counts));
        Self {
            cache_rx: rx,
            cache_tx: None,
        }
    }
}

// Per ADR-0022, the Observer trait requires UnwindSafe. WindowsUpdateObserver
// holds tokio watch channels whose internal implementation uses raw pointers
// in the waiter list, making them !UnwindSafe by default. However, the
// observer's observe() method only calls watch::Receiver::borrow(), which
// acquires a read lock atomically. A panic during observe() (which cannot
// happen in practice for a borrow()) would leave the channels in a consistent
// state: the read guard is dropped during unwinding, the sender and receiver
// are intact, and the next call to observe() operates normally. The mutex
// around the observer in ObserverSlot handles any state-crossing concern.
impl std::panic::UnwindSafe for WindowsUpdateObserver {}

impl Observer for WindowsUpdateObserver {
    type Output = WindowsUpdateStatus;
    const KIND: ObserverKind = ObserverKind::BackgroundRefresh;

    fn name(&self) -> &'static str {
        "windows_update"
    }

    fn observe(&mut self, tick: &TickBoundary) -> WindowsUpdateStatus {
        let cached = *self.cache_rx.borrow();

        // The registry-based fields (last_search_time, last_install_time) are
        // cheap reads not covered by ADR-0020. They remain Unobserved until
        // a separate cheap-observer implementation lands.
        let (pending_update_count, pending_critical_update_count) = match cached {
            Some(counts) => (
                tick.tag(counts.total, counts.observed_at),
                tick.tag(counts.critical, counts.observed_at),
            ),
            None => (Observation::Unobserved, Observation::Unobserved),
        };

        WindowsUpdateStatus {
            last_search_time: Observation::Unobserved,
            last_install_time: Observation::Unobserved,
            pending_update_count,
            pending_critical_update_count,
        }
    }

    fn start(&mut self, lifecycle: ObserverLifecycle) {
        // Take the sender out — start() is called exactly once.
        let Some(cache_tx) = self.cache_tx.take() else {
            return;
        };

        #[cfg(windows)]
        {
            let shutdown = lifecycle.shutdown_signal().clone();
            lifecycle.spawn_background(async move {
                wua_background_loop(cache_tx, shutdown).await;
            });
        }

        #[cfg(not(windows))]
        {
            let _ = lifecycle;
            // Drop the sender so the receiver permanently returns None,
            // producing Unobserved for all fields on non-Windows.
            drop(cache_tx);
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
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
        let (_tx, rx) = watch::channel(None);
        let mut obs = WindowsUpdateObserver::new_with_receiver(rx);
        let tick = TickBoundary::now();
        let status = obs.observe(&tick);
        assert_eq!(status.pending_update_count, Observation::Unobserved);
        assert_eq!(
            status.pending_critical_update_count,
            Observation::Unobserved
        );
    }

    #[test]
    fn observe_returns_cached_when_cache_is_old() {
        let old_time = OffsetDateTime::now_utc() - time::Duration::hours(2);
        let counts = WuaCounts {
            total: 5,
            critical: 2,
            observed_at: old_time,
        };
        let (_tx, rx) = watch::channel(Some(counts));
        let mut obs = WindowsUpdateObserver::new_with_receiver(rx);
        // Tick boundary is now, which is after old_time → Cached.
        let tick = TickBoundary::now();
        let status = obs.observe(&tick);

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
    fn observe_returns_fresh_when_observed_at_or_after_tick() {
        let now = OffsetDateTime::now_utc();
        let counts = WuaCounts {
            total: 3,
            critical: 1,
            observed_at: now,
        };
        let (_tx, rx) = watch::channel(Some(counts));
        let mut obs = WindowsUpdateObserver::new_with_receiver(rx);
        // Tick boundary at or before observed_at → Fresh.
        let tick = TickBoundary::at(now - time::Duration::milliseconds(1));
        let status = obs.observe(&tick);

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
        let now = OffsetDateTime::now_utc();
        let counts = WuaCounts {
            total: 0,
            critical: 0,
            observed_at: now,
        };
        let (_tx, rx) = watch::channel(Some(counts));
        let mut obs = WindowsUpdateObserver::new_with_receiver(rx);
        let tick = TickBoundary::at(now);
        let status = obs.observe(&tick);

        match &status.pending_update_count {
            Observation::Fresh { value, .. } => assert_eq!(*value, 0),
            other => panic!("expected Fresh with value 0, got {other:?}"),
        }
    }

    #[test]
    fn failure_produces_unobserved() {
        // When the background task has never succeeded (cache is None),
        // the observer reports Unobserved for both fields.
        let (_tx, rx) = watch::channel(None);
        let mut obs = WindowsUpdateObserver::new_with_receiver(rx);
        let tick = TickBoundary::now();
        let status = obs.observe(&tick);
        assert_eq!(status.pending_update_count, Observation::Unobserved);
        assert_eq!(
            status.pending_critical_update_count,
            Observation::Unobserved
        );
    }

    #[test]
    fn both_counts_always_paired() {
        // Per ADR-0020: the two counts are always emitted as a pair.
        let now = OffsetDateTime::now_utc();
        let counts = WuaCounts {
            total: 7,
            critical: 3,
            observed_at: now,
        };
        let (_tx, rx) = watch::channel(Some(counts));
        let mut obs = WindowsUpdateObserver::new_with_receiver(rx);
        let tick = TickBoundary::at(now);
        let status = obs.observe(&tick);

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
