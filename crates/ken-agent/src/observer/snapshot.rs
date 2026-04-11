//! Orchestration that assembles a full OS status snapshot from
//! individual observers per ADR-0018 and ADR-0021.
//!
//! Synchronous observers are invoked via `tokio::task::spawn_blocking`
//! with a per-observer time budget. Background-refresh observers are
//! invoked directly because their read path is non-blocking by contract.
//!
//! Panics are caught at the `spawn_blocking` boundary for synchronous
//! observers and result in the observer's slot being cleared (the struct
//! is consumed by the panic). Other observers are unaffected.

use std::time::Duration;

use ken_protocol::status::{
    BitLockerStatus, DefenderStatus, FirewallStatus, Observation, OsStatusSnapshot,
    WindowsUpdateStatus,
};
use time::OffsetDateTime;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::lifecycle::{BackgroundTaskHandles, ObserverLifecycle, SHUTDOWN_GRACE_PERIOD};
use super::tick::TickBoundary;
use super::trait_def::Observer;
use super::{
    BitLockerObserver, DefenderObserver, EventLogObserver, FirewallObserver, WindowsUpdateObserver,
};

/// Per-observer read budget for background-refresh observers.
///
/// Background-refresh observers are non-blocking by contract (their
/// `observe` method reads from a cache), so this budget is much
/// smaller than the synchronous observer budget. It exists as a
/// safeguard against a grossly misused `watch::borrow()` that could
/// in principle deadlock. A few hundred milliseconds is generous for
/// a channel read. Enforced as a timing warning, not an abort.
const BACKGROUND_READ_BUDGET: Duration = Duration::from_millis(200);

/// The set of all observers held by the worker across heartbeat ticks.
///
/// Per ADR-0018 and ADR-0021, each observer is a struct that owns its
/// state. The `ObserverSet` holds one observer per subsystem and
/// provides the async `collect_snapshot` method that the worker loop
/// calls. Background tasks spawned through [`ObserverLifecycle`] are
/// tracked via shared join handles and joined on [`shutdown`](Self::shutdown).
pub struct ObserverSet {
    defender: Option<DefenderObserver>,
    firewall: Option<FirewallObserver>,
    bitlocker: Option<BitLockerObserver>,
    windows_update: Option<WindowsUpdateObserver>,
    event_log: Option<EventLogObserver>,

    /// Per-observer time budget for synchronous observers (ADR-0018).
    sync_budget: Duration,

    /// Join handles for background tasks, shared with [`ObserverLifecycle`].
    handles: BackgroundTaskHandles,
}

impl ObserverSet {
    /// Create a new observer set with the given per-observer time budget.
    ///
    /// Per ADR-0021, the constructor takes the resources needed to build
    /// an [`ObserverLifecycle`] for each observer: the shutdown signal
    /// and the Tokio runtime handle. Each observer's
    /// [`start`](Observer::start) lifecycle hook is called exactly once,
    /// in deterministic order.
    #[must_use]
    pub fn new(
        budget: Duration,
        shutdown: &Arc<AtomicBool>,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        let handles: BackgroundTaskHandles = Arc::new(std::sync::Mutex::new(Vec::new()));

        let mut defender = DefenderObserver::new();
        let mut firewall = FirewallObserver::new();
        let mut bitlocker = BitLockerObserver::new();
        let mut windows_update = WindowsUpdateObserver::new();
        let mut event_log = EventLogObserver::new();

        // Call lifecycle hooks in deterministic order per ADR-0021.
        defender.start(ObserverLifecycle::new(
            Arc::clone(shutdown),
            runtime.clone(),
            handles.clone(),
        ));
        firewall.start(ObserverLifecycle::new(
            Arc::clone(shutdown),
            runtime.clone(),
            handles.clone(),
        ));
        bitlocker.start(ObserverLifecycle::new(
            Arc::clone(shutdown),
            runtime.clone(),
            handles.clone(),
        ));
        windows_update.start(ObserverLifecycle::new(
            Arc::clone(shutdown),
            runtime.clone(),
            handles.clone(),
        ));
        event_log.start(ObserverLifecycle::new(
            Arc::clone(shutdown),
            runtime,
            handles.clone(),
        ));

        Self {
            defender: Some(defender),
            firewall: Some(firewall),
            bitlocker: Some(bitlocker),
            windows_update: Some(windows_update),
            event_log: Some(event_log),
            sync_budget: budget,
            handles,
        }
    }

    /// Collect a complete OS status snapshot by invoking all observers.
    ///
    /// Per ADR-0021, a single [`TickBoundary`] is computed once at the
    /// start and passed to every observer. Synchronous observers run in
    /// `spawn_blocking` with the per-observer budget (ADR-0018).
    /// Background-refresh observers are invoked directly because their
    /// read path is non-blocking by contract.
    pub async fn collect_snapshot(&mut self) -> OsStatusSnapshot {
        let collected_at = OffsetDateTime::now_utc();
        let tick = TickBoundary::now();
        let budget = self.sync_budget;

        // --- Synchronous observers: via run_observer (ADR-0018) ---

        let (obs, defender) =
            run_observer(self.defender.take(), &unobserved_defender(), budget, &tick).await;
        self.defender = obs;

        let (obs, firewall) =
            run_observer(self.firewall.take(), &unobserved_firewall(), budget, &tick).await;
        self.firewall = obs;

        let (obs, bitlocker) = run_observer(
            self.bitlocker.take(),
            &unobserved_bitlocker(),
            budget,
            &tick,
        )
        .await;
        self.bitlocker = obs;

        let (obs, recent_security_events) = run_observer(
            self.event_log.take(),
            &Observation::Unobserved,
            budget,
            &tick,
        )
        .await;
        self.event_log = obs;

        // --- Background-refresh observers: invoked directly (ADR-0021) ---

        let windows_update =
            read_background_observer(&mut self.windows_update, &tick, unobserved_windows_update());

        tracing::debug!("status snapshot collected");

        OsStatusSnapshot {
            collected_at,
            defender,
            firewall,
            bitlocker,
            windows_update,
            recent_security_events,
        }
    }

    /// Join all background observer tasks with the bounded grace period.
    ///
    /// Per ADR-0021, tasks that do not join within [`SHUTDOWN_GRACE_PERIOD`]
    /// are abandoned (their `JoinHandle` is dropped). The worker loop
    /// calls this on its exit path.
    pub async fn shutdown(&mut self) {
        let handles = {
            let mut guard = self
                .handles
                .lock()
                .expect("background task handle lock poisoned");
            std::mem::take(&mut *guard)
        };

        if handles.is_empty() {
            return;
        }

        tracing::debug!(count = handles.len(), "joining background observer tasks");

        let join_all = async {
            for handle in handles {
                let _ = handle.await;
            }
        };

        if tokio::time::timeout(SHUTDOWN_GRACE_PERIOD, join_all)
            .await
            .is_err()
        {
            tracing::warn!(
                grace_period_secs = SHUTDOWN_GRACE_PERIOD.as_secs(),
                "background observer tasks did not join within grace period, abandoning"
            );
        }
    }

    /// Inject a background task handle for testing. Allows tests to
    /// exercise `shutdown` without requiring Windows-only observers.
    #[cfg(test)]
    fn inject_background_task(&self, handle: tokio::task::JoinHandle<()>) {
        self.handles
            .lock()
            .expect("background task handle lock poisoned")
            .push(handle);
    }
}

/// Read a background-refresh observer's cached value directly.
///
/// Per ADR-0021, background-refresh observers are invoked directly
/// because their read path is non-blocking by contract. A timing
/// check warns if the read exceeds [`BACKGROUND_READ_BUDGET`], but
/// does not abort the call.
fn read_background_observer<O>(
    observer: &mut Option<O>,
    tick: &TickBoundary,
    fallback: O::Output,
) -> O::Output
where
    O: Observer,
{
    match observer.as_mut() {
        Some(obs) => {
            let start = std::time::Instant::now();
            let result = obs.observe(tick);
            let elapsed = start.elapsed();
            if elapsed > BACKGROUND_READ_BUDGET {
                tracing::warn!(
                    observer = obs.name(),
                    elapsed_ms = elapsed.as_millis(),
                    budget_ms = BACKGROUND_READ_BUDGET.as_millis(),
                    "background observer read exceeded budget"
                );
            }
            result
        }
        None => fallback,
    }
}

/// Run a single synchronous observer with `spawn_blocking`, budget
/// timeout, and panic isolation.
///
/// Per ADR-0018:
/// - The observer body runs in `spawn_blocking` (sync body, async boundary).
/// - A `tokio::time::timeout` enforces the per-observer budget.
/// - A panic in the observer is caught via the `JoinError` from `spawn_blocking`.
/// - On timeout or panic, the fallback value is returned.
///
/// Returns `(Option<observer>, output)`. The observer is `None` if it
/// panicked (the struct was consumed by the panic) or if the input was
/// already `None`.
async fn run_observer<O>(
    observer: Option<O>,
    fallback: &O::Output,
    budget: Duration,
    tick: &TickBoundary,
) -> (Option<O>, O::Output)
where
    O: Observer,
    O::Output: Clone + Send + 'static,
{
    let Some(mut obs) = observer else {
        // Observer was lost to a previous panic. Return fallback.
        return (None, fallback.clone());
    };

    let name = obs.name().to_string();
    let start = std::time::Instant::now();
    let tick = *tick;

    // Move the observer into spawn_blocking and get it back with the result.
    let result = tokio::time::timeout(
        budget,
        tokio::task::spawn_blocking(move || {
            let output = obs.observe(&tick);
            (obs, output)
        }),
    )
    .await;

    let elapsed = start.elapsed();

    match result {
        Ok(Ok((obs_back, output))) => {
            tracing::debug!(
                observer = name,
                elapsed_ms = elapsed.as_millis(),
                "observer completed"
            );
            (Some(obs_back), output)
        }
        Ok(Err(join_err)) => {
            // Panic in the observer — ADR-0018 failure isolation.
            // The observer struct is lost (consumed by the panic).
            tracing::warn!(
                observer = name,
                elapsed_ms = elapsed.as_millis(),
                error = %join_err,
                "observer panicked, using cached value"
            );
            (None, fallback.clone())
        }
        Err(_timeout) => {
            // Budget exceeded — ADR-0018 bounded per-tick budget.
            // The observer is still running in the background thread;
            // we cannot cancel it (sync FFI is not cancellable, as
            // ADR-0018 accepted). The observer struct is lost to the
            // in-flight task.
            tracing::warn!(
                observer = name,
                budget_ms = budget.as_millis(),
                "observer exceeded budget, using cached value"
            );
            (None, fallback.clone())
        }
    }
}

fn unobserved_defender() -> DefenderStatus {
    DefenderStatus {
        antivirus_enabled: Observation::Unobserved,
        real_time_protection_enabled: Observation::Unobserved,
        tamper_protection_enabled: Observation::Unobserved,
        signature_version: Observation::Unobserved,
        signature_last_updated: Observation::Unobserved,
        signature_age_days: Observation::Unobserved,
        last_full_scan: Observation::Unobserved,
        last_quick_scan: Observation::Unobserved,
    }
}

fn unobserved_firewall() -> FirewallStatus {
    FirewallStatus {
        domain_profile: Observation::Unobserved,
        private_profile: Observation::Unobserved,
        public_profile: Observation::Unobserved,
    }
}

fn unobserved_bitlocker() -> BitLockerStatus {
    BitLockerStatus {
        volumes: Observation::Unobserved,
    }
}

fn unobserved_windows_update() -> WindowsUpdateStatus {
    WindowsUpdateStatus {
        last_search_time: Observation::Unobserved,
        last_install_time: Observation::Unobserved,
        pending_update_count: Observation::Unobserved,
        pending_critical_update_count: Observation::Unobserved,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::super::lifecycle::ObserverLifecycle;
    use super::super::trait_def::ObserverKind;
    use super::*;

    // ---------------------------------------------------------------
    // Existing tests (mechanically adapted to new trait shape)
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn snapshot_collects_without_panic() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let runtime = tokio::runtime::Handle::current();
        let mut set = ObserverSet::new(Duration::from_millis(500), &shutdown, runtime);
        let snapshot = set.collect_snapshot().await;
        // On non-Windows, all observers return Unobserved.
        assert_eq!(snapshot.recent_security_events, Observation::Unobserved);
        assert_eq!(snapshot.defender.antivirus_enabled, Observation::Unobserved);
    }

    /// ADR-0018 load-bearing test: a panicking observer must not affect
    /// other observers or the worker loop. The panicking observer's
    /// contribution to the snapshot is its fallback value (Unobserved
    /// when there is no prior value).
    #[tokio::test]
    async fn panicking_observer_is_isolated() {
        struct PanickingObserver;
        impl Observer for PanickingObserver {
            type Output = FirewallStatus;
            const KIND: ObserverKind = ObserverKind::Synchronous;
            fn name(&self) -> &'static str {
                "panicking"
            }
            fn observe(&mut self, _tick: &TickBoundary) -> FirewallStatus {
                panic!("deliberate test panic in observer");
            }
            fn start(&mut self, _lifecycle: ObserverLifecycle) {}
        }

        struct GoodObserver;
        impl Observer for GoodObserver {
            type Output = FirewallStatus;
            const KIND: ObserverKind = ObserverKind::Synchronous;
            fn name(&self) -> &'static str {
                "good"
            }
            fn observe(&mut self, _tick: &TickBoundary) -> FirewallStatus {
                FirewallStatus {
                    domain_profile: Observation::Fresh {
                        value: ken_protocol::status::FirewallProfileState {
                            enabled: true,
                            default_inbound_action: "block".to_string(),
                        },
                        observed_at: OffsetDateTime::now_utc(),
                    },
                    private_profile: Observation::Unobserved,
                    public_profile: Observation::Unobserved,
                }
            }
            fn start(&mut self, _lifecycle: ObserverLifecycle) {}
        }

        let fallback = unobserved_firewall();
        let budget = Duration::from_secs(2);
        let tick = TickBoundary::now();

        // The panicking observer should return the fallback and lose
        // its observer struct (None).
        let (obs_back, result) =
            run_observer(Some(PanickingObserver), &fallback, budget, &tick).await;
        assert!(obs_back.is_none(), "panicked observer should be lost");
        assert_eq!(result.domain_profile, Observation::Unobserved);

        // The good observer should work normally despite the previous panic.
        let (obs_back, result) = run_observer(Some(GoodObserver), &fallback, budget, &tick).await;
        assert!(obs_back.is_some(), "good observer should be returned");
        assert!(result.domain_profile.value().is_some());
    }

    /// Verify that the budget timeout produces the fallback value.
    #[tokio::test]
    async fn budget_timeout_returns_fallback() {
        struct SlowObserver;
        impl Observer for SlowObserver {
            type Output = FirewallStatus;
            const KIND: ObserverKind = ObserverKind::Synchronous;
            fn name(&self) -> &'static str {
                "slow"
            }
            fn observe(&mut self, _tick: &TickBoundary) -> FirewallStatus {
                std::thread::sleep(Duration::from_secs(5));
                FirewallStatus {
                    domain_profile: Observation::Fresh {
                        value: ken_protocol::status::FirewallProfileState {
                            enabled: true,
                            default_inbound_action: "block".to_string(),
                        },
                        observed_at: OffsetDateTime::now_utc(),
                    },
                    private_profile: Observation::Unobserved,
                    public_profile: Observation::Unobserved,
                }
            }
            fn start(&mut self, _lifecycle: ObserverLifecycle) {}
        }

        let fallback = unobserved_firewall();
        // Very short budget to trigger timeout.
        let budget = Duration::from_millis(10);
        let tick = TickBoundary::now();

        let (obs_back, result) = run_observer(Some(SlowObserver), &fallback, budget, &tick).await;
        // Observer is lost because it's still running.
        assert!(obs_back.is_none());
        // Should get the fallback because the observer exceeded the budget.
        assert_eq!(result.domain_profile, Observation::Unobserved);
    }

    // ---------------------------------------------------------------
    // New tests per ADR-0021
    // ---------------------------------------------------------------

    /// Test 1: Tick-boundary re-tagging is correct.
    ///
    /// A fake observer with a known `observed_at` is read with a tick
    /// boundary after the timestamp (→ Cached) and before it (→ Fresh).
    /// This is the test that the old one-second heuristic would fail.
    #[tokio::test]
    async fn tick_boundary_retagging_is_correct() {
        let observed_at = OffsetDateTime::now_utc() - time::Duration::hours(1);

        // Tick boundary AFTER observed_at → Cached.
        let tick_after = TickBoundary::at(OffsetDateTime::now_utc());
        let result = tick_after.tag(42u32, observed_at);
        match result {
            Observation::Cached { value, .. } => assert_eq!(value, 42),
            other => panic!("expected Cached with tick after, got {other:?}"),
        }

        // Tick boundary BEFORE observed_at → Fresh.
        let tick_before = TickBoundary::at(observed_at - time::Duration::seconds(1));
        let result = tick_before.tag(42u32, observed_at);
        match result {
            Observation::Fresh { value, .. } => assert_eq!(value, 42),
            other => panic!("expected Fresh with tick before, got {other:?}"),
        }

        // Tick boundary EQUAL to observed_at → Fresh (>= comparison).
        let tick_equal = TickBoundary::at(observed_at);
        let result = tick_equal.tag(42u32, observed_at);
        match result {
            Observation::Fresh { value, .. } => assert_eq!(value, 42),
            other => panic!("expected Fresh with tick equal, got {other:?}"),
        }
    }

    /// Test 2: Synchronous observer happy path.
    ///
    /// A synchronous observer returning a known `Fresh` value is
    /// invoked through `run_observer` and the value appears with the
    /// correct tag.
    #[tokio::test]
    async fn sync_observer_happy_path() {
        struct FreshObserver(OffsetDateTime);
        impl Observer for FreshObserver {
            type Output = FirewallStatus;
            const KIND: ObserverKind = ObserverKind::Synchronous;
            fn name(&self) -> &'static str {
                "fresh_test"
            }
            fn observe(&mut self, tick: &TickBoundary) -> FirewallStatus {
                FirewallStatus {
                    domain_profile: tick.tag(
                        ken_protocol::status::FirewallProfileState {
                            enabled: true,
                            default_inbound_action: "block".to_string(),
                        },
                        self.0,
                    ),
                    private_profile: Observation::Unobserved,
                    public_profile: Observation::Unobserved,
                }
            }
            fn start(&mut self, _lifecycle: ObserverLifecycle) {}
        }

        let now = OffsetDateTime::now_utc();
        let fallback = unobserved_firewall();
        let budget = Duration::from_secs(2);
        // Tick boundary at or before `now` so the value is Fresh.
        let tick = TickBoundary::at(now);

        let (obs_back, result) =
            run_observer(Some(FreshObserver(now)), &fallback, budget, &tick).await;
        assert!(obs_back.is_some());
        match &result.domain_profile {
            Observation::Fresh { value, .. } => assert!(value.enabled),
            other => panic!("expected Fresh, got {other:?}"),
        }
    }

    /// Test 3: Background-refresh observer happy path.
    ///
    /// A background observer whose internal cache contains a value
    /// is read through `read_background_observer` and the value
    /// is tagged according to the tick boundary.
    #[test]
    fn background_observer_happy_path() {
        let observed_at = OffsetDateTime::now_utc() - time::Duration::minutes(30);
        let mut obs = Some(WindowsUpdateObserver::new_with_cache(5, 2, observed_at));

        // Tick is after observed_at → Cached.
        let tick = TickBoundary::now();
        let result = read_background_observer(&mut obs, &tick, unobserved_windows_update());
        match &result.pending_update_count {
            Observation::Cached { value, .. } => assert_eq!(*value, 5),
            other => panic!("expected Cached, got {other:?}"),
        }

        // Tick is before observed_at → Fresh.
        let tick = TickBoundary::at(observed_at - time::Duration::seconds(1));
        let result = read_background_observer(&mut obs, &tick, unobserved_windows_update());
        match &result.pending_update_count {
            Observation::Fresh { value, .. } => assert_eq!(*value, 5),
            other => panic!("expected Fresh, got {other:?}"),
        }
    }

    /// Test 4: Tick boundary is shared across observers within one call.
    ///
    /// Two observers in the same `run_observer` sequence both receive
    /// the same `TickBoundary`. Verified by recording the tick inside
    /// each fake observer.
    #[tokio::test]
    async fn tick_boundary_shared_across_observers() {
        use std::sync::Mutex as StdMutex;

        struct TickRecorder {
            ticks: Arc<StdMutex<Vec<OffsetDateTime>>>,
        }
        impl Observer for TickRecorder {
            type Output = FirewallStatus;
            const KIND: ObserverKind = ObserverKind::Synchronous;
            fn name(&self) -> &'static str {
                "tick_recorder"
            }
            fn observe(&mut self, tick: &TickBoundary) -> FirewallStatus {
                self.ticks.lock().unwrap().push(tick.timestamp());
                unobserved_firewall()
            }
            fn start(&mut self, _lifecycle: ObserverLifecycle) {}
        }

        let ticks1 = Arc::new(StdMutex::new(Vec::new()));
        let ticks2 = Arc::new(StdMutex::new(Vec::new()));

        let obs1 = TickRecorder {
            ticks: ticks1.clone(),
        };
        let obs2 = TickRecorder {
            ticks: ticks2.clone(),
        };

        let tick = TickBoundary::now();
        let fallback = unobserved_firewall();
        let budget = Duration::from_secs(2);

        let _ = run_observer(Some(obs1), &fallback, budget, &tick).await;
        let _ = run_observer(Some(obs2), &fallback, budget, &tick).await;

        let recorded1 = ticks1.lock().unwrap();
        let recorded2 = ticks2.lock().unwrap();
        assert_eq!(recorded1.len(), 1);
        assert_eq!(recorded2.len(), 1);
        assert_eq!(
            recorded1[0], recorded2[0],
            "both observers must receive the same tick boundary"
        );
    }

    /// Test 5a: Shutdown joins background tasks within the grace period.
    ///
    /// A background task that respects shutdown completes within the
    /// grace period when the shutdown signal is set.
    #[tokio::test]
    async fn shutdown_joins_cooperative_tasks() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let runtime = tokio::runtime::Handle::current();
        let mut set = ObserverSet::new(Duration::from_millis(500), &shutdown, runtime);

        // Inject a cooperative background task.
        let sd = shutdown.clone();
        set.inject_background_task(tokio::spawn(async move {
            while !sd.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }));

        shutdown.store(true, Ordering::SeqCst);

        let start = std::time::Instant::now();
        set.shutdown().await;
        // Cooperative task should join well within the grace period.
        assert!(start.elapsed() < SHUTDOWN_GRACE_PERIOD);
    }

    /// Test 5b: Shutdown returns within the grace period even when a
    /// background task ignores the shutdown signal.
    #[tokio::test]
    async fn shutdown_abandons_stuck_tasks() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let runtime = tokio::runtime::Handle::current();
        let mut set = ObserverSet::new(Duration::from_millis(500), &shutdown, runtime);

        // Inject a task that ignores shutdown entirely.
        set.inject_background_task(tokio::spawn(async {
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        }));

        shutdown.store(true, Ordering::SeqCst);

        let start = std::time::Instant::now();
        set.shutdown().await;
        // Must complete within grace period + tolerance.
        assert!(
            start.elapsed() < SHUTDOWN_GRACE_PERIOD + Duration::from_millis(500),
            "shutdown should not block beyond the grace period"
        );
    }
}
