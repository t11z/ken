# ADR-0018: Status snapshot observation semantics

- **Status:** Accepted
- **Date:** 2026-04-10
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

The `OsStatusSnapshot` type in `ken-protocol` is the structured payload that the agent reports to the server in every heartbeat. It is the place where observer output crosses the wire. Today the schema expresses three different ideas with two different mechanisms, and the result is incoherent in ways that have not yet caused damage only because the observers are still stubs.

Subsystem-level absence is expressed as `Option<T>` on the snapshot itself: `defender: Option<DefenderStatus>`, `windows_update: Option<WindowsUpdateStatus>`, and so on. The doc comment is explicit that `None` means "the observer could not collect data, not that the feature is disabled." Inside a subsystem, some fields are `Option<T>` for a different reason — `last_full_scan: Option<OffsetDateTime>` means "the value is `None` if no full scan has ever happened," which is a property of the data, not a property of the observation. And other fields, notably `pending_update_count: u32` inside `WindowsUpdateStatus`, are not optional at all: they are bare values that the current Phase 1 stub fills with `0` regardless of whether the observer actually looked. A server reading such a field cannot tell whether the agent has zero pending updates or whether the agent has no idea.

The freshness of values is also implicit. `OsStatusSnapshot` carries a single `collected_at` timestamp at the top level, which today reflects the moment the snapshot struct was assembled. This was correct under the previous execution model, where every observer ran inline once per heartbeat tick. ADR-0017 has now committed Ken to an execution model in which observers may serve cached values across multiple ticks, refreshing on their own schedules. Under that model, a single top-level `collected_at` overstates the freshness of every cached field — the snapshot might claim "collected at 14:00:00" while a Windows Update value inside it was actually observed at 10:00:00. ADR-0017 acknowledges this as a temporary inconsistency and defers the fix to this ADR.

The decision is forced now because the first observer that will exercise both problems at once — the Windows Update observer in issue #4 — cannot be specified without a wire format that can express "the value of `pending_update_count` is 7, observed 4 hours ago, served from cache." Adding that observer against the current schema would either hardcode the lie (a stale value with no indication of staleness) or invent a per-observer ad-hoc convention (a magic sentinel, a parallel timestamp field, a special `0` meaning "unknown") that the next observer would either copy or contradict. Neither is tolerable.

This ADR governs the *expression* of observation state in the wire format. It does not govern the execution model that produces those observations — that is ADR-0017. It does not govern the specific fields of any individual observer — those are decided in the ADRs that introduce each observer.

## Decision

Every value contributed by an observer to `OsStatusSnapshot` is wrapped in a generic `Observation<T>` type defined in `ken-protocol`. The type has three variants that distinguish the three states an observer can be in for any given field:

```rust
pub enum Observation<T> {
    Fresh { value: T, observed_at: OffsetDateTime },
    Cached { value: T, observed_at: OffsetDateTime },
    Unobserved,
}
```

The semantics are exact and exhaustive:

**`Fresh`** means the observer collected this value during the current heartbeat tick. The `observed_at` timestamp records when the underlying Windows API call returned. A reader may treat `Fresh` values as the agent's current understanding of the endpoint state.

**`Cached`** means the observer did not collect this value during the current heartbeat tick and is serving its last successfully collected value, in line with the per-observer caching policy defined by ADR-0017. The `observed_at` timestamp records when that earlier collection happened. The value is the agent's best available answer; it is not the current state, and a reader that needs current state must reason about the age explicitly.

**`Unobserved`** means the observer has no value to report for this field. This covers two situations that are deliberately collapsed into one variant: the observer has never successfully collected this field (cold start, persistent failure, feature absent on this Windows edition), and the observer attempted to collect during this tick but failed and has no prior cached value to fall back on. Both cases are expressed identically because, from the server's perspective, the operationally relevant fact is the same: there is no number to render. Distinguishing the two would require carrying failure history in the wire format, which is a heavier commitment than this ADR is willing to make.

The wrapper applies to every observer-contributed field of every subsystem. It does not apply to fields that are not observer output — for example, the snapshot's own `collected_at` (which now means *when the snapshot struct was assembled*, not *when its contents were observed*) remains a bare `OffsetDateTime`.

The wrapper also does not collapse with the existing `Option` semantics inside subsystem types. A field that is genuinely optional in the data model — `last_full_scan: Option<OffsetDateTime>` meaning "no full scan has ever been recorded by Defender" — becomes `Observation<Option<OffsetDateTime>>`. The outer `Observation` says whether the observer looked; the inner `Option` says whether Defender's own state contained a value. This is verbose but unambiguous, and the verbosity is a feature: it forces the implementer of each observer to think about both axes separately, which is exactly what this ADR exists to enforce.

The subsystem-level `Option` wrappers on `OsStatusSnapshot` itself (`defender: Option<DefenderStatus>`, etc.) are removed. They are replaced with non-optional subsystem types whose fields are individually `Observation<T>`. A subsystem that has never collected anything is now expressed as a struct in which every field is `Unobserved`, rather than as a `None` at the snapshot level. This eliminates the third meaning of `None` from the schema entirely: the only `None` in the snapshot now is `Option<T>` *inside* an `Observation<Option<T>>`, with the data-model semantics described above.

Serialization uses serde's default tagged-enum format (`{"kind": "fresh", "value": ..., "observed_at": "..."}`), with `#[serde(rename_all = "snake_case")]` and `#[serde(tag = "kind")]`. The `Unobserved` variant serializes as `{"kind": "unobserved"}` with no other fields. A round-trip test for each variant lives in `crates/ken-protocol/tests/`.

The `SCHEMA_VERSION` constant in `ken-protocol` is bumped. This is a breaking change to the wire format. Phase 1 has no deployed agents and no committed compatibility, so the cost is absorbed in a single coordinated change across `ken-protocol`, `ken-server`, and `ken-agent`. Old payloads in tests and fixtures are rewritten as part of the same change.

## Consequences

**Easier:**

- An observer that switches from "always fresh" to "cached with TTL" — the kind of change ADR-0017 explicitly permits as a local decision — has zero impact on the wire format. The observer simply starts emitting `Cached` instead of `Fresh` for some ticks. No schema migration, no server update, no protocol-version bump. The implementation cost of caching is paid where caching is decided, not in three crates at once.
- The server's rendering logic becomes a pattern match on three explicit variants. There is no rule like "if the value is zero, check whether the timestamp is recent, and if so, treat it as unobserved" — that kind of brittle convention is exactly what `Observation<T>` exists to replace. The admin UI can render `Cached` values with a "last observed N hours ago" indicator without inventing the meaning of "stale."
- The schema can no longer express "I have no idea, but here's a zero." The category of bug where a stub returns a default value and the server treats it as ground truth becomes structurally impossible. A field that the observer did not produce is `Unobserved` and renders as such; a field that the observer produced as actually zero is `Fresh { value: 0, ... }` and renders as zero. The two are visibly different in JSON, in tests, and in the database.
- ADR-0017's explicit acknowledgement of "a heartbeat may carry a mix of fresh and cached values" stops being a footnote and becomes a structural property of every heartbeat. There is no implicit assumption about freshness anywhere in the protocol.

**Harder:**

- Every observer-facing struct in `ken-protocol` is rewritten. `DefenderStatus`, `FirewallStatus`, `BitLockerStatus`, `WindowsUpdateStatus`, and the embedded sub-types all change shape. This is a one-time cost that lands in a single coordinated change, but it is a real change that touches every observer module in `ken-agent` and every status-rendering code path in `ken-server`.
- The JSON payload grows. A field that was `7` is now `{"kind": "fresh", "value": 7, "observed_at": "..."}`. For the heartbeat sizes Ken deals with (a handful of subsystems with a few fields each, sent once per minute), the absolute cost is negligible — tens of bytes per heartbeat — but the schema is visibly more verbose to a human reading the JSON.
- Observers must construct `Observation` values explicitly. The implementer of each observer has to decide, for each field, whether this tick produced a fresh value, served a cached one, or has nothing to offer. This is more work than returning a bare struct. It is also exactly the work that this ADR is forcing into the open, and it is the same work that would otherwise have to happen ad-hoc on the server side with worse information.
- Server-side handlers must pattern-match on three variants per field, even when they only care about the value. A helper method like `Observation::value()` returning `Option<&T>` keeps simple consumers concise, but the explicit variants are the contract; the helper is convenience.
- Tests for individual observers must cover all three variants, not just the happy path. This is a discipline cost, paid in test code, in exchange for a guarantee that no observer can silently emit nonsense for an unobserved field.

**Accepted:**

- The schema becomes verbose. `Observation<Option<OffsetDateTime>>` is a real type that real implementers will read and curse mildly. This is the price of distinguishing observation state from data state, and the alternative — collapsing them into a single `Option` — is what this ADR exists to reject.
- Cold-start failures and persistent failures are indistinguishable on the wire. Both are `Unobserved`. A future ADR could introduce a richer failure model if operational experience shows that the distinction matters; today, it does not, and adding the distinction speculatively would inflate the schema for no concrete reader.
- The schema-version bump consumes a number that could otherwise have been saved for a "real" change. In Phase 1 this is free, but it sets a precedent that wire-format ADRs are expected to bump the version. This is intended.
- The schema does not carry an explicit "this observer is currently disabled by configuration" state. If a future ADR allows disabling observers (which today's design does not), that ADR will need to decide whether disabled observers report `Unobserved` or whether the schema gains a fourth variant. Today, all observers are always trying, so the question does not arise.

## Alternatives considered

**`Option<T>` everywhere, plus a separate `last_observed_at: OffsetDateTime` field per subsystem.** Rejected because it conflates three meanings of `None`: subsystem-level absence, data-model optionality (e.g., "no full scan has ever run"), and per-tick non-collection. A reader of the JSON cannot distinguish them. The rule "if the value is `None` and `last_observed_at` is recent, the observer probably failed" is exactly the kind of brittle convention that the schema should make impossible. It also leaves no room to express "this value is from a successful collection four hours ago" — the only timestamp is per-subsystem, and a single subsystem can have multiple fields with different freshness.

**Hybrid: bare values for cheap observers, `Observation<T>` only for expensive ones.** Rejected because it couples the implementation choice ("is this observer cached?") to the wire format. The moment a previously-cheap observer needs to start caching — for example, if a Windows update changes WMI behavior and Defender queries become slow — the schema would have to change in lockstep, dragging in a coordinated agent-server release for what should be a local agent change. ADR-0017 went out of its way to make caching a local decision; the wire format must match.

**A flag-style approach: bare value plus a per-field `_is_cached: bool` and `_is_unobserved: bool`.** Rejected because it allows illegal combinations. A field can have `is_cached: true, is_unobserved: true` and a value of `0`; what does that mean? The whole class of bugs where two flags drift out of sync is the kind of bug that an enum prevents at compile time. Two booleans where there should be three variants is a Rust anti-pattern.

**A single `freshness: Freshness` enum at the subsystem level, with all fields inside the subsystem implicitly inheriting it.** Rejected because it forces an entire subsystem to be either all-fresh or all-cached. Real observers may collect some of their fields successfully and fail on others — a WMI query that gets `antivirus_enabled` but fails on `signature_age_days`, for example. The wire format must allow the successful field to be `Fresh` and the failed one to be `Unobserved`, in the same subsystem, in the same tick. A subsystem-level freshness enum cannot express that.

**Defer the decision and let issue #4 invent its own convention for `pending_update_count: 0 or unobserved`.** Rejected because it is exactly the path that produces drift. Issue #4 is the first observer with this problem, but it will not be the last. If it invents its own answer, the next observer will either copy that answer (locking the convention in without an ADR) or contradict it (forcing a future cleanup that touches both observers). The right time to set the model is now, before any production observer has been written.
