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

    /// The service refused to start because the kill switch is active
    /// (ADR-0012 step 6).
    KillSwitchStartupRefused,

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

    /// The service launched a tray app in an interactive session.
    TrayLaunched {
        /// The Windows session ID where the tray app was launched.
        session_id: u32,
        /// What triggered the launch.
        trigger: TrayLaunchTrigger,
    },

    /// The service failed to launch a tray app in an interactive session.
    TrayLaunchFailed {
        /// The Windows session ID where the launch was attempted.
        session_id: u32,
        /// The OS or application error that caused the failure.
        error: String,
    },

    /// The service terminated a tray app process.
    TrayTerminated {
        /// The Windows session ID whose tray app was terminated.
        session_id: u32,
        /// Why the tray app was terminated.
        reason: TrayTerminationReason,
    },

    /// An error occurred during agent operation.
    Error {
        /// What the agent was doing when the error occurred.
        context: String,
    },
}

/// What triggered a tray app launch.
///
/// Used as a structured field in `AuditEventKind::TrayLaunched` to
/// distinguish between the startup-time enumeration and a live
/// session-change event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrayLaunchTrigger {
    /// The service found an existing interactive session at startup.
    Startup,
    /// The service reacted to a `WTS_SESSION_LOGON` event.
    SessionLogon,
}

/// Why the service terminated a tray app process.
///
/// Used as a structured field in `AuditEventKind::TrayTerminated` to
/// distinguish between logoff-driven and shutdown-driven termination.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrayTerminationReason {
    /// The user logged off (`WTS_SESSION_LOGOFF`).
    SessionLogoff,
    /// The service is shutting down.
    ServiceShutdown,
}
