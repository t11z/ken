//! Integration tests for the mTLS verifier-to-handler bridge.
//!
//! These tests exercise the real `KenAcceptor`, the real
//! `KenClientCertVerifier`, and the real agent router on an ephemeral
//! TLS listener. They prove the eight invariants listed in the test
//! names below.
//!
//! See ADR-0004 (two-listener split), ADR-0008 (verifier design),
//! ADR-0016 (identity from cert only), and ADR-0017 (acceptor bridge).

use std::net::SocketAddr;
use std::sync::Arc;

use ken_protocol::enrollment::EnrollmentRequest;
use ken_protocol::heartbeat::Heartbeat;
use ken_protocol::ids::{EndpointId, HeartbeatId};
use ken_protocol::status::{
    BitLockerStatus, DefenderStatus, FirewallStatus, Observation, OsStatusSnapshot,
    WindowsUpdateStatus,
};
use ken_protocol::SCHEMA_VERSION;
use ken_server::ca::Ca;
use ken_server::config::Config;
use ken_server::http;
use ken_server::state::AppState;
use ken_server::storage::{NewEndpoint, Storage};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use time::OffsetDateTime;

/// Format a timestamp as RFC 3339 for database fields.
fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

/// Build a minimal `Config` by parsing an empty TOML string (all fields
/// have `#[serde(default)]` so this produces a valid config).
fn test_config() -> Config {
    toml::from_str("").unwrap()
}

/// Build a heartbeat body. After ADR-0016, the heartbeat does not carry
/// the sender's identity -- that comes from the mTLS certificate.
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

/// Build a reqwest client that trusts the given CA and presents a client
/// certificate identity. Mirrors the shape of `KenApiClient::new` in
/// `crates/ken-agent/src/net/client.rs`.
fn build_mtls_client(ca_cert_pem: &str, cert_pem: &str, key_pem: &str) -> reqwest::Client {
    let ca = reqwest::Certificate::from_pem(ca_cert_pem.as_bytes()).unwrap();
    let identity_pem = format!("{cert_pem}\n{key_pem}");
    let identity = reqwest::Identity::from_pem(identity_pem.as_bytes()).unwrap();

    reqwest::Client::builder()
        .use_rustls_tls()
        .tls_built_in_root_certs(false)
        .add_root_certificate(ca)
        .identity(identity)
        .build()
        .unwrap()
}

/// Everything a test needs to talk to a running agent listener.
///
/// Each test creates its own instance via `start()` so tests are fully
/// isolated: own tempdir, own CA, own database, own server port.
struct AgentTestServer {
    /// The address the agent listener is bound to.
    addr: SocketAddr,
    /// Storage handle for assertions via the `Storage` API.
    storage: Storage,
    /// The Ken CA, for signing client certificates.
    ca: Arc<Ca>,
    /// Root CA certificate PEM (trusted root for the client).
    ca_cert_pem: String,
    /// CA private key PEM, for `rcgen`-based custom cert generation in
    /// tests that need unusual certificates (expired, non-UUID CN).
    ca_key_pem: String,
    /// Raw pool connected to the same database file, for direct SQL
    /// assertions (`SELECT COUNT(*) FROM heartbeats`) and for setting
    /// columns like `revoked_at` that the `Storage` API does not expose.
    raw_pool: SqlitePool,
    /// Held to keep the tempdir alive for the test's duration.
    _dir: tempfile::TempDir,
}

impl AgentTestServer {
    /// Start a real agent listener on an ephemeral port with the full
    /// mTLS stack: `KenClientCertVerifier` + `KenAcceptor` + `agent_router`.
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();

        // --- TLS / CA ---
        let tls_config = ken_server::config::TlsConfig {
            ca_certificate_path: dir.path().join("ca/root.crt"),
            ca_key_path: dir.path().join("ca/root.key"),
            server_certificate_path: dir.path().join("server/server.crt"),
            server_key_path: dir.path().join("server/server.key"),
        };
        let ca = Ca::load_or_create(&tls_config, "localhost").unwrap();
        let ca_cert_pem = ca.root_certificate_pem().to_string();
        let ca_key_pem = std::fs::read_to_string(&tls_config.ca_key_path).unwrap();

        // --- Database (file-based so we can open a raw pool too) ---
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let storage_config = ken_server::config::StorageConfig {
            data_dir: data_dir.clone(),
        };
        let storage = Storage::connect(&storage_config).await.unwrap();
        storage.migrate().await.unwrap();

        let db_path = data_dir.join("ken.db");
        let raw_options = SqliteConnectOptions::new()
            .filename(&db_path)
            .foreign_keys(true);
        let raw_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(raw_options)
            .await
            .unwrap();

        // --- Server TLS + acceptor ---
        let ca = Arc::new(ca);
        let client_verifier =
            Arc::new(http::tls::KenClientCertVerifier::new(storage.clone(), &ca).unwrap());
        let server_cert_pem = std::fs::read_to_string(&tls_config.server_certificate_path).unwrap();
        let server_key_pem = std::fs::read_to_string(&tls_config.server_key_path).unwrap();
        let server_tls_config = http::tls::build_server_tls_config(
            &server_cert_pem,
            &server_key_pem,
            Some(client_verifier),
        )
        .unwrap();

        let rustls_config =
            axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(server_tls_config));
        let acceptor = http::tls::KenAcceptor::new(rustls_config);

        let config = test_config();
        let state = AppState {
            storage: storage.clone(),
            ca: ca.clone(),
            config: Arc::new(config),
        };
        let agent_app = http::agent_router(state);

        // --- Bind and serve ---
        let handle = axum_server::Handle::new();
        let server_handle = handle.clone();

        tokio::spawn(async move {
            axum_server::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
                .handle(server_handle)
                .acceptor(acceptor)
                .serve(agent_app.into_make_service())
                .await
                .unwrap();
        });

        let addr = handle.listening().await.unwrap();

        Self {
            addr,
            storage,
            ca,
            ca_cert_pem,
            ca_key_pem,
            raw_pool,
            _dir: dir,
        }
    }

    fn heartbeat_url(&self) -> String {
        format!("https://localhost:{}/api/v1/heartbeat", self.addr.port())
    }

    /// Assert that no heartbeat rows exist in the database -- proof that
    /// a rejected handshake never reached a handler.
    async fn assert_no_heartbeat_rows(&self) {
        let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM heartbeats")
            .fetch_one(&self.raw_pool)
            .await
            .unwrap();
        assert_eq!(
            count.0, 0,
            "no heartbeat row should exist after a rejected handshake"
        );
    }
}

// ---------------------------------------------------------------------------
// Test cases
// ---------------------------------------------------------------------------

/// 1. A valid enrolled endpoint can POST a heartbeat and the server
/// records it. This proves the full mTLS bridge works end-to-end.
#[tokio::test(flavor = "multi_thread")]
async fn happy_path_authenticated_client_reaches_handler() {
    let s = AgentTestServer::start().await;

    let endpoint_id = EndpointId::new();
    let signed = s.ca.sign_client_certificate(&endpoint_id, 365).unwrap();
    s.storage
        .create_endpoint(&NewEndpoint {
            id: endpoint_id.to_string(),
            hostname: "HAPPY-PC".to_string(),
            os_version: "Windows 11".to_string(),
            agent_version: "0.1.0".to_string(),
            enrolled_at: rfc3339(OffsetDateTime::now_utc()),
            certificate_pem: signed.certificate_pem.clone(),
            certificate_expires_at: rfc3339(signed.expires_at),
            display_name: None,
        })
        .await
        .unwrap();

    let client = build_mtls_client(
        &s.ca_cert_pem,
        &signed.certificate_pem,
        &signed.private_key_pem,
    );
    let heartbeat = make_heartbeat();

    let resp = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.post(s.heartbeat_url()).json(&heartbeat).send(),
    )
    .await
    .expect("request timed out")
    .expect("request should succeed");

    assert_eq!(resp.status(), 200, "enrolled endpoint should get 200");

    let ack: ken_protocol::heartbeat::HeartbeatAck = resp.json().await.unwrap();
    assert_eq!(ack.next_heartbeat_interval_seconds, 60);

    // Verify the heartbeat was recorded in the database.
    let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM heartbeats")
        .fetch_one(&s.raw_pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1, "exactly one heartbeat row should exist");

    let endpoint = s
        .storage
        .get_endpoint(&endpoint_id)
        .await
        .unwrap()
        .expect("endpoint should exist");
    assert!(
        endpoint.last_heartbeat_at.is_some(),
        "heartbeat should have been recorded"
    );
}

/// 2. A revoked endpoint is rejected at the TLS handshake (ADR-0008
/// step 5). The request never reaches a handler.
#[tokio::test(flavor = "multi_thread")]
async fn revoked_endpoint_is_rejected_at_handshake() {
    let s = AgentTestServer::start().await;

    let endpoint_id = EndpointId::new();
    let signed = s.ca.sign_client_certificate(&endpoint_id, 365).unwrap();
    s.storage
        .create_endpoint(&NewEndpoint {
            id: endpoint_id.to_string(),
            hostname: "REVOKED-PC".to_string(),
            os_version: "Windows 11".to_string(),
            agent_version: "0.1.0".to_string(),
            enrolled_at: rfc3339(OffsetDateTime::now_utc()),
            certificate_pem: signed.certificate_pem.clone(),
            certificate_expires_at: rfc3339(signed.expires_at),
            display_name: None,
        })
        .await
        .unwrap();

    // Revoke the endpoint via raw SQL (Storage API does not expose this).
    sqlx::query("UPDATE endpoints SET revoked_at = ? WHERE id = ?")
        .bind(rfc3339(OffsetDateTime::now_utc()))
        .bind(endpoint_id.to_string())
        .execute(&s.raw_pool)
        .await
        .unwrap();

    let client = build_mtls_client(
        &s.ca_cert_pem,
        &signed.certificate_pem,
        &signed.private_key_pem,
    );

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client
            .post(s.heartbeat_url())
            .json(&make_heartbeat())
            .send(),
    )
    .await
    .expect("request timed out");

    assert!(
        result.is_err(),
        "revoked client should fail at TLS handshake, got: {result:?}"
    );

    s.assert_no_heartbeat_rows().await;

    let endpoint = s.storage.get_endpoint(&endpoint_id).await.unwrap().unwrap();
    assert!(
        endpoint.last_heartbeat_at.is_none(),
        "last_heartbeat_at should still be NULL"
    );
}

/// 3. A client presenting a certificate signed by a different CA is
/// rejected at the TLS handshake (ADR-0008 step 1, `WebPki` chain check).
#[tokio::test(flavor = "multi_thread")]
async fn wrong_ca_is_rejected_at_handshake() {
    let s = AgentTestServer::start().await;

    // Generate a second CA that the server does not trust.
    let wrong_ca = Ca::generate_ca_for_test();
    let fake_endpoint_id = EndpointId::new();
    let wrong_signed = wrong_ca
        .sign_client_certificate(&fake_endpoint_id, 365)
        .unwrap();

    // The client trusts the *real* CA (so it accepts the server's cert)
    // but presents a client certificate signed by the wrong CA.
    let client = build_mtls_client(
        &s.ca_cert_pem,
        &wrong_signed.certificate_pem,
        &wrong_signed.private_key_pem,
    );

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client
            .post(s.heartbeat_url())
            .json(&make_heartbeat())
            .send(),
    )
    .await
    .expect("request timed out");

    assert!(
        result.is_err(),
        "wrong-CA client should fail at TLS handshake, got: {result:?}"
    );

    s.assert_no_heartbeat_rows().await;
}

/// 4a. A client certificate whose `notAfter` is in the past is rejected
/// at the TLS handshake by `WebPki` (ADR-0008 step 1). The endpoint row
/// in the database is fully valid.
#[tokio::test(flavor = "multi_thread")]
async fn cert_with_expired_notafter_is_rejected_at_handshake() {
    let s = AgentTestServer::start().await;

    let endpoint_id = EndpointId::new();

    // Create a valid endpoint row (certificate_expires_at far in the future).
    let future_expiry = OffsetDateTime::now_utc() + time::Duration::days(365);
    s.storage
        .create_endpoint(&NewEndpoint {
            id: endpoint_id.to_string(),
            hostname: "EXPIRED-CERT-PC".to_string(),
            os_version: "Windows 11".to_string(),
            agent_version: "0.1.0".to_string(),
            enrolled_at: rfc3339(OffsetDateTime::now_utc()),
            certificate_pem: "placeholder".to_string(),
            certificate_expires_at: rfc3339(future_expiry),
            display_name: None,
        })
        .await
        .unwrap();

    // Use rcgen directly to create a cert that is already expired.
    let client_key = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::default();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, endpoint_id.to_string());
    params
        .key_usages
        .push(rcgen::KeyUsagePurpose::DigitalSignature);
    params
        .extended_key_usages
        .push(rcgen::ExtendedKeyUsagePurpose::ClientAuth);

    let yesterday = OffsetDateTime::now_utc() - time::Duration::days(1);
    params.not_before = yesterday - time::Duration::days(1);
    params.not_after = yesterday;

    let ca_key = rcgen::KeyPair::from_pem(&s.ca_key_pem).unwrap();
    let issuer = rcgen::Issuer::from_ca_cert_pem(s.ca.root_certificate_pem(), &ca_key).unwrap();
    let cert = params.signed_by(&client_key, &issuer).unwrap();

    let cert_pem = cert.pem();
    let key_pem = client_key.serialize_pem();

    let client = build_mtls_client(&s.ca_cert_pem, &cert_pem, &key_pem);

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client
            .post(s.heartbeat_url())
            .json(&make_heartbeat())
            .send(),
    )
    .await
    .expect("request timed out");

    assert!(
        result.is_err(),
        "expired-notAfter cert should fail at TLS handshake, got: {result:?}"
    );

    s.assert_no_heartbeat_rows().await;
}

/// 4b. A client certificate with a valid `notAfter` but an expired
/// `certificate_expires_at` database record is rejected at the TLS
/// handshake (ADR-0008 step 5, database-driven expiry). This lets the
/// operator expire a cert without rotating key material.
#[tokio::test(flavor = "multi_thread")]
async fn endpoint_with_expired_db_record_is_rejected_at_handshake() {
    let s = AgentTestServer::start().await;

    let endpoint_id = EndpointId::new();
    let signed = s.ca.sign_client_certificate(&endpoint_id, 365).unwrap();

    // Create endpoint with certificate_expires_at in the past.
    let past_expiry = OffsetDateTime::now_utc() - time::Duration::days(1);
    s.storage
        .create_endpoint(&NewEndpoint {
            id: endpoint_id.to_string(),
            hostname: "EXPIRED-DB-PC".to_string(),
            os_version: "Windows 11".to_string(),
            agent_version: "0.1.0".to_string(),
            enrolled_at: rfc3339(OffsetDateTime::now_utc()),
            certificate_pem: signed.certificate_pem.clone(),
            certificate_expires_at: rfc3339(past_expiry),
            display_name: None,
        })
        .await
        .unwrap();

    let client = build_mtls_client(
        &s.ca_cert_pem,
        &signed.certificate_pem,
        &signed.private_key_pem,
    );

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client
            .post(s.heartbeat_url())
            .json(&make_heartbeat())
            .send(),
    )
    .await
    .expect("request timed out");

    assert!(
        result.is_err(),
        "expired DB record should fail at TLS handshake, got: {result:?}"
    );

    s.assert_no_heartbeat_rows().await;

    let endpoint = s.storage.get_endpoint(&endpoint_id).await.unwrap().unwrap();
    assert!(
        endpoint.last_heartbeat_at.is_none(),
        "last_heartbeat_at should still be NULL"
    );
}

/// 5a. A client certificate signed by the Ken CA but with a non-UUID
/// CN is rejected at the TLS handshake (ADR-0008 step 3).
#[tokio::test(flavor = "multi_thread")]
async fn client_cert_with_non_uuid_cn_is_rejected_at_handshake() {
    let s = AgentTestServer::start().await;

    // Use rcgen directly because Ca::sign_client_certificate takes a
    // typed EndpointId and would refuse a non-UUID string.
    let client_key = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::default();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "not-a-uuid");
    params
        .key_usages
        .push(rcgen::KeyUsagePurpose::DigitalSignature);
    params
        .extended_key_usages
        .push(rcgen::ExtendedKeyUsagePurpose::ClientAuth);

    let now = OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + time::Duration::days(365);

    let ca_key = rcgen::KeyPair::from_pem(&s.ca_key_pem).unwrap();
    let issuer = rcgen::Issuer::from_ca_cert_pem(s.ca.root_certificate_pem(), &ca_key).unwrap();
    let cert = params.signed_by(&client_key, &issuer).unwrap();

    let cert_pem = cert.pem();
    let key_pem = client_key.serialize_pem();

    let client = build_mtls_client(&s.ca_cert_pem, &cert_pem, &key_pem);

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client
            .post(s.heartbeat_url())
            .json(&make_heartbeat())
            .send(),
    )
    .await
    .expect("request timed out");

    assert!(
        result.is_err(),
        "non-UUID CN should fail at TLS handshake, got: {result:?}"
    );

    s.assert_no_heartbeat_rows().await;
}

/// 5b. A client certificate with a syntactically valid UUID CN that
/// does not correspond to any enrolled endpoint is rejected at the TLS
/// handshake (ADR-0008 step 4, database lookup returns `None`).
#[tokio::test(flavor = "multi_thread")]
async fn client_cert_with_unknown_endpoint_uuid_is_rejected_at_handshake() {
    let s = AgentTestServer::start().await;

    // Sign a cert for an endpoint ID that is NOT in the database.
    let unknown_id = EndpointId::new();
    let signed = s.ca.sign_client_certificate(&unknown_id, 365).unwrap();

    let client = build_mtls_client(
        &s.ca_cert_pem,
        &signed.certificate_pem,
        &signed.private_key_pem,
    );

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client
            .post(s.heartbeat_url())
            .json(&make_heartbeat())
            .send(),
    )
    .await
    .expect("request timed out");

    assert!(
        result.is_err(),
        "unknown endpoint UUID should fail at TLS handshake, got: {result:?}"
    );

    s.assert_no_heartbeat_rows().await;
}

/// 6. The admin listener accepts connections without a client certificate
/// (ADR-0004 invariant). The handler returns 404 because the enrollment
/// token does not exist, but the point is that the TLS handshake
/// succeeded without a client identity.
#[tokio::test(flavor = "multi_thread")]
async fn admin_listener_does_not_require_client_cert() {
    let dir = tempfile::tempdir().unwrap();
    let tls_config = ken_server::config::TlsConfig {
        ca_certificate_path: dir.path().join("ca/root.crt"),
        ca_key_path: dir.path().join("ca/root.key"),
        server_certificate_path: dir.path().join("server/server.crt"),
        server_key_path: dir.path().join("server/server.key"),
    };

    let ca = Ca::load_or_create(&tls_config, "localhost").unwrap();
    let ca_cert_pem = ca.root_certificate_pem().to_string();

    let storage = Storage::connect_in_memory().await.unwrap();
    storage.migrate().await.unwrap();

    // Admin listener: server TLS only, no client cert required.
    let server_cert_pem = std::fs::read_to_string(&tls_config.server_certificate_path).unwrap();
    let server_key_pem = std::fs::read_to_string(&tls_config.server_key_path).unwrap();
    let admin_tls_config =
        http::tls::build_server_tls_config(&server_cert_pem, &server_key_pem, None).unwrap();

    let rustls_config =
        axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(admin_tls_config));

    let config = test_config();
    let state = AppState {
        storage,
        ca: Arc::new(ca),
        config: Arc::new(config),
    };
    let admin_app = http::admin_router(state);

    let handle = axum_server::Handle::new();
    let server_handle = handle.clone();

    tokio::spawn(async move {
        axum_server::bind_rustls("127.0.0.1:0".parse::<SocketAddr>().unwrap(), rustls_config)
            .handle(server_handle)
            .serve(admin_app.into_make_service())
            .await
            .unwrap();
    });

    let addr = handle.listening().await.unwrap();

    // Build a client that trusts the Ken CA but presents NO client identity.
    let ca = reqwest::Certificate::from_pem(ca_cert_pem.as_bytes()).unwrap();
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .tls_built_in_root_certs(false)
        .add_root_certificate(ca)
        .build()
        .unwrap();

    let enrollment_request = EnrollmentRequest {
        schema_version: SCHEMA_VERSION,
        enrollment_token: "fake-token".to_string(),
        agent_version: "0.1.0".to_string(),
        os_version: "Windows 11".to_string(),
        hostname: "TEST-PC".to_string(),
        requested_at: OffsetDateTime::now_utc(),
    };

    let url = format!("https://localhost:{}/enroll/fake-token", addr.port());

    let resp = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.post(&url).json(&enrollment_request).send(),
    )
    .await
    .expect("request timed out")
    .expect("TLS handshake should succeed without client cert");

    // The handler returns 404 because the token does not exist in the
    // database. The assertion is that the request reached the handler at
    // all -- proving the admin listener does not require a client cert.
    assert_eq!(
        resp.status(),
        404,
        "should reach handler and get 404 for missing token"
    );
}
