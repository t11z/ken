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
mod state;
mod storage;

use ca::Ca;
use config::Config;
use state::AppState;
use storage::Storage;

/// Initialize the tracing subscriber based on config.
fn init_tracing(logging: &config::LoggingConfig) -> anyhow::Result<()> {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&logging.level));

    match logging.format {
        config::LogFormat::Json => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .json()
                .init();
        }
        config::LogFormat::Text => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .init();
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI args for --config
    let config_path = parse_config_path();

    let config = Config::load(config_path.as_deref())?;
    init_tracing(&config.logging)?;
    config.log_summary();

    let storage = Storage::connect(&config.storage).await?;
    storage.migrate().await?;

    let tls_config = config.resolved_tls();
    let ca = Ca::load_or_create(&tls_config, &config.server.public_url)?;

    let _state = AppState {
        storage,
        ca: Arc::new(ca),
        config: Arc::new(config),
    };

    tracing::info!("ken-server foundations initialized");

    // HTTP server wiring comes in section 4
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
