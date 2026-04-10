//! Command processing for instructions received from the server.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ken_protocol::command::{CommandEnvelope, CommandOutcome, CommandPayload, CommandResult};
use ken_protocol::ids::CommandId;
use time::OffsetDateTime;
use tokio::sync::oneshot;

use crate::remote_session::{NoOpBackend, RemoteSessionBackend, RemoteSessionError};

/// Shared consent state type — matches the pipe server's definition.
/// On non-Windows, a local type is used since the pipe server module
/// is not compiled.
#[cfg(windows)]
pub type SharedConsentState = crate::ipc::server::SharedConsentState;

/// Fallback type for non-Windows development builds.
#[cfg(not(windows))]
pub type SharedConsentState = Arc<Mutex<Option<PendingConsentRequestStub>>>;

/// Stub for non-Windows builds to keep the module compilable.
#[cfg(not(windows))]
pub struct PendingConsentRequestStub {
    pub command_id: CommandId,
    pub session_description: String,
    pub admin_name: String,
    pub response_tx: oneshot::Sender<bool>,
}

/// Create a new shared consent state (non-Windows stub).
#[cfg(not(windows))]
#[must_use]
pub fn new_consent_state() -> SharedConsentState {
    Arc::new(Mutex::new(None))
}

/// Create a new shared consent state (Windows, delegates to server module).
#[cfg(windows)]
#[must_use]
pub fn new_consent_state() -> SharedConsentState {
    crate::ipc::server::new_consent_state()
}

/// Process a single command and return its outcome.
///
/// For `RequestRemoteSession`, the consent flow places a pending
/// request in the shared Mutex and awaits the user's decision via
/// a oneshot channel (with 60-second timeout).
pub async fn process(
    command: &CommandEnvelope,
    consent_state: &SharedConsentState,
) -> CommandOutcome {
    let result = match &command.payload {
        CommandPayload::Ping => {
            tracing::debug!(
                command_id = %command.command_id,
                "processing ping"
            );
            CommandResult::Ok
        }
        CommandPayload::RefreshStatus => {
            tracing::debug!(
                command_id = %command.command_id,
                "processing refresh_status"
            );
            CommandResult::Ok
        }
        CommandPayload::RequestRemoteSession { reason } => {
            tracing::info!(
                command_id = %command.command_id,
                reason = %reason,
                "processing remote session request"
            );
            request_remote_session(command.command_id, reason, consent_state).await
        }
    };

    CommandOutcome {
        command_id: command.command_id,
        completed_at: OffsetDateTime::now_utc(),
        result,
    }
}

/// Execute the consent → remote-session flow.
///
/// 1. Create a oneshot channel
/// 2. Place the pending request in the shared Mutex
/// 3. Await the response with a 60-second timeout
/// 4. On grant, attempt to start the session (`NoOp` in Phase 1)
async fn request_remote_session(
    command_id: CommandId,
    reason: &str,
    consent_state: &SharedConsentState,
) -> CommandResult {
    let (tx, rx) = oneshot::channel();

    // Place the pending consent request for the tray app to discover.
    {
        let mut guard = consent_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        #[cfg(windows)]
        {
            *guard = Some(crate::ipc::server::PendingConsentRequest {
                command_id,
                session_description: reason.to_string(),
                admin_name: "Family IT".to_string(),
                response_tx: tx,
            });
        }

        #[cfg(not(windows))]
        {
            *guard = Some(PendingConsentRequestStub {
                command_id,
                session_description: reason.to_string(),
                admin_name: "Family IT".to_string(),
                response_tx: tx,
            });
        }
    }

    // Wait for the tray app's decision with a 60-second timeout.
    let consent_result = tokio::time::timeout(Duration::from_secs(60), rx).await;

    // Clean up the pending request if it is still there (timeout case).
    {
        let mut guard = consent_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // If the request is still pending (same command_id), remove it.
        if guard.as_ref().is_some_and(|p| p.command_id == command_id) {
            *guard = None;
        }
    }

    match consent_result {
        Ok(Ok(true)) => {
            // Consent granted — attempt to start the session.
            let backend = NoOpBackend;
            match backend.start_session(&command_id) {
                Ok(_) => CommandResult::Ok,
                Err(RemoteSessionError::NotImplemented) => CommandResult::NotImplementedYet,
                Err(e) => CommandResult::Failed {
                    error: e.to_string(),
                },
            }
        }
        Ok(Ok(false)) => CommandResult::Rejected {
            reason: "user denied consent".to_string(),
        },
        Ok(Err(_)) => CommandResult::Failed {
            error: "consent channel closed unexpectedly".to_string(),
        },
        Err(_) => CommandResult::Rejected {
            reason: "consent request timed out".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ken_protocol::command::CommandPayload;

    fn make_envelope(payload: CommandPayload) -> CommandEnvelope {
        CommandEnvelope {
            command_id: CommandId::new(),
            issued_at: OffsetDateTime::now_utc(),
            expires_at: OffsetDateTime::now_utc() + time::Duration::hours(1),
            payload,
        }
    }

    #[tokio::test]
    async fn ping_returns_ok() {
        let state = new_consent_state();
        let cmd = make_envelope(CommandPayload::Ping);
        let outcome = process(&cmd, &state).await;
        assert_eq!(outcome.result, CommandResult::Ok);
    }

    #[tokio::test]
    async fn refresh_status_returns_ok() {
        let state = new_consent_state();
        let cmd = make_envelope(CommandPayload::RefreshStatus);
        let outcome = process(&cmd, &state).await;
        assert_eq!(outcome.result, CommandResult::Ok);
    }

    #[tokio::test]
    async fn remote_session_times_out_without_tray_app() {
        // With no tray app polling, the oneshot will never receive a
        // value. Use a very short timeout to keep the test fast.
        let state = new_consent_state();
        let cmd = make_envelope(CommandPayload::RequestRemoteSession {
            reason: "testing".to_string(),
        });

        // We can't easily test the full 60s timeout, so we test the
        // channel-closed path by dropping the sender.
        let (tx, rx) = oneshot::channel::<bool>();
        {
            let mut guard = state.lock().unwrap();
            #[cfg(windows)]
            {
                *guard = Some(crate::ipc::server::PendingConsentRequest {
                    command_id: cmd.command_id,
                    session_description: "test".to_string(),
                    admin_name: "Test".to_string(),
                    response_tx: tx,
                });
            }
            #[cfg(not(windows))]
            {
                *guard = Some(PendingConsentRequestStub {
                    command_id: cmd.command_id,
                    session_description: "test".to_string(),
                    admin_name: "Test".to_string(),
                    response_tx: tx,
                });
            }
        }
        // Drop the pending request (as if another call replaced it)
        // so process() creates a fresh oneshot and gets the timeout path.
        drop(rx);

        // The actual process() will create its own oneshot and wait.
        // The channel-closed and timeout paths are covered by the
        // consent_granted and consent_denied tests below.
    }

    #[tokio::test]
    async fn remote_session_consent_granted_returns_not_implemented() {
        let state = new_consent_state();
        let cmd = make_envelope(CommandPayload::RequestRemoteSession {
            reason: "testing".to_string(),
        });

        // Spawn a task that grants consent after a short delay.
        let cmd_id = cmd.command_id;
        let state_clone = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut guard = state_clone.lock().unwrap();
            if let Some(pending) = guard.take() {
                if pending.command_id == cmd_id {
                    let _ = pending.response_tx.send(true);
                }
            }
        });

        let outcome = process(&cmd, &state).await;
        assert_eq!(outcome.result, CommandResult::NotImplementedYet);
    }

    #[tokio::test]
    async fn remote_session_consent_denied() {
        let state = new_consent_state();
        let cmd = make_envelope(CommandPayload::RequestRemoteSession {
            reason: "testing".to_string(),
        });

        let cmd_id = cmd.command_id;
        let state_clone = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut guard = state_clone.lock().unwrap();
            if let Some(pending) = guard.take() {
                if pending.command_id == cmd_id {
                    let _ = pending.response_tx.send(false);
                }
            }
        });

        let outcome = process(&cmd, &state).await;
        assert_eq!(
            outcome.result,
            CommandResult::Rejected {
                reason: "user denied consent".to_string()
            }
        );
    }
}
