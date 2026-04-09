//! Audit event types that travel from agent to server.
//!
//! Per ADR-0001 T1-5, every action the agent takes is recorded in a
//! local audit log. A tail of recent events is included in each
//! heartbeat so the server has visibility into agent activity.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::command::CommandResult;
use crate::ids::CommandId;

/// A single entry from the agent's audit log that is eligible to
/// travel to the server in a heartbeat's `audit_tail`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditEvent {
    /// Unique identifier for this event.
    pub event_id: Uuid,

    /// When the event occurred on the agent.
    #[serde(with = "time::serde::rfc3339")]
    pub occurred_at: OffsetDateTime,

    /// What kind of event this is.
    pub kind: AuditEventKind,

    /// Human-readable description of the event.
    pub message: String,
}

/// Classification of audit events.
///
/// Some variants reference Phase 2 concepts (consent, kill switch) and
/// are defined now so the wire protocol is stable when Phase 2 lands.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuditEventKind {
    /// The agent service started.
    ServiceStarted,

    /// The agent service stopped.
    ServiceStopped,

    /// A heartbeat was sent to the server.
    HeartbeatSent,

    /// A command was received from the server.
    CommandReceived {
        /// The command that was received.
        command_id: CommandId,
    },

    /// A command finished processing.
    CommandCompleted {
        /// The command that completed.
        command_id: CommandId,
        /// How the command concluded.
        result: CommandResult,
    },

    /// A consent dialog was shown to the user (Phase 2).
    ConsentRequested,

    /// The user granted consent (Phase 2).
    ConsentGranted,

    /// The user denied consent (Phase 2).
    ConsentDenied,

    /// The user activated the local kill switch (ADR-0001 T1-6).
    KillSwitchActivated,

    /// The agent checked for updates.
    UpdateCheckPerformed,

    /// An update was downloaded.
    UpdateDownloaded {
        /// Version that was downloaded.
        version: String,
    },

    /// An update was installed.
    UpdateInstalled {
        /// Version that was installed.
        version: String,
    },

    /// An error occurred during agent operation.
    Error {
        /// What the agent was doing when the error occurred.
        context: String,
    },
}
