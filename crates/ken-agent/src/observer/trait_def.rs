//! Observer trait definition per ADR-0018 and ADR-0021.
//!
//! Each observer is a struct that owns its state and implements a
//! sync `observe` method. The worker loop dispatches observers based
//! on their [`ObserverKind`]:
//!
//! - **Synchronous** observers do their work in the `observe` body,
//!   called via `spawn_blocking` with the per-observer time budget
//!   from ADR-0018.
//! - **Background-refresh** observers do their work in a background
//!   task spawned through [`ObserverLifecycle`], and their `observe`
//!   method is a non-blocking cache read.
//!
//! Per ADR-0022, the `Observer` trait requires `UnwindSafe` as a
//! supertrait. This forces every implementor to either be naturally
//! `UnwindSafe` (the common case for simple structs) or to document
//! why it is safe to cross an unwind boundary with their internal
//! state. The worker loop catches panics inside `spawn_blocking`
//! closures using `std::panic::catch_unwind`; the `UnwindSafe` bound
//! is the type-level signal that the observer's state is safe to use
//! after an unwind.

use std::panic::UnwindSafe;

use super::lifecycle::ObserverLifecycle;
use super::tick::TickBoundary;

/// Whether an observer does its work in the read path (synchronous)
/// or in a background task (background-refresh).
///
/// Per ADR-0021, the criterion for choosing between the two kinds is
/// whether the observer can fit inside the per-observer time budget
/// that ADR-0018 commits to. Observers whose underlying data source
/// responds within the budget (WMI queries for Defender, Firewall,
/// `BitLocker`, and the Event Log) use [`Synchronous`](Self::Synchronous).
/// Observers whose data source may take seconds or minutes (the WUA
/// COM API for Windows Update) use
/// [`BackgroundRefresh`](Self::BackgroundRefresh).
///
/// An associated constant is used rather than a method or marker type
/// because the observer kind is a static property of the implementation
/// that does not change at runtime. The worker loop dispatches on it
/// once at construction time and again in `collect_snapshot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserverKind {
    /// The observer does its work in the `observe` method body.
    /// The worker loop calls this via `spawn_blocking` with the
    /// per-observer budget from ADR-0018.
    Synchronous,

    /// The observer does its work in a background task spawned
    /// through [`ObserverLifecycle::spawn_background`]. The `observe`
    /// method reads from an internal cache and returns immediately.
    BackgroundRefresh,
}

/// The contract that every observer must satisfy per ADR-0018 and
/// ADR-0021.
///
/// A single trait for both synchronous and background-refresh
/// observers. The observer's [`KIND`](Observer::KIND) associated
/// constant tells the worker loop how to dispatch it:
///
/// - **Synchronous** observers (e.g., Defender, Firewall) are invoked
///   via `spawn_blocking` with the per-observer time budget. They do
///   their work inside [`observe`](Observer::observe).
/// - **Background-refresh** observers (e.g., Windows Update) are
///   invoked directly because their `observe` is a non-blocking cache
///   read. They do their real work in a background task spawned
///   through the [`ObserverLifecycle`] hook.
///
/// The criterion from ADR-0021: pick `Synchronous` if the observer
/// can reliably fit inside the per-observer time budget from ADR-0018.
/// Pick `BackgroundRefresh` if it cannot.
///
/// # `UnwindSafe` requirement (ADR-0022)
///
/// The `UnwindSafe` supertrait requires that every observer's internal
/// state is safe to use after an unwind. For synchronous observers,
/// the worker loop wraps each `observe` call in
/// `std::panic::catch_unwind`; if the call panics, the observer struct
/// must be in a state that the next call can use without corruption.
///
/// For most observers (simple structs, unit structs), `UnwindSafe` is
/// automatic. Observers that hold types with interior mutability that
/// crosses an unwind boundary (e.g., `RefCell`, raw pointers) must
/// wrap the unsafe fields in `std::panic::AssertUnwindSafe` with a
/// justification comment, or implement `UnwindSafe` explicitly with
/// a safety argument.
pub trait Observer: Send + 'static + UnwindSafe {
    /// The subsystem type this observer contributes to the snapshot.
    type Output: Send + 'static;

    /// Whether this observer is synchronous or background-refresh.
    const KIND: ObserverKind;

    /// A human-readable name for logging and diagnostics.
    fn name(&self) -> &'static str;

    /// Perform one observation tick.
    ///
    /// For synchronous observers, this method does the actual data
    /// collection and runs inside `spawn_blocking`.
    ///
    /// For background-refresh observers, this method reads from an
    /// internal cache and returns immediately. The `tick` parameter
    /// is used to decide between `Fresh` and `Cached` via
    /// [`TickBoundary::tag`].
    fn observe(&mut self, tick: &TickBoundary) -> Self::Output;

    /// Lifecycle hook called once by the worker loop after construction.
    ///
    /// Background-refresh observers use this to spawn their background
    /// task via [`ObserverLifecycle::spawn_background`]. Synchronous
    /// observers receive the hook too but are expected to ignore it;
    /// this keeps the construction surface uniform.
    fn start(&mut self, lifecycle: ObserverLifecycle);
}
