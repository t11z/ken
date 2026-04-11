//! Kill switch mechanism per ADR-0012.
//!
//! The kill switch is a state file at a well-known path that prevents
//! the Ken agent service from starting. When activated, the service
//! checks for this file on every startup and refuses to run if present.
//!
//! The state file is written by the tray app or CLI and persists across
//! reboots. Only explicit administrator action (deleting the file and
//! re-enabling the service) reverses it.
//!
//! This module also contains [`set_service_disabled`], the only function
//! in the codebase that sets the service's startup type to
//! `SERVICE_DISABLED` via the Win32 SCM API (ADR-0012 steps 5 and 6).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use ken_protocol::audit::AuditEventKind;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// JSON document written to the kill switch state file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KillSwitchState {
    /// When the kill switch was activated.
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,
    /// Which user activated the kill switch.
    pub user: String,
    /// Why the kill switch was activated.
    pub reason: String,
}

/// Check whether the kill switch is active (state file exists).
#[must_use]
pub fn is_active(kill_switch_path: &Path) -> bool {
    kill_switch_path.exists()
}

/// Activate the kill switch by writing the state file atomically.
///
/// Writes to a temporary file first, then renames to the final path
/// to avoid partial writes.
///
/// # Errors
///
/// Returns an error if the state directory cannot be created or the
/// file cannot be written.
pub fn activate(kill_switch_path: &Path, reason: &str, user: &str) -> Result<(), anyhow::Error> {
    if let Some(parent) = kill_switch_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let state = KillSwitchState {
        timestamp: OffsetDateTime::now_utc(),
        user: user.to_string(),
        reason: reason.to_string(),
    };

    let json = serde_json::to_string_pretty(&state)?;

    // Atomic write: write to .tmp, then rename
    let tmp_path = kill_switch_path.with_extension("tmp");
    std::fs::write(&tmp_path, &json)?;

    // On Unix, rename is atomic. On Windows, it replaces atomically
    // if the target doesn't exist (which it shouldn't for first activation).
    std::fs::rename(&tmp_path, kill_switch_path)?;

    tracing::warn!(
        path = %kill_switch_path.display(),
        user = %user,
        reason = %reason,
        "kill switch activated"
    );

    Ok(())
}

/// Deactivate the kill switch by removing the state file.
///
/// This is called only by the MSI uninstaller or by a human
/// administrator; the tray app does not call it.
///
/// # Errors
///
/// Returns an error if the file cannot be removed.
pub fn deactivate(kill_switch_path: &Path) -> Result<(), anyhow::Error> {
    if kill_switch_path.exists() {
        std::fs::remove_file(kill_switch_path)?;
        tracing::info!(
            path = %kill_switch_path.display(),
            "kill switch deactivated"
        );
    }
    Ok(())
}

/// Set the service startup type to `SERVICE_DISABLED` via the Win32
/// Service Control Manager.
///
/// This is the **only** function in the codebase that touches the
/// service's startup type via Win32. Any future change to the service
/// startup type should go through this function.
///
/// Requires `SeChangeNotifyPrivilege`, which the SYSTEM account has by
/// default.
///
/// Per ADR-0012 steps 5 and 6, this function is called in two places:
/// - During kill-switch activation (step 5), before the service reports
///   `Stopped` to the SCM.
/// - During startup-refused (step 6), when the service finds the kill
///   switch state file present on startup.
///
/// # Errors
///
/// Returns an error if the SCM cannot be opened, the service cannot be
/// opened, or the configuration change fails. Callers should treat
/// failure as non-fatal: the state file is the primary defense against
/// restart, and `SERVICE_DISABLED` is a secondary hardening measure.
#[cfg(windows)]
pub fn set_service_disabled(service_name: &str) -> Result<(), anyhow::Error> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Services::{
        ChangeServiceConfigW, CloseServiceHandle, OpenSCManagerW, OpenServiceW, SC_MANAGER_CONNECT,
        SERVICE_CHANGE_CONFIG, SERVICE_DISABLED, SERVICE_NO_CHANGE,
    };

    if service_name.is_empty() {
        return Err(anyhow::anyhow!("service name must not be empty"));
    }

    /// RAII wrapper for Win32 `SC_HANDLE`. Calls `CloseServiceHandle`
    /// on drop to prevent handle leaks.
    struct ScHandle(windows::Win32::System::Services::SC_HANDLE);

    impl Drop for ScHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseServiceHandle(self.0);
            }
        }
    }

    // Step 1: Open the SCM with minimal permissions.
    let scm = unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }
        .map_err(|e| anyhow::anyhow!("OpenSCManagerW failed: {e}"))?;
    let _scm_guard = ScHandle(scm);

    // Step 2: Open our own service with CHANGE_CONFIG permission.
    let name_wide: Vec<u16> = service_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let service = unsafe { OpenServiceW(scm, PCWSTR(name_wide.as_ptr()), SERVICE_CHANGE_CONFIG) }
        .map_err(|e| anyhow::anyhow!("OpenServiceW failed for '{service_name}': {e}"))?;
    let _service_guard = ScHandle(service);

    // Step 3: Set startup type to SERVICE_DISABLED, leaving all other
    // configuration fields unchanged (SERVICE_NO_CHANGE).
    unsafe {
        ChangeServiceConfigW(
            service,
            SERVICE_NO_CHANGE,          // dwServiceType
            SERVICE_DISABLED,           // dwStartType
            SERVICE_NO_CHANGE.0.into(), // dwErrorControl
            PCWSTR::null(),             // lpBinaryPathName
            PCWSTR::null(),             // lpLoadOrderGroup
            None,                       // lpdwTagId
            PCWSTR::null(),             // lpDependencies
            PCWSTR::null(),             // lpServiceStartName
            PCWSTR::null(),             // lpPassword
            PCWSTR::null(),             // lpDisplayName
        )
    }
    .map_err(|e| anyhow::anyhow!("ChangeServiceConfigW failed for '{service_name}': {e}"))?;

    tracing::info!(service_name, "service startup type set to SERVICE_DISABLED");
    Ok(())
}

/// Non-Windows stub for [`set_service_disabled`].
///
/// `SERVICE_DISABLED` is a Windows SCM concept. On non-Windows platforms
/// (used for development and CI), this function is a no-op. The state
/// file remains the primary defense against restart on all platforms.
#[cfg(not(windows))]
pub fn set_service_disabled(service_name: &str) -> Result<(), anyhow::Error> {
    if service_name.is_empty() {
        return Err(anyhow::anyhow!("service name must not be empty"));
    }
    tracing::info!(
        service_name,
        "set_service_disabled is a no-op on non-Windows"
    );
    Ok(())
}

/// Finalize kill-switch activation per ADR-0012 step 5.
///
/// Called after the `KillSwitchActivated` response has been sent to the
/// tray app. Attempts to set the service startup type to
/// `SERVICE_DISABLED`, logs any failure to the audit log, and signals
/// shutdown regardless of whether the disable call succeeded.
///
/// The `disable_fn` parameter exists so that tests can inject a mock
/// without requiring a real SCM connection.
pub(crate) fn finalize_activation(
    audit: &crate::audit::AuditLogger,
    shutdown: &AtomicBool,
    service_name: &str,
    disable_fn: impl FnOnce(&str) -> Result<(), anyhow::Error>,
) {
    if let Err(e) = disable_fn(service_name) {
        audit.log(
            AuditEventKind::KillSwitchActivated,
            &format!(
                "kill switch activation: failed to set service startup type to SERVICE_DISABLED: {e}"
            ),
        );
    }
    tracing::info!("kill switch activated via IPC, signalling shutdown");
    shutdown.store(true, Ordering::SeqCst);
}

/// Handle the startup-refused case per ADR-0012 step 6.
///
/// Called when the service starts and finds the kill switch state file
/// present. Logs a `KillSwitchStartupRefused` audit entry, attempts to
/// re-assert `SERVICE_DISABLED` (in case something re-enabled the
/// service), and logs any failure.
///
/// The `disable_fn` parameter exists so that tests can inject a mock
/// without requiring a real SCM connection.
pub(crate) fn handle_startup_refused(
    audit: &crate::audit::AuditLogger,
    service_name: &str,
    disable_fn: impl FnOnce(&str) -> Result<(), anyhow::Error>,
) {
    audit.log(
        AuditEventKind::KillSwitchStartupRefused,
        "kill switch is active, refusing to start",
    );
    if let Err(e) = disable_fn(service_name) {
        audit.log(
            AuditEventKind::KillSwitchStartupRefused,
            &format!(
                "kill switch startup refused: failed to set service startup type to SERVICE_DISABLED: {e}"
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ken_protocol::audit::AuditEventKind;

    #[test]
    fn not_active_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kill-switch-requested");
        assert!(!is_active(&path));
    }

    #[test]
    fn activate_creates_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("kill-switch-requested");

        activate(&path, "user requested", "testuser").unwrap();
        assert!(is_active(&path));

        let contents = std::fs::read_to_string(&path).unwrap();
        let state: KillSwitchState = serde_json::from_str(&contents).unwrap();
        assert_eq!(state.user, "testuser");
        assert_eq!(state.reason, "user requested");
    }

    #[test]
    fn deactivate_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kill-switch-requested");

        activate(&path, "test", "user").unwrap();
        assert!(is_active(&path));

        deactivate(&path).unwrap();
        assert!(!is_active(&path));
    }

    #[test]
    fn deactivate_noop_when_not_active() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kill-switch-requested");

        // Should not error when file doesn't exist
        deactivate(&path).unwrap();
    }

    // --- set_service_disabled parameter validation ---

    #[test]
    fn set_service_disabled_rejects_empty_name() {
        let result = set_service_disabled("");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("service name must not be empty"),
            "unexpected error message: {msg}"
        );
    }

    // --- finalize_activation tests (ADR-0012 step 5) ---

    #[test]
    fn finalize_activation_calls_disable_fn_and_sets_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let audit =
            crate::audit::AuditLogger::open(&dir.path().join("audit.log"), 1024 * 1024).unwrap();
        let shutdown = AtomicBool::new(false);
        let called = AtomicBool::new(false);

        finalize_activation(&audit, &shutdown, "TestService", |name| {
            assert_eq!(name, "TestService");
            called.store(true, Ordering::SeqCst);
            Ok(())
        });

        assert!(
            called.load(Ordering::SeqCst),
            "disable_fn must be called exactly once"
        );
        assert!(
            shutdown.load(Ordering::SeqCst),
            "shutdown flag must be set after finalize_activation"
        );

        // No failure entry should be present when disable_fn succeeds.
        let recent = audit.recent(10);
        assert!(
            !recent
                .iter()
                .any(|e| e.message.contains("failed to set service startup type")),
            "no failure audit entry expected on success"
        );
    }

    #[test]
    fn finalize_activation_failure_is_non_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let audit =
            crate::audit::AuditLogger::open(&dir.path().join("audit.log"), 1024 * 1024).unwrap();
        let shutdown = AtomicBool::new(false);

        finalize_activation(&audit, &shutdown, "TestService", |_name| {
            Err(anyhow::anyhow!("mock SCM failure"))
        });

        // Shutdown flag must still be set even when disable_fn fails.
        assert!(
            shutdown.load(Ordering::SeqCst),
            "shutdown flag must be set even when disable_fn fails"
        );

        // A failure audit entry must be written.
        let recent = audit.recent(10);
        assert!(
            recent.iter().any(|e| {
                matches!(e.kind, AuditEventKind::KillSwitchActivated)
                    && e.message
                        .contains("failed to set service startup type to SERVICE_DISABLED")
                    && e.message.contains("mock SCM failure")
            }),
            "expected failure audit entry, got: {recent:?}"
        );
    }

    // --- handle_startup_refused tests (ADR-0012 step 6) ---

    #[test]
    fn handle_startup_refused_calls_disable_fn_and_logs() {
        let dir = tempfile::tempdir().unwrap();
        let audit =
            crate::audit::AuditLogger::open(&dir.path().join("audit.log"), 1024 * 1024).unwrap();
        let called = AtomicBool::new(false);

        handle_startup_refused(&audit, "TestService", |name| {
            assert_eq!(name, "TestService");
            called.store(true, Ordering::SeqCst);
            Ok(())
        });

        assert!(
            called.load(Ordering::SeqCst),
            "disable_fn must be called exactly once"
        );

        // The refused audit entry must be present.
        let recent = audit.recent(10);
        assert!(
            recent.iter().any(|e| {
                matches!(e.kind, AuditEventKind::KillSwitchStartupRefused)
                    && e.message
                        .contains("kill switch is active, refusing to start")
            }),
            "expected KillSwitchStartupRefused audit entry, got: {recent:?}"
        );

        // No failure entry when disable_fn succeeds.
        assert!(
            !recent
                .iter()
                .any(|e| e.message.contains("failed to set service startup type")),
            "no failure audit entry expected on success"
        );
    }

    #[test]
    fn handle_startup_refused_failure_is_non_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let audit =
            crate::audit::AuditLogger::open(&dir.path().join("audit.log"), 1024 * 1024).unwrap();

        handle_startup_refused(&audit, "TestService", |_name| {
            Err(anyhow::anyhow!("mock SCM failure"))
        });

        let recent = audit.recent(10);

        // The refused entry must still be present.
        assert!(
            recent.iter().any(|e| {
                matches!(e.kind, AuditEventKind::KillSwitchStartupRefused)
                    && e.message
                        .contains("kill switch is active, refusing to start")
            }),
            "expected KillSwitchStartupRefused audit entry, got: {recent:?}"
        );

        // A failure audit entry must be written naming the error.
        assert!(
            recent.iter().any(|e| {
                matches!(e.kind, AuditEventKind::KillSwitchStartupRefused)
                    && e.message
                        .contains("failed to set service startup type to SERVICE_DISABLED")
                    && e.message.contains("mock SCM failure")
            }),
            "expected failure audit entry, got: {recent:?}"
        );
    }
}
