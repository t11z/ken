//! Interactive session management: launch and terminate tray app processes.
//!
//! Per ADR-0009, the tray app is `ken-agent.exe tray`, launched by the
//! SYSTEM service when it detects an interactive session. Per ADR-0010,
//! the service reacts to `WTS_SESSION_LOGON` and `WTS_SESSION_LOGOFF`
//! events via `SERVICE_ACCEPT_SESSIONCHANGE`.
//!
//! This module provides the Windows-specific logic for:
//! - Enumerating active interactive sessions at service startup
//! - Launching the tray app into a session via `CreateProcessAsUser`
//! - Terminating tray app processes on logoff or service shutdown
//!
//! On non-Windows platforms, stub implementations allow cross-platform CI.

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::*;

/// Information about a tray app process launched in a specific session.
///
/// Stored in the tray process map, keyed by session ID. One entry
/// per active interactive session.
#[cfg(windows)]
#[derive(Debug)]
pub struct TrayProcessInfo {
    /// The Windows session ID this tray app is running in.
    pub session_id: u32,
    /// The process handle returned by `CreateProcessAsUser`.
    /// Owned by this struct; closed on drop.
    pub process_handle: windows::Win32::Foundation::HANDLE,
    /// The process ID, for logging and debugging.
    pub pid: u32,
}

// SAFETY: Windows HANDLE values are kernel object handles that are
// explicitly safe to use from any thread. The same pattern is used
// by SecurityDescriptorHolder in crates/ken-agent/src/ipc/server.rs.
#[cfg(windows)]
unsafe impl Send for TrayProcessInfo {}

// TODO: The service does not currently detect when a tray process exits
// on its own (crash, user kills it via Task Manager). The map entry
// becomes stale. Restart-on-crash is a separate issue, not part of the
// current scope (Issue #10).

/// Map of session IDs to their tracked tray app processes.
#[cfg(windows)]
pub type TrayProcessMap = std::collections::HashMap<u32, TrayProcessInfo>;

/// A session-change event forwarded from the service control handler
/// to the main service loop via a channel.
#[derive(Debug, Clone)]
pub enum SessionChangeEvent {
    /// A user logged on to the given session.
    Logon { session_id: u32 },
    /// A user logged off from the given session.
    Logoff { session_id: u32 },
}

// --- Non-Windows stubs ---

#[cfg(not(windows))]
#[derive(Debug)]
pub struct TrayProcessInfo {
    pub session_id: u32,
    pub pid: u32,
}

#[cfg(not(windows))]
pub type TrayProcessMap = std::collections::HashMap<u32, TrayProcessInfo>;
