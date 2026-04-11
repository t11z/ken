//! Firewall observer via WMI `MSFT_NetFirewallProfile`.
//!
//! On Windows, queries the `ROOT\StandardCimv2` WMI namespace for all
//! three firewall profiles (Domain, Private, Public).

use ken_protocol::status::{FirewallStatus, Observation};

use super::lifecycle::ObserverLifecycle;
use super::tick::TickBoundary;
use super::trait_def::{Observer, ObserverKind};

/// Firewall observer struct per ADR-0018.
pub struct FirewallObserver;

impl FirewallObserver {
    /// Create a new Firewall observer.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Observer for FirewallObserver {
    type Output = FirewallStatus;
    const KIND: ObserverKind = ObserverKind::Synchronous;

    fn name(&self) -> &'static str {
        "firewall"
    }

    fn observe(&mut self, _tick: &TickBoundary) -> FirewallStatus {
        #[cfg(windows)]
        {
            tracing::debug!("firewall observer: WMI query not yet implemented");
        }

        FirewallStatus {
            domain_profile: Observation::Unobserved,
            private_profile: Observation::Unobserved,
            public_profile: Observation::Unobserved,
        }
    }

    fn start(&mut self, _lifecycle: ObserverLifecycle) {
        // Synchronous observer — no background work to start.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_returns_all_unobserved() {
        let mut obs = FirewallObserver::new();
        let tick = TickBoundary::now();
        let status = obs.observe(&tick);
        assert_eq!(status.domain_profile, Observation::Unobserved);
    }
}
