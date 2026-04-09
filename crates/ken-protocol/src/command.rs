//! Command types for server-to-agent instructions.
//!
//! Commands are issued by the family IT chief through the admin UI and
//! delivered to the agent in heartbeat acknowledgments. Each command has
//! a unique identifier, an expiry, and a typed payload.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::ids::CommandId;

/// A command from the server to the agent, wrapped in a delivery envelope.
///
/// Commands that expire before the agent picks them up are silently
/// discarded by the agent. This prevents stale instructions from
/// executing long after they were relevant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandEnvelope {
    /// Unique identifier for this command.
    pub command_id: CommandId,

    /// When the server issued this command.
    #[serde(with = "time::serde::rfc3339")]
    pub issued_at: OffsetDateTime,

    /// After this time the command should be discarded if not yet executed.
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,

    /// The typed command payload.
    pub payload: CommandPayload,
}

/// The body of a command from server to agent.
///
/// Uses `#[serde(tag = "type")]` for a tagged union so the JSON includes
/// a `"type"` field that identifies the variant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandPayload {
    /// Simple liveness check. The agent responds with `CommandResult::Ok`
    /// in the next heartbeat.
    Ping,

    /// Request to start a remote session. Phase 2 functionality — the
    /// type is defined now so the wire protocol is stable. Phase 1
    /// agents respond with `CommandResult::NotImplementedYet`.
    RequestRemoteSession {
        /// Human-readable reason for the session request, shown in
        /// the consent dialog on the endpoint.
        reason: String,
    },

    /// Tells the agent to collect and send a fresh OS status snapshot
    /// immediately, without waiting for the next scheduled heartbeat.
    RefreshStatus,
}

/// The agent's response to a command, reported in the next heartbeat.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandOutcome {
    /// Which command this outcome is for.
    pub command_id: CommandId,

    /// When the agent completed processing the command.
    #[serde(with = "time::serde::rfc3339")]
    pub completed_at: OffsetDateTime,

    /// The result of the command execution.
    pub result: CommandResult,
}

/// Result of a command execution on the agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CommandResult {
    /// Command executed successfully.
    Ok,

    /// Command is recognized but the implementing subsystem is not
    /// yet available (e.g., remote sessions in Phase 1).
    NotImplementedYet,

    /// Command was refused by the agent's policy (e.g., user denied
    /// consent for a remote session).
    Rejected {
        /// Why the command was rejected.
        reason: String,
    },

    /// Command was attempted but failed during execution.
    Failed {
        /// Description of what went wrong.
        error: String,
    },
}
