//! Firewall observer via WMI `MSFT_NetFirewallProfile`.
//!
//! On Windows, queries the `ROOT\StandardCimv2` WMI namespace for all
//! three firewall profiles (Domain, Private, Public).

use ken_protocol::status::{FirewallStatus, Observation};

/// Collect firewall status.
///
/// Returns a `FirewallStatus` with all profiles `Unobserved` until the
/// WMI query is implemented.
pub fn collect() -> FirewallStatus {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_returns_all_unobserved() {
        let status = collect();
        assert_eq!(status.domain_profile, Observation::Unobserved);
    }
}
