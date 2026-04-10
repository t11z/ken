//! `BitLocker` observer via WMI `Win32_EncryptableVolume`.
//!
//! On Windows, queries the `ROOT\CIMV2\Security\MicrosoftVolumeEncryption`
//! namespace. Requires SYSTEM privileges (the agent has them).

use ken_protocol::status::BitLockerStatus;

/// Collect `BitLocker` status.
pub fn collect() -> Option<BitLockerStatus> {
    #[cfg(windows)]
    {
        tracing::debug!("bitlocker observer: WMI query not yet implemented");
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
