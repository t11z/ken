//! Orchestration that assembles a full OS status snapshot from
//! individual observers per ADR-0018.
//!
//! Each observer is invoked via `tokio::task::spawn_blocking` with a
//! per-observer time budget. Panics are caught at the `spawn_blocking`
//! boundary and result in the observer's contribution being its last
//! cached value (or the default if none). Other observers are unaffected.

use std::time::Duration;

use ken_protocol::status::{
    BitLockerStatus, DefenderStatus, FirewallStatus, Observation, OsStatusSnapshot, SecurityEvent,
    WindowsUpdateStatus,
};
use time::OffsetDateTime;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::trait_def::Observer;
use super::{
    BitLockerObserver, DefenderObserver, EventLogObserver, FirewallObserver, WindowsUpdateObserver,
};

/// The set of all observers held by the worker across heartbeat ticks.
///
/// Per ADR-0018, each observer is a struct that owns its state. The
/// `ObserverSet` holds one observer per subsystem and provides the
/// async `collect_snapshot` method that the worker loop calls.
pub struct ObserverSet {
    defender: Option<DefenderObserver>,
    firewall: Option<FirewallObserver>,
    bitlocker: Option<BitLockerObserver>,
    windows_update: Option<WindowsUpdateObserver>,
    event_log: Option<EventLogObserver>,

    // Last known values for failure fallback (ADR-0018).
    last_defender: DefenderStatus,
    last_firewall: FirewallStatus,
    last_bitlocker: BitLockerStatus,
    last_windows_update: WindowsUpdateStatus,
    last_event_log: Observation<Vec<SecurityEvent>>,

    budget: Duration,
}

impl ObserverSet {
    /// Create a new observer set with default observers and the given
    /// per-observer time budget. The `shutdown` signal is passed to the
    /// Windows Update observer's background task (ADR-0020).
    #[must_use]
    pub fn new(budget: Duration, shutdown: Arc<AtomicBool>) -> Self {
        Self {
            defender: Some(DefenderObserver::new()),
            firewall: Some(FirewallObserver::new()),
            bitlocker: Some(BitLockerObserver::new()),
            windows_update: Some(WindowsUpdateObserver::new(shutdown)),
            event_log: Some(EventLogObserver::new()),
            last_defender: unobserved_defender(),
            last_firewall: unobserved_firewall(),
            last_bitlocker: unobserved_bitlocker(),
            last_windows_update: unobserved_windows_update(),
            last_event_log: Observation::Unobserved,
            budget,
        }
    }

    /// Collect a complete OS status snapshot by invoking all observers.
    ///
    /// Per ADR-0018, each observer runs in `spawn_blocking` with a
    /// per-observer time budget. On timeout, the last cached value is
    /// used. On panic, the observer's contribution is its last cached
    /// value and other observers are unaffected.
    pub async fn collect_snapshot(&mut self) -> OsStatusSnapshot {
        let collected_at = OffsetDateTime::now_utc();
        let budget = self.budget;

        let (obs, defender) = run_observer(self.defender.take(), &self.last_defender, budget).await;
        self.defender = obs;
        self.last_defender = defender.clone();

        let (obs, firewall) = run_observer(self.firewall.take(), &self.last_firewall, budget).await;
        self.firewall = obs;
        self.last_firewall = firewall.clone();

        let (obs, bitlocker) =
            run_observer(self.bitlocker.take(), &self.last_bitlocker, budget).await;
        self.bitlocker = obs;
        self.last_bitlocker = bitlocker.clone();

        let (obs, windows_update) = run_observer(
            self.windows_update.take(),
            &self.last_windows_update,
            budget,
        )
        .await;
        self.windows_update = obs;
        self.last_windows_update = windows_update.clone();

        let (obs, recent_security_events) =
            run_observer(self.event_log.take(), &self.last_event_log, budget).await;
        self.event_log = obs;
        self.last_event_log = recent_security_events.clone();

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
}

/// Run a single observer with `spawn_blocking`, budget timeout, and panic isolation.
///
/// Per ADR-0018:
/// - The observer body runs in `spawn_blocking` (sync body, async boundary).
/// - A `tokio::time::timeout` enforces the per-observer budget.
/// - A panic in the observer is caught via the `JoinError` from `spawn_blocking`.
/// - On timeout or panic, the last cached value is returned.
///
/// Returns `(Option<observer>, output)`. The observer is `None` if it
/// panicked (the struct was consumed by the panic) or if the input was
/// already `None`.
async fn run_observer<O>(
    observer: Option<O>,
    fallback: &O::Output,
    budget: Duration,
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

    // Move the observer into spawn_blocking and get it back with the result.
    let result = tokio::time::timeout(
        budget,
        tokio::task::spawn_blocking(move || {
            let output = obs.observe();
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
    use super::*;

    #[tokio::test]
    async fn snapshot_collects_without_panic() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut set = ObserverSet::new(Duration::from_millis(500), shutdown);
        let snapshot = set.collect_snapshot().await;
        // On non-Windows, all observers return Unobserved
        assert_eq!(snapshot.recent_security_events, Observation::Unobserved);
        assert_eq!(snapshot.defender.antivirus_enabled, Observation::Unobserved);
    }

    /// ADR-0018 load-bearing test: a panicking observer must not affect
    /// other observers or the worker loop. The panicking observer's
    /// contribution to the snapshot is its last cached value (Unobserved
    /// when there is no prior value).
    #[tokio::test]
    async fn panicking_observer_is_isolated() {
        struct PanickingObserver;
        impl Observer for PanickingObserver {
            type Output = FirewallStatus;
            fn name(&self) -> &'static str {
                "panicking"
            }
            fn observe(&mut self) -> FirewallStatus {
                panic!("deliberate test panic in observer");
            }
        }

        struct GoodObserver;
        impl Observer for GoodObserver {
            type Output = FirewallStatus;
            fn name(&self) -> &'static str {
                "good"
            }
            fn observe(&mut self) -> FirewallStatus {
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
        }

        let fallback = unobserved_firewall();
        let budget = Duration::from_secs(2);

        // The panicking observer should return the fallback and lose
        // its observer struct (None).
        let (obs_back, result) = run_observer(Some(PanickingObserver), &fallback, budget).await;
        assert!(obs_back.is_none(), "panicked observer should be lost");
        assert_eq!(result.domain_profile, Observation::Unobserved);

        // The good observer should work normally despite the previous panic.
        let (obs_back, result) = run_observer(Some(GoodObserver), &fallback, budget).await;
        assert!(obs_back.is_some(), "good observer should be returned");
        assert!(result.domain_profile.value().is_some());
    }

    /// Verify that the budget timeout produces the fallback value.
    #[tokio::test]
    async fn budget_timeout_returns_fallback() {
        struct SlowObserver;
        impl Observer for SlowObserver {
            type Output = FirewallStatus;
            fn name(&self) -> &'static str {
                "slow"
            }
            fn observe(&mut self) -> FirewallStatus {
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
        }

        let fallback = unobserved_firewall();
        // Very short budget to trigger timeout.
        let budget = Duration::from_millis(10);

        let (obs_back, result) = run_observer(Some(SlowObserver), &fallback, budget).await;
        // Observer is lost because it's still running.
        assert!(obs_back.is_none());
        // Should get the fallback because the observer exceeded the budget.
        assert_eq!(result.domain_profile, Observation::Unobserved);
    }
}
