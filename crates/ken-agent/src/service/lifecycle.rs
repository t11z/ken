//! Service lifecycle: start, stop, and control handler.
//!
//! This module contains the Windows service entry point and the main
//! service loop. On non-Windows platforms, it provides stub
//! implementations so the crate compiles for cross-platform CI.

use std::sync::atomic::AtomicBool;
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
pub async fn service_loop(shutdown: Arc<AtomicBool>) {
    let data_dir = crate::config::data_dir();
    let paths = crate::config::DataPaths::new(&data_dir);

    // ADR-0012: Check kill switch before any other work.
    if crate::killswitch::is_active(&paths.kill_switch_file) {
        tracing::warn!("kill switch is active, refusing to start");
        // In a full Windows service implementation, we would also:
        // 1. Write an audit log entry KillSwitchStartupRefused
        // 2. Set service startup type to SERVICE_DISABLED via ChangeServiceConfigW
        // 3. Report ServiceState::Stopped to the SCM
        // For now, just return immediately.
        return;
    }

    tracing::info!("service loop started");

    // Run the worker loop (heartbeat, commands, status collection).
    // The worker loop handles its own kill-switch checks internally.
    if let Err(e) = crate::worker::main_loop::run(shutdown, &paths).await {
        tracing::error!(error = %e, "worker loop failed");
    }

    tracing::info!("service loop exiting");
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
