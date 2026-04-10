//! Main worker loop for the Ken agent service.
//!
//! Orchestrates heartbeat collection, server communication, and
//! command processing. Runs inside the service's tokio runtime.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ken_protocol::audit::AuditEventKind;
use ken_protocol::heartbeat::Heartbeat;
use ken_protocol::ids::HeartbeatId;

use crate::audit::AuditLogger;
use crate::config::{AgentConfig, DataPaths, EnrolledCredentials};
use crate::net::client::KenApiClient;
use crate::observer::snapshot::collect_snapshot;
use crate::worker::commands;

/// Run the main worker loop until shutdown is signalled.
///
/// # Errors
///
/// Returns an error if the initial configuration or client setup fails.
pub async fn run(shutdown: Arc<AtomicBool>, paths: &DataPaths) -> Result<(), anyhow::Error> {
    let config = AgentConfig::load(&paths.config_file)?;

    if !config.is_enrolled() {
        tracing::warn!("agent is not enrolled, worker loop idle");
        while !shutdown.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        return Ok(());
    }

    let credentials = EnrolledCredentials::load(paths)?;
    let client = KenApiClient::new(&credentials, &config.server.url)?;
    let audit = Arc::new(AuditLogger::open(
        &paths.audit_log,
        config.audit.max_log_size_bytes,
    )?);

    audit.log(AuditEventKind::ServiceStarted, "worker loop started");

    // Create shared consent state for the pipe server and command
    // processor to communicate through.
    let consent_state = commands::new_consent_state();

    // Start the Named Pipe IPC server on Windows.
    #[cfg(windows)]
    {
        let cs = consent_state.clone();
        let sd = shutdown.clone();
        let au = audit.clone();
        let pa = Arc::new(DataPaths::new(&crate::config::data_dir()));
        tokio::task::spawn_blocking(move || {
            crate::ipc::server::run(cs, sd, au, pa);
        });
    }

    let mut heartbeat_interval = Duration::from_secs(u64::from(config.heartbeat.interval_seconds));

    while !shutdown.load(Ordering::SeqCst) {
        if crate::killswitch::is_active(&paths.kill_switch_file) {
            tracing::warn!("kill switch active, exiting worker loop");
            break;
        }

        let status = collect_snapshot();

        let heartbeat = Heartbeat {
            heartbeat_id: HeartbeatId::new(),
            endpoint_id: credentials.endpoint_id,
            schema_version: ken_protocol::SCHEMA_VERSION,
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            sent_at: time::OffsetDateTime::now_utc(),
            status,
            audit_tail: audit.recent(50),
        };

        match client.send_heartbeat(&heartbeat).await {
            Ok(ack) => {
                audit.log(AuditEventKind::HeartbeatSent, "heartbeat acknowledged");
                heartbeat_interval =
                    Duration::from_secs(u64::from(ack.next_heartbeat_interval_seconds));

                for command in &ack.pending_commands {
                    audit.log(
                        AuditEventKind::CommandReceived {
                            command_id: command.command_id,
                        },
                        &format!("received command {}", command.command_id),
                    );

                    let outcome = commands::process(command, &consent_state).await;
                    audit.log(
                        AuditEventKind::CommandCompleted {
                            command_id: outcome.command_id,
                            result: outcome.result.clone(),
                        },
                        &format!("command {} completed", outcome.command_id),
                    );

                    if let Err(e) = client.report_command_outcomes(&[outcome]).await {
                        tracing::warn!("failed to report outcome: {e}");
                    }
                }
            }
            Err(e) => {
                tracing::warn!("heartbeat failed: {e}, will retry");
                audit.log(
                    AuditEventKind::Error {
                        context: "heartbeat failed".to_string(),
                    },
                    &format!("{e}"),
                );
            }
        }

        // Sleep with jitter, respecting shutdown.
        let jitter_secs = u64::from(config.heartbeat.jitter_seconds);
        let jitter = if jitter_secs > 0 {
            Duration::from_secs(simple_random() % (jitter_secs + 1))
        } else {
            Duration::ZERO
        };
        let total = heartbeat_interval + jitter;

        tokio::select! {
            () = tokio::time::sleep(total) => {},
            () = wait_for_shutdown(&shutdown) => break,
        }
    }

    audit.log(AuditEventKind::ServiceStopped, "worker loop exiting");
    Ok(())
}

async fn wait_for_shutdown(shutdown: &AtomicBool) {
    while !shutdown.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn simple_random() -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn worker_exits_on_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::new(dir.path());

        // Create a minimal config (not enrolled).
        std::fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        std::fs::write(&paths.config_file, "").unwrap();

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        shutdown_clone.store(true, Ordering::SeqCst);

        let result = run(shutdown, &paths).await;
        assert!(result.is_ok());
    }
}
