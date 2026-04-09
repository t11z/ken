//! `BitLocker` observer via WMI `Win32_EncryptableVolume`.
//!
//! On Windows, queries the `ROOT\CIMV2\Security\MicrosoftVolumeEncryption`
//! namespace. Requires SYSTEM privileges (the agent has them).

use ken_protocol::status::BitLockerStatus;

/// Collect `BitLocker` status.
pub fn collect() -> Option<BitLockerStatus> {
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
fn collect_windows() -> Result<Option<BitLockerStatus>, anyhow::Error> {
    tracing::debug!("bitlocker observer: WMI query not yet implemented");
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
