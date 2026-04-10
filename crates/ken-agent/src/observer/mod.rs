//! Passive OS observability collectors per ADR-0001 T2-1.
//!
//! Each sub-observer reads a specific aspect of Windows state (Defender,
//! firewall, `BitLocker`, Windows Update, Event Log) and returns the
//! corresponding type from `ken_protocol::status`.
//!
//! Per ADR-0018, observers are structs implementing the [`Observer`] trait.
//! The worker loop invokes them via `spawn_blocking` with a per-observer
//! time budget.

pub mod bitlocker;
pub mod defender;
pub mod event_log;
pub mod firewall;
pub mod snapshot;
pub mod trait_def;
pub mod windows_update;

pub use bitlocker::BitLockerObserver;
pub use defender::DefenderObserver;
pub use event_log::EventLogObserver;
pub use firewall::FirewallObserver;
pub use windows_update::WindowsUpdateObserver;
