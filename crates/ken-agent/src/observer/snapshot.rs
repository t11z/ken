//! Orchestration that assembles a full OS status snapshot from
//! individual observers.

use ken_protocol::status::OsStatusSnapshot;
use time::OffsetDateTime;

/// Collect a complete OS status snapshot from all observers.
///
/// Each observer handles errors independently: if one fails, it returns
/// `None` and the rest still run. An error from one observer never
/// poisons the rest.
pub fn collect_snapshot() -> OsStatusSnapshot {
    let collected_at = OffsetDateTime::now_utc();

    let defender = super::collect_defender();
    let firewall = super::collect_firewall();
    let bitlocker = super::collect_bitlocker();
    let windows_update = super::collect_windows_update();
    let recent_security_events = super::collect_security_events();

    tracing::debug!(
        defender = defender.is_some(),
        firewall = firewall.is_some(),
        bitlocker = bitlocker.is_some(),
        windows_update = windows_update.is_some(),
        events = recent_security_events.len(),
        "status snapshot collected"
    );

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
    use super::*;

    #[test]
    fn snapshot_collects_without_panic() {
        let snapshot = collect_snapshot();
        // On non-Windows, all observers return None/empty
        assert!(snapshot.recent_security_events.is_empty());
    }
}
