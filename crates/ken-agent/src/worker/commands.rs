//! Command processing for instructions received from the server.

use ken_protocol::command::{CommandEnvelope, CommandOutcome, CommandPayload, CommandResult};
use time::OffsetDateTime;

use crate::ipc::ConsentOutcome;
use crate::remote_session::{NoOpBackend, RemoteSessionBackend, RemoteSessionError};

/// Process a single command and return its outcome.
///
/// The consent flow for `RequestRemoteSession` is exercised even in
/// Phase 1 — the only difference is that after consent, the no-op
/// backend returns `NotImplementedYet`.
pub fn process(command: &CommandEnvelope) -> CommandOutcome {
    let result = match &command.payload {
        CommandPayload::Ping => {
            tracing::debug!(command_id = %command.command_id, "processing ping");
            CommandResult::Ok
        }
        CommandPayload::RefreshStatus => {
            tracing::debug!(command_id = %command.command_id, "processing refresh_status");
            // The next heartbeat will carry a fresh snapshot
            CommandResult::Ok
        }
        CommandPayload::RequestRemoteSession { reason } => {
            tracing::info!(
                command_id = %command.command_id,
                reason = %reason,
                "processing remote session request"
            );

            // In Phase 1, the consent flow runs but the actual session
            // is not yet implemented. Simulate consent being granted
            // for testing purposes, then let the no-op backend refuse.
            let consent = simulate_consent();

            match consent {
                ConsentOutcome::Granted => {
                    let backend = NoOpBackend;
                    match backend.start_session(&command.command_id) {
                        Ok(_) => CommandResult::Ok,
                        Err(RemoteSessionError::NotImplemented) => CommandResult::NotImplementedYet,
                        Err(e) => CommandResult::Failed {
                            error: e.to_string(),
                        },
                    }
                }
                ConsentOutcome::Denied => CommandResult::Rejected {
                    reason: "user denied consent".to_string(),
                },
                ConsentOutcome::TimedOut => CommandResult::Rejected {
                    reason: "consent request timed out".to_string(),
                },
            }
        }
    };

    CommandOutcome {
        command_id: command.command_id,
        completed_at: OffsetDateTime::now_utc(),
        result,
    }
}

/// Simulate the consent flow for Phase 1.
///
/// In the real implementation, this would send an IPC request to the
/// Tray App and wait for the user's response. In Phase 1, it always
/// returns `Granted` so the `NoOpBackend` path is exercised.
fn simulate_consent() -> ConsentOutcome {
    ConsentOutcome::Granted
}

#[cfg(test)]
mod tests {
    use super::*;
    use ken_protocol::command::CommandPayload;
    use ken_protocol::ids::CommandId;

    fn make_envelope(payload: CommandPayload) -> CommandEnvelope {
        CommandEnvelope {
            command_id: CommandId::new(),
            issued_at: OffsetDateTime::now_utc(),
            expires_at: OffsetDateTime::now_utc() + time::Duration::hours(1),
            payload,
        }
    }

    #[test]
    fn ping_returns_ok() {
        let cmd = make_envelope(CommandPayload::Ping);
        let outcome = process(&cmd);
        assert_eq!(outcome.result, CommandResult::Ok);
    }

    #[test]
    fn refresh_status_returns_ok() {
        let cmd = make_envelope(CommandPayload::RefreshStatus);
        let outcome = process(&cmd);
        assert_eq!(outcome.result, CommandResult::Ok);
    }

    #[test]
    fn remote_session_returns_not_implemented_yet() {
        let cmd = make_envelope(CommandPayload::RequestRemoteSession {
            reason: "testing".to_string(),
        });
        let outcome = process(&cmd);
        assert_eq!(outcome.result, CommandResult::NotImplementedYet);
    }
}
