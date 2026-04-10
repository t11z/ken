# ADR-0018: Observer Execution Model

- **Status:** Accepted
- **Date:** 2026-04-10
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

The Ken agent collects passive OS state through a set of *observers* — one each for Windows Defender, Windows Firewall, BitLocker, Windows Update, and the security event log. Each observer reads a specific Windows data source and contributes a typed fragment to the `OsStatusSnapshot` defined in `ken-protocol`. The architecture diagram has shown them as a distinct subsystem since the project began, but the *execution model* — how observers run, when they run, what happens when they fail or hang, and how their work relates to the heartbeat loop — has never been written down.

The current code reflects this absence. `crates/ken-agent/src/observer/snapshot.rs` defines a single free function `collect_snapshot()` that calls each observer's `collect()` function in sequence and assembles the result. Each observer is itself a free function returning `Option<T>`, and every implementation is currently a Phase 1 stub that returns `None`. The worker loop in `crates/ken-agent/src/worker/main_loop.rs` calls `collect_snapshot()` directly from its async context, with no `spawn_blocking`, no timeout, and no isolation:

```rust
let status = collect_snapshot();
let heartbeat = Heartbeat { ..., status, ... };
```

This works today because the observers do nothing. It will stop working the moment any observer makes a real call. WMI queries for Defender take hundreds of milliseconds and run on a synchronous COM interface; the Windows Update Agent COM API (issue #4) takes 10–30 seconds on its first call and may trigger a check; reading the security event log via `EvtQuery` is fast per call but unbounded if the bookmark falls behind. Each of these, called inline from a Tokio worker thread, would block the runtime, delay the heartbeat, and — if the call hangs — wedge the agent until process restart. A panic in any FFI call would propagate up and kill the worker.

The decision is forced now because issue #4 is the first observer that cannot pretend to be cheap, and writing it without an execution model would either bake an implicit model into one observer (which the next observer would then have to either copy or contradict), or block the heartbeat for half a minute every time the agent starts. Neither is acceptable, and neither should be settled by an implementer making a local choice.

Two related questions are explicitly *not* settled by this ADR. The semantics of unknown, stale, or partial values in the wire format — currently expressed inconsistently in `OsStatusSnapshot`, where subsystem-level absence is `Option<T>` but field-level absence inside `WindowsUpdateStatus` is `0` masquerading as a real value — is the subject of a separate ADR. The specific behavior of the Windows Update observer, including its TTL, its failure modes, and its mapping to status fields is the subject of a third ADR that consumes both this one and the schema ADR.

## Decision

Observers run as **synchronous Rust functions invoked from the async worker loop via `tokio::task::spawn_blocking`**. Each observer owns its own state, including its last known good value and the timestamp at which that value was collected. The worker loop queries each observer once per heartbeat tick and always receives an answer immediately — either a freshly collected value or the observer's last known value, depending on whether the observer chose to refresh on this tick.

The observer subsystem has the following properties, which are part of the architectural contract and not subject to local override by individual observer implementations:

**Synchronous body, async boundary.** The body of each observer is sync Rust code that calls Windows APIs directly. The boundary between the async worker loop and the sync observer is `tokio::task::spawn_blocking`, called by the loop, not by the observer. Observers do not depend on Tokio and are not async functions. This keeps the Windows-API code free of async complications and confines all `spawn_blocking` calls to one place where the threading model can be reasoned about.

**Per-observer state and per-observer caching.** Each observer is a struct that holds, at minimum, its last successfully collected value, the timestamp at which that value was collected, and its configured refresh policy. Cheap observers (Defender, Firewall, BitLocker as currently understood) may refresh on every tick. Expensive observers (Windows Update, and any future observer whose collection cost exceeds a small fraction of the heartbeat interval) refresh less frequently and serve cached values in between. The decision of "cheap or expensive" is made per observer in the ADR that introduces it, not by the worker loop.

**Bounded per-tick budget.** The worker loop enforces a per-observer time budget on each `spawn_blocking` call. If an observer does not return within the budget, the loop proceeds with whatever value the observer had cached before the current call started, and the in-flight call is left to either complete or be abandoned at process exit. The budget value is a configuration parameter with a conservative default; this ADR does not fix the number, but commits to its existence. Sync code is not cancellable on Windows in the general case, so "abandoning" means the worker stops waiting, not that the OS-level call is killed.

**Failure isolation.** A panic inside an observer is caught at the `spawn_blocking` boundary and converted into a tick-level failure for that observer alone. The observer's last known value (if any) is used for the snapshot; if there is no last known value, the observer's contribution to the snapshot is absent in whatever form the schema ADR specifies. Other observers and the worker loop are unaffected. An observer that panics repeatedly is logged but not disabled — disabling is a behavior change that would require its own decision.

**Refresh decisions are local.** An observer decides on each invocation whether to refresh or to return its cached value, based on its own clock and its own policy. The worker loop does not orchestrate refresh schedules across observers. This keeps the loop simple and lets each observer's refresh logic be unit-tested in isolation, without simulating a scheduler.

**One observer, one subsystem.** Each observer is responsible for exactly one subsystem of `OsStatusSnapshot`. Multi-subsystem observers are not introduced by this ADR; if a future use case needs cross-subsystem coordination, it goes through its own ADR.

This ADR governs the *shape* of observer execution. It does not prescribe a specific Rust trait, struct layout, or function signature — those are implementation details for the prompt that will introduce the trait. The properties above are what a future implementer must satisfy; the specific names and methods are not.

## Consequences

**Easier:**

- The first expensive observer (Windows Update, issue #4) has a model to implement against. It does not have to invent caching, threading, or failure handling on its own, and it does not set a precedent that the next observer has to either copy or reject.
- The worker loop is decoupled from observer cost. Adding a slow observer no longer forces a redesign of the loop, because the loop already assumes observers may take real time.
- Failure isolation is structural rather than aspirational. A bug in the WMI bindings or the WUA COM bridge cannot kill the agent; it can only blank one field of one heartbeat. This matches the fail-safe-toward-the-user principle in `crates/ken-agent/CLAUDE.md`.
- The model is testable without a Windows runtime. A test can construct an observer struct, advance a fake clock, and verify that refresh and cache behavior match expectations, all on Linux.

**Harder:**

- Every observer now carries state. The current free-function shape has to be replaced by structs that the worker holds across ticks. This is a small refactor for the existing stubs but it is a refactor, and it touches every observer module at once.
- The per-observer time budget is a parameter that has to be chosen, configured, and explained. Default budgets that are too tight will produce spurious cache hits; too loose, and they fail to protect the loop. The first deployment will likely tune them.
- Sync observers running in `spawn_blocking` consume blocking-pool threads. Tokio's default blocking pool is generous enough for five observers tickling once per minute, but the model implies discipline: a future change that fans observers out into many parallel sub-tasks would need to revisit pool sizing.

**Accepted:**

- Hung observers cannot be killed. If a Windows API call inside an observer deadlocks, the worker stops waiting after the budget expires, but the underlying thread remains stuck until the process exits. This is the unavoidable cost of using sync FFI, and the alternative — wrapping every Windows call in its own process — is far too heavy for the value it would deliver in this threat model. The mitigation is that hung observers are logged and visible in the heartbeat as missing data, so the family IT chief notices.
- A heartbeat may carry a mix of fresh and cached values from different observers. The schema must be able to express which is which; this ADR commits to the *existence* of that distinction and defers its *expression* to the schema ADR. Until the schema ADR lands, the existing single `collected_at` on `OsStatusSnapshot` overstates the freshness of any cached field. This is a known temporary inconsistency.
- Observers do not coordinate. An observer cannot say "skip me this tick, the system is under load" or "refresh me now, an event just happened." If those capabilities turn out to be needed, they require a follow-up ADR; this one deliberately keeps the model flat.

## Alternatives considered

**Keep the current synchronous-in-loop model and accept that observers must be cheap.** Rejected because the constraint is not enforceable. Defining "cheap" precisely enough that an implementer can self-check is hard, and the first observer that violates the rule (issue #4 is exactly this) breaks the agent in a way that is invisible until deployment. The model has to assume that some observers are expensive, because some of them are.

**Async-trait observers, with each observer responsible for its own internal `spawn_blocking`.** Rejected because it scatters the sync/async boundary across every observer and makes the threading model harder to reason about. Concentrating `spawn_blocking` in the worker loop means there is exactly one place to read to understand how observer code reaches a thread. The async-trait shape would also pull `async-trait` macros into a crate that otherwise has no async dependencies in its observer modules, for no benefit.

**A central observer scheduler that runs all observers in a long-lived background task and writes results into a shared snapshot cache, which the worker loop then reads.** Rejected as overengineering for five observers in a single-tenant agent. The central scheduler would need its own lifecycle management, its own failure handling, its own coordination with the worker loop's shutdown signal, and its own tests. Per-observer state with the worker loop as the orchestrator gives the same outcome with much less infrastructure. The central-scheduler shape may become correct later if Ken grows to dozens of observers with complex inter-dependencies, but committing to it now solves a problem Ken does not have.

**One worker thread per observer, communicating with the main loop via channels.** Rejected for the same reason as the central scheduler, plus an additional one: it inverts the control flow. The worker loop would no longer ask "what is the current state" but would instead receive pushes and have to reason about ordering and freshness across multiple producers. This is a heavier model than the heartbeat loop needs, and it would make the per-tick semantics of "send the current snapshot" much harder to define.

**Spawn each observer in its own OS process, isolated from the agent.** Rejected as wildly disproportionate. The threat model is family-IT endpoints, not multi-tenant infrastructure. A panicking observer is a bug to fix, not a security boundary to enforce at the OS level.
