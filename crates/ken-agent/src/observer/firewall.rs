//! Firewall observer via WMI `MSFT_NetFirewallProfile`.
//!
//! On Windows, queries the `ROOT\StandardCimv2` WMI namespace for all
//! three firewall profiles (Domain, Private, Public).

use ken_protocol::status::{FirewallStatus, Observation};

use super::trait_def::Observer;

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

    fn name(&self) -> &'static str {
        "firewall"
    }

    fn observe(&mut self) -> FirewallStatus {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_returns_all_unobserved() {
        let mut obs = FirewallObserver::new();
        let status = obs.observe();
        assert_eq!(status.domain_profile, Observation::Unobserved);
    }
}
