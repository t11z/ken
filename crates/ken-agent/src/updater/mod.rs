//! Agent update mechanism per ADR-0011.
//!
//! The update checker periodically queries the server for the latest
//! agent version. If a newer version is available, it downloads the
//! signed MSI, verifies the Authenticode signature, and schedules
//! installation at the maintenance window.
//!
//! Phase 1: the server always returns version "0.0.0" (no update),
//! so the checker exercises the "no update available" path only.
//! Real MSI builds, Authenticode verification, and maintenance window
//! scheduling are Phase 2 work.

use serde::Deserialize;

/// Response from the server's `/updates/latest.json` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct LatestUpdateInfo {
    /// The latest available agent version string.
    pub version: String,
}

/// Check whether an update is available.
///
/// Compares the server's reported latest version against the running
/// agent version. Returns `true` if the server version is strictly
/// greater (semver comparison).
///
/// Phase 1 stub: always returns `false` because the server returns "0.0.0".
#[must_use]
pub fn is_update_available(server_version: &str, running_version: &str) -> bool {
    // Simple semver comparison: split on dots and compare numerically.
    let parse =
        |v: &str| -> Vec<u64> { v.split('.').filter_map(|s| s.parse::<u64>().ok()).collect() };

    let server = parse(server_version);
    let running = parse(running_version);

    server > running
}

/// Verify an MSI's Authenticode signature.
///
/// On Windows, calls `WinVerifyTrust` from `windows::Win32::Security::WinTrust`.
/// On non-Windows, this is a no-op stub.
///
/// # Errors
///
/// Returns an error if the signature is invalid or verification fails.
#[cfg(windows)]
pub fn verify_authenticode(_msi_path: &std::path::Path) -> Result<(), anyhow::Error> {
    // Phase 1 stub. Real implementation would use:
    // windows::Win32::Security::WinTrust::WinVerifyTrust
    tracing::debug!("authenticode verification: not yet implemented");
    Ok(())
}

#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)] // Signature matches Windows version which can fail
pub fn verify_authenticode(_msi_path: &std::path::Path) -> Result<(), anyhow::Error> {
    tracing::debug!("authenticode verification: not available on non-Windows");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_update_when_server_returns_zero() {
        assert!(!is_update_available("0.0.0", "0.1.0"));
    }

    #[test]
    fn update_available_when_server_is_newer() {
        assert!(is_update_available("0.2.0", "0.1.0"));
        assert!(is_update_available("1.0.0", "0.9.9"));
    }

    #[test]
    fn no_update_when_versions_equal() {
        assert!(!is_update_available("0.1.0", "0.1.0"));
    }

    #[test]
    fn no_update_when_running_is_newer() {
        assert!(!is_update_available("0.1.0", "0.2.0"));
    }
}
