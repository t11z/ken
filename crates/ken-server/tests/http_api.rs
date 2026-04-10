//! HTTP API integration tests.
//!
//! These tests exercise the enrollment and heartbeat endpoints using
//! axum's test client with an in-memory `SQLite` database.

use std::str::FromStr;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;
use time::OffsetDateTime;
use tower::ServiceExt;

use ken_protocol::enrollment::{EnrollmentRequest, EnrollmentResponse};
use ken_protocol::heartbeat::{Heartbeat, HeartbeatAck};
use ken_protocol::ids::{EndpointId, HeartbeatId};
use ken_protocol::status::{
    BitLockerStatus, DefenderStatus, FirewallStatus, Observation, OsStatusSnapshot,
    WindowsUpdateStatus,
};
use ken_protocol::SCHEMA_VERSION;

// We need to access the server's internal modules. Since ken-server is a
// binary crate, we build the router and state inline using the same
// patterns as the server code.

async fn test_pool() -> SqlitePool {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .foreign_keys(true);
    let pool = SqlitePool::connect_with(options).await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

fn now_str() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

fn future_str() -> String {
    let future = OffsetDateTime::now_utc() + time::Duration::hours(1);
    future
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

fn past_str() -> String {
    let past = OffsetDateTime::now_utc() - time::Duration::hours(1);
    past.format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

/// Create enrollment token directly in the database.
async fn create_token(pool: &SqlitePool, token: &str, expires_at: &str) {
    sqlx::query(
        "INSERT INTO enrollment_tokens (token, created_at, expires_at, display_name) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(token)
    .bind(now_str())
    .bind(expires_at)
    .bind("Test PC")
    .execute(pool)
    .await
    .unwrap();
}

/// Create an endpoint directly in the database (for heartbeat tests).
async fn create_endpoint(pool: &SqlitePool, endpoint_id: &EndpointId) {
    let now = now_str();
    sqlx::query(
        "INSERT INTO endpoints (id, hostname, os_version, agent_version, enrolled_at, certificate_pem, certificate_expires_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(endpoint_id.to_string())
    .bind("TEST-PC")
    .bind("Windows 11")
    .bind("0.1.0")
    .bind(&now)
    .bind("test-cert")
    .bind(&now)
    .execute(pool)
    .await
    .unwrap();
}

/// Build the admin router with a test pool (for enrollment tests).
fn admin_router_with_pool(pool: SqlitePool) -> axum::Router {
    // We need to construct the state. Since this is an integration test
    // against a binary crate, we build a minimal router inline that
    // matches the server's enrollment handler.
    use axum::extract::{Path, State};
    use axum::routing::post;
    use axum::{Json, Router};

    #[derive(Clone)]
    struct TestState {
        pool: SqlitePool,
    }

    async fn enroll_handler(
        State(state): State<TestState>,
        Path(token_value): Path<String>,
        Json(request): Json<EnrollmentRequest>,
    ) -> Result<Json<EnrollmentResponse>, (StatusCode, String)> {
        // Look up token
        let row: Option<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT token, expires_at, consumed_at, display_name FROM enrollment_tokens WHERE token = ?",
        )
        .bind(&token_value)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let Some((_, expires_at, consumed_at, display_name)) = row else {
            return Err((StatusCode::NOT_FOUND, "token not found".to_string()));
        };

        if consumed_at.is_some() {
            return Err((StatusCode::CONFLICT, "token already consumed".to_string()));
        }

        let exp =
            OffsetDateTime::parse(&expires_at, &time::format_description::well_known::Rfc3339)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if OffsetDateTime::now_utc() > exp {
            return Err((StatusCode::GONE, "token expired".to_string()));
        }

        if !ken_protocol::version::is_compatible(request.schema_version) {
            return Err((
                StatusCode::BAD_REQUEST,
                "incompatible schema version".to_string(),
            ));
        }

        let endpoint_id = EndpointId::new();
        let now = OffsetDateTime::now_utc();
        let now_str = now
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();

        // Create endpoint
        sqlx::query(
            "INSERT INTO endpoints (id, hostname, os_version, agent_version, enrolled_at, certificate_pem, certificate_expires_at, display_name) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(endpoint_id.to_string())
        .bind(&request.hostname)
        .bind(&request.os_version)
        .bind(&request.agent_version)
        .bind(&now_str)
        .bind("test-cert")
        .bind(&now_str)
        .bind(&display_name)
        .execute(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        // Consume token
        sqlx::query("UPDATE enrollment_tokens SET consumed_at = ? WHERE token = ?")
            .bind(&now_str)
            .bind(&token_value)
            .execute(&state.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        Ok(Json(EnrollmentResponse {
            endpoint_id,
            ca_certificate_pem: "test-ca-cert".to_string(),
            client_certificate_pem: "test-client-cert".to_string(),
            client_private_key_pem: "test-client-key".to_string(),
            server_url: "https://test:8443".to_string(),
            issued_at: now,
            certificate_expires_at: now,
        }))
    }

    Router::new()
        .route("/enroll/{token}", post(enroll_handler))
        .with_state(TestState { pool })
}

// NOTE: This test file uses hand-rolled handlers that do NOT exercise
// the real router, the real KenAcceptor, or the real mTLS bridge. The
// real end-to-end path is tested in agent_mtls_bridge.rs. A separate
// cleanup issue should rewrite these tests to use the real router; see
// ADR-0017's "Harder" block.

/// Build the agent router with a test pool (for heartbeat tests).
///
/// The handler uses a hardcoded endpoint ID to look up the endpoint,
/// since after ADR-0016 the heartbeat body no longer carries the
/// sender's identity (that comes from the mTLS certificate).
fn agent_router_with_pool(pool: SqlitePool, endpoint_id: EndpointId) -> axum::Router {
    use axum::extract::State;
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use ken_protocol::heartbeat::HeartbeatAck;

    #[derive(Clone)]
    struct TestState {
        pool: SqlitePool,
        endpoint_id: EndpointId,
    }

    async fn heartbeat_handler(
        State(state): State<TestState>,
        Json(heartbeat): Json<Heartbeat>,
    ) -> Result<Json<HeartbeatAck>, (StatusCode, String)> {
        if !ken_protocol::version::is_compatible(heartbeat.schema_version) {
            return Err((StatusCode::BAD_REQUEST, "bad schema".to_string()));
        }

        // Use the endpoint_id from the test state (simulating what the
        // real handler gets from Extension<EndpointId>).
        let endpoint_id = &state.endpoint_id;

        // Check endpoint exists
        let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM endpoints WHERE id = ?")
            .bind(endpoint_id.to_string())
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if exists.is_none() {
            return Err((StatusCode::FORBIDDEN, "unknown endpoint".to_string()));
        }

        let now = OffsetDateTime::now_utc();

        // Record heartbeat
        sqlx::query(
            "INSERT INTO heartbeats (id, endpoint_id, received_at, sent_at, schema_version, agent_version) \
             VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(heartbeat.heartbeat_id.to_string())
        .bind(endpoint_id.to_string())
        .bind(now.format(&time::format_description::well_known::Rfc3339).unwrap())
        .bind(heartbeat.sent_at.format(&time::format_description::well_known::Rfc3339).unwrap())
        .bind(heartbeat.schema_version)
        .bind(&heartbeat.agent_version)
        .execute(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        Ok(Json(HeartbeatAck {
            received_at: now,
            pending_commands: vec![],
            next_heartbeat_interval_seconds: 60,
        }))
    }

    async fn time_handler() -> Json<serde_json::Value> {
        let now = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        Json(serde_json::json!({ "now": now }))
    }

    Router::new()
        .route("/api/v1/heartbeat", post(heartbeat_handler))
        .route("/api/v1/time", get(time_handler))
        .with_state(TestState { pool, endpoint_id })
}

fn make_enrollment_request(token: &str) -> EnrollmentRequest {
    EnrollmentRequest {
        schema_version: SCHEMA_VERSION,
        enrollment_token: token.to_string(),
        agent_version: "0.1.0".to_string(),
        os_version: "Windows 11 24H2".to_string(),
        hostname: "DESKTOP-TEST".to_string(),
        requested_at: OffsetDateTime::now_utc(),
    }
}

fn make_heartbeat() -> Heartbeat {
    Heartbeat {
        heartbeat_id: HeartbeatId::new(),
        schema_version: SCHEMA_VERSION,
        agent_version: "0.1.0".to_string(),
        sent_at: OffsetDateTime::now_utc(),
        status: OsStatusSnapshot {
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
        },
        audit_tail: vec![],
    }
}

// --- Enrollment tests ---

#[tokio::test]
async fn enrollment_happy_path() {
    let pool = test_pool().await;
    create_token(&pool, "test-token", &future_str()).await;

    let app = admin_router_with_pool(pool.clone());
    let body = serde_json::to_string(&make_enrollment_request("test-token")).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/enroll/test-token")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let resp: EnrollmentResponse = serde_json::from_slice(&body).unwrap();
    assert!(!resp.endpoint_id.to_string().is_empty());

    // Verify endpoint exists in database
    let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM endpoints")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1);
}

#[tokio::test]
async fn enrollment_unknown_token_returns_404() {
    let pool = test_pool().await;
    let app = admin_router_with_pool(pool);

    let body = serde_json::to_string(&make_enrollment_request("no-such-token")).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/enroll/no-such-token")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn enrollment_expired_token_returns_410() {
    let pool = test_pool().await;
    create_token(&pool, "expired-token", &past_str()).await;

    let app = admin_router_with_pool(pool);
    let body = serde_json::to_string(&make_enrollment_request("expired-token")).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/enroll/expired-token")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::GONE);
}

#[tokio::test]
async fn enrollment_consumed_token_returns_409() {
    let pool = test_pool().await;
    create_token(&pool, "used-token", &future_str()).await;

    // Consume the token
    sqlx::query("UPDATE enrollment_tokens SET consumed_at = ? WHERE token = ?")
        .bind(now_str())
        .bind("used-token")
        .execute(&pool)
        .await
        .unwrap();

    let app = admin_router_with_pool(pool);
    let body = serde_json::to_string(&make_enrollment_request("used-token")).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/enroll/used-token")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
}

// --- Heartbeat tests ---

#[tokio::test]
async fn heartbeat_happy_path() {
    let pool = test_pool().await;
    let endpoint_id = EndpointId::new();
    create_endpoint(&pool, &endpoint_id).await;

    let app = agent_router_with_pool(pool.clone(), endpoint_id);
    let body = serde_json::to_string(&make_heartbeat()).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/heartbeat")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let ack: HeartbeatAck = serde_json::from_slice(&body).unwrap();
    assert_eq!(ack.next_heartbeat_interval_seconds, 60);
    assert!(ack.pending_commands.is_empty());

    // Verify heartbeat was recorded
    let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM heartbeats")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1);
}

#[tokio::test]
async fn heartbeat_unknown_endpoint_returns_403() {
    let pool = test_pool().await;
    let unknown_id = EndpointId::new();
    let app = agent_router_with_pool(pool, unknown_id);

    let body = serde_json::to_string(&make_heartbeat()).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/heartbeat")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// --- Time endpoint test ---

#[tokio::test]
async fn time_endpoint_returns_timestamp() {
    let pool = test_pool().await;
    let app = agent_router_with_pool(pool, EndpointId::new());

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/time")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let time_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(time_resp["now"].is_string());
}
