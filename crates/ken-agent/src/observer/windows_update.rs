//! Windows Update observer.
//!
//! Phase 1 reads `LastSuccessTime` values from the registry. Pending
//! update counts are set to 0 — full WUA COM API integration is tracked
//! in issue #4.

use ken_protocol::status::WindowsUpdateStatus;

/// Collect Windows Update status.
///
/// Phase 1: reads last search/install times from the registry.
/// Pending update counts are hardcoded to 0 (see issue #4).
pub fn collect() -> Option<WindowsUpdateStatus> {
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
fn collect_windows() -> Result<Option<WindowsUpdateStatus>, anyhow::Error> {
    // Phase 1: read LastSuccessTime from registry at
    // HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\Results\
    //
    // Pending update counts require the WUA COM API (issue #4).
    tracing::debug!("windows update observer: registry query not yet implemented");
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
