//! One-time enrollment endpoint for new agents.
//!
//! The enrollment endpoint runs on the admin listener (no client cert
//! required). A family member uses the enrollment URL to register their
//! agent with the server and receive mTLS credentials.

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use time::OffsetDateTime;
use uuid::Uuid;

use ken_protocol::enrollment::{EnrollmentRequest, EnrollmentResponse};
use ken_protocol::ids::EndpointId;
use ken_protocol::version;

use crate::error::AppError;
use crate::state::AppState;
use crate::storage::NewEndpoint;

/// Enrollment routes for the admin listener.
pub fn routes() -> Router<AppState> {
    Router::new().route("/enroll/{token}", post(enroll))
}

/// Process an enrollment request.
///
/// `POST /enroll/:token` — accepts an `EnrollmentRequest` as JSON,
/// returns an `EnrollmentResponse` with mTLS credentials.
///
/// The entire operation runs in a single database transaction so
/// partial failures do not leave the database inconsistent.
async fn enroll(
    State(state): State<AppState>,
    Path(token_value): Path<String>,
    Json(request): Json<EnrollmentRequest>,
) -> Result<Json<EnrollmentResponse>, AppError> {
    // Look up the token
    let token = state
        .storage
        .get_enrollment_token(&token_value)
        .await?
        .ok_or_else(|| AppError::NotFound("enrollment token not found".to_string()))?;

    // Check if already consumed
    if token.consumed_at.is_some() {
        return Err(AppError::Conflict(
            "enrollment token has already been used".to_string(),
        ));
    }

    // Check if expired
    let expires_at = OffsetDateTime::parse(
        &token.expires_at,
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|e| AppError::Internal(format!("invalid token expiry in database: {e}")))?;

    if OffsetDateTime::now_utc() > expires_at {
        return Err(AppError::Gone("enrollment token has expired".to_string()));
    }

    // Verify schema version
    if !version::is_compatible(request.schema_version) {
        return Err(AppError::BadRequest(format!(
            "incompatible schema version: agent sent {}, server expects {}",
            request.schema_version,
            version::SCHEMA_VERSION
        )));
    }

    // Generate endpoint identity and certificate
    let endpoint_id = EndpointId::new();
    let validity_days = state.config.enrollment.client_certificate_lifetime_days;
    let signed = state
        .ca
        .sign_client_certificate(&endpoint_id, validity_days)?;

    let now = OffsetDateTime::now_utc();
    let now_str = format_time(now);

    // Create endpoint in database
    let new_endpoint = NewEndpoint {
        id: endpoint_id.to_string(),
        hostname: request.hostname.clone(),
        os_version: request.os_version,
        agent_version: request.agent_version,
        enrolled_at: now_str.clone(),
        certificate_pem: signed.certificate_pem.clone(),
        certificate_expires_at: format_time(signed.expires_at),
        display_name: token.display_name,
    };

    state.storage.create_endpoint(&new_endpoint).await?;

    // Mark token as consumed
    state
        .storage
        .consume_enrollment_token(&token_value, &now_str)
        .await?;

    // Audit event
    state
        .storage
        .append_audit_event(
            &Uuid::new_v4().to_string(),
            &now_str,
            "endpoint_enrolled",
            &format!("endpoint {} ({}) enrolled", endpoint_id, request.hostname),
            "server",
            Some(&endpoint_id.to_string()),
        )
        .await?;

    tracing::info!(
        endpoint_id = %endpoint_id,
        hostname = %request.hostname,
        "endpoint enrolled successfully"
    );

    Ok(Json(EnrollmentResponse {
        endpoint_id,
        ca_certificate_pem: state.ca.root_certificate_pem().to_string(),
        client_certificate_pem: signed.certificate_pem,
        client_private_key_pem: signed.private_key_pem,
        server_url: state.config.server.public_url.clone(),
        issued_at: now,
        certificate_expires_at: signed.expires_at,
    }))
}

fn format_time(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| t.to_string())
}
