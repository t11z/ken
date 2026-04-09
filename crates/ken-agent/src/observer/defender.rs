//! Defender observer via WMI `MSFT_MpComputerStatus`.
//!
//! On Windows, queries the `ROOT\Microsoft\Windows\Defender` WMI namespace.
//! On other platforms, returns `None`.

use ken_protocol::status::DefenderStatus;

/// Collect Defender status from WMI.
///
/// On WMI connection failure or missing class, returns `None` —
/// the snapshot still succeeds with Defender status absent.
pub fn collect() -> Option<DefenderStatus> {
    #[cfg(windows)]
    {
        // WMI query via the `wmi` crate would go here.
        // For Phase 1, this is a placeholder that returns None.
        tracing::debug!("defender observer: WMI query not yet implemented");
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
