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
#[allow(dead_code)] // Many storage methods are prepared for use in later sections
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
    let agent_app = http::agent_router(state);

    tracing::info!(
        admin = %admin_addr,
        agent = %agent_addr,
        "starting listeners"
    );

    // Run both listeners concurrently.
    // The agent listener should use mTLS (rustls with client cert verifier),
    // but for Phase 1 we start both as plain TCP+TLS or plain HTTP for
    // development. Full mTLS enforcement is wired up when the TLS listener
    // module is completed.
    let admin_listener = tokio::net::TcpListener::bind(admin_addr).await?;
    let agent_listener = tokio::net::TcpListener::bind(agent_addr).await?;

    tracing::info!("ken-server ready");

    tokio::select! {
        result = axum::serve(admin_listener, admin_app) => {
            if let Err(e) = result {
                tracing::error!(error = %e, "admin listener failed");
            }
        }
        result = axum::serve(agent_listener, agent_app) => {
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
