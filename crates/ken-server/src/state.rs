//! Application state shared across all handlers.

use std::sync::Arc;

use crate::ca::Ca;
use crate::config::Config;
use crate::storage::Storage;

/// Shared application state for the Ken server.
///
/// Cloneable because axum requires `State<T>: Clone`. The internal
/// types are either already cheap to clone (`Storage` wraps an
/// `Arc<SqlitePool>`) or behind `Arc`.
#[derive(Clone)]
pub struct AppState {
    /// Database access layer.
    pub storage: Storage,
    /// The Ken certificate authority for signing client certificates.
    pub ca: Arc<Ca>,
    /// Resolved server configuration.
    pub config: Arc<Config>,
}
