//! IPC protocol between the SYSTEM service and user-mode Tray App.
//!
//! Uses Named Pipes on Windows. On non-Windows platforms, provides
//! stub implementations for development and testing.

use ken_protocol::ids::CommandId;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// A request from the Tray App to the service.
///
/// Per ADR-0010, the wire format is length-prefixed JSON: 4-byte
/// little-endian length followed by a UTF-8 JSON body. The tray app
/// is always the initiator; the service is always the responder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IpcRequest {
    /// Query the service's current status.
    GetStatus,
    /// Ask whether there is a pending consent request (tray app polls).
    GetPendingConsent,
    /// Report the user's consent decision back to the service.
    SubmitConsentResponse {
        /// Which command this response is for.
        command_id: CommandId,
        /// Whether the user granted consent.
        granted: bool,
    },
    /// Get the tail of the audit log.
    GetAuditLogTail {
        /// Number of recent lines to return.
        lines: u32,
    },
    /// Activate the local kill switch (ADR-0001 T1-6).
    ActivateKillSwitch,
}

/// A response from the service to the Tray App.
///
/// Per ADR-0010, serialized as length-prefixed JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IpcResponse {
    /// Current agent status.
    Status(AgentStatus),
    /// A consent request is pending; the tray app should show the dialog.
    ConsentPending {
        /// Which command this consent request is for.
        command_id: CommandId,
        /// Description of why the session is requested.
        session_description: String,
        /// Who is requesting the session.
        admin_name: String,
    },
    /// No consent request is pending.
    NoPendingConsent,
    /// The service received the user's consent decision.
    ConsentResponseAcknowledged,
    /// Tail of the audit log.
    AuditLogTail(Vec<String>),
    /// Kill switch was activated.
    KillSwitchActivated,
    /// An error occurred processing the request.
    Error(String),
}

/// Summary of the agent's current state, shown in the Tray App.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentStatus {
    /// Whether the SYSTEM service is running.
    pub service_running: bool,
    /// Whether the agent has been enrolled with a server.
    pub enrolled: bool,
    /// The endpoint ID, if enrolled.
    pub endpoint_id: Option<String>,
    /// When the last heartbeat was sent.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_heartbeat: Option<OffsetDateTime>,
    /// Number of commands pending execution.
    pub pending_commands: u32,
    /// Agent binary version.
    pub agent_version: String,
}

/// The outcome of a consent request from the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsentOutcome {
    /// User clicked "Allow".
    Granted,
    /// User clicked "Deny".
    Denied,
    /// 60 seconds passed with no response — auto-denied.
    TimedOut,
}

/// Information about a pending consent request, returned to the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingConsentInfo {
    /// Which command this consent request is for.
    pub command_id: CommandId,
    /// Description of why the session is requested.
    pub session_description: String,
    /// Who is requesting the session.
    pub admin_name: String,
}

#[cfg(all(windows, feature = "tray-app"))]
pub mod client;
#[cfg(windows)]
pub mod server;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_get_status_roundtrip() {
        let req = IpcRequest::GetStatus;
        let json = serde_json::to_string(&req).unwrap();
        let back: IpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn request_get_pending_consent_roundtrip() {
        let req = IpcRequest::GetPendingConsent;
        let json = serde_json::to_string(&req).unwrap();
        let back: IpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn request_submit_consent_response_roundtrip() {
        let req = IpcRequest::SubmitConsentResponse {
            command_id: CommandId::new(),
            granted: true,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: IpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn request_submit_consent_denied_roundtrip() {
        let req = IpcRequest::SubmitConsentResponse {
            command_id: CommandId::new(),
            granted: false,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: IpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn request_audit_tail_roundtrip() {
        let req = IpcRequest::GetAuditLogTail { lines: 50 };
        let json = serde_json::to_string(&req).unwrap();
        let back: IpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn request_kill_switch_roundtrip() {
        let req = IpcRequest::ActivateKillSwitch;
        let json = serde_json::to_string(&req).unwrap();
        let back: IpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn response_status_roundtrip() {
        let resp = IpcResponse::Status(AgentStatus {
            service_running: true,
            enrolled: true,
            endpoint_id: Some("test-id".to_string()),
            last_heartbeat: None,
            pending_commands: 0,
            agent_version: "0.1.0".to_string(),
        });
        let json = serde_json::to_string(&resp).unwrap();
        let back: IpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn response_consent_pending_roundtrip() {
        let resp = IpcResponse::ConsentPending {
            command_id: CommandId::new(),
            session_description: "checking Defender".to_string(),
            admin_name: "IT Admin".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: IpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn response_no_pending_consent_roundtrip() {
        let resp = IpcResponse::NoPendingConsent;
        let json = serde_json::to_string(&resp).unwrap();
        let back: IpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn response_consent_acknowledged_roundtrip() {
        let resp = IpcResponse::ConsentResponseAcknowledged;
        let json = serde_json::to_string(&resp).unwrap();
        let back: IpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn response_kill_switch_roundtrip() {
        let resp = IpcResponse::KillSwitchActivated;
        let json = serde_json::to_string(&resp).unwrap();
        let back: IpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn response_error_roundtrip() {
        let resp = IpcResponse::Error("something went wrong".to_string());
        let json = serde_json::to_string(&resp).unwrap();
        let back: IpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn response_audit_log_tail_roundtrip() {
        let resp = IpcResponse::AuditLogTail(vec!["line 1".to_string(), "line 2".to_string()]);
        let json = serde_json::to_string(&resp).unwrap();
        let back: IpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }
}
