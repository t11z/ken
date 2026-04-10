//! Defender observer via WMI `MSFT_MpComputerStatus`.
//!
//! On Windows, queries the `ROOT\Microsoft\Windows\Defender` WMI namespace.
//! On other platforms, returns all fields as `Unobserved`.

use ken_protocol::status::{DefenderStatus, Observation};

use super::trait_def::Observer;

/// Defender observer struct per ADR-0018.
///
/// Currently a stub that returns all fields as `Unobserved`. When the
/// WMI query is implemented, this struct will hold its last known values
/// and decide per-tick whether to refresh.
pub struct DefenderObserver;

impl DefenderObserver {
    /// Create a new Defender observer.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Observer for DefenderObserver {
    type Output = DefenderStatus;

    fn name(&self) -> &'static str {
        "defender"
    }

    fn observe(&mut self) -> DefenderStatus {
        #[cfg(windows)]
        {
            tracing::debug!("defender observer: WMI query not yet implemented");
        }

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_returns_all_unobserved() {
        let mut obs = DefenderObserver::new();
        let status = obs.observe();
        assert_eq!(status.antivirus_enabled, Observation::Unobserved);
        assert_eq!(status.signature_version, Observation::Unobserved);
    }
}
