//! `BitLocker` observer via WMI `Win32_EncryptableVolume`.
//!
//! On Windows, queries the `ROOT\CIMV2\Security\MicrosoftVolumeEncryption`
//! namespace. Requires SYSTEM privileges (the agent has them).

use ken_protocol::status::{BitLockerStatus, Observation};

/// Collect `BitLocker` status.
///
/// Returns a `BitLockerStatus` with volumes `Unobserved` until the
/// WMI query is implemented.
pub fn collect() -> BitLockerStatus {
    #[cfg(windows)]
    {
        tracing::debug!("bitlocker observer: WMI query not yet implemented");
    }

    BitLockerStatus {
        volumes: Observation::Unobserved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_returns_all_unobserved() {
        let status = collect();
        assert_eq!(status.volumes, Observation::Unobserved);
    }
}
