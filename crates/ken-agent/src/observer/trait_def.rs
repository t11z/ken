//! Observer trait definition per ADR-0018.
//!
//! Each observer is a struct that owns its last successfully collected
//! value and implements a sync `observe` method. The worker loop calls
//! observers via `spawn_blocking`, applies the per-observer time budget,
//! catches panics, and assembles the snapshot from individual outputs.

/// The contract that every observer must satisfy per ADR-0018.
///
/// Observers are sync Rust structs invoked from the async worker loop
/// via `tokio::task::spawn_blocking`. The `observe` method is called at
/// most once per heartbeat tick. The observer decides internally whether
/// to refresh or return a cached value.
///
/// The `Output` type is the subsystem type from `ken-protocol::status`
/// (e.g., `DefenderStatus`, `FirewallStatus`).
pub trait Observer: Send + 'static {
    /// The subsystem type this observer contributes to the snapshot.
    type Output: Send + 'static;

    /// A human-readable name for logging and diagnostics.
    fn name(&self) -> &'static str;

    /// Perform one observation tick.
    ///
    /// This method is sync and runs inside `spawn_blocking`. It should
    /// return promptly for cheap observers and may return a cached value
    /// for expensive ones. Per ADR-0018, the refresh decision is local
    /// to the observer.
    fn observe(&mut self) -> Self::Output;
}

