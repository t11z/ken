# ADR-0020: Windows Update Observer

- **Status:** Accepted
- **Date:** 2026-04-10
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

The Windows Update observer is one of the five subsystems the Ken agent reports in every heartbeat. Today it is a stub: `crates/ken-agent/src/observer/windows_update.rs` returns `None` and emits a `tracing::debug!` message that says the registry query is not yet implemented. The corresponding `WindowsUpdateStatus` type in `ken-protocol` carries `pending_update_count` and `pending_critical_update_count` as bare `u32` values that the stub fills with `0` whenever it does run, which is exactly the schema lie that ADR-0018 was written to make impossible.

Issue #4 in the repository proposes to replace the stub with a real implementation that uses the Windows Update Agent (WUA) COM API via the `windows-rs` crate: open an `IUpdateSession`, create an `IUpdateSearcher`, search for updates with the criterion `IsInstalled=0`, and count total and critical pending updates from the result. The issue notes that the call is synchronous and may take 10–30 seconds on its first invocation because it can trigger a Windows Update check against Microsoft's servers, and recommends running it on a background thread with a timeout.

The previous two ADRs in this sequence settled the surrounding architecture. ADR-0017 defined the observer execution model: observers are sync Rust functions invoked from the async worker loop via `spawn_blocking`, each owns its own state including its last known good value, and a per-observer time budget protects the heartbeat loop from slow observers. ADR-0018 defined the wire-format semantics for observer output: every observer-contributed value is wrapped in `Observation<T>` with `Fresh`, `Cached`, and `Unobserved` variants, and the schema can no longer express "I have no idea, but here's a zero." With both of those in place, this ADR can decide what specifically the Windows Update observer reads, when it reads it, how it caches the result, and how its failure modes map to the wire format — without re-litigating any of the model questions.

The decision is forced now because issue #4 cannot be implemented coherently without it. An implementer asked to "implement WUA pending update counts" would have to invent a TTL, invent a definition of "critical," invent a failure-mapping convention, and decide whether to run the COM call in the heartbeat path or in the background — four architectural choices that should not be made by whoever happens to pick up the issue.

## Decision

The Windows Update observer collects pending update counts from the Windows Update Agent COM API on a background-refresh schedule with a one-hour cache TTL. The heartbeat-tick path never blocks on a WUA call. All failure modes map to `Unobserved`. "Critical" is defined as `MsrcSeverity == "Critical"` on the `IUpdate` interface.

The observer follows the model from ADR-0017: it is a struct that owns its last successfully collected values and the timestamps at which they were collected. It implements the observer contract by exposing a sync `collect()` function (or trait method, depending on the eventual interface chosen by the implementation prompt) that returns immediately, never reaching out to WUA from the calling thread. WUA calls happen exclusively on a long-lived background task spawned at observer construction time, and the heartbeat-tick path only ever reads the cache.

The fields the observer is responsible for inside `WindowsUpdateStatus` are:

- `pending_update_count`: the total number of updates returned by `IUpdateSearcher::Search` with the criterion `IsInstalled=0`, expressed as `Observation<u32>`.
- `pending_critical_update_count`: the count, within that same result set, of `IUpdate` entries whose `MsrcSeverity` property equals the literal string `"Critical"`, expressed as `Observation<u32>`.

The two counts are produced by a single WUA search and a single iteration over the result set. They are always emitted as a pair: when one is `Fresh`, the other is also `Fresh` from the same observation; when one is `Cached`, so is the other; when one is `Unobserved`, so is the other. The observer never returns a partial result with one count fresh and the other unobserved, because both counts derive from the same underlying search and the same result iteration, which either succeeds entirely or fails entirely.

The other fields of `WindowsUpdateStatus` — `last_search_time` and `last_install_time`, which read from the Windows Update registry under `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\Results\` — are *not* covered by this ADR. They are cheap registry reads and belong to the cheap-observer path. Whether they live in the same observer struct as the WUA search or in a separate cheap registry observer is an implementation choice for the prompt that lands this work; this ADR governs only the expensive WUA search path and its caching.

### Background refresh

At observer construction, a Tokio background task is spawned that runs in a loop:

1. Wait until either the cache is empty (cold start) or the cached `observed_at` timestamp is more than one hour old.
2. Call `tokio::task::spawn_blocking` on the WUA search routine. The search initializes COM in the spawned thread with the apartment model required by WUA, creates an `IUpdateSession`, creates an `IUpdateSearcher`, calls `Search("IsInstalled=0")`, iterates the result, computes total and critical counts, and returns either the pair of counts on success or an error variant on any failure.
3. On success, write the new `(total, critical, observed_at = now)` triple into the observer's shared state. On failure, write nothing — the previous successful values, if any, remain in the cache and the next heartbeat will continue to serve them as `Cached`.
4. Loop.

The background task respects the same shutdown signal as the worker loop. It does not attempt to cancel an in-flight WUA call when shutdown is requested — sync FFI is not cancellable, as ADR-0017 already accepted — but it does not initiate a new WUA call once shutdown has been signalled.

The background task is independent of the heartbeat tick. The heartbeat loop reads the cache on every tick and either finds a value (from a previous successful refresh) or finds nothing (cold start, no successful refresh yet). When the cache contains a value, the observer reports `Fresh` for the values whose `observed_at` is within the current heartbeat's tick window and `Cached` otherwise. In practice, the background task refreshes much more often than necessary to keep values "fresh enough," so the distinction between `Fresh` and `Cached` is a matter of which side of a one-tick boundary the most recent successful refresh fell on. The wire format expresses this honestly via `Observation<T>`.

### TTL

The cache TTL is **one hour**, hardcoded for Phase 1, with no configuration field. The choice is deliberate and the reasoning is short: the WUA call costs nothing Ken owns. The wall-clock cost (10–30 seconds) is paid on a background task that does not block the heartbeat loop. The CPU cost is negligible because WUA spends its time on network IO against Microsoft's update infrastructure, which has no problem with the additional load. The only thing the TTL trades off is the staleness of the displayed pending-update count, and "as fresh as possible" is the right answer when freshness is free.

A longer TTL — six hours, twenty-four hours — would not save anything Ken owns; it would only let the displayed count drift further from reality between refreshes. There is no resource budget that argues for the longer value. One hour is short enough that a freshly released update is reflected in the admin UI within a useful interval after the agent's next refresh, and long enough that we are not gratuitously hammering WUA on every heartbeat.

If Phase 2 deployment experience shows that a different value is needed, the change is a constant in the observer source file, not an ADR-level change. This ADR commits to "the TTL exists and is short," not to "the TTL is one hour forever."

### Failure mapping

Every failure mode of the WUA search path maps to `Observation::Unobserved` for both fields. There are no exceptions and no special-cased failure variants.

Concretely, the following situations all produce `Unobserved`:

- COM initialization fails on the spawned thread.
- `CoCreateInstance` for `IUpdateSession` fails because the Windows Update service is unavailable, disabled, or corrupt.
- The search call returns a WUA-specific error `HRESULT` such as `WU_E_NO_CONNECTION`, `WU_E_NO_SERVICE`, or any other error from the WU error space.
- The search call exceeds an internal time guard (an upper bound far longer than the heartbeat budget — for example, 120 seconds — chosen to catch genuinely hung calls without preempting normal slow ones).
- Any panic or unexpected exception during result iteration.

A successful search that returns zero updates is *not* a failure. It maps to `Observation::Fresh { value: 0, observed_at: now }`. The wire format distinguishes this from `Unobserved`, and the distinction matters: "the agent confirmed there are no pending updates" is a different operational fact from "the agent has no idea whether there are pending updates."

The reasoning for collapsing all failures into `Unobserved` is the same reasoning ADR-0018 gave for not subdividing the variant in the first place: distinguishing failure modes on the wire would require carrying failure history, and the operationally relevant fact for a server-side reader is the same in every case — there is no number to render. The diagnostic information about *why* the observer failed lives where diagnostic information belongs: in the agent's local audit log (per ADR-0001 T1-5) and in `tracing` output. An admin investigating "why is my Windows Update count missing" reads the audit log on the affected endpoint, not a wire-format field.

### Definition of "critical"

"Critical" means `MsrcSeverity == "Critical"` on the `IUpdate` interface returned by the WUA search.

The WUA result set does not have a single "is this critical?" boolean. Microsoft expresses update urgency through several overlapping properties: `MsrcSeverity` (a string from the set `"Critical"`, `"Important"`, `"Moderate"`, `"Low"`, or empty), the `Categories` collection (which contains entries like `"Security Updates"` and `"Critical Updates"`), and the `AutoSelectOnWebSites` flag. None of these fully captures what an admin means when they ask "how many critical updates does this machine need," and Microsoft itself uses the terms inconsistently between WUA, the Update Catalog, and the MSRC severity ratings.

Ken adopts `MsrcSeverity == "Critical"` as a deliberate convention. It is the most precise of the available signals because it ties directly to Microsoft's published severity rating for the underlying vulnerability, not to the looser Windows Update category labels. It is also the most stable: the `MsrcSeverity` property is set once when the update is published and does not depend on the local machine's category configuration.

The convention is documented in the observer source and in the relevant section of `docs/architecture/`. Server-side rendering (admin UI tooltips, dashboard labels) refers to the count as "critical updates" without further qualification, but the underlying definition is fixed and citable. If a future ADR decides that a different definition serves the family-IT use case better, the change is bounded: rewrite the iteration filter in one observer, bump the schema version if the field semantics change, and document the new convention.

## Consequences

**Easier:**

- Issue #4 has a complete specification. An implementer following this ADR knows which COM interfaces to call, which property to filter on, where to put the call (background task), how often to call it (one-hour TTL), and what to do with every failure mode (`Unobserved`). The number of architectural choices left to the implementation prompt is zero.
- The heartbeat loop never waits for WUA. Even on the cold-start path, the loop ticks, the cache is empty, the observer reports `Unobserved` for both counts, and the heartbeat ships in milliseconds. The first successful background refresh — typically within seconds of service start — populates the cache, and from the next tick onward the values are present.
- Cold-start behavior is honest. The first few heartbeats after service start may report `Unobserved` for these fields, which is exactly what they are: not yet observed. There is no synthetic zero, no "loading" sentinel, no waiting for the first refresh to complete before sending the heartbeat. The schema and the execution model agree.
- The "critical" definition is decided once and citable. Server-side code, documentation, and future observers (e.g., a third-party patch observer) refer to the same convention without re-deciding.

**Harder:**

- The observer is the first one in Ken to spawn a long-lived background task. ADR-0017 anticipated this — its Decision section explicitly allows per-observer state including background work — but this is the first concrete instance, and the implementation prompt will need to specify the task's lifecycle (spawn at observer construction, shut down on agent shutdown, never panic the worker if it crashes) with care. Future observers that need similar treatment will follow this one's pattern.
- COM initialization happens on the `spawn_blocking` thread, not on the agent's main thread. This is correct (the WUA call is the only consumer of COM in this observer, and `wmi`-crate-based observers like Defender manage their own COM init independently), but it means the agent ends up with multiple threads each running their own COM apartment. This is supported by Windows and is the standard pattern for COM consumers in async Rust, but it is worth noting because it differs from the simpler "one COM init at process start" pattern used in single-threaded Windows applications.
- The 120-second internal time guard inside the WUA search routine is a number that has to be picked, and it is not the same number as ADR-0017's per-observer heartbeat budget. The two budgets serve different purposes: the heartbeat budget protects the per-tick path (which this observer never enters in a slow way, by design), while the WUA-internal guard protects the background task from a genuinely hung COM call. The implementation prompt will need to make this distinction clear.
- A user staring at the admin UI immediately after agent install will see "pending updates: not yet observed" for up to 30 seconds. This is correct behavior — the agent really does not know yet — but it is a UX moment that should be handled gracefully on the rendering side. This ADR does not specify how; it only commits to the wire format being honest about the state.

**Accepted:**

- The Windows Update observer reports `Unobserved` whenever the WUA service is disabled, broken, or unreachable, with no indication on the wire of *why*. An admin who wants to know "why has this machine's update count been missing for two days" must read the local audit log on that endpoint. This is the cost of refusing to subdivide `Unobserved`, and ADR-0018 accepted that cost in general; this ADR confirms it for the specific case of WUA failures.
- "Critical" follows MSRC severity, not the Windows Update category labels. A cumulative monthly rollup that is *categorized* as "Security Updates" but whose individual MSRC severity is "Important" will not count as critical in Ken. Some admins may find this surprising. The alternative — counting by category — produces inflated numbers and is less precise about actual vulnerability severity. The convention is documented; surprised admins can read the documentation.
- The TTL is one hour, full stop. There is no per-deployment override, no command-line flag, no admin-UI control. Adding configuration is a future decision and requires either an operational reason (which Phase 1 will not have) or a usability complaint (which would itself require deployment experience to surface).
- The observer is responsible for two fields of one subsystem. It does not also handle the registry-based fields (`last_search_time`, `last_install_time`) by mandate of this ADR — that arrangement is left to the implementation prompt. The two paths are very different in cost and reliability, and forcing them into one observer would couple a cheap reliable read to an expensive flaky one for no benefit.

## Alternatives considered

**Lazy refresh: call WUA from the heartbeat-tick path on cache miss, blocking until either the call returns or ADR-0017's per-observer budget expires.** Rejected because the cold-start behavior is bad. The first heartbeat after service start would attempt a WUA call, fail at the budget (the budget is sized for sub-second observers, not for 30-second WUA cold calls), and produce `Unobserved`. The second heartbeat one minute later would do the same thing, because no successful collection has happened yet to populate a cache. The pattern repeats indefinitely until either a heartbeat happens to catch a faster WUA response or never. The background-refresh model decouples observation from the heartbeat tick entirely, so the first WUA call gets as long as it actually needs (within a sane upper bound), and subsequent heartbeats benefit from the result.

**A six-hour TTL.** Rejected after consideration. The original instinct toward six hours came from the heuristic "expensive operations should be rare," which fails here because the operation costs nothing Ken owns. The wall-clock cost is paid on a background task; the CPU cost is negligible; the network cost is borne by Microsoft's update infrastructure, which is not something Ken needs to economize. The only thing a longer TTL trades for is staleness, and there is nothing on the credit side of the ledger to compensate. One hour is the conservative-toward-fresh choice and matches the principle "make the loud failure mode the default, not the silent one": a stale value drifts invisibly, a fresh-but-occasional refresh announces itself in the audit log.

**Subdivide `Unobserved` into specific failure variants on the wire.** Rejected as a violation of ADR-0018, which deliberately collapsed cold-start, persistent-failure, and transient-failure into a single variant. Re-opening that decision in the first observer to use the new wire format would be a rapid retreat from a fresh commitment, and the operational case for the distinction is weak: an admin who wants to know *why* a WUA observation is missing reads the local audit log, not a JSON field. The audit log carries arbitrary detail; the wire format carries the conclusion.

**Define "critical" as `Categories contains "Security Updates" or "Critical Updates"`.** Rejected because the category labels are looser and produce inflated counts. A monthly cumulative rollup is categorized as "Security Updates" regardless of whether any individual fix in the rollup is genuinely critical, so counting by category would label every Patch Tuesday as having at least one critical update, which is technically true but operationally meaningless. `MsrcSeverity` is the precise signal and should be the one Ken uses.

**Read pending update counts from the registry instead of from the WUA COM API.** Rejected because the registry does not contain reliable pending counts. It contains enough information to answer "when did Windows Update last successfully search?" and "when did it last install?" — which is why the cheap registry observer covers `last_search_time` and `last_install_time` — but it does not contain a current count of updates that the local Windows Update Agent considers pending. That count exists only in the WUA COM API surface, by design. There is no shortcut.

**Skip the Windows Update observer entirely for Phase 1 and ship without it.** Rejected because the observer is one of the five subsystems that the architecture diagram, the CLAUDE.md files, and the wire format have all anticipated since the project began. Shipping without it would leave a visible gap in the dashboard and would defer a decision that can be made cleanly now. The observer is one of the most operationally interesting things the agent can report — "your aunt has 3 pending security updates" is exactly the use case Ken exists for — and deferring it would weaken the Phase 1 demonstration considerably.
