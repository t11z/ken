//! Typed HTTP client for the Ken agent-server API.
//!
//! Phase 1 stub: the actual HTTP calls will work once reqwest is added
//! as a dependency. The client struct and API shape are in place so the
//! worker loop can be wired up.

use ken_protocol::command::CommandOutcome;
use ken_protocol::heartbeat::{Heartbeat, HeartbeatAck};

use crate::config::EnrolledCredentials;

/// HTTP client for communicating with the Ken server.
pub struct KenApiClient {
    server_url: String,
}

impl KenApiClient {
    /// Create a new client from enrolled credentials and server URL.
    pub fn new(_credentials: &EnrolledCredentials, server_url: &str) -> Self {
        Self {
            server_url: server_url.to_string(),
        }
    }

    /// Send a heartbeat to the server and receive an ack with pending commands.
    ///
    /// Phase 1 stub: returns a default ack with no pending commands.
    /// Will POST to `{server_url}/api/v1/heartbeat` when reqwest is added.
    #[allow(clippy::unused_async)] // Will be truly async with reqwest
    pub async fn send_heartbeat(
        &self,
        heartbeat: &Heartbeat,
    ) -> Result<HeartbeatAck, anyhow::Error> {
        tracing::debug!(
            url = %self.server_url,
            endpoint_id = %heartbeat.endpoint_id,
            "sending heartbeat (stub)"
        );

        Ok(HeartbeatAck {
            received_at: time::OffsetDateTime::now_utc(),
            pending_commands: vec![],
            next_heartbeat_interval_seconds: 60,
        })
    }

    /// Report command outcomes to the server.
    ///
    /// Phase 1 stub: logs the count and returns success.
    /// Will POST to `{server_url}/api/v1/command_outcomes` when reqwest is added.
    #[allow(clippy::unused_async)] // Will be truly async with reqwest
    pub async fn report_command_outcomes(
        &self,
        outcomes: &[CommandOutcome],
    ) -> Result<(), anyhow::Error> {
        tracing::debug!(count = outcomes.len(), "reporting command outcomes (stub)");
        Ok(())
    }
}
