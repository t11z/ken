//! Event Log observer with persistent bookmarks.
//!
//! On Windows, reads from three logs using the `EvtQuery` family of APIs:
//! - `Microsoft-Windows-Windows Defender/Operational`
//! - `Security` (failed logons)
//! - `Application` (crashes)
//!
//! Bookmarks are persisted to `%ProgramData%\Ken\state\bookmarks\` so
//! the service can resume after a restart.

use ken_protocol::status::{Observation, SecurityEvent};

use super::lifecycle::ObserverLifecycle;
use super::tick::TickBoundary;
use super::trait_def::{Observer, ObserverKind};

/// Event Log observer struct per ADR-0018.
pub struct EventLogObserver;

impl EventLogObserver {
    /// Create a new Event Log observer.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Observer for EventLogObserver {
    type Output = Observation<Vec<SecurityEvent>>;
    const KIND: ObserverKind = ObserverKind::Synchronous;

    fn name(&self) -> &'static str {
        "event_log"
    }

    fn observe(&mut self, _tick: &TickBoundary) -> Observation<Vec<SecurityEvent>> {
        #[cfg(windows)]
        {
            tracing::debug!("event log observer: EvtQuery not yet implemented");
        }

        Observation::Unobserved
    }

    fn start(&mut self, _lifecycle: ObserverLifecycle) {
        // Synchronous observer — no background work to start.
    }
}

/// Map a known event ID to a terse human-readable summary.
///
/// Known patterns:
/// - Defender 1006: "Malware detected"
/// - Defender 1008: "Malware quarantined"
/// - Defender 1116: "Malware action taken"
/// - Security 4625: "Failed logon attempt"
/// - Application 1000: "Application crash"
/// - Application 1001: "Application hang"
/// - Fallback: "Event ID {id} from {source}"
#[must_use]
pub fn event_id_to_summary(log_name: &str, event_id: u32, source: &str) -> String {
    match (log_name, event_id) {
        ("Microsoft-Windows-Windows Defender/Operational", 1006) => "Malware detected".to_string(),
        ("Microsoft-Windows-Windows Defender/Operational", 1008) => {
            "Malware quarantined".to_string()
        }
        ("Microsoft-Windows-Windows Defender/Operational", 1116) => {
            "Malware action taken".to_string()
        }
        ("Security", 4625) => "Failed logon attempt".to_string(),
        ("Application", 1000) => "Application crash".to_string(),
        ("Application", 1001) => "Application hang".to_string(),
        _ => format!("Event ID {event_id} from {source}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_returns_unobserved() {
        let mut obs = EventLogObserver::new();
        let tick = TickBoundary::now();
        assert_eq!(obs.observe(&tick), Observation::Unobserved);
    }

    #[test]
    fn known_defender_events() {
        let log = "Microsoft-Windows-Windows Defender/Operational";
        assert_eq!(
            event_id_to_summary(log, 1006, "Defender"),
            "Malware detected"
        );
        assert_eq!(
            event_id_to_summary(log, 1008, "Defender"),
            "Malware quarantined"
        );
        assert_eq!(
            event_id_to_summary(log, 1116, "Defender"),
            "Malware action taken"
        );
    }

    #[test]
    fn known_security_events() {
        assert_eq!(
            event_id_to_summary("Security", 4625, "Security"),
            "Failed logon attempt"
        );
    }

    #[test]
    fn known_application_events() {
        assert_eq!(
            event_id_to_summary("Application", 1000, "Application Error"),
            "Application crash"
        );
        assert_eq!(
            event_id_to_summary("Application", 1001, "Application Error"),
            "Application hang"
        );
    }

    #[test]
    fn fallback_for_unknown_event() {
        assert_eq!(
            event_id_to_summary("System", 9999, "SomeSource"),
            "Event ID 9999 from SomeSource"
        );
    }
}
