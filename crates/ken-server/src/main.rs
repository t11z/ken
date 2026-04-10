//! Ken server — observability dashboard and agent coordinator.
//!
//! The server runs on the family IT chief's hardware (typically a
//! Raspberry Pi) and accepts heartbeats from enrolled Ken agents
//! over mTLS. It provides an admin web UI for monitoring endpoint
//! health and issuing commands.

use std::path::PathBuf;
use std::sync::Arc;

mod ca;
mod config;
mod error;
mod http;
mod state;
mod storage;

use ca::Ca;
use config::Config;
use state::AppState;
use storage::Storage;

/// Initialize the tracing subscriber based on config.
fn init_tracing(logging: &config::LoggingConfig) {
    use tracing_subscriber::EnvFilter;

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&logging.level));

    match logging.format {
        config::LogFormat::Json => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .json()
                .init();
        }
        config::LogFormat::Text => {
            tracing_subscriber::fmt().with_env_filter(filter).init();
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI args for --config
    let config_path = parse_config_path();

    let config = Config::load(config_path.as_deref())?;
    init_tracing(&config.logging);
    config.log_summary();

    let storage = Storage::connect(&config.storage).await?;
    storage.migrate().await?;

    let tls_config = config.resolved_tls();
    let ca = Ca::load_or_create(&tls_config, &config.server.public_url)?;

    let admin_addr = config.server.admin_listen_address;
    let agent_addr = config.server.agent_listen_address;

    let state = AppState {
        storage,
        ca: Arc::new(ca),
        config: Arc::new(config),
    };

    // Ensure admin access token exists (generates and logs it on first run)
    http::auth::ensure_admin_token(&state).await?;

    // Build routers
    let admin_app = http::admin_router(state.clone());
    let agent_app = http::agent_router(state.clone());

    // Load server TLS certificate and key for both listeners
    let server_cert_pem = std::fs::read_to_string(&tls_config.server_certificate_path)?;
    let server_key_pem = std::fs::read_to_string(&tls_config.server_key_path)?;

    // Build the custom client cert verifier for the agent listener (ADR-0008)
    let client_verifier = Arc::new(http::tls::KenClientCertVerifier::new(
        state.storage.clone(),
        &state.ca,
    )?);

    // Admin listener: server TLS only, no client cert required
    let admin_tls_config =
        http::tls::build_server_tls_config(&server_cert_pem, &server_key_pem, None)?;
    let admin_rustls =
        axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(admin_tls_config));

    // Agent listener: mTLS with custom client cert verifier
    let agent_tls_config = http::tls::build_server_tls_config(
        &server_cert_pem,
        &server_key_pem,
        Some(client_verifier),
    )?;
    let agent_rustls =
        axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(agent_tls_config));

    tracing::info!(
        admin = %admin_addr,
        agent = %agent_addr,
        "starting TLS listeners"
    );

    tracing::info!("ken-server ready");

    // Run both TLS listeners concurrently (ADR-0004)
    tokio::select! {
        result = axum_server::bind_rustls(admin_addr, admin_rustls).serve(admin_app.into_make_service()) => {
            if let Err(e) = result {
                tracing::error!(error = %e, "admin listener failed");
            }
        }
        result = axum_server::bind_rustls(agent_addr, agent_rustls).serve(agent_app.into_make_service()) => {
            if let Err(e) = result {
                tracing::error!(error = %e, "agent listener failed");
            }
        }
    }

    Ok(())
}

/// Simple CLI argument parsing for the `--config` flag.
fn parse_config_path() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    for (i, arg) in args.iter().enumerate() {
        if arg == "--config" {
            return args.get(i + 1).map(PathBuf::from);
        }
        if let Some(path) = arg.strip_prefix("--config=") {
            return Some(PathBuf::from(path));
        }
    }
    None
}
