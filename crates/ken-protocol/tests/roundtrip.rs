//! Round-trip serialization tests for every public wire type.
//!
//! Each test constructs a realistic instance, serializes it to JSON,
//! deserializes it back, and asserts equality with the original.

use time::OffsetDateTime;
use uuid::Uuid;

use ken_protocol::audit::{AuditEvent, AuditEventKind, TrayLaunchTrigger, TrayTerminationReason};
use ken_protocol::command::{CommandEnvelope, CommandOutcome, CommandPayload, CommandResult};
use ken_protocol::enrollment::{EnrollmentRequest, EnrollmentResponse};
use ken_protocol::heartbeat::{Heartbeat, HeartbeatAck};
use ken_protocol::ids::{CommandId, EndpointId, HeartbeatId, SessionId};
use ken_protocol::status::{
    BitLockerStatus, BitLockerVolumeStatus, DefenderStatus, FirewallProfileState, FirewallStatus,
    Observation, OsStatusSnapshot, SecurityEvent, SecurityEventLevel, WindowsUpdateStatus,
};
use ken_protocol::SCHEMA_VERSION;

fn now() -> OffsetDateTime {
    // Use a fixed point in time for deterministic tests.
    OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap()
}

fn earlier() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_699_996_400).unwrap()
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
        ca_certificate_pem: "-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----"
            .to_string(),
        client_certificate_pem: "-----BEGIN CERTIFICATE-----\nclient\n-----END CERTIFICATE-----"
            .to_string(),
        client_private_key_pem: "-----BEGIN PRIVATE KEY-----\nkey\n-----END PRIVATE KEY-----"
            .to_string(),
        server_url: "https://ken.local:8443".to_string(),
        issued_at: now(),
        certificate_expires_at: now(),
    };
    assert_eq!(resp, roundtrip(&resp));
}

// --- Observation<T> variants (ADR-0019) ---

#[test]
fn observation_fresh_roundtrip() {
    let obs: Observation<u32> = Observation::Fresh {
        value: 7,
        observed_at: now(),
    };
    assert_eq!(obs, roundtrip(&obs));
}

#[test]
fn observation_cached_roundtrip() {
    let obs: Observation<u32> = Observation::Cached {
        value: 3,
        observed_at: earlier(),
    };
    assert_eq!(obs, roundtrip(&obs));
}

#[test]
fn observation_unobserved_roundtrip() {
    let obs: Observation<u32> = Observation::Unobserved;
    assert_eq!(obs, roundtrip(&obs));
}

#[test]
fn observation_fresh_json_shape() {
    let obs: Observation<u32> = Observation::Fresh {
        value: 7,
        observed_at: now(),
    };
    let json: serde_json::Value = serde_json::to_value(&obs).unwrap();
    assert_eq!(json["kind"], "fresh");
    assert_eq!(json["value"], 7);
    assert!(json["observed_at"].is_string());
}

#[test]
fn observation_unobserved_json_shape() {
    let obs: Observation<u32> = Observation::Unobserved;
    let json: serde_json::Value = serde_json::to_value(&obs).unwrap();
    assert_eq!(json["kind"], "unobserved");
    assert!(json.get("value").is_none());
}

#[test]
fn observation_option_datetime_roundtrip() {
    // ADR-0019: Observation<Option<T>> for data-model-optional fields.
    let with_value: Observation<Option<OffsetDateTime>> = Observation::Fresh {
        value: Some(now()),
        observed_at: now(),
    };
    assert_eq!(with_value, roundtrip(&with_value));

    let without_value: Observation<Option<OffsetDateTime>> = Observation::Fresh {
        value: None,
        observed_at: now(),
    };
    assert_eq!(without_value, roundtrip(&without_value));

    let unobserved: Observation<Option<OffsetDateTime>> = Observation::Unobserved;
    assert_eq!(unobserved, roundtrip(&unobserved));
}

#[test]
fn observation_value_helper() {
    let fresh: Observation<u32> = Observation::Fresh {
        value: 42,
        observed_at: now(),
    };
    assert_eq!(fresh.value(), Some(&42));

    let cached: Observation<u32> = Observation::Cached {
        value: 10,
        observed_at: now(),
    };
    assert_eq!(cached.value(), Some(&10));

    let unobserved: Observation<u32> = Observation::Unobserved;
    assert_eq!(unobserved.value(), None);
}

// --- Status types ---

fn sample_defender() -> DefenderStatus {
    DefenderStatus {
        antivirus_enabled: Observation::Fresh {
            value: true,
            observed_at: now(),
        },
        real_time_protection_enabled: Observation::Fresh {
            value: true,
            observed_at: now(),
        },
        tamper_protection_enabled: Observation::Fresh {
            value: true,
            observed_at: now(),
        },
        signature_version: Observation::Fresh {
            value: "1.401.622.0".to_string(),
            observed_at: now(),
        },
        signature_last_updated: Observation::Fresh {
            value: now(),
            observed_at: now(),
        },
        signature_age_days: Observation::Fresh {
            value: 0,
            observed_at: now(),
        },
        last_full_scan: Observation::Fresh {
            value: Some(now()),
            observed_at: now(),
        },
        last_quick_scan: Observation::Unobserved,
    }
}

fn unobserved_defender() -> DefenderStatus {
    DefenderStatus {
        antivirus_enabled: Observation::Unobserved,
        real_time_protection_enabled: Observation::Unobserved,
        tamper_protection_enabled: Observation::Unobserved,
        signature_version: Observation::Unobserved,
        signature_last_updated: Observation::Unobserved,
        signature_age_days: Observation::Unobserved,
        last_full_scan: Observation::Unobserved,
        last_quick_scan: Observation::Unobserved,
    }
}

fn unobserved_firewall() -> FirewallStatus {
    FirewallStatus {
        domain_profile: Observation::Unobserved,
        private_profile: Observation::Unobserved,
        public_profile: Observation::Unobserved,
    }
}

fn unobserved_bitlocker() -> BitLockerStatus {
    BitLockerStatus {
        volumes: Observation::Unobserved,
    }
}

fn unobserved_windows_update() -> WindowsUpdateStatus {
    WindowsUpdateStatus {
        last_search_time: Observation::Unobserved,
        last_install_time: Observation::Unobserved,
        pending_update_count: Observation::Unobserved,
        pending_critical_update_count: Observation::Unobserved,
    }
}

#[test]
fn defender_status_roundtrip() {
    let status = sample_defender();
    assert_eq!(status, roundtrip(&status));
}

#[test]
fn defender_status_all_unobserved_roundtrip() {
    let status = unobserved_defender();
    assert_eq!(status, roundtrip(&status));
}

#[test]
fn firewall_status_roundtrip() {
    let status = FirewallStatus {
        domain_profile: Observation::Fresh {
            value: FirewallProfileState {
                enabled: true,
                default_inbound_action: "block".to_string(),
            },
            observed_at: now(),
        },
        private_profile: Observation::Cached {
            value: FirewallProfileState {
                enabled: true,
                default_inbound_action: "block".to_string(),
            },
            observed_at: earlier(),
        },
        public_profile: Observation::Unobserved,
    };
    assert_eq!(status, roundtrip(&status));
}

#[test]
fn bitlocker_status_roundtrip() {
    let status = BitLockerStatus {
        volumes: Observation::Fresh {
            value: vec![BitLockerVolumeStatus {
                drive_letter: "C:".to_string(),
                protection_status: "on".to_string(),
                encryption_percentage: 100,
            }],
            observed_at: now(),
        },
    };
    assert_eq!(status, roundtrip(&status));
}

#[test]
fn windows_update_status_roundtrip() {
    let status = WindowsUpdateStatus {
        last_search_time: Observation::Fresh {
            value: Some(now()),
            observed_at: now(),
        },
        last_install_time: Observation::Unobserved,
        pending_update_count: Observation::Fresh {
            value: 3,
            observed_at: now(),
        },
        pending_critical_update_count: Observation::Cached {
            value: 1,
            observed_at: earlier(),
        },
    };
    assert_eq!(status, roundtrip(&status));
}

#[test]
fn windows_update_status_all_unobserved_roundtrip() {
    let status = unobserved_windows_update();
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
        defender: sample_defender(),
        firewall: unobserved_firewall(),
        bitlocker: unobserved_bitlocker(),
        windows_update: unobserved_windows_update(),
        recent_security_events: Observation::Unobserved,
    };
    assert_eq!(snapshot, roundtrip(&snapshot));
}

#[test]
fn os_status_snapshot_all_unobserved_roundtrip() {
    let snapshot = OsStatusSnapshot {
        collected_at: now(),
        defender: unobserved_defender(),
        firewall: unobserved_firewall(),
        bitlocker: unobserved_bitlocker(),
        windows_update: unobserved_windows_update(),
        recent_security_events: Observation::Unobserved,
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
        schema_version: SCHEMA_VERSION,
        agent_version: "0.1.0".to_string(),
        sent_at: now(),
        status: OsStatusSnapshot {
            collected_at: now(),
            defender: unobserved_defender(),
            firewall: unobserved_firewall(),
            bitlocker: unobserved_bitlocker(),
            windows_update: unobserved_windows_update(),
            recent_security_events: Observation::Unobserved,
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
        AuditEventKind::CommandReceived { command_id: cmd_id },
        AuditEventKind::CommandCompleted {
            command_id: cmd_id,
            result: CommandResult::Ok,
        },
        AuditEventKind::ConsentRequested,
        AuditEventKind::ConsentGranted,
        AuditEventKind::ConsentDenied,
        AuditEventKind::KillSwitchActivated,
        AuditEventKind::KillSwitchStartupRefused,
        AuditEventKind::UpdateCheckPerformed,
        AuditEventKind::UpdateDownloaded {
            version: "0.2.0".to_string(),
        },
        AuditEventKind::UpdateInstalled {
            version: "0.2.0".to_string(),
        },
        AuditEventKind::TrayLaunched {
            session_id: 1,
            trigger: TrayLaunchTrigger::SessionLogon,
        },
        AuditEventKind::TrayLaunchFailed {
            session_id: 2,
            error: "WTSQueryUserToken failed: access denied".to_string(),
        },
        AuditEventKind::TrayTerminated {
            session_id: 1,
            reason: TrayTerminationReason::SessionLogoff,
        },
        AuditEventKind::Error {
            context: "heartbeat failed".to_string(),
        },
    ];
    for kind in variants {
        assert_eq!(kind, roundtrip(&kind));
    }
}

#[test]
fn tray_launched_startup_roundtrip() {
    let kind = AuditEventKind::TrayLaunched {
        session_id: 1,
        trigger: TrayLaunchTrigger::Startup,
    };
    let event = AuditEvent {
        event_id: Uuid::new_v4(),
        occurred_at: now(),
        kind,
        message: "tray app launched in session 1 at service startup".to_string(),
    };
    assert_eq!(event, roundtrip(&event));
}

#[test]
fn tray_launched_session_logon_roundtrip() {
    let kind = AuditEventKind::TrayLaunched {
        session_id: 3,
        trigger: TrayLaunchTrigger::SessionLogon,
    };
    let event = AuditEvent {
        event_id: Uuid::new_v4(),
        occurred_at: now(),
        kind,
        message: "tray app launched in session 3 on logon".to_string(),
    };
    assert_eq!(event, roundtrip(&event));
}

#[test]
fn tray_launch_failed_roundtrip() {
    let kind = AuditEventKind::TrayLaunchFailed {
        session_id: 2,
        error: "CreateProcessAsUser failed: 0x80070005".to_string(),
    };
    let event = AuditEvent {
        event_id: Uuid::new_v4(),
        occurred_at: now(),
        kind,
        message: "failed to launch tray app in session 2".to_string(),
    };
    assert_eq!(event, roundtrip(&event));
}

#[test]
fn tray_terminated_logoff_roundtrip() {
    let kind = AuditEventKind::TrayTerminated {
        session_id: 1,
        reason: TrayTerminationReason::SessionLogoff,
    };
    let event = AuditEvent {
        event_id: Uuid::new_v4(),
        occurred_at: now(),
        kind,
        message: "tray app terminated on session logoff".to_string(),
    };
    assert_eq!(event, roundtrip(&event));
}

#[test]
fn tray_terminated_service_shutdown_roundtrip() {
    let kind = AuditEventKind::TrayTerminated {
        session_id: 1,
        reason: TrayTerminationReason::ServiceShutdown,
    };
    let event = AuditEvent {
        event_id: Uuid::new_v4(),
        occurred_at: now(),
        kind,
        message: "tray app terminated on service shutdown".to_string(),
    };
    assert_eq!(event, roundtrip(&event));
}

#[test]
fn tray_launch_trigger_all_variants() {
    for trigger in [TrayLaunchTrigger::Startup, TrayLaunchTrigger::SessionLogon] {
        assert_eq!(trigger, roundtrip(&trigger));
    }
}

#[test]
fn tray_termination_reason_all_variants() {
    for reason in [
        TrayTerminationReason::SessionLogoff,
        TrayTerminationReason::ServiceShutdown,
    ] {
        assert_eq!(reason, roundtrip(&reason));
    }
}
