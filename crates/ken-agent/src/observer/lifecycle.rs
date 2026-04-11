//! Observer lifecycle management per ADR-0021.
//!
//! The [`ObserverLifecycle`] handle is given to each observer once at
//! construction time. It provides a shutdown signal and a way to spawn
//! background tasks. The worker loop retains join targets for all
//! spawned tasks and enforces a bounded grace period on shutdown.
//!
//! ADR-0021 establishes the single-orchestrator property: the worker
//! loop is the only place that spawns observer tasks, even though
//! observers initiate the spawn through this handle.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;

/// Grace period for background observer tasks to complete on shutdown.
///
/// Per ADR-0021, the worker loop enforces a bounded wait when joining
/// background tasks during shutdown. Five seconds is a conservative
/// default: long enough for a watch-channel write or a WUA COM call
/// that is already completing to finish, short enough that agent
/// shutdown is not perceptibly delayed. Tasks that do not join within
/// this period are dropped (their `JoinHandle` is abandoned).
pub const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(5);

/// Shared storage for join handles of background tasks spawned through
/// [`ObserverLifecycle`]. The [`super::snapshot::ObserverSet`] holds
/// the same `Arc` and joins all tasks on shutdown.
pub(crate) type BackgroundTaskHandles = Arc<std::sync::Mutex<Vec<JoinHandle<()>>>>;

/// Handle given to each observer at construction time, per ADR-0021.
///
/// ADR-0021 establishes the single-orchestrator property: the worker
/// loop is the only place that spawns observer tasks, even though
/// observers initiate the spawn through this handle. Observers must
/// not call `tokio::spawn` directly; they spawn through
/// [`ObserverLifecycle::spawn_background`] so that the worker loop
/// retains a join target for every spawned task and can enforce the
/// shutdown grace period.
///
/// The shutdown signal is the same `Arc<AtomicBool>` used by the rest
/// of the agent. Reusing the existing type avoids introducing a new
/// signal mechanism and keeps the shutdown plumbing uniform across the
/// codebase.
pub struct ObserverLifecycle {
    shutdown: Arc<AtomicBool>,
    runtime: tokio::runtime::Handle,
    handles: BackgroundTaskHandles,
}

impl ObserverLifecycle {
    /// Create a new lifecycle handle.
    ///
    /// Called by [`super::snapshot::ObserverSet`] during construction,
    /// once per observer.
    pub(crate) fn new(
        shutdown: Arc<AtomicBool>,
        runtime: tokio::runtime::Handle,
        handles: BackgroundTaskHandles,
    ) -> Self {
        Self {
            shutdown,
            runtime,
            handles,
        }
    }

    /// The shutdown signal that background tasks should poll between
    /// Windows API calls.
    pub fn shutdown_signal(&self) -> &Arc<AtomicBool> {
        &self.shutdown
    }

    /// Spawn a background task on the Tokio runtime.
    ///
    /// The `JoinHandle` is stored internally so that
    /// [`super::snapshot::ObserverSet::shutdown`] can join all
    /// background tasks with a bounded grace period. Observers call
    /// this from their [`super::trait_def::Observer::start`] method,
    /// not `tokio::spawn` directly.
    pub fn spawn_background<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let handle = self.runtime.spawn(future);
        self.handles
            .lock()
            .expect("background task handle lock poisoned")
            .push(handle);
    }
}
