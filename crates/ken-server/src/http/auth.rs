//! Admin session handling and authentication.
//!
//! Phase 1 uses a simple token-based auth: on first startup, a random
//! token is generated and displayed once in the logs. The admin logs
//! in with this token and receives a session cookie.

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::state::AppState;
use crate::storage::AdminSession;

/// Cookie name for the admin session.
pub const SESSION_COOKIE: &str = "ken_session";

/// Key used to store the admin token hash in the admin_secrets table.
pub const ADMIN_TOKEN_KEY: &str = "admin_access_token_hash";

/// Session duration in hours.
const SESSION_DURATION_HOURS: i64 = 8;

/// An authenticated admin session, extracted from the request.
#[derive(Debug, Clone)]
pub struct AuthenticatedAdmin {
    pub session_id: String,
    pub csrf_token: String,
}

impl<S> FromRequestParts<S> for AuthenticatedAdmin
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let cookie_header = parts
            .headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let session_id = extract_cookie(cookie_header, SESSION_COOKIE);

        let session_id = match session_id {
            Some(id) => id,
            None => return Err(Redirect::to("/admin/login").into_response()),
        };

        let session = app_state
            .storage
            .get_admin_session(&session_id)
            .await
            .ok()
            .flatten();

        match session {
            Some(session) => {
                // Check expiry
                let expires = OffsetDateTime::parse(
                    &session.expires_at,
                    &time::format_description::well_known::Rfc3339,
                );
                match expires {
                    Ok(exp) if OffsetDateTime::now_utc() < exp => Ok(AuthenticatedAdmin {
                        session_id: session.id,
                        csrf_token: session.csrf_token,
                    }),
                    _ => {
                        // Session expired — clean up
                        let _ = app_state.storage.delete_admin_session(&session_id).await;
                        Err(Redirect::to("/admin/login").into_response())
                    }
                }
            }
            None => Err(Redirect::to("/admin/login").into_response()),
        }
    }
}

/// Trait for extracting `AppState` from a state container.
/// This allows `AuthenticatedAdmin` to work with `AppState` directly.
pub trait FromRef<T> {
    fn from_ref(input: &T) -> Self;
}

impl FromRef<AppState> for AppState {
    fn from_ref(input: &AppState) -> Self {
        input.clone()
    }
}

/// Ensure the admin access token exists; generate and log it if not.
pub async fn ensure_admin_token(state: &AppState) -> Result<(), crate::error::AppError> {
    let existing = state.storage.get_admin_secret(ADMIN_TOKEN_KEY).await?;
    if existing.is_some() {
        tracing::info!("admin access token already configured");
        return Ok(());
    }

    // Generate a new token
    let token = generate_token();
    let hash = hash_token(&token);

    state
        .storage
        .set_admin_secret(ADMIN_TOKEN_KEY, &hash)
        .await?;

    // Log the token prominently — this is the one time it's shown
    tracing::info!("===========================================================");
    tracing::info!("KEN ADMIN ACCESS TOKEN (shown once, save it now):");
    tracing::info!("{}", token);
    tracing::info!("===========================================================");

    Ok(())
}

/// Verify a submitted token against the stored hash.
pub async fn verify_token(state: &AppState, submitted: &str) -> Result<bool, crate::error::AppError> {
    let stored_hash = state.storage.get_admin_secret(ADMIN_TOKEN_KEY).await?;
    match stored_hash {
        Some(hash) => Ok(hash == hash_token(submitted)),
        None => Ok(false),
    }
}

/// Create a new admin session and return (session_id, csrf_token).
pub async fn create_session(state: &AppState) -> Result<(String, String), crate::error::AppError> {
    let session_id = Uuid::new_v4().to_string();
    let csrf_token = Uuid::new_v4().to_string();
    let now = OffsetDateTime::now_utc();
    let expires = now + time::Duration::hours(SESSION_DURATION_HOURS);

    let session = AdminSession {
        id: session_id.clone(),
        created_at: format_time(now),
        expires_at: format_time(expires),
        csrf_token: csrf_token.clone(),
    };

    state.storage.create_admin_session(&session).await?;

    Ok((session_id, csrf_token))
}

/// Generate a random 32-byte hex token.
fn generate_token() -> String {
    let bytes: [u8; 32] = rand_bytes();
    hex_encode(&bytes)
}

/// Simple SHA-256 hash of a token string.
fn hash_token(token: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    // For Phase 1, we use a simple hash. Production would use argon2.
    let mut hasher = DefaultHasher::new();
    token.hash(&mut hasher);
    format!("sha256:{:016x}", hasher.finish())
}

fn rand_bytes() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    // Use UUID v4 as a source of randomness (it uses the OS CSPRNG)
    let u1 = Uuid::new_v4();
    let u2 = Uuid::new_v4();
    bytes[..16].copy_from_slice(u1.as_bytes());
    bytes[16..].copy_from_slice(u2.as_bytes());
    bytes
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Extract a cookie value by name from a Cookie header string.
fn extract_cookie<'a>(header: &'a str, name: &str) -> Option<String> {
    header
        .split(';')
        .map(str::trim)
        .find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            if key.trim() == name {
                Some(value.trim().to_string())
            } else {
                None
            }
        })
}

fn format_time(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_extraction() {
        let header = "ken_session=abc123; other=xyz";
        assert_eq!(
            extract_cookie(header, "ken_session"),
            Some("abc123".to_string())
        );
        assert_eq!(extract_cookie(header, "missing"), None);
    }

    #[test]
    fn token_hash_is_deterministic() {
        let hash1 = hash_token("test-token");
        let hash2 = hash_token("test-token");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn different_tokens_produce_different_hashes() {
        let hash1 = hash_token("token-a");
        let hash2 = hash_token("token-b");
        assert_ne!(hash1, hash2);
    }
}
