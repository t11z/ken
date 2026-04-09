//! Round-trip serialization tests for every public wire type.
//!
//! Each test constructs a realistic instance, serializes it to JSON,
//! deserializes it back, and asserts equality with the original.

use time::OffsetDateTime;
use uuid::Uuid;

use ken_protocol::audit::{AuditEvent, AuditEventKind};
use ken_protocol::command::{
    CommandEnvelope, CommandOutcome, CommandPayload, CommandResult,
};
use ken_protocol::enrollment::{EnrollmentRequest, EnrollmentResponse};
use ken_protocol::heartbeat::{Heartbeat, HeartbeatAck};
use ken_protocol::ids::{CommandId, EndpointId, HeartbeatId, SessionId};
use ken_protocol::status::{
    BitLockerStatus, BitLockerVolumeStatus, DefenderStatus, FirewallProfileState,
    FirewallStatus, OsStatusSnapshot, SecurityEvent, SecurityEventLevel,
    WindowsUpdateStatus,
};
use ken_protocol::SCHEMA_VERSION;

fn now() -> OffsetDateTime {
    // Use a fixed point in time for deterministic tests.
    OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap()
}

fn roundtrip<T>(value: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let json = serde_json::to_string(value).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}

// --- Identifier types ---

#[test]
fn endpoint_id_roundtrip() {
    let id = EndpointId::new();
    let back = roundtrip(&id);
    assert_eq!(id, back);
}

#[test]
fn command_id_roundtrip() {
    let id = CommandId::new();
    let back = roundtrip(&id);
    assert_eq!(id, back);
}

#[test]
fn heartbeat_id_roundtrip() {
    let id = HeartbeatId::new();
    let back = roundtrip(&id);
    assert_eq!(id, back);
}

#[test]
fn session_id_roundtrip() {
    let id = SessionId::new();
    let back = roundtrip(&id);
    assert_eq!(id, back);
}

// --- Enrollment ---

#[test]
fn enrollment_request_roundtrip() {
    let req = EnrollmentRequest {
        schema_version: SCHEMA_VERSION,
        enrollment_token: "abc123".to_string(),
        agent_version: "0.1.0".to_string(),
        os_version: "Windows 11 24H2".to_string(),
        hostname: "DESKTOP-FAMILY".to_string(),
        requested_at: now(),
    };
    assert_eq!(req, roundtrip(&req));
}

#[test]
fn enrollment_response_roundtrip() {
    let resp = EnrollmentResponse {
        endpoint_id: EndpointId::new(),
        ca_certificate_pem: "-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----".to_string(),
        client_certificate_pem: "-----BEGIN CERTIFICATE-----\nclient\n-----END CERTIFICATE-----".to_string(),
        client_private_key_pem: "-----BEGIN PRIVATE KEY-----\nkey\n-----END PRIVATE KEY-----".to_string(),
        server_url: "https://ken.local:8443".to_string(),
        issued_at: now(),
        certificate_expires_at: now(),
    };
    assert_eq!(resp, roundtrip(&resp));
}

// --- Status types ---

fn sample_defender() -> DefenderStatus {
    DefenderStatus {
        antivirus_enabled: true,
        real_time_protection_enabled: true,
        tamper_protection_enabled: true,
        signature_version: "1.401.622.0".to_string(),
        signature_last_updated: now(),
        signature_age_days: 0,
        last_full_scan: Some(now()),
        last_quick_scan: None,
    }
}

#[test]
fn defender_status_roundtrip() {
    let status = sample_defender();
    assert_eq!(status, roundtrip(&status));
}

#[test]
fn firewall_status_roundtrip() {
    let status = FirewallStatus {
        domain_profile: FirewallProfileState {
            enabled: true,
            default_inbound_action: "block".to_string(),
        },
        private_profile: FirewallProfileState {
            enabled: true,
            default_inbound_action: "block".to_string(),
        },
        public_profile: FirewallProfileState {
            enabled: true,
            default_inbound_action: "block".to_string(),
        },
    };
    assert_eq!(status, roundtrip(&status));
}

#[test]
fn bitlocker_status_roundtrip() {
    let status = BitLockerStatus {
        volumes: vec![BitLockerVolumeStatus {
            drive_letter: "C:".to_string(),
            protection_status: "on".to_string(),
            encryption_percentage: 100,
        }],
    };
    assert_eq!(status, roundtrip(&status));
}

#[test]
fn windows_update_status_roundtrip() {
    let status = WindowsUpdateStatus {
        last_search_time: Some(now()),
        last_install_time: None,
        pending_update_count: 3,
        pending_critical_update_count: 1,
    };
    assert_eq!(status, roundtrip(&status));
}

#[test]
fn security_event_roundtrip() {
    let event = SecurityEvent {
        event_id: 4625,
        source: "Security".to_string(),
        level: SecurityEventLevel::Warning,
        occurred_at: now(),
        summary: "Failed logon attempt".to_string(),
    };
    assert_eq!(event, roundtrip(&event));
}

#[test]
fn security_event_level_all_variants() {
    for level in [
        SecurityEventLevel::Information,
        SecurityEventLevel::Warning,
        SecurityEventLevel::Error,
        SecurityEventLevel::Critical,
    ] {
        assert_eq!(level, roundtrip(&level));
    }
}

#[test]
fn os_status_snapshot_roundtrip() {
    let snapshot = OsStatusSnapshot {
        collected_at: now(),
        defender: Some(sample_defender()),
        firewall: None,
        bitlocker: None,
        windows_update: None,
        recent_security_events: vec![],
    };
    assert_eq!(snapshot, roundtrip(&snapshot));
}

// --- Command types ---

#[test]
fn command_payload_ping_roundtrip() {
    let payload = CommandPayload::Ping;
    assert_eq!(payload, roundtrip(&payload));
}

#[test]
fn command_payload_request_remote_session_roundtrip() {
    let payload = CommandPayload::RequestRemoteSession {
        reason: "Checking Defender settings".to_string(),
    };
    assert_eq!(payload, roundtrip(&payload));
}

#[test]
fn command_payload_refresh_status_roundtrip() {
    let payload = CommandPayload::RefreshStatus;
    assert_eq!(payload, roundtrip(&payload));
}

#[test]
fn command_envelope_roundtrip() {
    let envelope = CommandEnvelope {
        command_id: CommandId::new(),
        issued_at: now(),
        expires_at: now(),
        payload: CommandPayload::Ping,
    };
    assert_eq!(envelope, roundtrip(&envelope));
}

#[test]
fn command_result_all_variants() {
    let variants = vec![
        CommandResult::Ok,
        CommandResult::NotImplementedYet,
        CommandResult::Rejected {
            reason: "user denied".to_string(),
        },
        CommandResult::Failed {
            error: "timeout".to_string(),
        },
    ];
    for result in variants {
        assert_eq!(result, roundtrip(&result));
    }
}

#[test]
fn command_outcome_roundtrip() {
    let outcome = CommandOutcome {
        command_id: CommandId::new(),
        completed_at: now(),
        result: CommandResult::Ok,
    };
    assert_eq!(outcome, roundtrip(&outcome));
}

// --- Heartbeat types ---

#[test]
fn heartbeat_roundtrip() {
    let hb = Heartbeat {
        heartbeat_id: HeartbeatId::new(),
        endpoint_id: EndpointId::new(),
        schema_version: SCHEMA_VERSION,
        agent_version: "0.1.0".to_string(),
        sent_at: now(),
        status: OsStatusSnapshot {
            collected_at: now(),
            defender: None,
            firewall: None,
            bitlocker: None,
            windows_update: None,
            recent_security_events: vec![],
        },
        audit_tail: vec![],
    };
    assert_eq!(hb, roundtrip(&hb));
}

#[test]
fn heartbeat_ack_roundtrip() {
    let ack = HeartbeatAck {
        received_at: now(),
        pending_commands: vec![],
        next_heartbeat_interval_seconds: 60,
    };
    assert_eq!(ack, roundtrip(&ack));
}

#[test]
fn heartbeat_ack_with_commands_roundtrip() {
    let ack = HeartbeatAck {
        received_at: now(),
        pending_commands: vec![CommandEnvelope {
            command_id: CommandId::new(),
            issued_at: now(),
            expires_at: now(),
            payload: CommandPayload::RefreshStatus,
        }],
        next_heartbeat_interval_seconds: 30,
    };
    assert_eq!(ack, roundtrip(&ack));
}

// --- Audit types ---

#[test]
fn audit_event_roundtrip() {
    let event = AuditEvent {
        event_id: Uuid::new_v4(),
        occurred_at: now(),
        kind: AuditEventKind::ServiceStarted,
        message: "service started".to_string(),
    };
    assert_eq!(event, roundtrip(&event));
}

#[test]
fn audit_event_kind_all_variants() {
    let cmd_id = CommandId::new();
    let variants: Vec<AuditEventKind> = vec![
        AuditEventKind::ServiceStarted,
        AuditEventKind::ServiceStopped,
        AuditEventKind::HeartbeatSent,
        AuditEventKind::CommandReceived {
            command_id: cmd_id,
        },
        AuditEventKind::CommandCompleted {
            command_id: cmd_id,
            result: CommandResult::Ok,
        },
        AuditEventKind::ConsentRequested,
        AuditEventKind::ConsentGranted,
        AuditEventKind::ConsentDenied,
        AuditEventKind::KillSwitchActivated,
        AuditEventKind::UpdateCheckPerformed,
        AuditEventKind::UpdateDownloaded {
            version: "0.2.0".to_string(),
        },
        AuditEventKind::UpdateInstalled {
            version: "0.2.0".to_string(),
        },
        AuditEventKind::Error {
            context: "heartbeat failed".to_string(),
        },
    ];
    for kind in variants {
        assert_eq!(kind, roundtrip(&kind));
    }
}
