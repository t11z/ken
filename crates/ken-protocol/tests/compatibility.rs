//! Compatibility tests for the Ken protocol.
//!
//! These verify that payloads with unknown fields parse correctly
//! (forward compatibility) and that the schema version constant
//! has not been changed accidentally.

use ken_protocol::status::OsStatusSnapshot;
use ken_protocol::SCHEMA_VERSION;

#[test]
fn schema_version_is_one() {
    assert_eq!(
        SCHEMA_VERSION, 1,
        "SCHEMA_VERSION changed — this requires a dedicated ADR and migration plan"
    );
}

#[test]
fn unknown_fields_are_ignored_on_deserialization() {
    // A payload with extra fields that do not exist in the current schema
    // should still parse. This is the forward-compatibility guarantee:
    // a newer agent can add optional fields without breaking older servers.
    let json = r#"{
        "collected_at": "2024-01-01T00:00:00Z",
        "defender": null,
        "firewall": null,
        "bitlocker": null,
        "windows_update": null,
        "recent_security_events": [],
        "future_field_that_does_not_exist": "hello",
        "another_unknown": 42
    }"#;

    let snapshot: OsStatusSnapshot =
        serde_json::from_str(json).expect("should parse with unknown fields");
    assert!(snapshot.defender.is_none());
    assert!(snapshot.recent_security_events.is_empty());
}

#[test]
fn heartbeat_with_extra_fields_parses() {
    let json = r#"{
        "heartbeat_id": "00000000-0000-0000-0000-000000000001",
        "endpoint_id": "00000000-0000-0000-0000-000000000002",
        "schema_version": 1,
        "agent_version": "0.1.0",
        "sent_at": "2024-01-01T00:00:00Z",
        "status": {
            "collected_at": "2024-01-01T00:00:00Z",
            "defender": null,
            "firewall": null,
            "bitlocker": null,
            "windows_update": null,
            "recent_security_events": []
        },
        "audit_tail": [],
        "some_future_field": true
    }"#;

    let hb: ken_protocol::heartbeat::Heartbeat =
        serde_json::from_str(json).expect("should parse heartbeat with extra fields");
    assert_eq!(hb.schema_version, 1);
}
