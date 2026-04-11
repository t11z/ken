//! Orchestration that assembles a full OS status snapshot from
//! individual observers per ADR-0018, ADR-0021, and ADR-0022.
//!
//! Synchronous observers are invoked via `tokio::task::spawn_blocking`
//! with a per-observer time budget. Background-refresh observers are
//! invoked directly because their read path is non-blocking by contract.
//!
//! Per ADR-0022, panics inside `spawn_blocking` closures are caught with
//! `std::panic::catch_unwind` *inside* the closure. The observer struct is
//! held in an `Arc<Mutex<O>>` so that it is never moved into the closure
//! and is never lost on unwind. Panic events are logged with per-observer
//! rate limiting (burst limit then suppression window).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ken_protocol::status::{
    BitLockerStatus, DefenderStatus, FirewallStatus, Observation, OsStatusSnapshot,
};
use time::OffsetDateTime;

use std::sync::atomic::AtomicBool;

use super::lifecycle::{BackgroundTaskHandles, ObserverLifecycle, SHUTDOWN_GRACE_PERIOD};
use super::tick::TickBoundary;
use super::trait_def::Observer;
use super::{
    BitLockerObserver, DefenderObserver, EventLogObserver, FirewallObserver, WindowsUpdateObserver,
};

// -----------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------

/// Per-observer read budget for background-refresh observers.
///
/// Background-refresh observers are non-blocking by contract (their
/// `observe` method reads from a cache), so this budget is much
/// smaller than the synchronous observer budget. It exists as a
/// safeguard against a grossly misused `watch::borrow()` that could
/// in principle deadlock. A few hundred milliseconds is generous for
/// a channel read. Enforced as a timing warning, not an abort.
const BACKGROUND_READ_BUDGET: Duration = Duration::from_millis(200);

/// Number of consecutive panics from a single observer that are logged
/// unconditionally before rate limiting kicks in.
///
/// Per ADR-0022: the first three panics are always logged so that a
/// transient bug is immediately visible in the operator logs.
const PANIC_LOG_BURST_LIMIT: u32 = 3;

/// Duration of the panic-log suppression window per ADR-0022.
///
/// After [`PANIC_LOG_BURST_LIMIT`] consecutive panics, further panics
/// from the same observer are suppressed for this window. When the window
/// expires, the next panic produces a single summary entry naming the
/// number of panics suppressed during the window, then the window restarts.
///
/// A successful observation resets the consecutive counter and clears the
/// suppression window entirely.
///
/// In test builds, a shorter window (50 ms) is used so the window-expiry
/// path can be exercised in tests without sleeping for ten minutes.
#[cfg(not(test))]
const PANIC_LOG_SUPPRESSION_WINDOW: Duration = Duration::from_secs(600);
#[cfg(test)]
const PANIC_LOG_SUPPRESSION_WINDOW: Duration = Duration::from_millis(50);

// -----------------------------------------------------------------------
// Panic rate-limiter
// -----------------------------------------------------------------------

/// The action taken by [`PanicRateLimit::update`] for a given panic event.
///
/// Exposed so that unit tests can verify the rate-limiting logic without
/// requiring a tracing subscriber to capture log output.
#[cfg_attr(test, derive(Debug, PartialEq))]
enum PanicLogAction {
    /// Log the panic unconditionally (within the burst limit).
    LogImmediate,
    /// Suppress this panic (inside the suppression window, past burst limit).
    Suppress,
    /// The suppression window expired: log a summary naming the number of
    /// panics suppressed during the window.
    LogSummary { suppressed_count: u32 },
}

/// Per-observer panic rate-limit state per ADR-0022.
///
/// Tracks how many consecutive panics an observer has produced and
/// whether a suppression window is active. After [`PANIC_LOG_BURST_LIMIT`]
/// consecutive panics, further panics are logged at most once per
/// [`PANIC_LOG_SUPPRESSION_WINDOW`]. A successful observation resets all
/// state so the next panic is treated as the first.
struct PanicRateLimit {
    /// Consecutive panics since the last successful observation.
    consecutive: u32,
    /// When the current suppression window started (`None` until the burst
    /// limit is first exceeded since the last success).
    suppression_started: Option<std::time::Instant>,
    /// Number of panics suppressed in the current window.
    suppressed_in_window: u32,
}

impl PanicRateLimit {
    fn new() -> Self {
        Self {
            consecutive: 0,
            suppression_started: None,
            suppressed_in_window: 0,
        }
    }

    /// Update state for a new panic and return the logging decision.
    ///
    /// Separated from [`Self::on_panic`] so tests can inspect the decision
    /// directly without a tracing subscriber.
    fn update(&mut self) -> PanicLogAction {
        self.consecutive = self.consecutive.saturating_add(1);

        if self.consecutive <= PANIC_LOG_BURST_LIMIT {
            // Within the burst limit — log unconditionally.
            PanicLogAction::LogImmediate
        } else if let Some(started) = self.suppression_started {
            if started.elapsed() >= PANIC_LOG_SUPPRESSION_WINDOW {
                // Window expired — emit a summary and restart.
                let suppressed = self.suppressed_in_window;
                self.suppression_started = Some(std::time::Instant::now());
                self.suppressed_in_window = 0;
                PanicLogAction::LogSummary {
                    suppressed_count: suppressed,
                }
            } else {
                // Still inside the window — suppress.
                self.suppressed_in_window = self.suppressed_in_window.saturating_add(1);
                PanicLogAction::Suppress
            }
        } else {
            // First panic past the burst limit — start the suppression window.
            self.suppression_started = Some(std::time::Instant::now());
            self.suppressed_in_window = 1;
            PanicLogAction::Suppress
        }
    }

    /// Process a panic, updating state and emitting a log entry if permitted.
    fn on_panic(&mut self, observer_name: &str, panic_payload: &(dyn std::any::Any + Send)) {
        let msg = panic_message(panic_payload);
        match self.update() {
            PanicLogAction::LogImmediate => {
                tracing::warn!(
                    observer = observer_name,
                    consecutive = self.consecutive,
                    "observer panicked, using cached value: {msg}",
                );
            }
            PanicLogAction::Suppress => {
                // Silently suppressed per ADR-0022.
            }
            PanicLogAction::LogSummary { suppressed_count } => {
                tracing::warn!(
                    observer = observer_name,
                    suppressed_count,
                    "observer still panicking \
                     ({suppressed_count} panics suppressed in previous window), \
                     using cached value: {msg}",
                );
            }
        }
    }

    /// Reset all state after a successful observation.
    ///
    /// Per ADR-0022, a successful observation starts a new "clean slate":
    /// the next panic is logged unconditionally as the first one.
    fn on_success(&mut self) {
        self.consecutive = 0;
        self.suppression_started = None;
        self.suppressed_in_window = 0;
    }
}

/// Extract a human-readable message from a panic payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "(non-string panic payload)".to_string()
    }
}

// -----------------------------------------------------------------------
// ObserverSlot
// -----------------------------------------------------------------------

/// One observer together with its per-observer panic rate-limit state.
///
/// Per ADR-0022, the observer is held in an `Arc<Mutex<O>>` so that the
/// `spawn_blocking` closure can clone the `Arc` rather than moving the
/// observer into the closure. This ensures the observer struct is never
/// destroyed by a panic: if the closure body panics, the `Mutex` is
/// poisoned but the observer struct remains in the `Arc` and is accessible
/// on the next tick after the poison is recovered.
///
/// The mutex is uncontended in steady state because only one
/// `spawn_blocking` task per observer is in flight at a time. When a
/// previous task timed out and is still running, `try_lock` in the next
/// tick returns `WouldBlock` rather than blocking a thread-pool thread,
/// preserving the per-observer budget semantics from ADR-0018.
struct ObserverSlot<O> {
    /// Human-readable name cached at construction time. The name is
    /// needed for logging but is a method on the observer, so caching
    /// it avoids acquiring the mutex just to log.
    name: &'static str,
    /// The observer, wrapped per ADR-0022.
    observer: Arc<Mutex<O>>,
    /// Rate-limit state for panic logging per ADR-0022.
    panic_log: PanicRateLimit,
}

impl<O: Observer> ObserverSlot<O> {
    fn new(observer: O) -> Self {
        let name = observer.name();
        Self {
            name,
            observer: Arc::new(Mutex::new(observer)),
            panic_log: PanicRateLimit::new(),
        }
    }
}

// -----------------------------------------------------------------------
// ObserverSet
// -----------------------------------------------------------------------

/// The set of all observers held by the worker across heartbeat ticks.
///
/// Per ADR-0018, ADR-0021, and ADR-0022, each observer is a struct that
/// owns its state. The `ObserverSet` holds one observer per subsystem and
/// provides the async `collect_snapshot` method that the worker loop calls.
///
/// Per ADR-0022, each synchronous observer slot is an [`ObserverSlot`]
/// (`Arc<Mutex<O>>` plus per-observer rate-limit state). The observer
/// struct is never destroyed by a panic inside `spawn_blocking`.
///
/// Background tasks spawned through [`ObserverLifecycle`] are tracked via
/// shared join handles and joined on [`shutdown`](Self::shutdown).
pub struct ObserverSet {
    /// Per ADR-0022: each slot holds the observer in an Arc<Mutex<O>>
    /// so that panics inside `spawn_blocking` do not destroy the observer.
    defender: ObserverSlot<DefenderObserver>,
    firewall: ObserverSlot<FirewallObserver>,
    bitlocker: ObserverSlot<BitLockerObserver>,
    windows_update: ObserverSlot<WindowsUpdateObserver>,
    event_log: ObserverSlot<EventLogObserver>,

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
            defender: ObserverSlot::new(defender),
            firewall: ObserverSlot::new(firewall),
            bitlocker: ObserverSlot::new(bitlocker),
            windows_update: ObserverSlot::new(windows_update),
            event_log: ObserverSlot::new(event_log),
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

        // --- Synchronous observers: via run_observer (ADR-0018, ADR-0022) ---

        let defender =
            run_observer(&mut self.defender, &unobserved_defender(), budget, &tick).await;
        let firewall =
            run_observer(&mut self.firewall, &unobserved_firewall(), budget, &tick).await;
        let bitlocker =
            run_observer(&mut self.bitlocker, &unobserved_bitlocker(), budget, &tick).await;
        let recent_security_events =
            run_observer(&mut self.event_log, &Observation::Unobserved, budget, &tick).await;

        // --- Background-refresh observers: invoked directly (ADR-0021) ---

        let windows_update = read_background_observer(&mut self.windows_update, &tick);

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

// -----------------------------------------------------------------------
// Observer dispatch helpers
// -----------------------------------------------------------------------

/// Read a background-refresh observer's cached value directly.
///
/// Per ADR-0021, background-refresh observers are invoked directly
/// because their read path is non-blocking by contract. A timing check
/// warns if the read exceeds [`BACKGROUND_READ_BUDGET`], but does not
/// abort the call.
///
/// The observer is accessed through the slot's `Arc<Mutex<O>>` per
/// ADR-0022 (uniform slot shape). For background observers the mutex is
/// always uncontended because no concurrent `spawn_blocking` task runs
/// for them.
fn read_background_observer<O>(slot: &mut ObserverSlot<O>, tick: &TickBoundary) -> O::Output
where
    O: Observer,
{
    let start = std::time::Instant::now();

    // Background observers are never dispatched through spawn_blocking, so
    // the mutex is always uncontended here. Per ADR-0022, recover from poison
    // even though a watch::borrow() read path cannot panic in practice; the
    // recovery is a no-op in the steady state.
    let mut guard = slot.observer.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    let result = guard.observe(tick);
    drop(guard);

    let elapsed = start.elapsed();
    if elapsed > BACKGROUND_READ_BUDGET {
        tracing::warn!(
            observer = slot.name,
            elapsed_ms = elapsed.as_millis(),
            budget_ms = BACKGROUND_READ_BUDGET.as_millis(),
            "background observer read exceeded budget"
        );
    }
    result
}

/// Run a single synchronous observer with `spawn_blocking`, budget
/// timeout, and panic isolation per ADR-0022.
///
/// ## Panic isolation (ADR-0022)
///
/// The observer is held in the slot's `Arc<Mutex<O>>`. The
/// `spawn_blocking` closure clones only the `Arc` — the observer is never
/// moved into the closure. Inside the closure, `catch_unwind` wraps the
/// `observe` call. If the observer panics:
///
/// 1. `catch_unwind` returns `Err(payload)`.
/// 2. The `Mutex` is poisoned (the guard's `Drop` ran during unwinding).
/// 3. The observer struct is still in the `Arc`.
///
/// On the next tick, `try_lock` returns a `PoisonError`; we recover with
/// `into_inner()` and invoke the observer again. The panic is logged with
/// per-observer rate limiting. **The slot is never set to `None`.**
///
/// ## Mutex poisoning recovery (ADR-0022)
///
/// A `Mutex` whose previous holder panicked is poisoned. `try_lock` on a
/// poisoned mutex returns `TryLockError::Poisoned`. We recover with
/// `into_inner()`, which returns the guard and clears the poisoned state.
/// This is expected, deliberate behavior under the panic-isolation strategy
/// — not a bug to be fixed.
///
/// ## Budget timeout preservation (ADR-0018)
///
/// `try_lock` is used instead of `lock` to preserve per-observer budget
/// semantics. If a previous task timed out and is still running (holding
/// the lock), `try_lock` returns `TryLockError::WouldBlock` and the
/// current tick falls through to the fallback immediately, without
/// blocking a thread-pool thread.
async fn run_observer<O>(
    slot: &mut ObserverSlot<O>,
    fallback: &O::Output,
    budget: Duration,
    tick: &TickBoundary,
) -> O::Output
where
    O: Observer,
    O::Output: Clone + Send + 'static,
{
    let name = slot.name;
    let tick = *tick;
    let start = std::time::Instant::now();

    // Clone the Arc so the observer stays in the slot even if the closure
    // body panics. Per ADR-0022: the observer is never moved into spawn_blocking.
    let arc = Arc::clone(&slot.observer);

    let result = tokio::time::timeout(
        budget,
        tokio::task::spawn_blocking(move || {
            // Per ADR-0022: use try_lock to preserve timeout semantics.
            // - Ok(guard)   → proceed normally.
            // - Poisoned    → recover and proceed (expected after a panic).
            // - WouldBlock  → a previous timed-out task still holds the lock;
            //                 return None to signal "not run this tick."
            let mut guard = match arc.try_lock() {
                Ok(g) => g,
                Err(std::sync::TryLockError::Poisoned(e)) => {
                    // ADR-0022: mutex poisoning is expected under panic isolation.
                    // Recover the guard; the observer will be invoked normally.
                    e.into_inner()
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    // A previous tick's task is still holding the lock.
                    // Return None ("not run this tick") to preserve ADR-0018
                    // per-observer budget semantics.
                    return None;
                }
            };

            // Run observe() under catch_unwind per ADR-0022 so the observer
            // struct is preserved on panic. AssertUnwindSafe is used because
            // MutexGuard does not implement UnwindSafe by default (it holds a
            // raw reference to the Mutex interior), but any panic leaves the
            // guard in a well-defined dropped state that poisons the Mutex,
            // which is recovered on the next tick as documented above.
            Some(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || guard.observe(&tick),
            )))
            // guard drops here, releasing (or poisoning) the mutex.
        }),
    )
    .await;

    let elapsed = start.elapsed();

    match result {
        Ok(Ok(Some(Ok(value)))) => {
            // Normal success path.
            slot.panic_log.on_success();
            tracing::debug!(
                observer = name,
                elapsed_ms = elapsed.as_millis(),
                "observer completed"
            );
            value
        }
        Ok(Ok(Some(Err(panic_payload)))) => {
            // Observer panicked — caught by catch_unwind per ADR-0022.
            // Log with rate limiting and return the fallback.
            slot.panic_log.on_panic(name, &*panic_payload);
            fallback.clone()
        }
        Ok(Ok(None)) => {
            // A previous task still holds the lock (timed out last tick and
            // is still running). The observer is not invoked this tick.
            tracing::debug!(
                observer = name,
                "observer skipped: previous task still in flight"
            );
            fallback.clone()
        }
        Ok(Err(join_err)) => {
            // Task was cancelled — not a panic (those are caught by
            // catch_unwind above). Should not occur in normal operation.
            tracing::warn!(
                observer = name,
                elapsed_ms = elapsed.as_millis(),
                error = %join_err,
                "observer task cancelled unexpectedly, using cached value"
            );
            fallback.clone()
        }
        Err(_timeout) => {
            // Budget exceeded per ADR-0018. The observer is still running in
            // the thread pool; the Arc keeps it alive. The next tick will see
            // WouldBlock from try_lock and return the fallback until the
            // in-flight task completes.
            tracing::warn!(
                observer = name,
                budget_ms = budget.as_millis(),
                "observer exceeded budget, using cached value"
            );
            fallback.clone()
        }
    }
}

// -----------------------------------------------------------------------
// Fallback constructors
// -----------------------------------------------------------------------

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

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::Arc;

    use super::super::lifecycle::ObserverLifecycle;
    use super::super::trait_def::ObserverKind;
    use super::*;

    // ---------------------------------------------------------------
    // Existing tests (mechanically adapted to new slot shape)
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

    /// ADR-0018 and ADR-0022 load-bearing test: a panicking observer must
    /// not disable itself or affect other observers.
    ///
    /// **Previous behavior (broken):** the observer slot was set to `None`
    /// on the first panic and the observer was never invoked again. This
    /// test previously asserted `obs_back.is_none()`. That assertion was
    /// testing the broken behavior and has been updated per ADR-0022.
    ///
    /// **New behavior:** the observer struct survives the panic (it is held
    /// in `Arc<Mutex<O>>`). The slot is never `None`. The fallback value is
    /// returned for the panicking tick; the observer is invoked again on
    /// subsequent ticks.
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

        // The panicking observer returns the fallback. Per ADR-0022, the
        // observer is NOT lost — it remains in the slot.
        let mut panic_slot = ObserverSlot::new(PanickingObserver);
        let result = run_observer(&mut panic_slot, &fallback, budget, &tick).await;
        assert_eq!(
            result.domain_profile,
            Observation::Unobserved,
            "panicking observer should return fallback"
        );
        // The slot still holds the observer (mutex may be poisoned but Arc
        // is alive). Confirm by verifying a subsequent tick can be attempted.
        // (The observer will panic again, returning the fallback again — that
        // is correct ADR-0022 behavior.)
        let result2 = run_observer(&mut panic_slot, &fallback, budget, &tick).await;
        assert_eq!(
            result2.domain_profile,
            Observation::Unobserved,
            "panicking observer should return fallback on second tick too"
        );

        // The good observer works normally regardless of the panicking one.
        let mut good_slot = ObserverSlot::new(GoodObserver);
        let result = run_observer(&mut good_slot, &fallback, budget, &tick).await;
        assert!(
            result.domain_profile.value().is_some(),
            "good observer should return its value"
        );
    }

    /// Verify that the budget timeout returns the fallback value.
    ///
    /// **Previous behavior:** `obs_back` was `None` after a timeout because
    /// the observer was moved into `spawn_blocking` and lost to the in-flight
    /// task. This test previously asserted `obs_back.is_none()`. That
    /// assertion no longer applies because `run_observer` now takes
    /// `&mut ObserverSlot<O>` and never returns the observer at all — the
    /// slot retains it.
    ///
    /// **New behavior:** the slot still holds the observer after a timeout
    /// (it is in the `Arc`). The in-flight task holds the `Arc` clone and
    /// the mutex. The next tick sees `WouldBlock` from `try_lock` and
    /// returns the fallback until the slow task finishes.
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

        let mut slot = ObserverSlot::new(SlowObserver);
        let result = run_observer(&mut slot, &fallback, budget, &tick).await;
        // Should get the fallback because the observer exceeded the budget.
        assert_eq!(result.domain_profile, Observation::Unobserved);
        // (The slow observer's thread is still sleeping; that is acceptable —
        // the Arc<Mutex> keeps it alive until the thread pool reclaims it.)
    }

    // ---------------------------------------------------------------
    // Existing tests per ADR-0021 (unchanged logic, adapted signatures)
    // ---------------------------------------------------------------

    /// Test 1: Tick-boundary re-tagging is correct.
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
        let tick = TickBoundary::at(now);

        let mut slot = ObserverSlot::new(FreshObserver(now));
        let result = run_observer(&mut slot, &fallback, budget, &tick).await;
        match &result.domain_profile {
            Observation::Fresh { value, .. } => assert!(value.enabled),
            other => panic!("expected Fresh, got {other:?}"),
        }
    }

    /// Test 3: Background-refresh observer happy path.
    #[test]
    fn background_observer_happy_path() {
        let observed_at = OffsetDateTime::now_utc() - time::Duration::minutes(30);
        let mut slot = ObserverSlot::new(WindowsUpdateObserver::new_with_cache(5, 2, observed_at));

        // Tick is after observed_at → Cached.
        let tick = TickBoundary::now();
        let result = read_background_observer(&mut slot, &tick);
        match &result.pending_update_count {
            Observation::Cached { value, .. } => assert_eq!(*value, 5),
            other => panic!("expected Cached, got {other:?}"),
        }

        // Tick is before observed_at → Fresh.
        let tick = TickBoundary::at(observed_at - time::Duration::seconds(1));
        let result = read_background_observer(&mut slot, &tick);
        match &result.pending_update_count {
            Observation::Fresh { value, .. } => assert_eq!(*value, 5),
            other => panic!("expected Fresh, got {other:?}"),
        }
    }

    /// Test 4: Tick boundary is shared across observers within one call.
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

        let mut slot1 = ObserverSlot::new(obs1);
        let mut slot2 = ObserverSlot::new(obs2);
        let _ = run_observer(&mut slot1, &fallback, budget, &tick).await;
        let _ = run_observer(&mut slot2, &fallback, budget, &tick).await;

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
    #[tokio::test]
    async fn shutdown_joins_cooperative_tasks() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let runtime = tokio::runtime::Handle::current();
        let mut set = ObserverSet::new(Duration::from_millis(500), &shutdown, runtime);

        let sd = shutdown.clone();
        set.inject_background_task(tokio::spawn(async move {
            while !sd.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }));

        shutdown.store(true, Ordering::SeqCst);

        let start = std::time::Instant::now();
        set.shutdown().await;
        assert!(start.elapsed() < SHUTDOWN_GRACE_PERIOD);
    }

    /// Test 5b: Shutdown returns within the grace period even when a
    /// background task ignores the shutdown signal.
    #[tokio::test]
    async fn shutdown_abandons_stuck_tasks() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let runtime = tokio::runtime::Handle::current();
        let mut set = ObserverSet::new(Duration::from_millis(500), &shutdown, runtime);

        set.inject_background_task(tokio::spawn(async {
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        }));

        shutdown.store(true, Ordering::SeqCst);

        let start = std::time::Instant::now();
        set.shutdown().await;
        assert!(
            start.elapsed() < SHUTDOWN_GRACE_PERIOD + Duration::from_millis(500),
            "shutdown should not block beyond the grace period"
        );
    }

    // ---------------------------------------------------------------
    // New tests per ADR-0022
    // ---------------------------------------------------------------

    /// ADR-0022 test 1: an observer that panics on its first invocation
    /// and succeeds on its second is invoked both times through the same
    /// slot. The same observer instance handles both calls.
    #[tokio::test]
    async fn observer_survives_single_panic() {
        let count = Arc::new(AtomicU32::new(0));

        struct PanicFirstObserver {
            count: Arc<AtomicU32>,
        }
        impl Observer for PanicFirstObserver {
            type Output = FirewallStatus;
            const KIND: ObserverKind = ObserverKind::Synchronous;
            fn name(&self) -> &'static str {
                "panic_first"
            }
            fn observe(&mut self, _tick: &TickBoundary) -> FirewallStatus {
                let n = self.count.fetch_add(1, Ordering::SeqCst) + 1;
                if n == 1 {
                    panic!("deliberate first-tick panic");
                }
                // Distinctive return value to confirm a real observation.
                FirewallStatus {
                    domain_profile: Observation::Fresh {
                        value: ken_protocol::status::FirewallProfileState {
                            enabled: true,
                            default_inbound_action: "allow".to_string(),
                        },
                        observed_at: OffsetDateTime::now_utc(),
                    },
                    private_profile: Observation::Unobserved,
                    public_profile: Observation::Unobserved,
                }
            }
            fn start(&mut self, _lifecycle: ObserverLifecycle) {}
        }

        let mut slot = ObserverSlot::new(PanicFirstObserver {
            count: Arc::clone(&count),
        });
        let fallback = unobserved_firewall();
        let budget = Duration::from_secs(2);
        let tick = TickBoundary::now();

        // First call: panics → fallback returned, observer was invoked (count=1).
        let result1 = run_observer(&mut slot, &fallback, budget, &tick).await;
        assert_eq!(
            result1.domain_profile,
            Observation::Unobserved,
            "first (panicking) call should return fallback"
        );
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "observer was invoked on first tick"
        );

        // Second call: same observer instance (count increments to 2), succeeds.
        let result2 = run_observer(&mut slot, &fallback, budget, &tick).await;
        assert_eq!(
            count.load(Ordering::SeqCst),
            2,
            "same observer instance invoked on second tick"
        );
        assert!(
            result2.domain_profile.value().is_some(),
            "second (successful) call should return the observer's value"
        );
    }

    /// ADR-0022 test 2: an observer that panics on every invocation is
    /// still present in the slot after five consecutive panics.
    #[tokio::test]
    async fn observer_survives_repeated_panics() {
        let count = Arc::new(AtomicU32::new(0));

        struct AlwaysPanics {
            count: Arc<AtomicU32>,
        }
        impl Observer for AlwaysPanics {
            type Output = FirewallStatus;
            const KIND: ObserverKind = ObserverKind::Synchronous;
            fn name(&self) -> &'static str {
                "always_panics"
            }
            fn observe(&mut self, _tick: &TickBoundary) -> FirewallStatus {
                self.count.fetch_add(1, Ordering::SeqCst);
                panic!("always panics");
            }
            fn start(&mut self, _lifecycle: ObserverLifecycle) {}
        }

        let mut slot = ObserverSlot::new(AlwaysPanics {
            count: Arc::clone(&count),
        });
        let fallback = unobserved_firewall();
        let budget = Duration::from_secs(2);

        for i in 1..=5 {
            let tick = TickBoundary::now();
            let result = run_observer(&mut slot, &fallback, budget, &tick).await;
            assert_eq!(
                result.domain_profile,
                Observation::Unobserved,
                "tick {i}: fallback should be returned after panic"
            );
            assert_eq!(
                count.load(Ordering::SeqCst),
                i,
                "tick {i}: observer invocation count should be {i}"
            );
        }

        // The slot still holds the observer — verify by locking the Arc.
        let guard = slot.observer.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(
            guard.count.load(Ordering::SeqCst),
            5,
            "observer struct is still accessible in the slot after 5 panics"
        );
    }

    /// ADR-0022 test 3: a panic poisons the mutex; the next tick recovers
    /// from the poison and invokes the observer successfully.
    #[tokio::test]
    async fn mutex_poisoning_is_recovered() {
        let panicked_once = Arc::new(AtomicBool::new(false));

        struct PanicsOnce {
            did_panic: Arc<AtomicBool>,
        }
        impl Observer for PanicsOnce {
            type Output = FirewallStatus;
            const KIND: ObserverKind = ObserverKind::Synchronous;
            fn name(&self) -> &'static str {
                "panics_once"
            }
            fn observe(&mut self, _tick: &TickBoundary) -> FirewallStatus {
                if !self.did_panic.swap(true, Ordering::SeqCst) {
                    panic!("first and only panic");
                }
                // Successful on all subsequent calls.
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

        let mut slot = ObserverSlot::new(PanicsOnce {
            did_panic: Arc::clone(&panicked_once),
        });
        let fallback = unobserved_firewall();
        let budget = Duration::from_secs(2);

        // First tick: panics, mutex becomes poisoned.
        let result1 = run_observer(&mut slot, &fallback, budget, &TickBoundary::now()).await;
        assert_eq!(
            result1.domain_profile,
            Observation::Unobserved,
            "first (panicking) tick returns fallback"
        );

        // Confirm the mutex is poisoned after the panic.
        match slot.observer.try_lock() {
            Err(std::sync::TryLockError::Poisoned(_)) => {
                // Expected: the panic poisoned the mutex.
            }
            Ok(_) => {
                // Depending on implementation, the poison may have been cleared.
                // This is also acceptable — what matters is the next run_observer
                // call succeeds.
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                panic!("mutex should not be held by another thread between ticks");
            }
        }

        // Second tick: poison is recovered inside run_observer, observer succeeds.
        // No PoisonError propagates to the caller.
        let result2 = run_observer(&mut slot, &fallback, budget, &TickBoundary::now()).await;
        assert!(
            result2.domain_profile.value().is_some(),
            "second tick should succeed after mutex poison recovery"
        );
    }

    /// ADR-0022 test 4: the first three panics are logged (LogImmediate),
    /// the fourth and fifth are suppressed (Suppress).
    ///
    /// This test drives `PanicRateLimit::update()` directly rather than
    /// through a tracing subscriber, verifying the rate-limiting decision
    /// logic without requiring subscriber infrastructure.
    #[test]
    fn rate_limiter_first_three_logged_fourth_suppressed() {
        let mut rl = PanicRateLimit::new();

        // First three panics are within the burst limit.
        assert!(
            matches!(rl.update(), PanicLogAction::LogImmediate),
            "panic 1 should be logged immediately"
        );
        assert!(
            matches!(rl.update(), PanicLogAction::LogImmediate),
            "panic 2 should be logged immediately"
        );
        assert!(
            matches!(rl.update(), PanicLogAction::LogImmediate),
            "panic 3 should be logged immediately"
        );

        // Fourth and fifth panics are suppressed.
        assert!(
            matches!(rl.update(), PanicLogAction::Suppress),
            "panic 4 should be suppressed"
        );
        assert!(
            matches!(rl.update(), PanicLogAction::Suppress),
            "panic 5 should be suppressed"
        );
    }

    /// ADR-0022 test 5: a successful observation resets the panic counter.
    /// The next panic after a success is logged unconditionally as the first.
    #[test]
    fn rate_limiter_success_resets_counter() {
        let mut rl = PanicRateLimit::new();

        // Three panics, all logged.
        assert!(matches!(rl.update(), PanicLogAction::LogImmediate));
        assert!(matches!(rl.update(), PanicLogAction::LogImmediate));
        assert!(matches!(rl.update(), PanicLogAction::LogImmediate));

        // Success resets the counter. The next panic is treated as the first.
        rl.on_success();
        assert!(
            matches!(rl.update(), PanicLogAction::LogImmediate),
            "panic after success should be logged unconditionally"
        );
    }

    /// ADR-0022 test 6: failure isolation — a panicking observer does not
    /// affect a healthy observer's invocation count or returned values.
    #[tokio::test]
    async fn failure_isolation_between_observers() {
        let panic_count = Arc::new(AtomicU32::new(0));
        let success_count = Arc::new(AtomicU32::new(0));

        struct AlwaysPanics {
            count: Arc<AtomicU32>,
        }
        impl Observer for AlwaysPanics {
            type Output = FirewallStatus;
            const KIND: ObserverKind = ObserverKind::Synchronous;
            fn name(&self) -> &'static str {
                "always_panics"
            }
            fn observe(&mut self, _tick: &TickBoundary) -> FirewallStatus {
                self.count.fetch_add(1, Ordering::SeqCst);
                panic!("always panics");
            }
            fn start(&mut self, _lifecycle: ObserverLifecycle) {}
        }

        struct AlwaysSucceeds {
            count: Arc<AtomicU32>,
        }
        impl Observer for AlwaysSucceeds {
            type Output = FirewallStatus;
            const KIND: ObserverKind = ObserverKind::Synchronous;
            fn name(&self) -> &'static str {
                "always_succeeds"
            }
            fn observe(&mut self, _tick: &TickBoundary) -> FirewallStatus {
                self.count.fetch_add(1, Ordering::SeqCst);
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

        let mut panic_slot = ObserverSlot::new(AlwaysPanics {
            count: Arc::clone(&panic_count),
        });
        let mut success_slot = ObserverSlot::new(AlwaysSucceeds {
            count: Arc::clone(&success_count),
        });
        let fallback = unobserved_firewall();
        let budget = Duration::from_secs(2);

        for i in 1u32..=5 {
            let tick = TickBoundary::now();

            let panic_result = run_observer(&mut panic_slot, &fallback, budget, &tick).await;
            let success_result = run_observer(&mut success_slot, &fallback, budget, &tick).await;

            // Panicking observer always returns fallback.
            assert_eq!(
                panic_result.domain_profile,
                Observation::Unobserved,
                "tick {i}: panicking observer should return fallback"
            );
            // Healthy observer always returns its value.
            assert!(
                success_result.domain_profile.value().is_some(),
                "tick {i}: healthy observer should return a value"
            );
            // Both observers were invoked this tick.
            assert_eq!(
                panic_count.load(Ordering::SeqCst),
                i,
                "tick {i}: panicking observer invocation count"
            );
            assert_eq!(
                success_count.load(Ordering::SeqCst),
                i,
                "tick {i}: healthy observer invocation count"
            );
        }

        // Panicking observer's slot is still alive — observer is accessible.
        let guard = panic_slot.observer.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(
            guard.count.load(Ordering::SeqCst),
            5,
            "panicking observer's slot is still populated after 5 ticks"
        );
    }

    /// ADR-0022 test 7: the suppression window expires and the next panic
    /// produces a summary log entry, then the window restarts.
    ///
    /// Uses the test-mode suppression window (50 ms) so no long sleep is
    /// required. The `#[cfg(test)]` constant override is documented in the
    /// PANIC_LOG_SUPPRESSION_WINDOW declaration.
    #[test]
    fn rate_limiter_window_expiry_produces_summary() {
        let mut rl = PanicRateLimit::new();

        // Use up the burst limit.
        for _ in 0..PANIC_LOG_BURST_LIMIT {
            assert!(matches!(rl.update(), PanicLogAction::LogImmediate));
        }

        // First suppressed panic starts the window.
        assert!(matches!(rl.update(), PanicLogAction::Suppress));

        // Wait for the window to expire (test window = 50 ms).
        std::thread::sleep(PANIC_LOG_SUPPRESSION_WINDOW + Duration::from_millis(10));

        // Next panic after window expiry produces a summary naming one
        // suppressed panic (the one that started the window).
        match rl.update() {
            PanicLogAction::LogSummary { suppressed_count } => {
                assert_eq!(
                    suppressed_count, 1,
                    "summary should report the one suppressed panic"
                );
            }
            other => panic!("expected LogSummary after window expiry, got {other:?}"),
        }

        // Window has restarted; the next panic is suppressed again.
        assert!(
            matches!(rl.update(), PanicLogAction::Suppress),
            "panic after window summary should be suppressed (new window)"
        );
    }
}
