//! Admin web UI routes and handlers.
//!
//! The admin UI runs on the admin listener (default port 8444) and is
//! protected by session authentication per ADR-0024. All HTML is rendered via
//! askama templates per ADR-0013.
//!
//! ADR-0024 introduces a two-stage authentication model. Sessions created from
//! a bootstrap login carry `stage = "bootstrap"` and may only access the
//! set-password handler. All other handlers require a `stage = "full"` session
//! via the [`AuthenticatedFullAdmin`] extractor.

use askama::Template;
use axum::extract::{Form, Path, State};
use axum::http::header::SET_COOKIE;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;

use ken_protocol::command::{CommandEnvelope, CommandPayload};
use ken_protocol::ids::{CommandId, EndpointId};
use ken_protocol::status::OsStatusSnapshot;

use crate::error::AppError;
use crate::state::AppState;
use crate::storage::{Endpoint, EnrollmentToken, StoredAuditEvent};

use super::auth::{self, AuthenticatedAdmin, AuthenticatedFullAdmin, LoginResult, SESSION_COOKIE};

/// Admin routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/login", get(login_page))
        .route("/admin/login", post(login_submit))
        .route("/admin/logout", post(logout))
        .route("/admin/set-password", get(set_password_page))
        .route("/admin/set-password", post(set_password_submit))
        .route("/admin", get(dashboard))
        .route("/admin/endpoints/partial", get(endpoints_partial))
        .route("/admin/endpoints/{id}", get(endpoint_detail))
        .route("/admin/enroll", get(enroll_form))
        .route("/admin/enroll", post(enroll_submit))
        .route("/admin/audit", get(audit_log))
        .route("/admin/commands/{endpoint_id}", get(command_form))
        .route("/admin/commands/{endpoint_id}", post(command_submit))
}

// --- Template structs ---

/// Login page template (standalone, does not extend base).
#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    error: Option<String>,
}

/// Set-password page template (ADR-0024 Stage 1 → Stage 2 transition).
#[derive(Template)]
#[template(path = "set_password.html")]
struct SetPasswordTemplate {
    csrf_token: String,
    error: Option<String>,
}

/// Dashboard page showing all endpoints.
#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    csrf_token: String,
    endpoints: Vec<Endpoint>,
}

/// Partial template for htmx endpoint table body refresh.
#[derive(Template)]
#[template(path = "_endpoint_row.html")]
struct EndpointRowTemplate<'a> {
    endpoint: &'a Endpoint,
}

/// Endpoint detail page with pre-extracted snapshot fields.
///
/// Per ADR-0019, subsystems are no longer `Option` at the snapshot level.
/// When no snapshot exists yet, a default all-`Unobserved` snapshot is used.
#[derive(Template)]
#[template(path = "endpoint_detail.html")]
struct EndpointDetailTemplate {
    csrf_token: String,
    display_name: String,
    endpoint_id: String,
    hostname: String,
    os_version: String,
    agent_version: String,
    enrolled_at: String,
    last_seen: String,
    snapshot: Option<OsStatusSnapshot>,
}

/// Data for a generated enrollment URL.
struct GeneratedEnrollment {
    url: String,
    token: String,
    lifetime_minutes: u64,
}

/// Enrollment page (form or generated URL).
#[derive(Template)]
#[template(path = "enroll.html")]
struct EnrollTemplate {
    csrf_token: String,
    enrollment: Option<GeneratedEnrollment>,
}

/// Audit log page.
#[derive(Template)]
#[template(path = "audit.html")]
struct AuditTemplate {
    csrf_token: String,
    events: Vec<StoredAuditEvent>,
}

/// Command form page.
#[derive(Template)]
#[template(path = "commands.html")]
struct CommandFormTemplate {
    csrf_token: String,
    endpoint_id: String,
}

/// Command sent confirmation page.
#[derive(Template)]
#[template(path = "command_sent.html")]
struct CommandSentTemplate {
    csrf_token: String,
    command_type: String,
    endpoint_id: String,
    command_id: String,
}

// --- Handlers ---

async fn login_page() -> Result<Html<String>, AppError> {
    let template = LoginTemplate { error: None };
    Ok(Html(template.render().map_err(|e| {
        AppError::Internal(format!("template render error: {e}"))
    })?))
}

#[derive(Deserialize)]
struct LoginForm {
    password: String,
}

async fn login_submit(State(state): State<AppState>, Form(form): Form<LoginForm>) -> Response {
    let result = match auth::verify_login(&state.storage, &form.password).await {
        Ok(r) => r,
        Err(_) => return render_login_error("Internal error"),
    };

    match result {
        LoginResult::Invalid => render_login_error("Invalid password"),
        LoginResult::BootstrapAccepted => {
            match auth::create_session(&state.storage, "bootstrap").await {
                Ok((session_id, _csrf)) => {
                    let cookie = format!(
                        "{SESSION_COOKIE}={session_id}; HttpOnly; SameSite=Strict; Path=/"
                    );
                    let mut response = Redirect::to("/admin/set-password").into_response();
                    response
                        .headers_mut()
                        .insert(SET_COOKIE, cookie.parse().unwrap());
                    response
                }
                Err(_) => render_login_error("Internal error"),
            }
        }
        LoginResult::UserAccepted => {
            match auth::create_session(&state.storage, "full").await {
                Ok((session_id, _csrf)) => {
                    let cookie = format!(
                        "{SESSION_COOKIE}={session_id}; HttpOnly; SameSite=Strict; Path=/"
                    );
                    let mut response = Redirect::to("/admin").into_response();
                    response
                        .headers_mut()
                        .insert(SET_COOKIE, cookie.parse().unwrap());
                    response
                }
                Err(_) => render_login_error("Internal error"),
            }
        }
    }
}

/// Render the login page with an error message.
fn render_login_error(msg: &str) -> Response {
    let template = LoginTemplate {
        error: Some(msg.to_string()),
    };
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "template error",
        )
            .into_response(),
    }
}

async fn logout(State(state): State<AppState>, admin: AuthenticatedAdmin) -> Response {
    let _ = state.storage.delete_admin_session(&admin.session_id).await;
    let cookie = format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0");
    let mut response = Redirect::to("/admin/login").into_response();
    response
        .headers_mut()
        .insert(SET_COOKIE, cookie.parse().unwrap());
    response
}

/// GET /admin/set-password — ADR-0024 Stage 1 → Stage 2 transition.
///
/// Accessible to both bootstrap and full-stage sessions so that the page
/// renders correctly after a bootstrap login.
async fn set_password_page(admin: AuthenticatedAdmin) -> Result<Html<String>, AppError> {
    let template = SetPasswordTemplate {
        csrf_token: admin.csrf_token,
        error: None,
    };
    Ok(Html(template.render().map_err(|e| {
        AppError::Internal(format!("template render error: {e}"))
    })?))
}

#[derive(Deserialize)]
struct SetPasswordForm {
    new_password: String,
    confirm_password: String,
    csrf_token: String,
}

/// POST /admin/set-password — commit the permanent password (ADR-0024).
///
/// Validates the form, stores `admin_user_password_hash`, deletes
/// `admin_bootstrap_password_hash`, invalidates all sessions, then redirects
/// to `/admin/login` so the admin logs in with the new password.
async fn set_password_submit(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Form(form): Form<SetPasswordForm>,
) -> Response {
    if form.csrf_token != admin.csrf_token {
        return render_set_password_error("Invalid request", &admin.csrf_token);
    }

    if form.new_password != form.confirm_password {
        return render_set_password_error("Passwords do not match", &admin.csrf_token);
    }

    if form.new_password.len() < 12 {
        return render_set_password_error(
            "Password must be at least 12 characters",
            &admin.csrf_token,
        );
    }

    let hash = match auth::hash_password(&form.new_password) {
        Ok(h) => h,
        Err(_) => return render_set_password_error("Internal error", &admin.csrf_token),
    };

    // Store the user password hash first — this is the commit point.
    if state
        .storage
        .set_admin_secret(auth::USER_HASH_KEY, &hash)
        .await
        .is_err()
    {
        return render_set_password_error("Internal error", &admin.csrf_token);
    }

    // Delete the bootstrap hash and all sessions.  Failures here are
    // non-fatal: the user hash is already set, so verify_login will
    // only check it regardless.
    let _ = state
        .storage
        .delete_admin_secret(auth::BOOTSTRAP_HASH_KEY)
        .await;
    let _ = state.storage.delete_all_admin_sessions().await;

    Redirect::to("/admin/login").into_response()
}

/// Render the set-password page with an error message.
fn render_set_password_error(msg: &str, csrf_token: &str) -> Response {
    let template = SetPasswordTemplate {
        csrf_token: csrf_token.to_string(),
        error: Some(msg.to_string()),
    };
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "template error",
        )
            .into_response(),
    }
}

async fn dashboard(
    State(state): State<AppState>,
    admin: AuthenticatedFullAdmin,
) -> Result<Html<String>, AppError> {
    let endpoints = state.storage.list_endpoints().await?;

    let template = DashboardTemplate {
        csrf_token: admin.csrf_token,
        endpoints,
    };

    Ok(Html(template.render().map_err(|e| {
        AppError::Internal(format!("template render error: {e}"))
    })?))
}

/// htmx partial: returns just the endpoint rows for polling refresh.
async fn endpoints_partial(
    State(state): State<AppState>,
    _admin: AuthenticatedFullAdmin,
) -> Result<Html<String>, AppError> {
    let endpoints = state.storage.list_endpoints().await?;

    let mut html = String::new();
    for endpoint in &endpoints {
        let row = EndpointRowTemplate { endpoint };
        html.push_str(
            &row.render()
                .map_err(|e| AppError::Internal(format!("template render error: {e}")))?,
        );
    }

    Ok(Html(html))
}

async fn endpoint_detail(
    State(state): State<AppState>,
    admin: AuthenticatedFullAdmin,
    Path(id): Path<String>,
) -> Result<Html<String>, AppError> {
    let endpoint_id =
        EndpointId::parse(&id).map_err(|_| AppError::BadRequest("invalid endpoint ID".into()))?;

    let endpoint = state
        .storage
        .get_endpoint(&endpoint_id)
        .await?
        .ok_or_else(|| AppError::NotFound("endpoint not found".into()))?;

    let snapshot = state.storage.get_status_snapshot(&endpoint_id).await?;
    let display_name = endpoint
        .display_name
        .as_deref()
        .unwrap_or(&endpoint.hostname)
        .to_string();
    let last_seen = endpoint
        .last_heartbeat_at
        .as_deref()
        .unwrap_or("never")
        .to_string();

    let template = EndpointDetailTemplate {
        csrf_token: admin.csrf_token,
        display_name,
        endpoint_id: endpoint.id.clone(),
        hostname: endpoint.hostname,
        os_version: endpoint.os_version,
        agent_version: endpoint.agent_version,
        enrolled_at: endpoint.enrolled_at,
        last_seen,
        snapshot,
    };

    Ok(Html(template.render().map_err(|e| {
        AppError::Internal(format!("template render error: {e}"))
    })?))
}

async fn enroll_form(admin: AuthenticatedFullAdmin) -> Result<Html<String>, AppError> {
    let template = EnrollTemplate {
        csrf_token: admin.csrf_token,
        enrollment: None,
    };

    Ok(Html(template.render().map_err(|e| {
        AppError::Internal(format!("template render error: {e}"))
    })?))
}

#[derive(Deserialize)]
struct EnrollForm {
    display_name: Option<String>,
}

async fn enroll_submit(
    State(state): State<AppState>,
    admin: AuthenticatedFullAdmin,
    Form(form): Form<EnrollForm>,
) -> Result<Html<String>, AppError> {
    let token_value = Uuid::new_v4().to_string();
    let now = OffsetDateTime::now_utc();
    let lifetime_secs =
        i64::try_from(state.config.enrollment.token_lifetime_seconds).unwrap_or(i64::MAX);
    let expires = now + time::Duration::seconds(lifetime_secs);

    let token = EnrollmentToken {
        token: token_value.clone(),
        created_at: format_time(now),
        expires_at: format_time(expires),
        consumed_at: None,
        display_name: form.display_name.filter(|s| !s.trim().is_empty()),
    };

    state.storage.create_enrollment_token(&token).await?;

    let enroll_url = format!(
        "{}/enroll/{}",
        state.config.server.admin_listen_address, token_value
    );
    let lifetime_min = state.config.enrollment.token_lifetime_seconds / 60;

    let template = EnrollTemplate {
        csrf_token: admin.csrf_token,
        enrollment: Some(GeneratedEnrollment {
            url: enroll_url,
            token: token_value,
            lifetime_minutes: lifetime_min,
        }),
    };

    Ok(Html(template.render().map_err(|e| {
        AppError::Internal(format!("template render error: {e}"))
    })?))
}

async fn audit_log(
    State(state): State<AppState>,
    admin: AuthenticatedFullAdmin,
) -> Result<Html<String>, AppError> {
    let events = state.storage.recent_audit_events(100).await?;

    let template = AuditTemplate {
        csrf_token: admin.csrf_token,
        events,
    };

    Ok(Html(template.render().map_err(|e| {
        AppError::Internal(format!("template render error: {e}"))
    })?))
}

async fn command_form(
    admin: AuthenticatedFullAdmin,
    Path(endpoint_id): Path<String>,
) -> Result<Html<String>, AppError> {
    let template = CommandFormTemplate {
        csrf_token: admin.csrf_token,
        endpoint_id,
    };

    Ok(Html(template.render().map_err(|e| {
        AppError::Internal(format!("template render error: {e}"))
    })?))
}

#[derive(Deserialize)]
struct CommandForm {
    command_type: String,
}

async fn command_submit(
    State(state): State<AppState>,
    admin: AuthenticatedFullAdmin,
    Path(endpoint_id_str): Path<String>,
    Form(form): Form<CommandForm>,
) -> Result<Html<String>, AppError> {
    let endpoint_id = EndpointId::parse(&endpoint_id_str)
        .map_err(|_| AppError::BadRequest("invalid endpoint ID".into()))?;

    let payload = match form.command_type.as_str() {
        "ping" => CommandPayload::Ping,
        "refresh_status" => CommandPayload::RefreshStatus,
        _ => return Err(AppError::BadRequest("unknown command type".into())),
    };

    let now = OffsetDateTime::now_utc();
    let envelope = CommandEnvelope {
        command_id: CommandId::new(),
        issued_at: now,
        expires_at: now + time::Duration::hours(1),
        payload,
    };

    state.storage.queue_command(&endpoint_id, &envelope).await?;

    state
        .storage
        .append_audit_event(
            &Uuid::new_v4().to_string(),
            &format_time(now),
            &format!("command_issued_{}", form.command_type),
            &format!("command {} sent to {}", envelope.command_id, endpoint_id),
            "server",
            Some(&endpoint_id_str),
        )
        .await?;

    let template = CommandSentTemplate {
        csrf_token: admin.csrf_token,
        command_type: form.command_type,
        endpoint_id: endpoint_id_str,
        command_id: envelope.command_id.to_string(),
    };

    Ok(Html(template.render().map_err(|e| {
        AppError::Internal(format!("template render error: {e}"))
    })?))
}

fn format_time(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| t.to_string())
}
