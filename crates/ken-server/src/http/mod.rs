//! HTTP routing and server setup for the Ken server.
//!
//! The server runs two listeners:
//! - **Agent listener** (default port 8443): mTLS required, serves the
//!   agent API (heartbeats, command outcomes, time).
//! - **Admin listener** (default port 8444): server cert only (no client
//!   cert), serves enrollment, admin web UI, and static assets.

pub mod admin;
pub mod agent_api;
pub mod auth;
pub mod enrollment;
pub mod tls;

use axum::Router;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::state::AppState;

/// Build the router for the agent-facing mTLS API.
pub fn agent_router(state: AppState) -> Router {
    Router::new()
        .merge(agent_api::routes())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Build the router for the admin-facing listener (enrollment + admin UI + static assets).
pub fn admin_router(state: AppState) -> Router {
    Router::new()
        .merge(enrollment::routes())
        .merge(admin::routes())
        .nest_service("/static", ServeDir::new("crates/ken-server/static"))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
