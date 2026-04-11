# Phase 1 Closure Audit

This document records the architectural audit pass that closed Phase 1 of Ken. It is not a copy of the audit reports themselves — those were working artifacts of the closure session and are not preserved verbatim. It is a destillation: what was checked, what was found, how each finding was resolved, and what was deliberately left for later.

The document is written for a maintainer who knows the project but was not present for the closure pass. Reading it end-to-end should take fifteen minutes and should leave the reader able to answer two questions: *what made Phase 1 closable*, and *what is still pending under what name*.

ADRs remain the source of truth for individual architectural decisions. This document is the index across them and the rationale for the closure as a whole. Where it cites a decision, it cites it by ADR number and trusts the reader to read the ADR for substance.

## Method

The audit ran in four passes, each scoped to one architectural concern that ADR-0001's trust boundaries depend on or that Phase 1 closure depended on for technical reasons. Each pass compared the relevant ADRs against the actual code in `t11z/ken` and produced a list of findings categorized as one of: *real drift* (code disagrees with an accepted ADR), *verification gap* (the audit could not determine the state from its file scope), or *clarification opportunity* (the ADR is silent or ambiguous on a point the code makes a choice about).

The four passes were:

- **Pass 1 — mTLS bridge.** ADR-0004, ADR-0008, ADR-0016, ADR-0017 against `crates/ken-server/src/http/{tls,endpoint_id,agent_api}.rs` and the related test bridge.
- **Pass 2 — IPC and trust boundary.** ADR-0001 T1-4, ADR-0010, ADR-0012 against `crates/ken-agent/src/ipc/server.rs`, `crates/ken-agent/src/killswitch.rs`, and `crates/ken-agent/src/service/lifecycle.rs`.
- **Pass 3 — Observer subsystem.** Two sub-passes: P3a covered ADR-0018 and ADR-0019 against the observer trait, snapshot loop, and the four cheap observers; P3b covered ADR-0020 against `crates/ken-agent/src/observer/windows_update.rs` and the seam between ADR-0018, ADR-0019, and ADR-0020.
- **Pass 4 — Admin UI rendering.** ADR-0006 and ADR-0013 against `crates/ken-server/src/http/admin.rs` and the askama templates under `crates/ken-server/templates/`.

Each pass was scoped to a small file set rather than the full repository. This was a deliberate choice — narrow scope keeps each pass small enough to reason about end-to-end — but it had a cost that surfaced after closure: drift between subsystems, where the boundary itself contains the bug, is hard to see from inside either subsystem alone. The "Lessons learned" section at the end of this document records this as the most important methodological finding of the audit.

The audit also depended on a Wave 0 verification step that resolved several findings from "unknown in scope" to "actually verified." That step is treated as part of the audit, not as a preliminary, because some Wave 0 results changed the closure plan substantively.

## Findings and resolutions

### Pass 1 — mTLS bridge

Pass 1 found two verification gaps and two minor clarification opportunities. No real drift.

The verification gaps were both about wiring that lived outside the scoped file set. Specifically, whether `KenAcceptor` is actually mounted at the agent listener in `main.rs` (rather than being defined and unused), and whether the `require_endpoint_id` middleware is actually applied to the `agent_router` in `http/mod.rs`. Both questions matter for the same reason: the entire ADR-0017 bridge is meaningful only if these two wirings exist, and the test file alone can prove correct behavior under correct wiring without proving the wiring itself.

These were resolved in Wave 0 by reading the missing files. Both are correctly wired. `crates/ken-server/src/main.rs` constructs `agent_acceptor = http::tls::KenAcceptor::new(agent_rustls)` and serves the agent listener via `axum_server::bind(agent_addr).acceptor(agent_acceptor)`; `crates/ken-server/src/http/mod.rs` mounts `middleware::from_fn(endpoint_id::require_endpoint_id)` on the `agent_router`. Both bridge halves are present in the production binary.

The two clarification opportunities became ADR-0023:

The first was that `crates/ken-server/src/http/tls.rs` wraps the verifier's `Storage::get_endpoint(...)` call in `tokio::task::block_in_place(|| Handle::current().block_on(...))`. ADR-0008 specified only the `Handle::current().block_on(...)` part. The `block_in_place` outer wrapper is sachlich required — without it, the call panics under a current-thread runtime, which is the test runtime configuration — but ADR-0008 did not say so. A future maintainer comparing the ADR to the code would find an unexplained extra layer.

The second was that ADR-0008 step 5 is ambiguous about what "checks the endpoint's certificate `expires_at` field" means. The code does both: an explicit DB-side check in the custom verifier (against the `expires_at` value Ken stored at enrollment time) and an implicit X.509 `notAfter` rejection further down the chain via `WebPkiClientVerifier`. The end result is correct — an expired certificate is rejected on either path — but the dual-path design is not visible from ADR-0008's text.

ADR-0023 ("Clarify verifier's call pattern and dual-path certificate expiry check") was written to document both. It does not supersede ADR-0008; it acknowledges the `block_in_place` wrapper as part of the required call pattern and makes the dual-path expiry check explicit, distinguishing the DB-side primary check from the cryptographic defense-in-depth fallback. The code itself was unchanged.

### Pass 2 — IPC and trust boundary

Pass 2 found one substantive code drift, one structural finding that turned out to span the IPC and tray-launch subsystems, and one minor hardening opportunity that was deliberately deferred.

The substantive code drift was in the kill-switch implementation. ADR-0012 specifies a two-part mechanism: a state file at `%ProgramData%\Ken\state\kill-switch-requested` plus a service self-stop triggered via the Named Pipe IPC. Both parts existed in the code and worked. What was missing was the `ChangeServiceConfig` call that sets the service's startup type to `SERVICE_DISABLED`, which ADR-0012 step 5 requires before the service reports `Stopped` (so the SCM does not auto-restart it) and which step 6 requires during the startup-refused path (so any subsequent re-enable by Windows Update or admin action is countered). A grep for `ChangeServiceConfig` and `SERVICE_DISABLED` in `crates/ken-agent/` returned zero hits.

This was a Tier-1-near finding because ADR-0001 T1-6 commits Ken to giving the user an always-available way to disable Ken locally, and the kill switch is the implementation of that invariant. The `SERVICE_DISABLED` enforcement was the second of two safeguards that ADR-0012 deliberately layered: the state file persists across restarts but does not prevent restarts, and the SCM-level disable prevents restarts but does not survive an admin re-enable. Both together form the safeguard. Only one was enforced.

The drift was closed by issue #22, implemented in PR #23 ("Enforce SERVICE_DISABLED on kill-switch activation and startup refusal"). The PR added a `set_service_disabled()` function to `crates/ken-agent/src/killswitch.rs` as the sole `ChangeServiceConfigW` call site in the codebase, wired into the activation path via a `finalize_activation()` helper in `ipc/server.rs` and into the startup-refused path via a `handle_startup_refused()` helper in `service/lifecycle.rs`. Both call sites treat the SCM call as non-fatal — the failure is audit-logged via a new `KillSwitchStartupRefused` audit variant, and the surrounding sequence proceeds regardless. The state file remains the primary defense; `SERVICE_DISABLED` is the secondary hardening.

The structural finding was that ADR-0010 specifies one Named Pipe per active interactive session, with the service subscribing to `SERVICE_ACCEPT_SESSIONCHANGE` and managing pipe lifecycle in response to `WTS_SESSION_LOGON` and `WTS_SESSION_LOGOFF` events. The pipe server code in `ipc/server.rs` uses `WTSGetActiveConsoleSessionId` and creates exactly one pipe for the active console session — no enumeration, no session-change subscription, no pipe lifecycle. From inside Pass 2's file scope, this looked like a clean drift against ADR-0010 requiring either a code refactor or an ADR scope reduction.

The fuller picture became visible only after the closure pass, when PR #19 ("Auto-launch tray app on interactive session logon") was reviewed in context. PR #19 had landed before the audit and implemented the *other half* of the multi-session story: a Windows service dispatcher with `SERVICE_ACCEPT_SESSIONCHANGE`, a session-change event channel, an `enumerate_active_sessions()` helper using `WTSEnumerateSessionsW`, and per-session tray-app launch and termination via `CreateProcessAsUser`. The PR's body explicitly named the asymmetry: *"This PR wires up the first session-change consumer (the tray-launch handler). The same mechanism is what ADR-0010 requires for per-session pipe lifecycle — this provides the foundation."* The Pipe-server side was knowingly left as a follow-up — but no follow-up issue was filed at the time, and the closure pass missed it because Pass 2 looked only at `ipc/server.rs` and not at `service/session/win.rs`.

The result is that the codebase as it stands at the end of Phase 1 is in a partially-implemented state with respect to ADR-0010. Tray apps launch correctly in every interactive session, including secondary sessions reached via Fast User Switching. But only the active console session has a pipe to talk to the service. Tray apps in secondary sessions get spawned, render their UI, and would silently fail at any IPC operation — `RequestConsent` would never be answered, `ActivateKillSwitch` would never be received, status polling would return errors. From the user's perspective in a secondary session, the tray icon would appear functional and would silently betray its function. This is more dangerous than the original Single-session-only design, because the original design was honest about its scope.

The closure decision was to file a follow-up issue rather than block Phase 1 on the pipe-server refactor. The argument was twofold: the foundation laid by PR #19 means the refactor is mechanical (the session-change events are already flowing through the right channel and only need a second consumer), and the methodological lesson — that PR-body-documented follow-ups need explicit issue tracking — is more valuable as part of this audit than as a deferred lesson. The follow-up is tracked as issue #NN ("Per-session pipe lifecycle in ipc/server.rs as foundation laid down in PR #19"). Phase 1 is closed *with this one tracked open item*; it is not closed *as if* the item did not exist.

The minor hardening opportunity was the absence of `PIPE_REJECT_REMOTE_CLIENTS` on the `CreateNamedPipeW` flags. ADR-0010 does not require it. The ACL on the pipe already blocks the remote-client class of attacks the flag would catch. Adding the flag would be defense-in-depth that fails earlier and with a clearer error rather than relying on the ACL check, but it does not change the security envelope. This was deliberately deferred and is listed in "What was deliberately not fixed."

### Pass 3 — Observer subsystem

Pass 3 was the largest pass and produced the most consequential findings. It split into P3a and P3b because the observer subsystem has two distinct architectural seams: between observers and the worker loop (P3a), and between cheap observers and expensive ones (P3b).

P3a verified ADR-0018 (observer execution model) and ADR-0019 (snapshot observation semantics) against the observer trait, the worker loop, and the four cheap observers. Three findings emerged, all interrelated.

The first was that the `Observer` trait in `trait_def.rs` matched ADR-0018's stated execution model (sync body via `spawn_blocking`, panic catch at the boundary) but did not give the four cheap observers any per-observer state. The trait's doc comment said each observer should hold its last known good value, its timestamp, and its refresh policy; the code had none of these. This was tolerable while the observers were Phase 1 stubs returning `Unobserved`, but the first real WMI implementation would have to invent the missing scaffolding.

The second was that the `Observation<T>` wire-format distinction from ADR-0019 (Fresh vs Cached vs Unobserved) had no clear owner. The worker loop in `snapshot.rs` operated generically over `O::Output` and could not re-tag values; the observers had no cache to re-tag from. As long as everything was `Unobserved`, the gap was invisible. The first real observer that produced `Fresh` would expose it immediately.

The third was that the panic-isolation logic in `ObserverSet::run_observer` set the observer slot to `None` on the first panic and never reconstructed the observer. ADR-0018 explicitly said the opposite: *"An observer that panics repeatedly is logged but not disabled — disabling is a behavior change that would require its own decision."* The code's behavior was disable-on-first-panic, despite the ADR requiring keep-retrying-with-logging.

P3b verified ADR-0020 (Windows Update observer) against `windows_update.rs` and asked whether the WUA observer was a faithful instance of the ADR-0018 model. It was not. The WUA observer correctly used a background task to perform the long-running COM call and exposed the result through a `tokio::sync::watch` channel — but it did this through a constructor parameter (`shutdown: Arc<AtomicBool>`) that bypassed the trait, and the trait knew nothing about background tasks. The ADR-0018 model assumed all observers were synchronous structs invoked from `spawn_blocking`. The WUA observer satisfied the trait signature by reducing `observe()` to a `watch::borrow()` call but did its real work entirely outside the trait. Any second expensive observer would have to invent the same plumbing.

P3b also found a specific local bug in `windows_update.rs` line 106: the read path used a one-second age threshold to decide between `Fresh` and `Cached`, on the theory that this would distinguish "freshly written by the background task" from "already in the cache." Under realistic heartbeat tick intervals of 30 to 60 seconds, this heuristic produces `Cached` for essentially every read and `Fresh` essentially never. The wire format's `Fresh` distinction was hollow for this observer.

The three P3a findings and the two P3b findings traced back to one missing decision: the trait did not express the difference between cheap and expensive observers, and nothing in the system owned the `Fresh`-versus-`Cached` decision. ADR-0021 ("Observer background-refresh pattern") was written to fill that gap. It introduces an explicit `ObserverKind` distinction (`Synchronous` vs `BackgroundRefresh`), an `ObserverLifecycle` handle through which expensive observers receive their shutdown signal and spawn background tasks via a single orchestration point, and a `TickBoundary` parameter on the read path so observers can compute `Fresh`/`Cached` correctly against a value supplied by the worker loop rather than against a wall-clock heuristic. ADR-0021 does not supersede ADR-0018; it adds a path. The cheap-observer model from ADR-0018 remains in force.

ADR-0021 was implemented by issue #20, in PR #21 ("Refactor observers to background-refresh pattern (ADR-0021)"). The PR landed `crates/ken-agent/src/observer/lifecycle.rs` and `crates/ken-agent/src/observer/tick.rs` as new modules, rewrote the `Observer` trait to use the `KIND` associated constant pattern, moved the `Arc<AtomicBool>` shutdown out of the WUA observer's constructor and into the lifecycle hook, and replaced the one-second heuristic in `windows_update.rs` with `tick.tag(counts.total, counts.observed_at)`. The four cheap observers were migrated to the new trait shape but continued to return `Unobserved` for everything; the structural refactor is complete, real WMI implementations are deliberate Phase 2 work.

The panic-isolation finding required a separate ADR because it was a question that ADR-0018 had explicitly deferred ("disabling is a behavior change that would require its own decision"). ADR-0022 ("Observer panic isolation strategy") was written to settle it by *enforcing* ADR-0018's stated intent rather than codifying the broken behavior the audit had found. The Decision specifies catching unwinds inside the `spawn_blocking` closure, holding each observer in an `Arc<Mutex<O>>` so it survives panics, recovering from mutex poisoning explicitly via `PoisonError::into_inner()`, and rate-limiting panic logs at a per-observer cadence of three immediate logs followed by ten-minute suppression windows. ADR-0022 records the user-experience reasoning behind preferring self-healing-by-default over disable-on-first-panic, and explicitly preserves a counted-backoff circuit-breaker pattern as the future Phase 2 upgrade path if rate-limited noise becomes a problem in practice.

ADR-0022 was implemented by issue #24, in PR #27 ("Implement observer panic isolation per ADR-0022"). The PR replaced the `Option<O>` slot with `ObserverSlot<O>` holding `Arc<Mutex<O>>`, wrapped the `observe()` call in `std::panic::catch_unwind` with `AssertUnwindSafe` justified by a comment naming the `MutexGuard`'s `RefCell` interior, used `try_lock` rather than `lock` to preserve the per-observer budget timeout from ADR-0018, and added per-observer `PanicRateLimit` state with the constants `PANIC_LOG_BURST_LIMIT = 3` and `PANIC_LOG_SUPPRESSION_WINDOW = Duration::from_secs(600)`. The `WindowsUpdateObserver` required an explicit `impl std::panic::UnwindSafe` due to tokio's `watch::Receiver<T>` using raw pointers in its waiter list; the justification comment notes that `observe()` only calls `borrow()`, leaving the channel in a consistent state under any panic.

### Pass 4 — Admin UI rendering

Pass 4 found that ADR-0006 (server-rendered HTML stack) and ADR-0013 (askama template migration) were both fully implemented. The eight admin handlers in `admin.rs` route through `#[derive(Template)]` structs; the templates listed in ADR-0013 all exist under `crates/ken-server/templates/`; default HTML escaping is active everywhere (a grep for `| safe` returned zero hits); `agent_router` and `admin_router` remain separate, so the migration did not weaken the listener separation from ADR-0004.

One minor drift: ADR-0013 mandates the `askama_axum` feature for native `IntoResponse` integration, which would simplify the eleven hand-rolled `template.render().map_err(...)?` blocks across the handlers. The code does not use the feature; the boilerplate is verbose but functionally equivalent. This was deliberately deferred and is listed in "What was deliberately not fixed."

No code change resulted from Pass 4. The pass functioned as a confirmation that the ADR-0006 tech debt identified at the time of ADR-0013's writing has been retired.

## What was deliberately not fixed

The audit found a number of items that were either nice-to-have hardenings, polish refactors, or scope expansions that did not belong in Phase 1 closure. Recording them here is the point: future maintainers should find them documented as known, deliberately deferred items rather than rediscover them as drift.

**`PIPE_REJECT_REMOTE_CLIENTS` flag on `CreateNamedPipeW`.** Defense-in-depth hardening against SMB-mounted remote pipe clients. The ACL already blocks this attack class; the flag would cause earlier and clearer rejection. Two-line change. Phase 2 hardening pass.

**`askama_axum` feature activation.** Would reduce ~30 lines of boilerplate across `admin.rs` handlers by replacing the manual `template.render().map_err(...)?` pattern with native `IntoResponse`. Pure cosmetic improvement; aligns the code more closely with ADR-0013's wording. Single-sitting refactor, no semantic risk.

**Sequential vs. parallel observer execution.** `ObserverSet::collect_snapshot` invokes the five observers sequentially with `.await` between each. ADR-0018 says nothing explicit about ordering; the wording "bounded per-tick budget" is ambiguous between "per observer" and "per tick across all observers." With the current observers and budgets, sequential execution gives a worst-case tick duration of roughly five times the per-observer budget. This is tolerable for Phase 1 but would be worth either pinning explicitly in ADR-0018 or replacing with `tokio::join!` if real Windows API observers exhibit pathological budget consumption.

**`recent_security_events` as an atomic `Observation<Vec<SecurityEvent>>` rather than `Vec<Observation<SecurityEvent>>`.** A semantic special case in `OsStatusSnapshot`: the security event subsystem is either entirely observed or entirely not, with no partial observation possible. ADR-0019 permits both shapes; the current shape is consistent with how event-log queries work (all-or-nothing per `EvtQuery` call) but is worth either justifying in a doc comment or revisiting if event-log queries gain partial-failure modes.

**Off-by-one ADR references in code comments.** PR #21's body noted that ADR-0020 references "ADR-0017" in several places where it means ADR-0018, and that `windows_update.rs` constants reference "ADR-0018" while ADR-0020 refers to the same concept as "ADR-0017." The Numbers were stable from a certain point onward; the off-by-one is a fossil from the ADR-numbering shuffle during the observer-subsystem ADR sequence. A grep-and-replace pass would fix it. Cosmetic.

**Background-loop panic containment in `windows_update.rs`.** The `spawn_blocking` call inside the WUA background task is panic-protected, but the surrounding loop in the spawned Tokio task is not wrapped in `catch_unwind`. A panic in the loop's control flow (not in the `wua_search` call itself) would silently kill the task. Low probability — the loop is small and deterministic — but a two-line `catch_unwind` would make the lifetime guarantee honest. Phase 2 hardening pass.

**Multi-session pipe lifecycle in `ipc/server.rs`.** This is *not* a deliberately-not-fixed item — it is a tracked open item, listed here for completeness but resolved differently from the others. See the next section.

## What is still open and tracked

**Issue #NN — Per-session pipe lifecycle in `ipc/server.rs`.** As described under Pass 2, PR #19 implemented the tray-launch and session-event-handling foundation that ADR-0010 requires, and explicitly named the pipe-server side as a follow-up. The follow-up was not filed at the time and was missed by the closure audit's narrow scope. It is filed now, with a body that names PR #19 as the foundation and that scopes the work to extending `ipc/server.rs` to maintain a per-session pipe map driven by the same `SessionChangeEvent` channel that the tray-launch handler already consumes.

Phase 1 is closed with this one tracked open item. The justification for closing Phase 1 in this state rather than blocking on the refactor is that the refactor is mechanical (the foundation is in place), the audit's most valuable lesson is precisely that PR-body-documented follow-ups need explicit issue tracking, and surfacing this lesson in a closure document is more valuable than having an unnamed gap fade quietly into Phase 2.

## What the closure passed and did not pass

Phase 1 is considered closed when the following are true:

- ADR-0001's Tier 1 invariants are enforced by code, not by stubs or comments. Specifically: T1-4 (consent gate trust boundary at the Named Pipe ACL — enforced by `ipc/server.rs` SD construction), T1-5 (mTLS for agent-server traffic — enforced by `KenAcceptor` and the custom verifier per ADR-0008/0017), T1-6 (always-available kill switch — enforced by both halves of ADR-0012 after PR #23).
- All accepted ADRs in `docs/adr/` are either reflected by the code as written, or have a follow-up ADR (clarification, errata, or supersede) that records the resolved state. Specifically: ADR-0008 is clarified by ADR-0023; ADR-0018 is clarified by ADR-0022 (panic isolation) and extended by ADR-0021 (background-refresh pattern); ADR-0019 is implemented in `ken-protocol`'s `Observation<T>` and re-tagged correctly per ADR-0021's `TickBoundary`; ADR-0020 is implemented in `windows_update.rs` modulo the Fresh-heuristic fix from PR #21; ADR-0010 has one tracked follow-up (issue #NN).
- The wire-format `SCHEMA_VERSION` reflects the Phase 1 schema. Verified at value `2` in `crates/ken-protocol/src/version.rs`, bumped in the same commit as the ADR-0019 migration.
- The audit pass itself is recorded in `docs/maintainer/`, so a future maintainer can reconstruct what was checked, what was found, and how the resolutions were chosen — without access to chat logs or working artifacts.

Phase 1 explicitly does *not* pass on the strength of the items listed in "What was deliberately not fixed." Those are known, named, and acknowledged as Phase 2 work or as hardening passes that did not block closure.

Phase 1 also explicitly does not pass under the assumption that issue #NN is non-existent. The closure decision was to record it as an open tracked item rather than to silently include it in the deferred list — the asymmetry between PR #19's foundation and the missing pipe-server follow-up is too consequential to hide as a polish item, and naming it explicitly is part of how this audit addresses its own most important lesson.

## References

- `docs/adr/0001-trust-boundaries-and-current-scope.md` — Tier 1 / Tier 2 invariants
- `docs/adr/0007-architect-implementer-role-separation.md` — the role split this audit pass embodies
- `docs/adr/0008-mtls-implementation-via-custom-verifier.md` — clarified by ADR-0023
- `docs/adr/0010-named-pipe-ipc-between-service-and-tray.md` — open follow-up tracked in issue #NN
- `docs/adr/0012-kill-switch-architecture.md` — drift closed by PR #23
- `docs/adr/0018-observer-execution-model.md` — clarified by ADR-0022 (panic isolation), extended by ADR-0021 (background-refresh pattern)
- `docs/adr/0019-status-snapshot-observation-semantics.md` — implemented in `ken-protocol`, re-tagged per ADR-0021
- `docs/adr/0020-windows-update-observer.md` — Fresh-heuristic fix landed in PR #21
- `docs/adr/0021-observer-background-refresh-pattern.md`
- `docs/adr/0022-observer-panic-isolation-strategy.md`
- `docs/adr/0023-clarify-verifiers-call-pattern-and-dualpath-certificate-expiry-check.md`

PRs:

- PR #19 — Auto-launch tray app on interactive session logon (closed issue #10)
- PR #21 — Refactor observers to background-refresh pattern (ADR-0021) (closed issue #20)
- PR #23 — Enforce SERVICE_DISABLED on kill-switch activation and startup refusal (closed issue #22)
- PR #27 — Implement observer panic isolation per ADR-0022 (closed issue #24)

Issues:

- Issue #20 — Refactor observers to background-refresh pattern (ADR-0021)
- Issue #22 — Enforce SERVICE_DISABLED in kill-switch activation and startup-refused paths (ADR-0012)
- Issue #24 — Implement observer panic isolation per ADR-0022
- Issue #26 — Write phase-1-closure-audit.md (this document)
- Issue #NN — Per-session pipe lifecycle in `ipc/server.rs` as foundation laid down in PR #19
