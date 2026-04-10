//! Defender observer via WMI `MSFT_MpComputerStatus`.
//!
//! On Windows, queries the `ROOT\Microsoft\Windows\Defender` WMI namespace.
//! On other platforms, returns all fields as `Unobserved`.

use ken_protocol::status::{DefenderStatus, Observation};

/// Collect Defender status from WMI.
///
/// Returns a `DefenderStatus` with all fields `Unobserved` until the
/// WMI query is implemented. Per ADR-0019, subsystem-level `Option`
/// wrappers are removed; individual fields carry observation state.
pub fn collect() -> DefenderStatus {
    #[cfg(windows)]
    {
        // WMI query via the `wmi` crate would go here.
        // For Phase 1, this is a placeholder that returns Unobserved.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_returns_all_unobserved() {
        let status = collect();
        assert_eq!(status.antivirus_enabled, Observation::Unobserved);
        assert_eq!(status.signature_version, Observation::Unobserved);
    }
}
