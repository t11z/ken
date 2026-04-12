//! HTTP routing and server setup for the Ken server.
//!
//! The server runs two listeners:
//! - **Agent listener** (default port 8443): mTLS required, serves the
//!   agent API (heartbeats, command outcomes, time).
//! - **Admin listener** (default port 8444): server cert only (no client
//!   cert), serves enrollment, admin web UI, and static assets.
//!
//! Static assets (htmx, Tailwind CSS) are embedded in the binary at
//! compile time via `include_bytes!` so the server binary has no
//! runtime filesystem dependency on an external static directory.

pub mod admin;
pub mod agent_api;
pub mod auth;
pub mod endpoint_id;
pub mod enrollment;
pub mod tls;

use axum::body::Body;
use axum::middleware;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use http::{header, StatusCode};
use tower_http::trace::TraceLayer;

use crate::state::AppState;

static HTMX_JS: &[u8] = include_bytes!("../../static/htmx.min.js");
static TAILWIND_CSS: &[u8] = include_bytes!("../../static/tailwind.css");

async fn serve_htmx() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/javascript; charset=utf-8")
        .body(Body::from(HTMX_JS))
        .unwrap()
}

async fn serve_tailwind() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
        .body(Body::from(TAILWIND_CSS))
        .unwrap()
}

/// Build the router for the agent-facing mTLS API.
///
/// The `require_endpoint_id` middleware is mounted as defense-in-depth:
/// under correct wiring (agent listener served via `KenAcceptor`), the
/// `EndpointId` extension is always present. If it is absent, the
/// middleware returns 500 — see ADR-0017.
pub fn agent_router(state: AppState) -> Router {
    Router::new()
        .merge(agent_api::routes())
        .layer(middleware::from_fn(endpoint_id::require_endpoint_id))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Build the router for the admin-facing listener (enrollment + admin UI + static assets).
///
/// Static assets are served from bytes embedded at compile time; no
/// external static directory is required at runtime.
pub fn admin_router(state: AppState) -> Router {
    Router::new()
        .merge(enrollment::routes())
        .merge(admin::routes())
        .route("/static/htmx.min.js", get(serve_htmx))
        .route("/static/tailwind.css", get(serve_tailwind))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
