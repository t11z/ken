# ADR-0021: Observer background refresh pattern

- **Status:** Accepted
- **Date:** 2026-04-11
- **Deciders:** —
- **Supersedes:** —
- **Superseded by:** —

## Context

ADR-0018 defined the observer execution model under the assumption that every observer is a synchronous Rust function invoked from the worker loop via `spawn_blocking`, owning its own state, and deciding locally on each tick whether to refresh or to return its last known value. That model was correct for the four cheap observers (Defender, Firewall, BitLocker, security event log), and it remains correct for them. The model was deliberately written without commitment to a specific Rust trait or struct shape, on the grounds that those were implementation details for the prompt that would introduce the trait.

ADR-0020 then specified the Windows Update observer, which cannot fit inside the model as written. A WUA `IUpdateSearcher::Search` call may take 10–30 seconds; running it inside `spawn_blocking` from the worker loop would either blow the per-observer time budget on every refresh or — if the budget were widened to accommodate it — undo the protection that ADR-0018 set out to provide. ADR-0020 resolved this by spawning a long-lived background task at observer construction time, refreshing the WUA cache on a one-hour TTL out of band, and degrading the heartbeat-tick path to a non-blocking read of a `tokio::sync::watch` channel. The implementation in `crates/ken-agent/src/observer/windows_update.rs` follows that decision faithfully.

The implementation is correct in isolation, and ADR-0020 is correct on its own terms. The problem is the seam between them and ADR-0018. Three symptoms are visible in the current code, all of them traceable to the same missing decision:

First, the `Observer` trait in `crates/ken-agent/src/observer/trait_def.rs` reflects only the ADR-0018 model. It is sync, single-method, single-threaded, and has no lifecycle hooks. The Windows Update observer satisfies the trait signature by reducing `observe()` to a `watch::borrow()` call, but the substance of its work happens entirely outside the trait — in a constructor (`init_background`) that takes an `Arc<AtomicBool>` shutdown handle as a positional parameter and spawns a Tokio task that the trait knows nothing about. Any second expensive observer would have to invent the same plumbing again, by hand, with no guarantee that its lifecycle coupling matches.

Second, the `Fresh` versus `Cached` distinction defined in ADR-0019 has no clear owner under this pattern. ADR-0019 says that observed values flow through the wire format as `Observation<T>` with three variants, and that `Cached` is the variant for "observed earlier than the current heartbeat tick." ADR-0018 says caching happens inside the observer, which implies the observer also produces the variant. But the worker loop in `snapshot.rs` operates generically over `O::Output` and has no schema knowledge — it cannot re-tag a `Fresh` value as `Cached` after the fact. The Windows Update observer attempts to make the distinction itself, in `windows_update.rs:106`, using a one-second age threshold as a stand-in for the heartbeat tick window. Under realistic tick intervals (30–60 seconds), that heuristic produces `Cached` for essentially every read and `Fresh` essentially never, which empties the distinction of meaning for this observer specifically. The bug is local; the cause is that nobody owns the tick window.

Third, the resulting lack of a shared pattern means the next expensive observer — whichever subsystem comes next, whether it is a Defender definition refresh, a third-party patch checker, or something not yet imagined — will face the same four decisions ADR-0020 already made, with no scaffolding to reuse. Each one will be free to invent its own background-task lifecycle, its own cache representation, its own freshness convention, and its own shutdown coupling. The pattern will entrench by copy-paste, and the trait will become ceremonial.

The decision is forced now because the Phase 1 closure audit identified the `Cached`-tagging gap and the panic-isolation gap as blocking, and both fixes depend on settling who owns the cache and how the trait expresses background work. Resolving them piecemeal would cement a design that has not been agreed.

This ADR governs the *pattern* by which observers may perform background work and the *contract* by which the cache transition from `Fresh` to `Cached` is produced. It does not redefine the cheap-observer model from ADR-0018, which remains in force unchanged for observers that do not need a background task.

## Decision

Observers that cannot fit inside a per-tick time budget are modelled as a **background-refresh pattern** with explicit lifecycle coupling, and the responsibility for producing `Fresh` and `Cached` values is assigned to the observer itself, with the worker loop providing the tick boundary the observer needs to make that distinction.

The pattern has the following properties, which together define the contract that an expensive observer must satisfy and that the worker loop guarantees in return.

**Two observer kinds, one trait.** The `Observer` trait gains an explicit notion of *kind* — synchronous (the ADR-0018 default) or background-refresh (this ADR's addition). A synchronous observer is exactly what ADR-0018 already specifies; nothing about it changes. A background-refresh observer additionally exposes a lifecycle hook through which the worker loop hands it the resources it needs to spawn its background work at construction time and to wind it down at shutdown. The hook is part of the trait, not a positional constructor parameter, so that observer construction and observer lifecycle are governed by the same surface.

**Lifecycle handle, not raw shutdown flag.** The lifecycle resource is an `ObserverLifecycle` value passed to the observer once, at the moment the worker loop builds the `ObserverSet`. It carries the observer's shutdown signal, the Tokio runtime handle on which the observer's background task should be spawned, and any logging or tracing context the loop wants the background task to inherit. Observers do not receive an `Arc<AtomicBool>` directly, and they do not call `tokio::spawn` directly; both go through the lifecycle handle so that the worker loop retains a single point of orchestration over background tasks. This is the difference between "the observer happens to spawn a task" and "the worker loop allows the observer to spawn a task and knows what it spawned."

**Cache ownership lives in the observer.** A background-refresh observer holds its cache as `Observation<T>` directly. The observer is the only place that constructs `Fresh`, and it is the only place that constructs `Cached`. When its background task completes a successful refresh, the new value is stored as `Fresh`. When the heartbeat tick reads from the cache, the observer compares the value's `observed_at` timestamp against the *current tick boundary supplied by the worker loop* and re-emits the value as `Fresh` if and only if the observation occurred within the current tick window; otherwise it re-emits the value as `Cached`. This is the only place in the Ken codebase that knows enough to make that distinction correctly. The worker loop does not.

**Tick boundary is supplied by the loop.** The worker loop already knows when each tick begins, because it controls the tick interval. On each call to an observer's read path, the loop passes the tick start time as a parameter. The observer uses this value to compute the `Fresh` versus `Cached` decision; it does not invent a heuristic of its own and does not consult a wall clock for this purpose. Two observers reading on the same tick will agree about which values are fresh, because they were given the same boundary.

**Failure isolation extends to the background task.** A panic inside the background task is caught at the task boundary and converted into a logged failure that leaves the cache untouched. The next heartbeat tick reads whatever was last stored — `Fresh`, `Cached`, or `Unobserved` if the cache was never populated — and the observer's identity in the snapshot is preserved. Repeated panics are logged but do not disable the observer; this preserves the property from ADR-0018 that disabling is a separate decision that requires its own ADR. The same property applies symmetrically to synchronous observers, and ADR-0018's existing language on this point is reaffirmed without modification.

**Shutdown is cooperative and bounded.** When the worker loop signals shutdown, a background-refresh observer's task observes the signal at its next checkpoint — between Windows API calls, not in the middle of one — and exits. The lifecycle handle exposes a join target that the worker loop awaits on shutdown, with a bounded grace period, so that observer tasks are not abandoned silently. If the grace period expires, the task is dropped and the agent continues to exit; this matches ADR-0018's existing acceptance that hung sync code on Windows cannot be killed, only abandoned.

**One observer, one background task.** A background-refresh observer spawns at most one background task. Observers that need parallel sub-work do that work inside their own task, not by spawning siblings through the lifecycle handle. This keeps the number of long-lived tasks proportional to the number of observers and keeps the orchestration surface narrow.

**The cheap-observer model is unchanged.** The four synchronous observers (Defender, Firewall, BitLocker, security event log) continue to operate exactly as ADR-0018 specifies. They implement the synchronous variant of the trait, they do not receive a lifecycle handle, they do not spawn background tasks, and they construct `Observation<T>` values themselves as part of their normal `observe()` path — also using the tick boundary supplied by the worker loop, so that the `Fresh` versus `Cached` rule is uniform across both kinds of observer. ADR-0018 is not superseded. This ADR adds a path; it does not replace one.

This ADR governs the *shape* of the background-refresh pattern. It does not prescribe specific Rust type names, method signatures, channel types, or struct layouts for `ObserverLifecycle` — those are implementation details for the prompt that will introduce the trait change. The properties above are the contract; the names are not.

## Consequences

**Easier:**

- The next expensive observer has a model to implement against and does not have to negotiate background-task lifecycle, cache ownership, or freshness conventions ad hoc. The decision in front of its author is "is this observer cheap or expensive," not "how do I express expensive in code."
- The `Fresh` versus `Cached` distinction in ADR-0019 acquires a real owner, and the freshness claim in the wire format becomes meaningful for cached observers. A heartbeat that reports `Cached` does so because the value was observed before the current tick, not because of a one-second timing accident.
- The `Observer` trait stops modelling only the easy case. Reading the trait in isolation tells a future maintainer that two kinds of observer exist and what their respective contracts are, without having to read the Windows Update observer to discover the pattern by example.
- The worker loop retains visibility into background tasks. Shutdown becomes deterministic for background-refresh observers within the grace period, and the single-orchestrator property of ADR-0018 is preserved across both observer kinds.

**Harder:**

- The `Observer` trait gains a lifecycle hook and the read path gains a tick-boundary parameter. This is a small refactor across the existing observers, but it is a refactor, and the four synchronous observers have to be touched even though their behavior does not change.
- The worker loop has to track per-observer background-task handles and join them on shutdown with a grace period. This is more orchestration than the loop carries today.
- The `ObserverLifecycle` type is a new piece of infrastructure that has to be designed, named, and documented. It is not large, but it is a new surface in the agent crate and another concept for a maintainer to load into their head before writing an observer.

**Accepted:**

- A heartbeat tick still cannot guarantee that every observer's value is fresh. Background-refresh observers may serve `Cached` indefinitely if their refresh loop is failing; the wire format expresses this honestly via the `Observation<T>` variant, but the family IT chief reading the dashboard has to know what `Cached` means. This is a documentation problem more than an architectural one.
- The pattern is opinionated: a background-refresh observer that wants two parallel refresh schedules, or that wants to refresh in response to an external event rather than on a TTL, has to either fold that into its single background task or wait for a follow-up ADR. The decision is to keep the pattern flat for the same reason ADR-0018 kept its model flat — five observers in a single-tenant agent do not justify a scheduler.
- Two observers that read on the same tick will agree about freshness, but observers that read in different ticks are free to compute the boundary differently if a future change to the worker loop allows it. This ADR commits the loop to passing *a* tick boundary, not to a specific definition of one. If sub-tick reads ever become a thing, this ADR will need a follow-up.

## Alternatives considered

**Supersede ADR-0018 with a unified model that treats every observer as background-refresh.** Rejected because it pays the cost of the heavier model for the four observers that do not need it, and because ADR-0018 is right about its observers — folding Defender, Firewall, BitLocker, and the event log into a background-task pattern would add lifecycle plumbing to four modules that work today. The goal is to add a path, not to remove one.

**Leave ADR-0018 alone and treat the Windows Update observer as a one-off, with each future expensive observer free to invent its own pattern.** Rejected because the audit already shows what the one-off looks like in practice: a constructor parameter that bypasses the trait, a freshness heuristic that gives wrong answers, and a cache representation the worker loop cannot reason about. The next expensive observer would either copy these properties or contradict them, and the trait would lose its ability to communicate the architecture.

**Move cache ownership to the worker loop, with the loop holding `Observation<T>` per observer and re-tagging values as it reads them.** Rejected because the loop does not know the schema of any individual observer's output, and adding that knowledge would require either a generic re-tagging mechanism (which is what the loop tried to avoid by being generic over `O::Output` in the first place) or a per-observer special case in the loop (which puts observer-specific code in the orchestrator). The cache lives where the observer lives.

**Centralize background tasks in a single observer scheduler that owns all expensive observers as its sub-tasks.** Rejected for the same reason ADR-0018 rejected the central scheduler: the scheduler would need its own lifecycle, its own failure handling, its own coordination with the worker loop's shutdown, and its own tests. The pattern in this ADR achieves the same outcome by giving each expensive observer a thin lifecycle handle from the loop, without introducing a new orchestrator between them.

**Change `Observer` into an async trait so that background work and read paths can be expressed natively.** Rejected because it pulls async-trait macros into a module that ADR-0018 deliberately kept sync, scatters the sync/async boundary across every observer instead of concentrating it in the loop, and provides no benefit for the four observers that have no background work to express. The lifecycle hook captures what is actually needed without changing the read path's shape.
