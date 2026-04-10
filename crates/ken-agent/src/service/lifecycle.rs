//! Service lifecycle: start, stop, and control handler.
//!
//! This module contains the Windows service entry point and the main
//! service loop. On non-Windows platforms, it provides stub
//! implementations so the crate compiles for cross-platform CI.
//!
//! On Windows, the control handler registers `SERVICE_ACCEPT_SESSIONCHANGE`
//! and forwards `WTS_SESSION_LOGON` / `WTS_SESSION_LOGOFF` events to the
//! main service loop via a channel per ADR-0010.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The Windows service name as registered with the SCM.
pub const SERVICE_NAME: &str = "KenAgent";

/// The display name shown in services.msc.
pub const SERVICE_DISPLAY_NAME: &str = "Ken Agent";

/// Service description.
pub const SERVICE_DESCRIPTION: &str =
    "Ken \u{2014} observability and consent-gated remote access for family PCs. https://ken.family";

/// Run the main service loop.
///
/// This is called from the service entry point after the service is
/// registered and the status handle is obtained. It runs until the
/// shutdown flag is set by the control handler.
///
/// Per ADR-0012, the very first thing the service does is check the
/// kill switch state file. If active, the service refuses to start.
///
/// On non-Windows (development mode), `session_rx` is `None` because
/// there are no session-change events.
pub async fn service_loop(
    shutdown: Arc<AtomicBool>,
    #[cfg(windows)] session_rx: std::sync::mpsc::Receiver<
        crate::service::session::SessionChangeEvent,
    >,
) {
    let data_dir = crate::config::data_dir();
    let paths = crate::config::DataPaths::new(&data_dir);

    // ADR-0012: Check kill switch before any other work.
    if crate::killswitch::is_active(&paths.kill_switch_file) {
        tracing::warn!("kill switch is active, refusing to start");
        return;
    }

    tracing::info!("service loop started");

    // --- Tray process management (Windows only) ---
    // Per ADR-0009 and ADR-0010, the SYSTEM service launches
    // `ken-agent.exe tray` in each active interactive session.
    #[cfg(windows)]
    let _tray_session_handler = {
        use crate::service::session;

        let audit = Arc::new(
            crate::audit::AuditLogger::open(
                &paths.audit_log,
                crate::config::AgentConfig::load(&paths.config_file)
                    .unwrap_or_default()
                    .audit
                    .max_log_size_bytes,
            )
            .expect("failed to open audit log for tray management"),
        );

        let mut tray_map = session::TrayProcessMap::new();

        // Step 5 from the prompt: enumerate currently active sessions
        // at startup and launch tray apps for any that exist.
        session::launch_for_active_sessions(&mut tray_map, &audit);

        // Spawn a blocking task to drain session-change events from
        // the control handler channel. The control handler pushes
        // events here and returns immediately (never blocks).
        let session_audit = audit.clone();
        let session_shutdown = shutdown.clone();
        std::thread::spawn(move || {
            use crate::service::session::SessionChangeEvent;

            while !session_shutdown.load(Ordering::SeqCst) {
                // Use recv_timeout so we periodically check the shutdown flag.
                match session_rx.recv_timeout(std::time::Duration::from_millis(500)) {
                    Ok(SessionChangeEvent::Logon { session_id }) => {
                        session::handle_session_logon(
                            session_id,
                            ken_protocol::audit::TrayLaunchTrigger::SessionLogon,
                            &mut tray_map,
                            &session_audit,
                        );
                    }
                    Ok(SessionChangeEvent::Logoff { session_id }) => {
                        session::handle_session_logoff(session_id, &mut tray_map, &session_audit);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }

            // Service is shutting down — terminate all tracked tray
            // processes before we exit.
            session::terminate_all_tray_processes(&mut tray_map, &session_audit);
            tracing::info!("session-change handler exiting");
        });
    };
    // On non-Windows, session management is a no-op (no tray app).
    #[cfg(not(windows))]
    {}

    // Run the worker loop (heartbeat, commands, status collection).
    // The worker loop handles its own kill-switch checks internally.
    if let Err(e) = crate::worker::main_loop::run(shutdown.clone(), &paths).await {
        tracing::error!(error = %e, "worker loop failed");
    }

    // On Windows, signal the session-change handler thread to exit.
    // It will terminate all remaining tray processes before exiting.
    shutdown.store(true, Ordering::SeqCst);

    tracing::info!("service loop exiting");
}

// Generate the extern "system" FFI wrapper that
// service_dispatcher::start requires. The macro bridges from
// the raw (u32, *mut *mut u16) signature to our Rust-level
// service_main(Vec<OsString>).
#[cfg(windows)]
windows_service::define_windows_service!(ffi_service_main, service_main);

/// Launch the Windows service dispatcher.
///
/// This is the entry point called from `main()` when the agent is run
/// in service mode (`ken-agent.exe run-service`). It registers with
/// the SCM and delegates to the service entry point.
#[cfg(windows)]
pub fn run_service_dispatcher() {
    use windows_service::service_dispatcher;

    if let Err(e) = service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
        eprintln!("failed to start service dispatcher: {e}");
        std::process::exit(1);
    }
}

/// Service main called by the SCM via the FFI wrapper generated by
/// `define_windows_service!`.
///
/// Sets up the control handler, creates the session-change channel,
/// and launches the async service loop.
#[cfg(windows)]
fn service_main(_arguments: Vec<std::ffi::OsString>) {
    use std::sync::mpsc;
    use std::time::Duration;

    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType, SessionChangeReason,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};

    use crate::service::session::SessionChangeEvent;

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_handler = shutdown.clone();

    // Channel for forwarding session-change events from the control
    // handler (non-blocking) to the main service loop.
    let (session_tx, session_rx) = mpsc::channel::<SessionChangeEvent>();

    // Register the control handler. Per the skill file and the prompt:
    // do NOT block in the handler. Set a flag, push to a channel, return.
    let status_handle = match service_control_handler::register(
        SERVICE_NAME,
        move |control| -> ServiceControlHandlerResult {
            match control {
                ServiceControl::Stop | ServiceControl::Shutdown => {
                    shutdown_handler.store(true, Ordering::SeqCst);
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::SessionChange(param) => {
                    match param.reason {
                        SessionChangeReason::SessionLogon => {
                            let _ = session_tx.send(SessionChangeEvent::Logon {
                                session_id: param.notification.session_id,
                            });
                        }
                        SessionChangeReason::SessionLogoff => {
                            let _ = session_tx.send(SessionChangeEvent::Logoff {
                                session_id: param.notification.session_id,
                            });
                        }
                        _ => {
                            // Other session events (lock, unlock, remote
                            // connect/disconnect) are not relevant.
                        }
                    }
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        },
    ) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("failed to register service control handler: {e}");
            return;
        }
    };

    // Report Running status with SESSION_CHANGE acceptance.
    let running_status = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP
            | ServiceControlAccept::SHUTDOWN
            | ServiceControlAccept::SESSION_CHANGE,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::ZERO,
        process_id: None,
    };

    if let Err(e) = status_handle.set_service_status(running_status) {
        eprintln!("failed to set running status: {e}");
        return;
    }

    // Initialize tracing before entering the service loop.
    tracing_subscriber::fmt::init();

    // Run the async service loop with session-change events.
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(service_loop(shutdown.clone(), session_rx));

    // Report Stopped status.
    let stopped_status = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::ZERO,
        process_id: None,
    };

    let _ = status_handle.set_service_status(stopped_status);
}

/// Check if the kill switch has been activated.
///
/// Returns `true` if the kill-switch file exists, meaning a user
/// previously activated it and the service should refuse to start.
#[must_use]
pub fn is_kill_switch_active(kill_switch_path: &std::path::Path) -> bool {
    kill_switch_path.exists()
}

/// Install the Windows service.
///
/// On non-Windows platforms, this prints a message and returns.
#[cfg(windows)]
pub fn install_service() -> Result<(), anyhow::Error> {
    use std::ffi::OsString;
    use windows_service::service::{
        ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceType,
    };
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    let manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CREATE_SERVICE)?;

    let service_binary = std::env::current_exe()?;

    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: service_binary,
        launch_arguments: vec![OsString::from("run-service")],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };

    let _service = manager.create_service(&service_info, ServiceAccess::CHANGE_CONFIG)?;
    println!("Service '{SERVICE_NAME}' installed successfully.");
    Ok(())
}

#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)]
pub fn install_service() -> Result<(), anyhow::Error> {
    println!("Service installation is only supported on Windows.");
    Ok(())
}

/// Uninstall the Windows service.
#[cfg(windows)]
pub fn uninstall_service() -> Result<(), anyhow::Error> {
    use windows_service::service::ServiceAccess;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service =
        manager.open_service(SERVICE_NAME, ServiceAccess::DELETE | ServiceAccess::STOP)?;

    // Try to stop the service first
    let _ = service.stop();
    std::thread::sleep(std::time::Duration::from_secs(1));

    service.delete()?;
    println!("Service '{SERVICE_NAME}' uninstalled successfully.");
    Ok(())
}

#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)]
pub fn uninstall_service() -> Result<(), anyhow::Error> {
    println!("Service uninstallation is only supported on Windows.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_switch_not_active_when_file_missing() {
        let path = std::path::Path::new("/nonexistent/kill-switch");
        assert!(!is_kill_switch_active(path));
    }

    #[test]
    fn kill_switch_active_when_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kill-switch-requested");
        std::fs::write(&path, "activated").unwrap();
        assert!(is_kill_switch_active(&path));
    }
}
