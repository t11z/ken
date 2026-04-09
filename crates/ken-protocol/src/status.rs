//! OS status types reported by the agent in each heartbeat.
//!
//! These types represent the passive observations the agent makes about
//! the Windows endpoint. Per ADR-0001, the agent reads OS state but does
//! not modify it. Each field maps to a specific Windows API data source
//! documented in the struct-level doc comments.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Top-level container for a point-in-time snapshot of the endpoint's
/// OS security state.
///
/// Each subsystem is `Option` because not all systems are available on
/// all Windows versions (e.g., `BitLocker` is absent on Home editions).
/// A `None` value means the observer could not collect data, not that
/// the feature is disabled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OsStatusSnapshot {
    /// When this snapshot was collected on the agent.
    #[serde(with = "time::serde::rfc3339")]
    pub collected_at: OffsetDateTime,

    /// Windows Defender status from WMI `MSFT_MpComputerStatus`.
    pub defender: Option<DefenderStatus>,

    /// Windows Firewall per-profile state.
    pub firewall: Option<FirewallStatus>,

    /// `BitLocker` per-volume encryption state.
    pub bitlocker: Option<BitLockerStatus>,

    /// Windows Update status from the registry and WUA.
    pub windows_update: Option<WindowsUpdateStatus>,

    /// Recent security-relevant events from the Windows Event Log.
    /// Bounded to a reasonable number per heartbeat to keep payloads small.
    pub recent_security_events: Vec<SecurityEvent>,
}

/// Windows Defender state from the WMI `MSFT_MpComputerStatus` class.
///
/// The agent reads this via WMI in the `ROOT\Microsoft\Windows\Defender`
/// namespace. All fields map to documented WMI properties.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefenderStatus {
    /// Whether the antivirus component is enabled.
    pub antivirus_enabled: bool,

    /// Whether real-time protection is active.
    pub real_time_protection_enabled: bool,

    /// Whether tamper protection is engaged.
    pub tamper_protection_enabled: bool,

    /// Antivirus signature version string (e.g., "1.401.622.0").
    pub signature_version: String,

    /// When signatures were last updated.
    #[serde(with = "time::serde::rfc3339")]
    pub signature_last_updated: OffsetDateTime,

    /// Age of the signature database in days.
    pub signature_age_days: u32,

    /// When the last full scan completed, if ever.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_full_scan: Option<OffsetDateTime>,

    /// When the last quick scan completed, if ever.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_quick_scan: Option<OffsetDateTime>,
}

/// Windows Firewall state across all three network profiles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FirewallStatus {
    /// Domain network profile.
    pub domain_profile: FirewallProfileState,
    /// Private network profile.
    pub private_profile: FirewallProfileState,
    /// Public network profile.
    pub public_profile: FirewallProfileState,
}

/// State of a single Windows Firewall profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FirewallProfileState {
    /// Whether the profile's firewall is enabled.
    pub enabled: bool,
    /// Default inbound action: `"block"`, `"allow"`, or `"not_configured"`.
    pub default_inbound_action: String,
}

/// `BitLocker` encryption state across volumes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitLockerStatus {
    /// Per-volume encryption status. Only volumes with drive letters
    /// are reported; recovery partitions are skipped.
    pub volumes: Vec<BitLockerVolumeStatus>,
}

/// `BitLocker` status for a single volume.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitLockerVolumeStatus {
    /// Drive letter (e.g., "C:").
    pub drive_letter: String,
    /// Protection status: "on", "off", or "unknown".
    pub protection_status: String,
    /// Percentage of the volume that is encrypted (0–100).
    pub encryption_percentage: u8,
}

/// Windows Update status from the registry.
///
/// Phase 1 reads `last_search_time` and `last_install_time` from the
/// registry. Pending update counts require the Windows Update Agent
/// COM API and are deferred to a future enhancement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowsUpdateStatus {
    /// When Windows Update last searched for updates.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_search_time: Option<OffsetDateTime>,

    /// When updates were last installed.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_install_time: Option<OffsetDateTime>,

    /// Number of updates pending installation.
    pub pending_update_count: u32,

    /// Number of critical updates pending installation.
    pub pending_critical_update_count: u32,
}

/// A single security-relevant event from the Windows Event Log.
///
/// The agent constructs a brief human-readable `summary` from known
/// event ID patterns. Per ADR-0001, the full event body is never
/// transmitted — the agent reports observations, not raw OS data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityEvent {
    /// Windows event ID.
    pub event_id: u32,

    /// Event source (e.g., "Microsoft-Windows-Windows Defender").
    pub source: String,

    /// Severity level of the event.
    pub level: SecurityEventLevel,

    /// When the event occurred on the endpoint.
    #[serde(with = "time::serde::rfc3339")]
    pub occurred_at: OffsetDateTime,

    /// Brief human-readable summary constructed by the agent.
    pub summary: String,
}

/// Severity level for a security event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecurityEventLevel {
    /// Informational event.
    Information,
    /// Warning-level event.
    Warning,
    /// Error-level event.
    Error,
    /// Critical-level event.
    Critical,
}
