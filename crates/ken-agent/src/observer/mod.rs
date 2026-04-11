//! Passive OS observability collectors per ADR-0001 T2-1.
//!
//! Each sub-observer reads a specific aspect of Windows state (Defender,
//! firewall, `BitLocker`, Windows Update, Event Log) and returns the
//! corresponding type from `ken_protocol::status`.
//!
//! Per ADR-0018 and ADR-0021, observers are structs implementing the
//! [`Observer`](trait_def::Observer) trait. Synchronous observers are
//! invoked via `spawn_blocking` with a per-observer time budget.
//! Background-refresh observers are invoked directly and do their
//! real work in a task spawned through [`ObserverLifecycle`](lifecycle::ObserverLifecycle).

pub mod bitlocker;
pub mod defender;
pub mod event_log;
pub mod firewall;
pub mod lifecycle;
pub mod snapshot;
pub mod tick;
pub mod trait_def;
pub mod windows_update;

pub use bitlocker::BitLockerObserver;
pub use defender::DefenderObserver;
pub use event_log::EventLogObserver;
pub use firewall::FirewallObserver;
pub use windows_update::WindowsUpdateObserver;
