//! Integration tests for the Storage layer.
//!
//! These tests exercise the database operations against an in-memory
//! `SQLite` database to verify migrations and query correctness.

use ken_protocol::command::CommandPayload;
use ken_protocol::ids::{CommandId, EndpointId, HeartbeatId};
use ken_protocol::status::{
    BitLockerStatus, DefenderStatus, FirewallStatus, Observation, OsStatusSnapshot,
    WindowsUpdateStatus,
};
use ken_protocol::SCHEMA_VERSION;
use time::OffsetDateTime;

// We need to access the storage module. Since it's inside a binary crate,
// we test via the storage module being included through a test helper.
// For now, create a mini test harness that uses the same connection logic.

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;
use std::str::FromStr;

async fn test_pool() -> SqlitePool {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .foreign_keys(true);
    let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();

    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    pool
}

#[tokio::test]
async fn migrations_create_all_tables() {
    let pool = test_pool().await;

    // Verify all expected tables exist
    let tables: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations' ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let table_names: Vec<&str> = tables.iter().map(|t| t.0.as_str()).collect();

    assert!(
        table_names.contains(&"endpoints"),
        "missing endpoints table"
    );
    assert!(
        table_names.contains(&"enrollment_tokens"),
        "missing enrollment_tokens table"
    );
    assert!(
        table_names.contains(&"heartbeats"),
        "missing heartbeats table"
    );
    assert!(
        table_names.contains(&"status_snapshots"),
        "missing status_snapshots table"
    );
    assert!(table_names.contains(&"commands"), "missing commands table");
    assert!(
        table_names.contains(&"audit_events"),
        "missing audit_events table"
    );
    assert!(
        table_names.contains(&"admin_sessions"),
        "missing admin_sessions table"
    );
    assert!(
        table_names.contains(&"admin_secrets"),
        "missing admin_secrets table"
    );
}

fn now_str() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

#[tokio::test]
async fn enrollment_token_lifecycle() {
    let pool = test_pool().await;

    let token_val = "test-token-abc";
    let now = now_str();
    let expires = now.clone();

    // Create token
    sqlx::query("INSERT INTO enrollment_tokens (token, created_at, expires_at) VALUES (?, ?, ?)")
        .bind(token_val)
        .bind(&now)
        .bind(&expires)
        .execute(&pool)
        .await
        .unwrap();

    // Fetch token
    let row: Option<(String,)> =
        sqlx::query_as("SELECT token FROM enrollment_tokens WHERE token = ?")
            .bind(token_val)
            .fetch_optional(&pool)
            .await
            .unwrap();
    assert!(row.is_some());

    // Consume token
    sqlx::query("UPDATE enrollment_tokens SET consumed_at = ? WHERE token = ?")
        .bind(&now)
        .bind(token_val)
        .execute(&pool)
        .await
        .unwrap();

    // Verify consumed
    let row: (Option<String>,) =
        sqlx::query_as("SELECT consumed_at FROM enrollment_tokens WHERE token = ?")
            .bind(token_val)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(row.0.is_some());
}

#[tokio::test]
async fn endpoint_create_and_get() {
    let pool = test_pool().await;

    let endpoint_id = EndpointId::new();
    let now = now_str();

    // Insert endpoint
    sqlx::query(
        "INSERT INTO endpoints (id, hostname, os_version, agent_version, enrolled_at, certificate_pem, certificate_expires_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(endpoint_id.to_string())
    .bind("DESKTOP-TEST")
    .bind("Windows 11 24H2")
    .bind("0.1.0")
    .bind(&now)
    .bind("test-cert-pem")
    .bind(&now)
    .execute(&pool)
    .await
    .unwrap();

    // Fetch endpoint
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT id, hostname FROM endpoints WHERE id = ?")
            .bind(endpoint_id.to_string())
            .fetch_optional(&pool)
            .await
            .unwrap();

    let (id, hostname) = row.expect("endpoint should exist");
    assert_eq!(id, endpoint_id.to_string());
    assert_eq!(hostname, "DESKTOP-TEST");

    // List endpoints
    let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM endpoints")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1);
}

#[tokio::test]
async fn heartbeat_and_status_snapshot() {
    let pool = test_pool().await;

    let endpoint_id = EndpointId::new();
    let now = now_str();

    // Create endpoint first (foreign key)
    sqlx::query(
        "INSERT INTO endpoints (id, hostname, os_version, agent_version, enrolled_at, certificate_pem, certificate_expires_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(endpoint_id.to_string())
    .bind("DESKTOP-TEST")
    .bind("Windows 11")
    .bind("0.1.0")
    .bind(&now)
    .bind("cert")
    .bind(&now)
    .execute(&pool)
    .await
    .unwrap();

    // Insert heartbeat
    let hb_id = HeartbeatId::new();
    sqlx::query(
        "INSERT INTO heartbeats (id, endpoint_id, received_at, sent_at, schema_version, agent_version) \
         VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(hb_id.to_string())
    .bind(endpoint_id.to_string())
    .bind(&now)
    .bind(&now)
    .bind(SCHEMA_VERSION)
    .bind("0.1.0")
    .execute(&pool)
    .await
    .unwrap();

    // Verify heartbeat stored
    let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM heartbeats WHERE endpoint_id = ?")
        .bind(endpoint_id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1);

    // Insert status snapshot
    let snapshot = OsStatusSnapshot {
        collected_at: OffsetDateTime::now_utc(),
        defender: DefenderStatus {
            antivirus_enabled: Observation::Unobserved,
            real_time_protection_enabled: Observation::Unobserved,
            tamper_protection_enabled: Observation::Unobserved,
            signature_version: Observation::Unobserved,
            signature_last_updated: Observation::Unobserved,
            signature_age_days: Observation::Unobserved,
            last_full_scan: Observation::Unobserved,
            last_quick_scan: Observation::Unobserved,
        },
        firewall: FirewallStatus {
            domain_profile: Observation::Unobserved,
            private_profile: Observation::Unobserved,
            public_profile: Observation::Unobserved,
        },
        bitlocker: BitLockerStatus {
            volumes: Observation::Unobserved,
        },
        windows_update: WindowsUpdateStatus {
            last_search_time: Observation::Unobserved,
            last_install_time: Observation::Unobserved,
            pending_update_count: Observation::Unobserved,
            pending_critical_update_count: Observation::Unobserved,
        },
        recent_security_events: Observation::Unobserved,
    };
    let snapshot_json = serde_json::to_string(&snapshot).unwrap();

    sqlx::query(
        "INSERT INTO status_snapshots (endpoint_id, collected_at, snapshot_json) VALUES (?, ?, ?) \
         ON CONFLICT(endpoint_id) DO UPDATE SET collected_at = excluded.collected_at, snapshot_json = excluded.snapshot_json"
    )
    .bind(endpoint_id.to_string())
    .bind(&now)
    .bind(&snapshot_json)
    .execute(&pool)
    .await
    .unwrap();

    // Fetch and deserialize snapshot
    let row: (String,) =
        sqlx::query_as("SELECT snapshot_json FROM status_snapshots WHERE endpoint_id = ?")
            .bind(endpoint_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();

    let loaded: OsStatusSnapshot = serde_json::from_str(&row.0).unwrap();
    assert_eq!(loaded.defender.antivirus_enabled, Observation::Unobserved);
}

#[tokio::test]
async fn command_queue_and_delivery() {
    let pool = test_pool().await;

    let endpoint_id = EndpointId::new();
    let command_id = CommandId::new();
    let now = now_str();

    // Create endpoint first
    sqlx::query(
        "INSERT INTO endpoints (id, hostname, os_version, agent_version, enrolled_at, certificate_pem, certificate_expires_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(endpoint_id.to_string())
    .bind("TEST")
    .bind("Win11")
    .bind("0.1.0")
    .bind(&now)
    .bind("cert")
    .bind(&now)
    .execute(&pool)
    .await
    .unwrap();

    // Queue a command
    let payload = CommandPayload::Ping;
    let payload_json = serde_json::to_string(&payload).unwrap();
    sqlx::query(
        "INSERT INTO commands (id, endpoint_id, issued_at, expires_at, payload_json) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(command_id.to_string())
    .bind(endpoint_id.to_string())
    .bind(&now)
    .bind(&now)
    .bind(&payload_json)
    .execute(&pool)
    .await
    .unwrap();

    // Fetch pending commands
    let pending: Vec<(String,)> =
        sqlx::query_as("SELECT id FROM commands WHERE endpoint_id = ? AND delivered_at IS NULL")
            .bind(endpoint_id.to_string())
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(pending.len(), 1);

    // Mark delivered
    sqlx::query("UPDATE commands SET delivered_at = ? WHERE id = ?")
        .bind(&now)
        .bind(command_id.to_string())
        .execute(&pool)
        .await
        .unwrap();

    // Verify no pending
    let pending: Vec<(String,)> =
        sqlx::query_as("SELECT id FROM commands WHERE endpoint_id = ? AND delivered_at IS NULL")
            .bind(endpoint_id.to_string())
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(pending.len(), 0);
}
