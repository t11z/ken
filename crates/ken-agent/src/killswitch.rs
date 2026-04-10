//! Kill switch mechanism per ADR-0012.
//!
//! The kill switch is a state file at a well-known path that prevents
//! the Ken agent service from starting. When activated, the service
//! checks for this file on every startup and refuses to run if present.
//!
//! The state file is written by the tray app or CLI and persists across
//! reboots. Only explicit administrator action (deleting the file and
//! re-enabling the service) reverses it.

use std::path::Path;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
