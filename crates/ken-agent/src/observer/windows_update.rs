//! Windows Update observer.
//!
//! Phase 1 stub: returns all fields as `Unobserved`. Full WUA COM API
//! integration is tracked in issue #4 and specified by ADR-0020.

use ken_protocol::status::{Observation, WindowsUpdateStatus};

/// Collect Windows Update status.
///
/// Returns a `WindowsUpdateStatus` with all fields `Unobserved` until
/// the WUA COM background task is implemented (ADR-0020).
pub fn collect() -> WindowsUpdateStatus {
    #[cfg(windows)]
    {
        tracing::debug!("windows update observer: not yet implemented (see issue #4)");
    }

    WindowsUpdateStatus {
        last_search_time: Observation::Unobserved,
        last_install_time: Observation::Unobserved,
        pending_update_count: Observation::Unobserved,
        pending_critical_update_count: Observation::Unobserved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_returns_all_unobserved() {
        let status = collect();
        assert_eq!(status.pending_update_count, Observation::Unobserved);
        assert_eq!(status.pending_critical_update_count, Observation::Unobserved);
    }
}
