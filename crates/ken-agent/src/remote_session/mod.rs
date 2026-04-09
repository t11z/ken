//! Backend interface for the Phase 2 remote session subsystem.
//!
//! Phase 1 provides only a [`NoOpBackend`] that refuses every session
//! request with a clear "not yet implemented" result. Phase 2 introduces
//! a real implementation built on the `RustDesk` crates. The trait is
//! defined now so that:
//!
//! 1. The wire protocol is stable (the server can send
//!    `RequestRemoteSession` commands today, even if they are refused)
//! 2. The consent flow is exercised end-to-end in Phase 1, so any bugs
//!    in it are found before real sessions depend on it
//! 3. Adding the real backend in Phase 2 is an additive change, not a
//!    restructuring
//!
//! The ADR that will govern the Phase 2 backend is not yet written;
//! when it lands, it will be referenced here.

use ken_protocol::ids::{CommandId, SessionId};

/// Backend interface for managing remote sessions.
///
/// Implementors handle all session lifecycle concerns: signaling,
/// relay, codec, input, teardown. The caller has already obtained
/// explicit user consent before calling `start_session`.
pub trait RemoteSessionBackend: Send + Sync {
    /// Start a new remote session after consent has been granted.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be started.
    fn start_session(&self, command_id: &CommandId) -> Result<SessionId, RemoteSessionError>;

    /// Stop an active session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be stopped.
    fn stop_session(&self, session_id: &SessionId) -> Result<(), RemoteSessionError>;
}

/// Errors from the remote session backend.
#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum RemoteSessionError {
    /// The backend is not yet implemented (Phase 2 feature).
    #[error("remote session backend is not yet implemented (Phase 2 feature)")]
    NotImplemented,

    /// A backend-specific error occurred.
    #[error("session backend error: {0}")]
    Backend(String),
}

/// Phase 1 no-op backend that refuses all session requests.
pub struct NoOpBackend;

impl RemoteSessionBackend for NoOpBackend {
    fn start_session(&self, _command_id: &CommandId) -> Result<SessionId, RemoteSessionError> {
        Err(RemoteSessionError::NotImplemented)
    }

    fn stop_session(&self, _session_id: &SessionId) -> Result<(), RemoteSessionError> {
        Err(RemoteSessionError::NotImplemented)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_backend_refuses_start() {
        let backend = NoOpBackend;
        let result = backend.start_session(&CommandId::new());
        assert!(result.is_err());
        match result.unwrap_err() {
            RemoteSessionError::NotImplemented => {}
            other @ RemoteSessionError::Backend(_) => {
                panic!("expected NotImplemented, got: {other}")
            }
        }
    }

    #[test]
    fn noop_backend_refuses_stop() {
        let backend = NoOpBackend;
        let result = backend.stop_session(&SessionId::new());
        assert!(result.is_err());
    }
}
