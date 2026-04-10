//! OS status types reported by the agent in each heartbeat.
//!
//! These types represent the passive observations the agent makes about
//! the Windows endpoint. Per ADR-0001, the agent reads OS state but does
//! not modify it. Each field maps to a specific Windows API data source
//! documented in the struct-level doc comments.
//!
//! Every observer-contributed value is wrapped in [`Observation<T>`] per
//! ADR-0019, which distinguishes fresh, cached, and unobserved states.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Wrapper for every observer-contributed value in the status snapshot.
///
/// Per ADR-0019, this type distinguishes three states an observer can be
/// in for any given field: freshly collected this tick, served from cache,
/// or not yet observed. The semantics are exact and exhaustive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Observation<T> {
    /// The observer collected this value during the current heartbeat tick.
    /// The `observed_at` timestamp records when the underlying API call returned.
    Fresh {
        value: T,
        #[serde(with = "time::serde::rfc3339")]
        observed_at: OffsetDateTime,
    },
    /// The observer is serving its last successfully collected value per
    /// the caching policy defined by ADR-0018. The `observed_at` timestamp
    /// records when that earlier collection happened.
    Cached {
        value: T,
        #[serde(with = "time::serde::rfc3339")]
        observed_at: OffsetDateTime,
    },
    /// The observer has no value to report. Covers cold start, persistent
    /// failure, and transient failure with no prior cached value.
    Unobserved,
}

impl<T> Observation<T> {
    /// Returns a reference to the contained value, if any.
    ///
    /// Both `Fresh` and `Cached` variants carry a value; `Unobserved` does not.
    #[must_use]
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Fresh { value, .. } | Self::Cached { value, .. } => Some(value),
            Self::Unobserved => None,
        }
    }
}

/// Top-level container for a point-in-time snapshot of the endpoint's
/// OS security state.
///
/// Per ADR-0019, subsystems are no longer `Option` at the snapshot level.
/// Instead, each field within a subsystem is individually wrapped in
/// [`Observation<T>`]. A subsystem that has never collected anything is
/// expressed as a struct in which every field is `Unobserved`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OsStatusSnapshot {
    /// When this snapshot struct was assembled (not when its contents
    /// were observed — individual fields carry their own timestamps).
    #[serde(with = "time::serde::rfc3339")]
    pub collected_at: OffsetDateTime,

    /// Windows Defender status from WMI `MSFT_MpComputerStatus`.
    pub defender: DefenderStatus,

    /// Windows Firewall per-profile state.
    pub firewall: FirewallStatus,

    /// `BitLocker` per-volume encryption state.
    pub bitlocker: BitLockerStatus,

    /// Windows Update status from the registry and WUA.
    pub windows_update: WindowsUpdateStatus,

    /// Recent security-relevant events from the Windows Event Log.
    /// Bounded to a reasonable number per heartbeat to keep payloads small.
    /// Per ADR-0019, wrapped in [`Observation<T>`].
    pub recent_security_events: Observation<Vec<SecurityEvent>>,
}

/// Windows Defender state from the WMI `MSFT_MpComputerStatus` class.
///
/// The agent reads this via WMI in the `ROOT\Microsoft\Windows\Defender`
/// namespace. All fields map to documented WMI properties. Per ADR-0019,
/// every field is wrapped in [`Observation<T>`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefenderStatus {
    /// Whether the antivirus component is enabled.
    pub antivirus_enabled: Observation<bool>,

    /// Whether real-time protection is active.
    pub real_time_protection_enabled: Observation<bool>,

    /// Whether tamper protection is engaged.
    pub tamper_protection_enabled: Observation<bool>,

    /// Antivirus signature version string (e.g., "1.401.622.0").
    pub signature_version: Observation<String>,

    /// When signatures were last updated.
    pub signature_last_updated: Observation<OffsetDateTime>,

    /// Age of the signature database in days.
    pub signature_age_days: Observation<u32>,

    /// When the last full scan completed, if ever. `Observation<Option<_>>`
    /// per ADR-0019: the outer `Observation` says whether the observer
    /// looked; the inner `Option` says whether Defender has a value.
    pub last_full_scan: Observation<Option<OffsetDateTime>>,

    /// When the last quick scan completed, if ever. Same semantics as
    /// `last_full_scan`.
    pub last_quick_scan: Observation<Option<OffsetDateTime>>,
}

/// Windows Firewall state across all three network profiles.
///
/// Per ADR-0019, each profile is wrapped in [`Observation<T>`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FirewallStatus {
    /// Domain network profile.
    pub domain_profile: Observation<FirewallProfileState>,
    /// Private network profile.
    pub private_profile: Observation<FirewallProfileState>,
    /// Public network profile.
    pub public_profile: Observation<FirewallProfileState>,
}

/// State of a single Windows Firewall profile.
///
/// This is the inner value type carried by `Observation<FirewallProfileState>`.
/// Its fields are not individually wrapped because a single WMI query
/// returns the entire profile atomically.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FirewallProfileState {
    /// Whether the profile's firewall is enabled.
    pub enabled: bool,
    /// Default inbound action: `"block"`, `"allow"`, or `"not_configured"`.
    pub default_inbound_action: String,
}

/// `BitLocker` encryption state across volumes.
///
/// Per ADR-0019, the volumes list is wrapped in [`Observation<T>`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitLockerStatus {
    /// Per-volume encryption status. Only volumes with drive letters
    /// are reported; recovery partitions are skipped.
    pub volumes: Observation<Vec<BitLockerVolumeStatus>>,
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

/// Windows Update status from the registry and WUA COM API.
///
/// Per ADR-0019, every field is wrapped in [`Observation<T>`].
/// `last_search_time` and `last_install_time` are cheap registry reads.
/// `pending_update_count` and `pending_critical_update_count` come from
/// the WUA COM API via a background task (ADR-0020).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowsUpdateStatus {
    /// When Windows Update last searched for updates.
    /// `Observation<Option<_>>`: outer says whether observer looked,
    /// inner says whether the registry contained a value.
    pub last_search_time: Observation<Option<OffsetDateTime>>,

    /// When updates were last installed. Same semantics as `last_search_time`.
    pub last_install_time: Observation<Option<OffsetDateTime>>,

    /// Number of updates pending installation (ADR-0020).
    pub pending_update_count: Observation<u32>,

    /// Number of critical updates pending installation, where "critical"
    /// means `MsrcSeverity == "Critical"` on the `IUpdate` interface (ADR-0020).
    pub pending_critical_update_count: Observation<u32>,
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
