//! Firewall observer via WMI `MSFT_NetFirewallProfile`.
//!
//! On Windows, queries the `ROOT\StandardCimv2` WMI namespace for all
//! three firewall profiles (Domain, Private, Public).

use ken_protocol::status::FirewallStatus;

/// Collect firewall status.
pub fn collect() -> Option<FirewallStatus> {
    #[cfg(windows)]
    {
        tracing::debug!("firewall observer: WMI query not yet implemented");
        None
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_returns_none() {
        assert!(collect().is_none());
    }
}
