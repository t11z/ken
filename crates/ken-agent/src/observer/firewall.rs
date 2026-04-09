//! Firewall observer via WMI `MSFT_NetFirewallProfile`.
//!
//! On Windows, queries the `ROOT\StandardCimv2` WMI namespace for all
//! three firewall profiles (Domain, Private, Public).

use ken_protocol::status::FirewallStatus;

/// Collect firewall status.
pub fn collect() -> Option<FirewallStatus> {
    #[cfg(windows)]
    {
        collect_windows().ok().flatten()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
fn collect_windows() -> Result<Option<FirewallStatus>, anyhow::Error> {
    tracing::debug!("firewall observer: WMI query not yet implemented");
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_returns_none_on_non_windows() {
        assert!(collect().is_none());
    }
}
