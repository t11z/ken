//! Database layer for the Ken server.
//!
//! Wraps a `SQLite` connection pool and provides typed methods for every
//! query the application needs. The raw pool is never exposed to handlers.
//! Migrations are applied at startup via `sqlx::migrate!`.

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use time::OffsetDateTime;

use ken_protocol::command::{CommandEnvelope, CommandOutcome};
use ken_protocol::heartbeat::Heartbeat;
use ken_protocol::ids::{CommandId, EndpointId};
use ken_protocol::status::OsStatusSnapshot;

use crate::config::StorageConfig;
use crate::error::AppError;

/// Wrapper around the `SQLite` connection pool with typed query methods.
#[derive(Clone)]
pub struct Storage {
    pool: SqlitePool,
}

/// An enrolled endpoint as stored in the database.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub id: String,
    pub hostname: String,
    pub os_version: String,
    pub agent_version: String,
    pub enrolled_at: String,
    pub last_heartbeat_at: Option<String>,
    pub certificate_pem: String,
    /// When the endpoint's client certificate expires (checked by mTLS verifier).
    pub certificate_expires_at: String,
    pub revoked_at: Option<String>,
    pub display_name: Option<String>,
}

/// Data needed to create a new endpoint row.
pub struct NewEndpoint {
    pub id: String,
    pub hostname: String,
    pub os_version: String,
    pub agent_version: String,
    pub enrolled_at: String,
    pub certificate_pem: String,
    pub certificate_expires_at: String,
    pub display_name: Option<String>,
}

/// An enrollment token as stored in the database.
#[derive(Debug, Clone)]
pub struct EnrollmentToken {
    pub token: String,
    pub created_at: String,
    pub expires_at: String,
    pub consumed_at: Option<String>,
    pub display_name: Option<String>,
}

/// An audit event as stored in the database (with source and endpoint info).
#[derive(Debug, Clone)]
pub struct StoredAuditEvent {
    pub id: String,
    pub endpoint_id: Option<String>,
    pub occurred_at: String,
    pub source: String,
    pub kind: String,
    pub message: String,
}

/// An admin session as stored in the database.
///
/// The `stage` field distinguishes bootstrap sessions (ADR-0024 Stage 1,
/// restricted to `/admin/set-password`) from full sessions (Stage 2).
#[derive(Debug, Clone)]
pub struct AdminSession {
    pub id: String,
    pub created_at: String,
    pub expires_at: String,
    pub csrf_token: String,
    /// Session stage: `"bootstrap"` or `"full"`. See ADR-0024.
    pub stage: String,
}

impl Storage {
    /// Connect to the `SQLite` database, creating the file if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the data directory cannot be created or if the
    /// database file cannot be opened (e.g. a permissions problem on the
    /// mounted volume).
    pub async fn connect(config: &StorageConfig) -> Result<Self, AppError> {
        let db_path = config.data_dir.join("ken.db");

        std::fs::create_dir_all(&config.data_dir).map_err(|e| {
            AppError::Internal(format!(
                "failed to create data directory '{}': {e}",
                config.data_dir.display()
            ))
        })?;

        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(|e| {
                AppError::Internal(format!(
                    "failed to open database at '{}': {e}\n\
                     Hint: ensure the data directory is writable by the server \
                     process. When using a bind mount, the host directory must \
                     be writable by the container user (uid 0 for the default \
                     image). Using a Docker named volume avoids this entirely.",
                    db_path.display()
                ))
            })?;

        tracing::info!(path = %db_path.display(), "connected to SQLite database");
        Ok(Self { pool })
    }

    /// Connect to an in-memory database for testing.
    ///
    /// Not gated by `#[cfg(test)]` because the integration test
    /// `agent_mtls_bridge.rs` (Phase 1 of ADR-0017) needs it, and
    /// integration tests compile the library without the test flag.
    pub async fn connect_in_memory() -> Result<Self, AppError> {
        use std::str::FromStr;
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;

        Ok(Self { pool })
    }

    /// Run database migrations.
    ///
    /// # Errors
    ///
    /// Returns an error if any migration fails.
    pub async fn migrate(&self) -> Result<(), AppError> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("migration failed: {e}")))?;
        tracing::info!("database migrations applied");
        Ok(())
    }

    // --- Enrollment tokens ---

    /// Create a new enrollment token.
    pub async fn create_enrollment_token(&self, token: &EnrollmentToken) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO enrollment_tokens (token, created_at, expires_at, consumed_at, display_name) \
             VALUES (?, ?, ?, ?, ?)"
        )
        .bind(&token.token)
        .bind(&token.created_at)
        .bind(&token.expires_at)
        .bind(&token.consumed_at)
        .bind(&token.display_name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Look up and consume an enrollment token atomically.
    ///
    /// Returns the token if found, regardless of whether it is expired
    /// or already consumed — the caller must check those conditions.
    pub async fn get_enrollment_token(
        &self,
        token_value: &str,
    ) -> Result<Option<EnrollmentToken>, AppError> {
        let row = sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>)>(
            "SELECT token, created_at, expires_at, consumed_at, display_name \
             FROM enrollment_tokens WHERE token = ?",
        )
        .bind(token_value)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(token, created_at, expires_at, consumed_at, display_name)| EnrollmentToken {
                token,
                created_at,
                expires_at,
                consumed_at,
                display_name,
            },
        ))
    }

    /// Mark an enrollment token as consumed.
    pub async fn consume_enrollment_token(
        &self,
        token_value: &str,
        consumed_at: &str,
    ) -> Result<(), AppError> {
        sqlx::query("UPDATE enrollment_tokens SET consumed_at = ? WHERE token = ?")
            .bind(consumed_at)
            .bind(token_value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // --- Endpoints ---

    /// Create a new enrolled endpoint.
    pub async fn create_endpoint(&self, endpoint: &NewEndpoint) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO endpoints \
             (id, hostname, os_version, agent_version, enrolled_at, certificate_pem, \
              certificate_expires_at, display_name) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&endpoint.id)
        .bind(&endpoint.hostname)
        .bind(&endpoint.os_version)
        .bind(&endpoint.agent_version)
        .bind(&endpoint.enrolled_at)
        .bind(&endpoint.certificate_pem)
        .bind(&endpoint.certificate_expires_at)
        .bind(&endpoint.display_name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Look up an endpoint by ID.
    pub async fn get_endpoint(&self, id: &EndpointId) -> Result<Option<Endpoint>, AppError> {
        let row = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                Option<String>,
                String,
                String,
                Option<String>,
                Option<String>,
            ),
        >(
            "SELECT id, hostname, os_version, agent_version, enrolled_at, \
             last_heartbeat_at, certificate_pem, certificate_expires_at, \
             revoked_at, display_name \
             FROM endpoints WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(
                id,
                hostname,
                os_version,
                agent_version,
                enrolled_at,
                last_heartbeat_at,
                certificate_pem,
                certificate_expires_at,
                revoked_at,
                display_name,
            )| {
                Endpoint {
                    id,
                    hostname,
                    os_version,
                    agent_version,
                    enrolled_at,
                    last_heartbeat_at,
                    certificate_pem,
                    certificate_expires_at,
                    revoked_at,
                    display_name,
                }
            },
        ))
    }

    /// List all enrolled endpoints.
    pub async fn list_endpoints(&self) -> Result<Vec<Endpoint>, AppError> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                Option<String>,
                String,
                String,
                Option<String>,
                Option<String>,
            ),
        >(
            "SELECT id, hostname, os_version, agent_version, enrolled_at, \
             last_heartbeat_at, certificate_pem, certificate_expires_at, \
             revoked_at, display_name \
             FROM endpoints ORDER BY enrolled_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    hostname,
                    os_version,
                    agent_version,
                    enrolled_at,
                    last_heartbeat_at,
                    certificate_pem,
                    certificate_expires_at,
                    revoked_at,
                    display_name,
                )| {
                    Endpoint {
                        id,
                        hostname,
                        os_version,
                        agent_version,
                        enrolled_at,
                        last_heartbeat_at,
                        certificate_pem,
                        certificate_expires_at,
                        revoked_at,
                        display_name,
                    }
                },
            )
            .collect())
    }

    // --- Heartbeats ---

    /// Record a heartbeat and update the endpoint's last-seen timestamp.
    pub async fn record_heartbeat(
        &self,
        endpoint_id: &EndpointId,
        heartbeat: &Heartbeat,
        received_at: OffsetDateTime,
    ) -> Result<(), AppError> {
        let received_str = format_time(received_at);
        let sent_str = format_time(heartbeat.sent_at);
        let endpoint_str = endpoint_id.to_string();
        let hb_id = heartbeat.heartbeat_id.to_string();

        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO heartbeats (id, endpoint_id, received_at, sent_at, schema_version, agent_version) \
             VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(&hb_id)
        .bind(&endpoint_str)
        .bind(&received_str)
        .bind(&sent_str)
        .bind(heartbeat.schema_version)
        .bind(&heartbeat.agent_version)
        .execute(&mut *tx)
        .await?;

        sqlx::query("UPDATE endpoints SET last_heartbeat_at = ?, agent_version = ? WHERE id = ?")
            .bind(&received_str)
            .bind(&heartbeat.agent_version)
            .bind(&endpoint_str)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    // --- Status snapshots ---

    /// Insert or update the latest status snapshot for an endpoint.
    pub async fn upsert_status_snapshot(
        &self,
        endpoint_id: &EndpointId,
        snapshot: &OsStatusSnapshot,
    ) -> Result<(), AppError> {
        let json = serde_json::to_string(snapshot)
            .map_err(|e| AppError::Internal(format!("failed to serialize snapshot: {e}")))?;
        let collected_str = format_time(snapshot.collected_at);

        sqlx::query(
            "INSERT INTO status_snapshots (endpoint_id, collected_at, snapshot_json) \
             VALUES (?, ?, ?) \
             ON CONFLICT(endpoint_id) DO UPDATE SET collected_at = excluded.collected_at, \
             snapshot_json = excluded.snapshot_json",
        )
        .bind(endpoint_id.to_string())
        .bind(collected_str)
        .bind(json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get the latest status snapshot for an endpoint.
    pub async fn get_status_snapshot(
        &self,
        endpoint_id: &EndpointId,
    ) -> Result<Option<OsStatusSnapshot>, AppError> {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT snapshot_json FROM status_snapshots WHERE endpoint_id = ?",
        )
        .bind(endpoint_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some((json,)) => {
                let snapshot = serde_json::from_str(&json).map_err(|e| {
                    AppError::Internal(format!("failed to deserialize snapshot: {e}"))
                })?;
                Ok(Some(snapshot))
            }
            None => Ok(None),
        }
    }

    // --- Commands ---

    /// Queue a command for an endpoint.
    pub async fn queue_command(
        &self,
        endpoint_id: &EndpointId,
        envelope: &CommandEnvelope,
    ) -> Result<(), AppError> {
        let payload_json = serde_json::to_string(&envelope.payload)
            .map_err(|e| AppError::Internal(format!("failed to serialize command: {e}")))?;

        sqlx::query(
            "INSERT INTO commands (id, endpoint_id, issued_at, expires_at, payload_json) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(envelope.command_id.to_string())
        .bind(endpoint_id.to_string())
        .bind(format_time(envelope.issued_at))
        .bind(format_time(envelope.expires_at))
        .bind(payload_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get pending (undelivered) commands for an endpoint.
    pub async fn pending_commands_for(
        &self,
        endpoint_id: &EndpointId,
    ) -> Result<Vec<CommandEnvelope>, AppError> {
        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT id, issued_at, expires_at, payload_json \
             FROM commands \
             WHERE endpoint_id = ? AND delivered_at IS NULL AND completed_at IS NULL \
             ORDER BY issued_at ASC",
        )
        .bind(endpoint_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut envelopes = Vec::with_capacity(rows.len());
        for (id, issued_at, expires_at, payload_json) in rows {
            let command_id = CommandId::parse(&id)
                .map_err(|e| AppError::Internal(format!("invalid command ID in db: {e}")))?;
            let issued = parse_time(&issued_at)?;
            let expires = parse_time(&expires_at)?;
            let payload = serde_json::from_str(&payload_json)
                .map_err(|e| AppError::Internal(format!("invalid command payload in db: {e}")))?;

            envelopes.push(CommandEnvelope {
                command_id,
                issued_at: issued,
                expires_at: expires,
                payload,
            });
        }
        Ok(envelopes)
    }

    /// Mark a command as delivered.
    pub async fn mark_command_delivered(
        &self,
        command_id: &CommandId,
        delivered_at: OffsetDateTime,
    ) -> Result<(), AppError> {
        sqlx::query("UPDATE commands SET delivered_at = ? WHERE id = ?")
            .bind(format_time(delivered_at))
            .bind(command_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Record a command outcome.
    pub async fn record_command_outcome(&self, outcome: &CommandOutcome) -> Result<(), AppError> {
        let outcome_json = serde_json::to_string(&outcome.result)
            .map_err(|e| AppError::Internal(format!("failed to serialize outcome: {e}")))?;

        sqlx::query("UPDATE commands SET completed_at = ?, outcome_json = ? WHERE id = ?")
            .bind(format_time(outcome.completed_at))
            .bind(outcome_json)
            .bind(outcome.command_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // --- Audit events ---

    /// Append an audit event to the server-side log.
    pub async fn append_audit_event(
        &self,
        id: &str,
        occurred_at: &str,
        kind: &str,
        message: &str,
        source: &str,
        endpoint_id: Option<&str>,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO audit_events (id, endpoint_id, occurred_at, source, kind, message) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(endpoint_id)
        .bind(occurred_at)
        .bind(source)
        .bind(kind)
        .bind(message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get recent audit events, newest first.
    pub async fn recent_audit_events(&self, limit: u32) -> Result<Vec<StoredAuditEvent>, AppError> {
        let rows = sqlx::query_as::<_, (String, Option<String>, String, String, String, String)>(
            "SELECT id, endpoint_id, occurred_at, source, kind, message \
             FROM audit_events ORDER BY occurred_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, endpoint_id, occurred_at, source, kind, message)| StoredAuditEvent {
                    id,
                    endpoint_id,
                    occurred_at,
                    source,
                    kind,
                    message,
                },
            )
            .collect())
    }

    // --- Admin sessions ---

    /// Create a new admin session.
    ///
    /// The `stage` field must be `"bootstrap"` or `"full"` per ADR-0024.
    pub async fn create_admin_session(&self, session: &AdminSession) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO admin_sessions (id, created_at, expires_at, csrf_token, stage) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&session.id)
        .bind(&session.created_at)
        .bind(&session.expires_at)
        .bind(&session.csrf_token)
        .bind(&session.stage)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Look up an admin session by ID.
    pub async fn get_admin_session(&self, id: &str) -> Result<Option<AdminSession>, AppError> {
        let row = sqlx::query_as::<_, (String, String, String, String, String)>(
            "SELECT id, created_at, expires_at, csrf_token, stage \
             FROM admin_sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(id, created_at, expires_at, csrf_token, stage)| AdminSession {
                id,
                created_at,
                expires_at,
                csrf_token,
                stage,
            },
        ))
    }

    /// Delete a single admin session by ID.
    pub async fn delete_admin_session(&self, id: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM admin_sessions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete all admin sessions, e.g. after a password reset.
    ///
    /// Called by `ken-server admin reset-password` and by the set-password
    /// handler after the user sets their permanent password (ADR-0024).
    pub async fn delete_all_admin_sessions(&self) -> Result<(), AppError> {
        sqlx::query("DELETE FROM admin_sessions")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // --- Admin secrets ---

    /// Get an admin secret by key.
    pub async fn get_admin_secret(&self, key: &str) -> Result<Option<String>, AppError> {
        let row = sqlx::query_as::<_, (String,)>("SELECT value FROM admin_secrets WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|(v,)| v))
    }

    /// Set an admin secret, inserting or replacing the existing value.
    pub async fn set_admin_secret(&self, key: &str, value: &str) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO admin_secrets (key, value) VALUES (?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete an admin secret by key. No-op if the key does not exist.
    ///
    /// Used by the set-password flow to atomically remove the bootstrap
    /// password hash once the user password hash has been stored (ADR-0024).
    pub async fn delete_admin_secret(&self, key: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM admin_secrets WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn format_time(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| t.to_string())
}

fn parse_time(s: &str) -> Result<OffsetDateTime, AppError> {
    OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|e| AppError::Internal(format!("invalid timestamp in database: {s}: {e}")))
}

/// Resolve the database path from a data directory, used by tests.
#[must_use]
pub fn db_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("ken.db")
}
