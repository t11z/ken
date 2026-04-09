//! Heartbeat types for the periodic agent-to-server status report.
//!
//! The heartbeat is the primary communication channel: the agent sends
//! one at a configurable interval (default 60s) containing its current
//! OS status snapshot and a tail of recent audit events. The server
//! responds with an acknowledgment that may include pending commands.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::audit::AuditEvent;
use crate::command::CommandEnvelope;
use crate::ids::{EndpointId, HeartbeatId};
use crate::status::OsStatusSnapshot;

/// The periodic heartbeat from agent to server.
///
/// Contains the agent's current identity, version, OS status, and a
/// bounded tail of recent audit events (up to 50) so heartbeats stay
/// small even if the agent has been busy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Heartbeat {
    /// Unique identifier for this heartbeat.
    pub heartbeat_id: HeartbeatId,

    /// Which endpoint is sending this heartbeat.
    pub endpoint_id: EndpointId,

    /// Protocol schema version.
    pub schema_version: u32,

    /// Agent binary version (semver).
    pub agent_version: String,

    /// When the agent sent this heartbeat.
    #[serde(with = "time::serde::rfc3339")]
    pub sent_at: OffsetDateTime,

    /// Current OS security state snapshot.
    pub status: OsStatusSnapshot,

    /// Recent audit events, bounded to 50 entries per heartbeat.
    pub audit_tail: Vec<AuditEvent>,
}

/// The server's acknowledgment of a heartbeat.
///
/// Carries any commands the server wants the agent to execute and
/// allows the server to adjust the agent's heartbeat cadence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeartbeatAck {
    /// When the server received and processed the heartbeat.
    #[serde(with = "time::serde::rfc3339")]
    pub received_at: OffsetDateTime,

    /// Commands queued for this endpoint since the last heartbeat.
    pub pending_commands: Vec<CommandEnvelope>,

    /// How many seconds the agent should wait before the next heartbeat.
    /// The server can use this to throttle agents or increase cadence
    /// during active troubleshooting.
    pub next_heartbeat_interval_seconds: u32,
}
