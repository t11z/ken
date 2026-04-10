//! Integration tests for the mTLS verifier-to-handler bridge.
//!
//! These tests exercise the real `KenAcceptor`, the real
//! `KenClientCertVerifier`, and the real agent router on an ephemeral
//! TLS listener. They prove that:
//!
//! 1. An enrolled endpoint can send a heartbeat and the server records
//!    it against the certificate-derived `EndpointId`.
//! 2. A non-enrolled endpoint is rejected at the TLS handshake layer.
//!
//! See ADR-0008 and ADR-0017 for the architectural justification of this
//! bridge, and ADR-0016 for the single-source identity rule.

use std::net::SocketAddr;
use std::sync::Arc;

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
use time::OffsetDateTime;

/// Helper to format a timestamp as RFC 3339 for database fields.
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
/// the sender's identity — that comes from the mTLS certificate.
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

/// Shared setup: creates a test CA, an in-memory storage, an enrolled
/// endpoint with a signed client certificate, and starts the real agent
/// listener on an ephemeral port. Returns everything the tests need.
struct TestHarness {
    /// The address the agent listener is bound to.
    addr: SocketAddr,
    /// The enrolled endpoint's ID.
    good_endpoint_id: EndpointId,
    /// PEM certificate + key for the enrolled endpoint.
    good_cert_pem: String,
    good_key_pem: String,
    /// A non-enrolled endpoint's certificate + key (signed by the same CA).
    rogue_cert_pem: String,
    rogue_key_pem: String,
    /// Root CA certificate PEM (trusted root for the client).
    ca_cert_pem: String,
    /// Storage handle for assertions.
    storage: Storage,
}

impl TestHarness {
    async fn start() -> Self {
        // Use load_or_create with a temp dir to get a CA + server cert
        // that are signed by the same root. This is the simplest way to
        // get a complete TLS setup for testing.
        let dir = tempfile::tempdir().unwrap();
        let tls_config = ken_server::config::TlsConfig {
            ca_certificate_path: dir.path().join("ca/root.crt"),
            ca_key_path: dir.path().join("ca/root.key"),
            server_certificate_path: dir.path().join("server/server.crt"),
            server_key_path: dir.path().join("server/server.key"),
        };

        let ca = Ca::load_or_create(&tls_config, "localhost").unwrap();
        let ca_cert_pem = ca.root_certificate_pem().to_string();

        // Create in-memory storage and run migrations.
        let storage = Storage::connect_in_memory().await.unwrap();
        storage.migrate().await.unwrap();

        // Enroll the "good" endpoint.
        let good_endpoint_id = EndpointId::new();
        let good_signed = ca.sign_client_certificate(&good_endpoint_id, 365).unwrap();

        storage
            .create_endpoint(&NewEndpoint {
                id: good_endpoint_id.to_string(),
                hostname: "GOOD-PC".to_string(),
                os_version: "Windows 11".to_string(),
                agent_version: "0.1.0".to_string(),
                enrolled_at: rfc3339(OffsetDateTime::now_utc()),
                certificate_pem: good_signed.certificate_pem.clone(),
                certificate_expires_at: rfc3339(good_signed.expires_at),
                display_name: Some("Good PC".to_string()),
            })
            .await
            .unwrap();

        // Sign a certificate for a "rogue" endpoint that is NOT enrolled.
        let rogue_endpoint_id = EndpointId::new();
        let rogue_signed = ca.sign_client_certificate(&rogue_endpoint_id, 365).unwrap();

        // Build the server TLS config with the custom verifier.
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
            ca: Arc::new(ca),
            config: Arc::new(config),
        };

        let agent_app = http::agent_router(state);

        // Bind to an ephemeral port and start serving.
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
            good_endpoint_id,
            good_cert_pem: good_signed.certificate_pem,
            good_key_pem: good_signed.private_key_pem,
            rogue_cert_pem: rogue_signed.certificate_pem,
            rogue_key_pem: rogue_signed.private_key_pem,
            ca_cert_pem,
            storage,
        }
    }

    /// Build a reqwest client with the given client certificate identity.
    fn client(&self, cert_pem: &str, key_pem: &str) -> reqwest::Client {
        let ca = reqwest::Certificate::from_pem(self.ca_cert_pem.as_bytes()).unwrap();
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

    /// Build a client with the good (enrolled) endpoint's identity.
    fn good_client(&self) -> reqwest::Client {
        self.client(&self.good_cert_pem, &self.good_key_pem)
    }

    /// Build a client with the rogue (not enrolled) endpoint's identity.
    fn rogue_client(&self) -> reqwest::Client {
        self.client(&self.rogue_cert_pem, &self.rogue_key_pem)
    }

    fn heartbeat_url(&self) -> String {
        format!("https://localhost:{}/api/v1/heartbeat", self.addr.port())
    }
}

// --- Test cases ---

/// An enrolled endpoint can send a heartbeat and the server records it
/// against the certificate-derived `EndpointId`.
#[tokio::test(flavor = "multi_thread")]
async fn enrolled_endpoint_heartbeat_succeeds() {
    let h = TestHarness::start().await;
    let client = h.good_client();

    let heartbeat = make_heartbeat();

    let resp = client
        .post(h.heartbeat_url())
        .json(&heartbeat)
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 200, "enrolled endpoint should get 200");

    let ack: ken_protocol::heartbeat::HeartbeatAck = resp.json().await.unwrap();
    assert_eq!(ack.next_heartbeat_interval_seconds, 60);

    // Verify the heartbeat was recorded in the database.
    let endpoint = h
        .storage
        .get_endpoint(&h.good_endpoint_id)
        .await
        .unwrap()
        .expect("endpoint should exist");
    assert!(
        endpoint.last_heartbeat_at.is_some(),
        "heartbeat should have been recorded"
    );
}

/// A non-enrolled endpoint is rejected at the TLS handshake layer — the
/// request never reaches the handler.
#[tokio::test(flavor = "multi_thread")]
async fn unenrolled_endpoint_rejected_at_handshake() {
    let h = TestHarness::start().await;
    let client = h.rogue_client();

    let heartbeat = make_heartbeat();

    let result = client.post(h.heartbeat_url()).json(&heartbeat).send().await;

    // The connection should fail at the TLS layer.
    assert!(
        result.is_err(),
        "rogue client should fail at TLS handshake, got: {result:?}"
    );
}

/// The `require_endpoint_id` middleware returns 500 when no `EndpointId`
/// extension is present. This tests the defense-in-depth path that
/// catches wiring bugs.
#[tokio::test(flavor = "multi_thread")]
async fn middleware_returns_500_without_endpoint_id_extension() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    // Build the agent router WITHOUT going through KenAcceptor — the
    // EndpointId extension will be absent.
    let storage = Storage::connect_in_memory().await.unwrap();
    storage.migrate().await.unwrap();

    let ca = Ca::generate_ca_for_test();
    let config = test_config();

    let state = AppState {
        storage,
        ca: Arc::new(ca),
        config: Arc::new(config),
    };

    let router = http::agent_router(state);

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/heartbeat")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "missing EndpointId should yield 500"
    );
}
