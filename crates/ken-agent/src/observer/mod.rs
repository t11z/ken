//! Passive OS observability collectors.
//!
//! Each sub-observer reads a specific aspect of Windows state (Defender,
//! firewall, `BitLocker`, Windows Update, Event Log) and returns the
//! corresponding type from `ken_protocol::status`.
//!
//! On non-Windows platforms, all observers return `None` so the agent
//! can compile and test on Linux.

pub mod snapshot;

use ken_protocol::status::{
    BitLockerStatus, DefenderStatus, FirewallStatus, SecurityEvent, WindowsUpdateStatus,
};

/// Collect a Defender status snapshot.
///
/// On Windows, queries WMI `MSFT_MpComputerStatus`.
/// On other platforms, returns `None`.
pub fn collect_defender() -> Option<DefenderStatus> {
    // WMI query implementation goes here (Section 9)
    None
}

/// Collect firewall status.
pub fn collect_firewall() -> Option<FirewallStatus> {
    None
}

/// Collect `BitLocker` status.
pub fn collect_bitlocker() -> Option<BitLockerStatus> {
    None
}

/// Collect Windows Update status.
pub fn collect_windows_update() -> Option<WindowsUpdateStatus> {
    None
}

/// Collect recent security events from the Event Log.
pub fn collect_security_events() -> Vec<SecurityEvent> {
    Vec::new()
}
