//! Defender observer via WMI `MSFT_MpComputerStatus`.
//!
//! On Windows, queries the `ROOT\Microsoft\Windows\Defender` WMI namespace.
//! On other platforms, returns `None`.

use ken_protocol::status::DefenderStatus;

/// Collect Defender status from WMI.
///
/// On WMI connection failure or missing class, returns `Ok(None)` —
/// the snapshot still succeeds with Defender status absent.
pub fn collect() -> Option<DefenderStatus> {
    #[cfg(windows)]
    {
        collect_windows().ok().flatten()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Windows implementation using WMI.
#[cfg(windows)]
fn collect_windows() -> Result<Option<DefenderStatus>, anyhow::Error> {
    // WMI query via windows-rs COM interfaces would go here.
    // The wmi crate (not yet added as a dependency) provides a
    // higher-level API:
    //
    //   let wmi = WMIConnection::with_namespace("ROOT\\Microsoft\\Windows\\Defender")?;
    //   let results: Vec<MpComputerStatus> = wmi.query()?;
    //
    // For Phase 1, this is a placeholder that returns None.
    // The actual WMI integration requires adding the `wmi` crate
    // and implementing the field mapping.
    tracing::debug!("defender observer: WMI query not yet implemented");
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_returns_none_on_non_windows() {
        // On non-Windows, always returns None
        assert!(collect().is_none());
    }
}
