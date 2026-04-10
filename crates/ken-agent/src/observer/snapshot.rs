//! Orchestration that assembles a full OS status snapshot from
//! individual observers per ADR-0001 T2-1.

use ken_protocol::status::OsStatusSnapshot;
use time::OffsetDateTime;

use super::{bitlocker, defender, event_log, firewall, windows_update};

/// Collect a complete OS status snapshot from all observers.
///
/// Each observer returns its subsystem type with individual fields
/// wrapped in `Observation<T>` per ADR-0019. Subsystems are no longer
/// `Option` at the snapshot level.
pub fn collect_snapshot() -> OsStatusSnapshot {
    let collected_at = OffsetDateTime::now_utc();

    let start = std::time::Instant::now();
    let defender = defender::collect();
    tracing::debug!(
        elapsed_ms = start.elapsed().as_millis(),
        "defender observer"
    );

    let start = std::time::Instant::now();
    let firewall = firewall::collect();
    tracing::debug!(
        elapsed_ms = start.elapsed().as_millis(),
        "firewall observer"
    );

    let start = std::time::Instant::now();
    let bitlocker = bitlocker::collect();
    tracing::debug!(
        elapsed_ms = start.elapsed().as_millis(),
        "bitlocker observer"
    );

    let start = std::time::Instant::now();
    let windows_update = windows_update::collect();
    tracing::debug!(
        elapsed_ms = start.elapsed().as_millis(),
        "windows_update observer"
    );

    let start = std::time::Instant::now();
    let recent_security_events = event_log::collect();
    tracing::debug!(
        elapsed_ms = start.elapsed().as_millis(),
        "event_log observer"
    );

    tracing::debug!("status snapshot collected");

    OsStatusSnapshot {
        collected_at,
        defender,
        firewall,
        bitlocker,
        windows_update,
        recent_security_events,
    }
}

#[cfg(test)]
mod tests {
    use ken_protocol::status::Observation;

    use super::*;

    #[test]
    fn snapshot_collects_without_panic() {
        let snapshot = collect_snapshot();
        // On non-Windows, all observers return Unobserved
        assert_eq!(snapshot.recent_security_events, Observation::Unobserved);
        assert_eq!(
            snapshot.defender.antivirus_enabled,
            Observation::Unobserved
        );
    }
}
