//! Application error type for the Ken server.
//!
//! All handler errors flow through [`AppError`], which implements
//! `IntoResponse` so handlers can use `?` freely. User-visible
//! responses are sanitized; full details are logged via `tracing`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Crate-level error type encompassing all failure modes.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Database operation failed.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    /// Filesystem or IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// TLS or certificate error.
    #[error("TLS error: {0}")]
    Tls(String),

    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(String),

    /// Requested resource not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// Client sent invalid data.
    #[error("bad request: {0}")]
    BadRequest(String),

    /// Client is not authorized for this action.
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// Resource conflict (e.g., token already consumed).
    #[error("conflict: {0}")]
    Conflict(String),

    /// Resource has expired (e.g., enrollment token).
    #[error("gone: {0}")]
    Gone(String),

    /// Internal error that does not fit other categories.
    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, user_message) = match &self {
            Self::Database(e) => {
                tracing::error!(error = %e, "database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "an internal error occurred".to_string(),
                )
            }
            Self::Io(e) => {
                tracing::error!(error = %e, "IO error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "an internal error occurred".to_string(),
                )
            }
            Self::Tls(msg) => {
                tracing::error!(error = %msg, "TLS error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "an internal error occurred".to_string(),
                )
            }
            Self::Config(msg) => {
                tracing::error!(error = %msg, "configuration error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "server misconfigured".to_string(),
                )
            }
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            Self::Forbidden(msg) => {
                tracing::warn!(reason = %msg, "forbidden request");
                (StatusCode::FORBIDDEN, msg.clone())
            }
            Self::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            Self::Gone(msg) => (StatusCode::GONE, msg.clone()),
            Self::Internal(msg) => {
                tracing::error!(error = %msg, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "an internal error occurred".to_string(),
                )
            }
        };

        (status, user_message).into_response()
    }
}
