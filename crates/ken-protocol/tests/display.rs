//! Tests for the Display implementations of identifier types.

use ken_protocol::ids::{CommandId, EndpointId, HeartbeatId, SessionId};

#[test]
fn endpoint_id_display_is_uuid_format() {
    let id = EndpointId::new();
    let display = id.to_string();
    // UUID v4 format: 8-4-4-4-12 hex chars
    assert_eq!(display.len(), 36);
    assert_eq!(display.chars().filter(|&c| c == '-').count(), 4);
}

#[test]
fn command_id_display_is_uuid_format() {
    let id = CommandId::new();
    let display = id.to_string();
    assert_eq!(display.len(), 36);
    assert_eq!(display.chars().filter(|&c| c == '-').count(), 4);
}

#[test]
fn heartbeat_id_display_is_uuid_format() {
    let id = HeartbeatId::new();
    let display = id.to_string();
    assert_eq!(display.len(), 36);
    assert_eq!(display.chars().filter(|&c| c == '-').count(), 4);
}

#[test]
fn session_id_display_is_uuid_format() {
    let id = SessionId::new();
    let display = id.to_string();
    assert_eq!(display.len(), 36);
    assert_eq!(display.chars().filter(|&c| c == '-').count(), 4);
}

#[test]
fn endpoint_id_parse_display_roundtrip() {
    let id = EndpointId::new();
    let parsed = EndpointId::parse(&id.to_string()).unwrap();
    assert_eq!(id, parsed);
}

#[test]
fn endpoint_id_parse_rejects_invalid() {
    assert!(EndpointId::parse("not-a-uuid").is_err());
    assert!(EndpointId::parse("").is_err());
}
