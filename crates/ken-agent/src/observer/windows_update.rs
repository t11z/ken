//! Windows Update observer.
//!
//! Phase 1 stub: returns all fields as `Unobserved`. Full WUA COM API
//! integration is tracked in issue #4 and specified by ADR-0020.

use ken_protocol::status::{Observation, WindowsUpdateStatus};

use super::trait_def::Observer;

/// Windows Update observer struct per ADR-0018.
///
/// Currently a stub. ADR-0020 specifies the background WUA task that
/// will populate `pending_update_count` and `pending_critical_update_count`.
pub struct WindowsUpdateObserver;

impl WindowsUpdateObserver {
    /// Create a new Windows Update observer.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Observer for WindowsUpdateObserver {
    type Output = WindowsUpdateStatus;

    fn name(&self) -> &'static str {
        "windows_update"
    }

    fn observe(&mut self) -> WindowsUpdateStatus {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_returns_all_unobserved() {
        let mut obs = WindowsUpdateObserver::new();
        let status = obs.observe();
        assert_eq!(status.pending_update_count, Observation::Unobserved);
        assert_eq!(
            status.pending_critical_update_count,
            Observation::Unobserved
        );
    }
}
