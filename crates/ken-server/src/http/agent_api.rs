//! Agent-facing API endpoints, protected by mTLS.
//!
//! All endpoints here require a valid client certificate. The endpoint
//! ID is extracted from the client certificate's CN by the mTLS
//! middleware and stored in request extensions.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

use ken_protocol::command::CommandOutcome;
use ken_protocol::heartbeat::{Heartbeat, HeartbeatAck};
use ken_protocol::ids::EndpointId;
use ken_protocol::version;

use crate::error::AppError;
use crate::state::AppState;

/// Agent API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/heartbeat", post(heartbeat))
        .route("/api/v1/command_outcomes", post(command_outcomes))
        .route("/api/v1/time", get(server_time))
        .route("/updates/latest.json", get(latest_update))
}

/// Process a heartbeat from an agent.
///
/// `POST /api/v1/heartbeat` — accepts a `Heartbeat` as JSON, returns
/// a `HeartbeatAck` with pending commands.
///
/// The endpoint identity comes from the mTLS client certificate, bridged
/// into the request extensions by `KenAcceptor` and the `AddEndpointId`
/// service wrapper per ADR-0017 and ADR-0016. The handler never reads
/// identity from the request body.
async fn heartbeat(
    Extension(verified_endpoint_id): Extension<EndpointId>,
    State(state): State<AppState>,
    Json(heartbeat): Json<Heartbeat>,
) -> Result<Json<HeartbeatAck>, AppError> {
    let endpoint_id = &verified_endpoint_id;

    // Verify schema version
    if !version::is_compatible(heartbeat.schema_version) {
        return Err(AppError::BadRequest(format!(
            "incompatible schema version: agent sent {}, server expects {}",
            heartbeat.schema_version,
            version::SCHEMA_VERSION
        )));
    }

    // Verify the endpoint exists
    let endpoint = state.storage.get_endpoint(endpoint_id).await?;
    if endpoint.is_none() {
        return Err(AppError::Forbidden(format!(
            "unknown endpoint: {endpoint_id}"
        )));
    }

    let now = OffsetDateTime::now_utc();

    // Record heartbeat
    state
        .storage
        .record_heartbeat(endpoint_id, &heartbeat, now)
        .await?;

    // Upsert status snapshot
    state
        .storage
        .upsert_status_snapshot(endpoint_id, &heartbeat.status)
        .await?;

    // Store audit events from the heartbeat's audit_tail
    for event in &heartbeat.audit_tail {
        let kind_str = serde_json::to_string(&event.kind).unwrap_or_default();
        state
            .storage
            .append_audit_event(
                &event.event_id.to_string(),
                &format_time(event.occurred_at),
                &kind_str,
                &event.message,
                "agent",
                Some(&endpoint_id.to_string()),
            )
            .await
            .ok(); // Best-effort: don't fail the heartbeat for audit log issues
    }

    // Fetch pending commands
    let pending_commands = state.storage.pending_commands_for(endpoint_id).await?;

    // Mark commands as delivered
    for cmd in &pending_commands {
        state
            .storage
            .mark_command_delivered(&cmd.command_id, now)
            .await?;
    }

    tracing::debug!(
        endpoint_id = %endpoint_id,
        pending_commands = pending_commands.len(),
        "heartbeat processed"
    );

    Ok(Json(HeartbeatAck {
        received_at: now,
        pending_commands,
        next_heartbeat_interval_seconds: 60,
    }))
}

/// Report command outcomes from the agent.
///
/// `POST /api/v1/command_outcomes` — accepts a `Vec<CommandOutcome>`,
/// returns 204 No Content.
///
/// The endpoint identity comes from the mTLS client certificate via
/// `Extension<EndpointId>` (ADR-0017, ADR-0016). Outcomes are recorded
/// as belonging to the verified endpoint.
async fn command_outcomes(
    Extension(verified_endpoint_id): Extension<EndpointId>,
    State(state): State<AppState>,
    Json(outcomes): Json<Vec<CommandOutcome>>,
) -> Result<StatusCode, AppError> {
    for outcome in &outcomes {
        state.storage.record_command_outcome(outcome).await?;

        let kind_str = serde_json::to_string(&outcome.result).unwrap_or_default();
        state
            .storage
            .append_audit_event(
                &Uuid::new_v4().to_string(),
                &format_time(outcome.completed_at),
                &format!("command_completed_{}", outcome.command_id),
                &kind_str,
                "agent",
                Some(&verified_endpoint_id.to_string()),
            )
            .await
            .ok();
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Server time endpoint for clock skew detection.
///
/// `GET /api/v1/time` — returns `{"now": "RFC3339 timestamp"}`.
async fn server_time() -> Json<ServerTimeResponse> {
    Json(ServerTimeResponse {
        now: format_time(OffsetDateTime::now_utc()),
    })
}

/// Response for the `/api/v1/time` endpoint.
#[derive(Serialize)]
struct ServerTimeResponse {
    now: String,
}

/// Update check endpoint per ADR-0011.
///
/// `GET /updates/latest.json` — returns the latest available agent version.
/// Phase 1 stub: always returns version "0.0.0" meaning no update available.
/// Real MSI builds and signing are Phase 2 work.
async fn latest_update() -> Json<LatestUpdateResponse> {
    Json(LatestUpdateResponse {
        version: "0.0.0".to_string(),
    })
}

/// Response for the `/updates/latest.json` endpoint.
#[derive(Serialize)]
struct LatestUpdateResponse {
    version: String,
}

fn format_time(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| t.to_string())
}
