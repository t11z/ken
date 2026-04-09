//! Admin web UI routes and handlers.
//!
//! The admin UI runs on the admin listener (default port 8444) and
//! is protected by token-based session authentication.

use axum::extract::{Form, Path, State};
use axum::http::header::SET_COOKIE;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;

use ken_protocol::command::{CommandEnvelope, CommandPayload};
use ken_protocol::ids::{CommandId, EndpointId};

use crate::error::AppError;
use crate::state::AppState;
use crate::storage::EnrollmentToken;

use super::auth::{self, AuthenticatedAdmin, SESSION_COOKIE};

/// Admin routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/login", get(login_page))
        .route("/admin/login", post(login_submit))
        .route("/admin/logout", post(logout))
        .route("/admin", get(dashboard))
        .route("/admin/endpoints/{id}", get(endpoint_detail))
        .route("/admin/enroll", get(enroll_form))
        .route("/admin/enroll", post(enroll_submit))
        .route("/admin/audit", get(audit_log))
        .route("/admin/commands/{endpoint_id}", get(command_form))
        .route("/admin/commands/{endpoint_id}", post(command_submit))
}

// --- Login ---

async fn login_page() -> Html<String> {
    Html(render_login(None))
}

#[derive(Deserialize)]
struct LoginForm {
    token: String,
}

async fn login_submit(
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> Response {
    match auth::verify_token(&state, &form.token).await {
        Ok(true) => {
            match auth::create_session(&state).await {
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
                Err(_) => Html(render_login(Some("Internal error"))).into_response(),
            }
        }
        Ok(false) => Html(render_login(Some("Invalid access token"))).into_response(),
        Err(_) => Html(render_login(Some("Internal error"))).into_response(),
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

// --- Dashboard ---

async fn dashboard(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
) -> Result<Html<String>, AppError> {
    let endpoints = state.storage.list_endpoints().await?;

    let mut rows = String::new();
    for ep in &endpoints {
        let display = ep.display_name.as_deref().unwrap_or(&ep.hostname);
        let last_seen = ep
            .last_heartbeat_at
            .as_deref()
            .unwrap_or("never");
        let status_class = if ep.revoked_at.is_some() {
            "text-red-600"
        } else if ep.last_heartbeat_at.is_some() {
            "text-green-600"
        } else {
            "text-gray-400"
        };

        rows.push_str(&format!(
            r#"<tr class="border-b">
                <td class="px-4 py-3 font-medium">{display}</td>
                <td class="px-4 py-3">{}</td>
                <td class="px-4 py-3">{last_seen}</td>
                <td class="px-4 py-3"><span class="{status_class}">●</span></td>
                <td class="px-4 py-3">
                    <a href="/admin/endpoints/{}" class="text-blue-600 hover:underline">Details</a>
                </td>
            </tr>"#,
            ep.os_version, ep.id
        ));
    }

    let content = format!(
        r#"<h1 class="text-2xl font-bold mb-6">Endpoints</h1>
        <div class="bg-white rounded shadow overflow-hidden">
            <table class="w-full text-sm">
                <thead class="bg-gray-50 text-left text-gray-500 uppercase text-xs">
                    <tr>
                        <th class="px-4 py-3">Name</th>
                        <th class="px-4 py-3">OS</th>
                        <th class="px-4 py-3">Last seen</th>
                        <th class="px-4 py-3">Status</th>
                        <th class="px-4 py-3">Actions</th>
                    </tr>
                </thead>
                <tbody hx-get="/admin" hx-trigger="every 10s" hx-select="tbody" hx-swap="outerHTML">
                    {rows}
                </tbody>
            </table>
        </div>
        <div class="mt-4">
            <a href="/admin/enroll" class="bg-blue-600 text-white px-4 py-2 rounded hover:bg-blue-700">
                Enroll new endpoint
            </a>
        </div>"#
    );

    Ok(Html(render_page("Dashboard", &content)))
}

// --- Endpoint detail ---

async fn endpoint_detail(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
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
    let display_name = endpoint.display_name.as_deref().unwrap_or(&endpoint.hostname);

    let mut content = format!(
        r#"<h1 class="text-2xl font-bold mb-6">{display_name}</h1>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4 mb-6">
            <div class="bg-white rounded shadow p-4">
                <h2 class="font-semibold text-gray-600 mb-2">Identity</h2>
                <dl class="text-sm space-y-1">
                    <div class="flex"><dt class="w-32 text-gray-500">Hostname:</dt><dd>{}</dd></div>
                    <div class="flex"><dt class="w-32 text-gray-500">OS:</dt><dd>{}</dd></div>
                    <div class="flex"><dt class="w-32 text-gray-500">Agent:</dt><dd>v{}</dd></div>
                    <div class="flex"><dt class="w-32 text-gray-500">Enrolled:</dt><dd>{}</dd></div>
                    <div class="flex"><dt class="w-32 text-gray-500">Last seen:</dt><dd>{}</dd></div>
                </dl>
            </div>"#,
        endpoint.hostname,
        endpoint.os_version,
        endpoint.agent_version,
        endpoint.enrolled_at,
        endpoint.last_heartbeat_at.as_deref().unwrap_or("never"),
    );

    // Status snapshot cards
    if let Some(snap) = snapshot {
        if let Some(ref defender) = snap.defender {
            let av_status = if defender.antivirus_enabled {
                r#"<span class="text-green-600">Enabled</span>"#
            } else {
                r#"<span class="text-red-600">Disabled</span>"#
            };
            let rtp_status = if defender.real_time_protection_enabled {
                r#"<span class="text-green-600">On</span>"#
            } else {
                r#"<span class="text-red-600">Off</span>"#
            };

            content.push_str(&format!(
                r#"<div class="bg-white rounded shadow p-4">
                    <h2 class="font-semibold text-gray-600 mb-2">Defender</h2>
                    <dl class="text-sm space-y-1">
                        <div class="flex"><dt class="w-32 text-gray-500">Antivirus:</dt><dd>{av_status}</dd></div>
                        <div class="flex"><dt class="w-32 text-gray-500">Real-time:</dt><dd>{rtp_status}</dd></div>
                        <div class="flex"><dt class="w-32 text-gray-500">Signatures:</dt><dd>{}</dd></div>
                        <div class="flex"><dt class="w-32 text-gray-500">Sig age:</dt><dd>{} days</dd></div>
                    </dl>
                </div>"#,
                defender.signature_version,
                defender.signature_age_days,
            ));
        }

        if let Some(ref wu) = snap.windows_update {
            content.push_str(&format!(
                r#"<div class="bg-white rounded shadow p-4">
                    <h2 class="font-semibold text-gray-600 mb-2">Windows Update</h2>
                    <dl class="text-sm space-y-1">
                        <div class="flex"><dt class="w-32 text-gray-500">Pending:</dt><dd>{}</dd></div>
                        <div class="flex"><dt class="w-32 text-gray-500">Critical:</dt><dd>{}</dd></div>
                    </dl>
                </div>"#,
                wu.pending_update_count,
                wu.pending_critical_update_count,
            ));
        }
    }

    content.push_str(r#"</div>
        <div class="flex gap-2">
            <a href="/admin/commands/"#);
    content.push_str(&id);
    content.push_str(r#"" class="bg-blue-600 text-white px-4 py-2 rounded hover:bg-blue-700">Send command</a>
            <a href="/admin" class="bg-gray-200 text-gray-700 px-4 py-2 rounded hover:bg-gray-300">Back</a>
        </div>"#);

    Ok(Html(render_page(display_name, &content)))
}

// --- Enrollment ---

async fn enroll_form(_admin: AuthenticatedAdmin) -> Html<String> {
    let content = r#"<h1 class="text-2xl font-bold mb-6">Enroll new endpoint</h1>
        <form method="post" action="/admin/enroll" class="bg-white rounded shadow p-6 max-w-md">
            <div class="mb-4">
                <label for="display_name" class="block text-sm font-medium text-gray-700 mb-1">
                    Display name (optional)
                </label>
                <input type="text" name="display_name" id="display_name"
                    class="w-full border rounded px-3 py-2 text-sm"
                    placeholder="e.g., Mom's Laptop">
            </div>
            <button type="submit"
                class="bg-blue-600 text-white px-4 py-2 rounded hover:bg-blue-700">
                Create enrollment URL
            </button>
        </form>"#;

    Html(render_page("Enroll", content))
}

#[derive(Deserialize)]
struct EnrollForm {
    display_name: Option<String>,
}

async fn enroll_submit(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
    Form(form): Form<EnrollForm>,
) -> Result<Html<String>, AppError> {
    let token_value = Uuid::new_v4().to_string();
    let now = OffsetDateTime::now_utc();
    let expires = now + time::Duration::seconds(state.config.enrollment.token_lifetime_seconds as i64);

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

    let content = format!(
        r#"<h1 class="text-2xl font-bold mb-6">Enrollment URL created</h1>
        <div class="bg-white rounded shadow p-6 max-w-lg">
            <p class="text-sm text-gray-600 mb-4">
                Send this URL to the family member. It expires in {lifetime_min} minutes.
            </p>
            <div class="bg-gray-50 border rounded p-3 font-mono text-sm break-all mb-4">
                {enroll_url}
            </div>
            <p class="text-xs text-gray-400">
                Token: {token_value}
            </p>
        </div>
        <div class="mt-4">
            <a href="/admin" class="bg-gray-200 text-gray-700 px-4 py-2 rounded hover:bg-gray-300">Back to dashboard</a>
        </div>"#
    );

    Ok(Html(render_page("Enrollment URL", &content)))
}

// --- Audit log ---

async fn audit_log(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
) -> Result<Html<String>, AppError> {
    let events = state.storage.recent_audit_events(100).await?;

    let mut rows = String::new();
    for event in &events {
        let ep = event.endpoint_id.as_deref().unwrap_or("-");
        rows.push_str(&format!(
            r#"<tr class="border-b text-sm">
                <td class="px-4 py-2">{}</td>
                <td class="px-4 py-2">{}</td>
                <td class="px-4 py-2 font-mono text-xs">{ep}</td>
                <td class="px-4 py-2">{}</td>
                <td class="px-4 py-2">{}</td>
            </tr>"#,
            event.occurred_at, event.source, event.kind, event.message
        ));
    }

    let content = format!(
        r#"<h1 class="text-2xl font-bold mb-6">Audit log</h1>
        <div class="bg-white rounded shadow overflow-hidden">
            <table class="w-full text-sm">
                <thead class="bg-gray-50 text-left text-gray-500 uppercase text-xs">
                    <tr>
                        <th class="px-4 py-3">Time</th>
                        <th class="px-4 py-3">Source</th>
                        <th class="px-4 py-3">Endpoint</th>
                        <th class="px-4 py-3">Kind</th>
                        <th class="px-4 py-3">Message</th>
                    </tr>
                </thead>
                <tbody>{rows}</tbody>
            </table>
        </div>"#
    );

    Ok(Html(render_page("Audit log", &content)))
}

// --- Commands ---

async fn command_form(
    _admin: AuthenticatedAdmin,
    Path(endpoint_id): Path<String>,
) -> Html<String> {
    let content = format!(
        r#"<h1 class="text-2xl font-bold mb-6">Send command</h1>
        <form method="post" action="/admin/commands/{endpoint_id}" class="bg-white rounded shadow p-6 max-w-md">
            <div class="mb-4">
                <label class="block text-sm font-medium text-gray-700 mb-2">Command type</label>
                <div class="space-y-2">
                    <label class="flex items-center">
                        <input type="radio" name="command_type" value="ping" checked class="mr-2">
                        <span>Ping</span>
                    </label>
                    <label class="flex items-center">
                        <input type="radio" name="command_type" value="refresh_status" class="mr-2">
                        <span>Refresh status</span>
                    </label>
                    <label class="flex items-center text-gray-400">
                        <input type="radio" name="command_type" value="remote_session" disabled class="mr-2">
                        <span>Request remote session (Phase 2)</span>
                    </label>
                </div>
            </div>
            <button type="submit"
                class="bg-blue-600 text-white px-4 py-2 rounded hover:bg-blue-700">
                Send
            </button>
        </form>"#
    );

    Html(render_page("Send command", &content))
}

#[derive(Deserialize)]
struct CommandForm {
    command_type: String,
}

async fn command_submit(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
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

    state
        .storage
        .queue_command(&endpoint_id, &envelope)
        .await?;

    // Audit event
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

    let content = format!(
        r#"<h1 class="text-2xl font-bold mb-6">Command sent</h1>
        <div class="bg-white rounded shadow p-6 max-w-md">
            <p class="text-sm">
                <strong>{}</strong> command queued for endpoint {endpoint_id_str}.
            </p>
            <p class="text-xs text-gray-400 mt-2">
                Command ID: {}
            </p>
        </div>
        <div class="mt-4">
            <a href="/admin/endpoints/{endpoint_id_str}" class="bg-gray-200 text-gray-700 px-4 py-2 rounded hover:bg-gray-300">Back to endpoint</a>
        </div>"#,
        form.command_type, envelope.command_id
    );

    Ok(Html(render_page("Command sent", &content)))
}

// --- Template rendering helpers ---

/// Render the login page.
fn render_login(error: Option<&str>) -> String {
    let error_html = error
        .map(|e| format!(r#"<div class="bg-red-50 text-red-700 p-3 rounded mb-4 text-sm">{e}</div>"#))
        .unwrap_or_default();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Ken — Login</title>
    <link rel="stylesheet" href="/static/tailwind.css">
</head>
<body class="bg-gray-100 min-h-screen flex items-center justify-center">
    <div class="bg-white rounded shadow p-8 max-w-sm w-full">
        <h1 class="text-2xl font-bold mb-6 text-center">Ken</h1>
        {error_html}
        <form method="post" action="/admin/login">
            <div class="mb-4">
                <label for="token" class="block text-sm font-medium text-gray-700 mb-1">
                    Access token
                </label>
                <input type="password" name="token" id="token" required autofocus
                    class="w-full border rounded px-3 py-2 text-sm"
                    placeholder="Paste your admin token">
            </div>
            <button type="submit"
                class="w-full bg-blue-600 text-white px-4 py-2 rounded hover:bg-blue-700">
                Log in
            </button>
        </form>
    </div>
</body>
</html>"#
    )
}

/// Render a full page with the shared layout.
fn render_page(title: &str, content: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Ken — {title}</title>
    <link rel="stylesheet" href="/static/tailwind.css">
    <script src="/static/htmx.min.js"></script>
</head>
<body class="bg-gray-100 min-h-screen">
    <nav class="bg-white shadow-sm mb-6">
        <div class="max-w-5xl mx-auto px-4 py-3 flex items-center justify-between">
            <a href="/admin" class="text-xl font-bold text-gray-800">Ken</a>
            <div class="flex gap-4 text-sm">
                <a href="/admin" class="text-gray-600 hover:text-gray-800">Dashboard</a>
                <a href="/admin/enroll" class="text-gray-600 hover:text-gray-800">Enroll</a>
                <a href="/admin/audit" class="text-gray-600 hover:text-gray-800">Audit</a>
                <form method="post" action="/admin/logout" class="inline">
                    <button type="submit" class="text-gray-600 hover:text-gray-800">Logout</button>
                </form>
            </div>
        </div>
    </nav>
    <main class="max-w-5xl mx-auto px-4 pb-8">
        {content}
    </main>
</body>
</html>"#
    )
}

fn format_time(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| t.to_string())
}
