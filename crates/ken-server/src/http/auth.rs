//! Admin session handling and authentication (ADR-0024).
//!
//! Implements the two-stage authentication model:
//!
//! 1. **Bootstrap stage**: on first startup, a cryptographically random
//!    password is generated, hashed with Argon2id, stored under
//!    `admin_bootstrap_password_hash`, and logged once.  A session created
//!    from the bootstrap password is restricted to `/admin/set-password`.
//!
//! 2. **User-chosen stage**: after first login the admin sets a permanent
//!    password. The bootstrap hash is deleted, the user hash is stored under
//!    `admin_user_password_hash`, and all existing sessions are invalidated.
//!    All subsequent logins use the user hash and receive full-access sessions.

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Redirect, Response};
use rand_core::{OsRng, RngCore};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::state::AppState;
use crate::storage::{AdminSession, Storage};

/// Cookie name for the admin session.
pub const SESSION_COOKIE: &str = "ken_session";

/// Key for the bootstrap password hash in `admin_secrets`. ADR-0024.
pub const BOOTSTRAP_HASH_KEY: &str = "admin_bootstrap_password_hash";

/// Key for the user-chosen password hash in `admin_secrets`. ADR-0024.
pub const USER_HASH_KEY: &str = "admin_user_password_hash";

/// Session duration in hours.
const SESSION_DURATION_HOURS: i64 = 8;

/// The outcome of a login verification attempt. ADR-0024.
pub enum LoginResult {
    /// Neither the bootstrap nor the user password matches.
    Invalid,
    /// The bootstrap password matched. The session must be `stage = "bootstrap"`.
    BootstrapAccepted,
    /// The user-chosen password matched. The session must be `stage = "full"`.
    UserAccepted,
}

/// An authenticated admin session, extracted from the request cookie.
///
/// Accepts both `"bootstrap"` and `"full"` stage sessions. Most protected
/// handlers should use [`AuthenticatedFullAdmin`] instead, which rejects
/// bootstrap sessions. Use this extractor only for handlers that must be
/// accessible during the bootstrap flow (logout, set-password).
#[derive(Debug, Clone)]
pub struct AuthenticatedAdmin {
    /// The session ID stored in the cookie.
    pub session_id: String,
    /// CSRF token for this session.
    pub csrf_token: String,
    /// Session stage: `"bootstrap"` or `"full"`. ADR-0024.
    pub stage: String,
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

        let Some(session_id) = session_id else {
            return Err(Redirect::to("/admin/login").into_response());
        };

        let session = app_state
            .storage
            .get_admin_session(&session_id)
            .await
            .ok()
            .flatten();

        match session {
            Some(session) => {
                let expires = OffsetDateTime::parse(
                    &session.expires_at,
                    &time::format_description::well_known::Rfc3339,
                );
                match expires {
                    Ok(exp) if OffsetDateTime::now_utc() < exp => Ok(AuthenticatedAdmin {
                        session_id: session.id,
                        csrf_token: session.csrf_token,
                        stage: session.stage,
                    }),
                    _ => {
                        let _ = app_state.storage.delete_admin_session(&session_id).await;
                        Err(Redirect::to("/admin/login").into_response())
                    }
                }
            }
            None => Err(Redirect::to("/admin/login").into_response()),
        }
    }
}

/// An authenticated admin session restricted to full-access sessions.
///
/// Bootstrap-stage sessions are redirected to `/admin/set-password`. All
/// handlers that require a fully configured admin account should use this
/// extractor rather than [`AuthenticatedAdmin`].
#[derive(Debug, Clone)]
pub struct AuthenticatedFullAdmin {
    /// The session ID stored in the cookie.
    pub session_id: String,
    /// CSRF token for this session.
    pub csrf_token: String,
}

impl<S> FromRequestParts<S> for AuthenticatedFullAdmin
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let admin = AuthenticatedAdmin::from_request_parts(parts, state).await?;
        if admin.stage == "bootstrap" {
            return Err(Redirect::to("/admin/set-password").into_response());
        }
        Ok(AuthenticatedFullAdmin {
            session_id: admin.session_id,
            csrf_token: admin.csrf_token,
        })
    }
}

/// Ensure the bootstrap password exists. Called once at server startup.
///
/// Behaviour (ADR-0024):
/// - If `admin_user_password_hash` is present: log that auth is configured,
///   do nothing.
/// - If `admin_bootstrap_password_hash` is present but user hash is absent:
///   log that the server is awaiting first login, do nothing.
/// - If neither key is present: generate a random 24-character password, hash
///   it with Argon2id, store the hash, and log the plaintext exactly once.
///
/// The plaintext password is never written to a file, never returned from
/// this function, and never re-logged on subsequent startups.
pub async fn ensure_admin_bootstrap(storage: &Storage) -> Result<(), crate::error::AppError> {
    let user_hash = storage.get_admin_secret(USER_HASH_KEY).await?;
    if user_hash.is_some() {
        tracing::info!("admin authentication is configured (user password set)");
        return Ok(());
    }

    let bootstrap_hash = storage.get_admin_secret(BOOTSTRAP_HASH_KEY).await?;
    if bootstrap_hash.is_some() {
        tracing::info!("admin server is awaiting first login (bootstrap password active)");
        return Ok(());
    }

    // Neither key present — first startup. Generate bootstrap password.
    let password = generate_password();
    let hash = hash_password(&password)?;
    storage.set_admin_secret(BOOTSTRAP_HASH_KEY, &hash).await?;

    tracing::info!("=====================================================");
    tracing::info!("KEN BOOTSTRAP PASSWORD (shown once — log in now):");
    tracing::info!("{}", password);
    tracing::info!("=====================================================");

    Ok(())
}

/// Verify a submitted password and return the [`LoginResult`].
///
/// ADR-0024: checks only the active credential.  If `admin_user_password_hash`
/// is present it is the only credential checked.  If only
/// `admin_bootstrap_password_hash` is present it is the only credential
/// checked.  Both are never checked in the same pass.
pub async fn verify_login(
    storage: &Storage,
    submitted: &str,
) -> Result<LoginResult, crate::error::AppError> {
    let user_hash = storage.get_admin_secret(USER_HASH_KEY).await?;
    if let Some(hash) = user_hash {
        return if verify_password(submitted, &hash)? {
            Ok(LoginResult::UserAccepted)
        } else {
            Ok(LoginResult::Invalid)
        };
    }

    let bootstrap_hash = storage.get_admin_secret(BOOTSTRAP_HASH_KEY).await?;
    if let Some(hash) = bootstrap_hash {
        if verify_password(submitted, &hash)? {
            return Ok(LoginResult::BootstrapAccepted);
        }
    }

    Ok(LoginResult::Invalid)
}

/// Create a new admin session and return `(session_id, csrf_token)`.
///
/// `stage` must be `"bootstrap"` or `"full"`. ADR-0024.
pub async fn create_session(
    storage: &Storage,
    stage: &str,
) -> Result<(String, String), crate::error::AppError> {
    let session_id = Uuid::new_v4().to_string();
    let csrf_token = Uuid::new_v4().to_string();
    let now = OffsetDateTime::now_utc();
    let expires = now + time::Duration::hours(SESSION_DURATION_HOURS);

    let session = AdminSession {
        id: session_id.clone(),
        created_at: format_time(now),
        expires_at: format_time(expires),
        csrf_token: csrf_token.clone(),
        stage: stage.to_string(),
    };

    storage.create_admin_session(&session).await?;

    Ok((session_id, csrf_token))
}

/// Hash a password using Argon2id (OWASP-recommended parameters).
///
/// Returns a PHC-format string suitable for storage in `admin_secrets`.
/// The PHC string encodes the algorithm, parameters, salt, and hash, so
/// no separate salt storage is needed.
pub fn hash_password(password: &str) -> Result<String, crate::error::AppError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| crate::error::AppError::Internal(format!("password hashing failed: {e}")))
}

/// Verify a password against a stored PHC hash string.
///
/// Returns `true` if the password matches, `false` otherwise. Never compares
/// hash strings directly.
fn verify_password(password: &str, hash: &str) -> Result<bool, crate::error::AppError> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| crate::error::AppError::Internal(format!("invalid stored hash: {e}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

/// Generate a cryptographically random 24-character password.
///
/// Uses a 70-character alphabet (A-Z, a-z, 0-9, `!@#$%^&*`). Bytes are drawn
/// from the OS CSPRNG via rejection sampling to eliminate modular bias.
fn generate_password() -> String {
    const CHARSET: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*";
    // 70 chars. Cutoff for bias-free rejection sampling: floor(256/70)*70 = 210.
    const CUTOFF: usize = (256 / CHARSET.len()) * CHARSET.len();

    let mut password = String::with_capacity(24);
    // 64 bytes yields ~52 accepted on average (P_accept ≈ 82%), well above 24.
    let mut buf = [0u8; 64];
    OsRng.fill_bytes(&mut buf);

    for byte in buf {
        if password.len() == 24 {
            break;
        }
        let n = usize::from(byte);
        if n < CUTOFF {
            password.push(char::from(CHARSET[n % CHARSET.len()]));
        }
    }

    // Extremely rare fallback: refill one byte at a time until we have 24 chars.
    while password.len() < 24 {
        let mut b = [0u8; 1];
        OsRng.fill_bytes(&mut b);
        let n = usize::from(b[0]);
        if n < CUTOFF {
            password.push(char::from(CHARSET[n % CHARSET.len()]));
        }
    }

    password
}

/// Extract a cookie value by name from a `Cookie` header string.
fn extract_cookie(header: &str, name: &str) -> Option<String> {
    header.split(';').map(str::trim).find_map(|pair| {
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
    fn password_hash_roundtrip() {
        let hash = hash_password("correct-horse-battery-staple").unwrap();
        // PHC string must start with argon2id variant identifier
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("correct-horse-battery-staple", &hash).unwrap());
        assert!(!verify_password("wrong-password", &hash).unwrap());
    }

    #[test]
    fn generate_password_length_and_charset() {
        const CHARSET: &[u8] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*";
        let pw = generate_password();
        assert_eq!(pw.len(), 24);
        // Every byte must be in the allowed set
        for byte in pw.bytes() {
            assert!(
                CHARSET.contains(&byte),
                "unexpected byte in password: {byte}"
            );
        }
    }
}
