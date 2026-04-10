//! Passive OS observability collectors per ADR-0001 T2-1.
//!
//! Each sub-observer reads a specific aspect of Windows state (Defender,
//! firewall, `BitLocker`, Windows Update, Event Log) and returns the
//! corresponding type from `ken_protocol::status`.
//!
//! On non-Windows platforms, all observers return `None`/empty so the
//! agent can compile and test on Linux.

pub mod bitlocker;
pub mod defender;
pub mod event_log;
pub mod firewall;
pub mod snapshot;
pub mod windows_update;
