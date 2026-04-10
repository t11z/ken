//! Compatibility tests for the Ken protocol.
//!
//! These verify that payloads with unknown fields parse correctly
//! (forward compatibility) and that the schema version constant
//! has not been changed accidentally.

use ken_protocol::status::{Observation, OsStatusSnapshot};
use ken_protocol::SCHEMA_VERSION;

#[test]
fn schema_version_is_two() {
    assert_eq!(
        SCHEMA_VERSION, 2,
        "SCHEMA_VERSION changed — this requires a dedicated ADR and migration plan"
    );
}

#[test]
fn unknown_fields_are_ignored_on_deserialization() {
    // A payload with extra fields that do not exist in the current schema
    // should still parse. This is the forward-compatibility guarantee:
    // a newer agent can add optional fields without breaking older servers.
    //
    // Rewritten for schema v2 (ADR-0019): subsystems are no longer Option,
    // and observer-contributed fields use Observation<T>.
    let json = r#"{
        "collected_at": "2024-01-01T00:00:00Z",
        "defender": {
            "antivirus_enabled": {"kind": "unobserved"},
            "real_time_protection_enabled": {"kind": "unobserved"},
            "tamper_protection_enabled": {"kind": "unobserved"},
            "signature_version": {"kind": "unobserved"},
            "signature_last_updated": {"kind": "unobserved"},
            "signature_age_days": {"kind": "unobserved"},
            "last_full_scan": {"kind": "unobserved"},
            "last_quick_scan": {"kind": "unobserved"}
        },
        "firewall": {
            "domain_profile": {"kind": "unobserved"},
            "private_profile": {"kind": "unobserved"},
            "public_profile": {"kind": "unobserved"}
        },
        "bitlocker": {
            "volumes": {"kind": "unobserved"}
        },
        "windows_update": {
            "last_search_time": {"kind": "unobserved"},
            "last_install_time": {"kind": "unobserved"},
            "pending_update_count": {"kind": "unobserved"},
            "pending_critical_update_count": {"kind": "unobserved"}
        },
        "recent_security_events": {"kind": "unobserved"},
        "future_field_that_does_not_exist": "hello",
        "another_unknown": 42
    }"#;

    let snapshot: OsStatusSnapshot =
        serde_json::from_str(json).expect("should parse with unknown fields");
    assert_eq!(
        snapshot.defender.antivirus_enabled,
        Observation::Unobserved
    );
    assert_eq!(
        snapshot.recent_security_events,
        Observation::Unobserved
    );
}

#[test]
fn heartbeat_with_extra_fields_parses() {
    let json = r#"{
        "heartbeat_id": "00000000-0000-0000-0000-000000000001",
        "schema_version": 2,
        "agent_version": "0.1.0",
        "sent_at": "2024-01-01T00:00:00Z",
        "status": {
            "collected_at": "2024-01-01T00:00:00Z",
            "defender": {
                "antivirus_enabled": {"kind": "unobserved"},
                "real_time_protection_enabled": {"kind": "unobserved"},
                "tamper_protection_enabled": {"kind": "unobserved"},
                "signature_version": {"kind": "unobserved"},
                "signature_last_updated": {"kind": "unobserved"},
                "signature_age_days": {"kind": "unobserved"},
                "last_full_scan": {"kind": "unobserved"},
                "last_quick_scan": {"kind": "unobserved"}
            },
            "firewall": {
                "domain_profile": {"kind": "unobserved"},
                "private_profile": {"kind": "unobserved"},
                "public_profile": {"kind": "unobserved"}
            },
            "bitlocker": {
                "volumes": {"kind": "unobserved"}
            },
            "windows_update": {
                "last_search_time": {"kind": "unobserved"},
                "last_install_time": {"kind": "unobserved"},
                "pending_update_count": {"kind": "unobserved"},
                "pending_critical_update_count": {"kind": "unobserved"}
            },
            "recent_security_events": {"kind": "unobserved"}
        },
        "audit_tail": [],
        "some_future_field": true
    }"#;

    let hb: ken_protocol::heartbeat::Heartbeat =
        serde_json::from_str(json).expect("should parse heartbeat with extra fields");
    assert_eq!(hb.schema_version, 2);
}

#[test]
fn heartbeat_without_endpoint_id_parses() {
    // After ADR-0016, the heartbeat no longer carries endpoint_id.
    // Verify the new wire shape parses correctly and all fields are
    // populated. Updated for schema v2 (ADR-0019).
    let json = r#"{
        "heartbeat_id": "00000000-0000-0000-0000-000000000001",
        "schema_version": 2,
        "agent_version": "0.2.0",
        "sent_at": "2024-06-15T12:00:00Z",
        "status": {
            "collected_at": "2024-06-15T12:00:00Z",
            "defender": {
                "antivirus_enabled": {"kind": "unobserved"},
                "real_time_protection_enabled": {"kind": "unobserved"},
                "tamper_protection_enabled": {"kind": "unobserved"},
                "signature_version": {"kind": "unobserved"},
                "signature_last_updated": {"kind": "unobserved"},
                "signature_age_days": {"kind": "unobserved"},
                "last_full_scan": {"kind": "unobserved"},
                "last_quick_scan": {"kind": "unobserved"}
            },
            "firewall": {
                "domain_profile": {"kind": "unobserved"},
                "private_profile": {"kind": "unobserved"},
                "public_profile": {"kind": "unobserved"}
            },
            "bitlocker": {
                "volumes": {"kind": "unobserved"}
            },
            "windows_update": {
                "last_search_time": {"kind": "unobserved"},
                "last_install_time": {"kind": "unobserved"},
                "pending_update_count": {"kind": "unobserved"},
                "pending_critical_update_count": {"kind": "unobserved"}
            },
            "recent_security_events": {"kind": "unobserved"}
        },
        "audit_tail": []
    }"#;

    let hb: ken_protocol::heartbeat::Heartbeat =
        serde_json::from_str(json).expect("should parse heartbeat without endpoint_id");
    assert_eq!(hb.schema_version, 2);
    assert_eq!(hb.agent_version, "0.2.0");
    assert_eq!(
        hb.status.defender.antivirus_enabled,
        Observation::Unobserved
    );
    assert!(hb.audit_tail.is_empty());
}
