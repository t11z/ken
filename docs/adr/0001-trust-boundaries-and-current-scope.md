# ADR-0001: Trust Boundaries and Current Scope

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** — (T1-7 partially extracted into ADR-0014, see Notes in ADR-0014)

## Context

Ken is a privileged piece of software running on personal computers belonging to people who trust the family IT chief — typically relatives, partners, close friends. The technical capability to do almost anything on those machines is given. What makes Ken trustworthy is not technical inability; it is a deliberate and documented framework of what Ken will and will not do.

There are two very different kinds of "Ken will not do X" statements, and conflating them is a mistake. Some limits are architectural — they define what Ken *is*. Lifting them would not produce an extended Ken, it would produce a different product that happens to share a name. Other limits describe what Ken *currently does*, and are entirely legitimate candidates for community-driven evolution. A family IT tool that today only observes might, with careful design and community consensus, one day also help remediate. Foreclosing that possibility today would be the architect making a decision on behalf of a community that does not yet exist.

This ADR separates the two categories explicitly. Tier 1 lists the invariants — commitments that are load-bearing for Ken's trust model and are not open to negotiation, ever, by anyone. Tier 2 lists the current scope boundaries — things Ken does not do today, that may be reconsidered through a defined process.

The distinction matters because trust is not the same as rigidity. A project that refuses to evolve is not more trustworthy than one that evolves in public with discipline; it is just more brittle. What makes a project trustworthy is that its users can see, in advance, which commitments are permanent and which are current, and what path exists from "current" to "different."

## Decision

### Tier 1 — Architectural Invariants

These commitments are binding on all versions of Ken, all components, all deployments, and all future contributors. They cannot be loosened by a superseding ADR. An amendment to Tier 1 is possible only in the direction of making it stricter, never weaker. A project that wishes to violate any Tier 1 invariant is not Ken; it is a fork that must take a different name.

**T1-1. No telemetry to project maintainers.** Ken will never communicate with any server operated by the project maintainers, the original author, or any central entity associated with the Ken project. No analytics, no crash reports, no usage pings, no update checks against a public URL, no "phone home" of any kind. The only network endpoint Ken trusts is the one the family IT chief has configured at install time, identified by its mTLS certificate.

**T1-2. No central service operated by maintainers.** There is no Ken SaaS, no Ken account, no central registry of Ken installations, and no component of Ken's normal operation depends on infrastructure operated by the project maintainers. Each deployment is fully self-contained on the family IT chief's hardware. This is an architectural invariant, not a pricing decision — the maintainers could not offer a hosted Ken even if they wanted to, because Ken is not designed to support one.

**T1-3. Single tenant per deployment.** One Ken instance serves exactly one household or one trust-group, as defined by the family IT chief who runs it. Multi-tenant separation is not a feature and will never be added. Family IT chiefs who manage multiple unrelated groups run multiple independent Ken instances. This is enforced architecturally, not by convention, so that accidental cross-tenant data exposure is structurally impossible.

**T1-4. Consent before any remote control session.** Any capability that allows a remote party to see or interact with the endpoint's screen, input, or active session requires an explicit, per-session consent click in the Tray App on the endpoint itself, at the moment the session is requested. This consent cannot be pre-granted, cannot be remembered across sessions, cannot be bypassed by the admin, cannot be obtained through the network, and cannot be implied by any prior action. This invariant binds Ken's current remote-control feature and any future capability that falls within the same category — if the community one day adds screen sharing for training, or remote troubleshooting with audio, or any other form of live presence on the endpoint, each of those still requires per-session consent at the moment of use.

**T1-5. Local audit log visible to the user.** Every action Ken takes on an endpoint is recorded in a local audit log that is readable by the user of that endpoint, without credentials, without tools, without the family IT chief's permission. The user can always see exactly what Ken has done and is doing on their machine. Ken's transparency to its subject is not a feature of the current version — it is part of what Ken is.

**T1-6. Kill switch available to the user.** The user of an endpoint can always disable or uninstall Ken locally, immediately, without the family IT chief's permission and without any network round-trip. A running Ken that the user wants to stop is a running Ken that stops. This is the ultimate safeguard of the consent model: trust is only meaningful when withdrawal is possible.

**T1-7. Source availability under AGPL-3.0 or stricter.** Ken's licensing
commitment is recorded in its own ADR-0014. The substance has not changed:
Ken is licensed AGPL-3.0-or-later, future relicensing toward a more permissive
license is not allowed, future relicensing toward a stricter copyleft is
allowed. The commitment exists so that users can audit Ken's behavior at any
point and so that no future owner of the project can quietly close it. See
ADR-0014 for the full rationale, consequences, and alternatives considered.

### Tier 2 — Current Scope Boundaries

These are things Ken does not do *today*. They are not architectural invariants — they describe the current scope of the project. The community, through the ADR process, may decide to lift any of them. Lifting a Tier 2 boundary requires a dedicated ADR that names the specific boundary being changed, explains the motivation, and defines the new consent and audit mechanisms that accompany the expanded capability. A Tier 2 change is never quiet; it is always an explicit, documented architectural decision.

The purpose of this list is not to forbid evolution, but to ensure that evolution is visible. When a user installs Ken, they can read this list and know with precision what Ken does not do at that moment. When the scope changes, they can read the superseding ADR and understand what changed and why.

**T2-1. Ken does not currently modify endpoint configuration.** The current version of Ken observes. It reads Defender state, update state, firewall state, BitLocker state, and similar operating-system properties. It does not change them, does not enforce policies, does not remediate. A future ADR could introduce carefully scoped configuration actions — for example, "trigger a Defender quick scan on user confirmation" — but such an addition requires its own ADR defining the trigger, the consent mechanism, and the audit trail. Control-plane functionality is not forbidden; it is deliberately absent from the initial version.

**T2-2. Ken does not currently read user files or user data.** The agent's current read scope is limited to operating-system state. It does not read document contents, browser history, cookies, clipboard, saved credentials, or application user data. A future ADR could introduce narrow, consent-gated exceptions — for example, "read the contents of the Windows event log file at a user-approved path" — but broad access to user data is out of scope today and any expansion requires an explicit decision.

**T2-3. Ken does not currently capture keystrokes.** No keylogger, no input recording, no typing-pattern telemetry. The only input events Ken processes today are those flowing through an active consented remote-control session. Any future expansion — for example, ergonomic monitoring or accessibility features — would require its own ADR with a consent model appropriate to the sensitivity of the data.

**T2-4. Ken does not currently take scheduled screenshots or screen recordings.** Screen capture today exists only within active remote-control sessions. There is no "snapshot every N minutes" mode. A future ADR could introduce consent-gated scheduled capture for specific use cases — for example, helping a non-technical user produce a support bundle — but the default and the current behavior is session-only capture.

**T2-5. Ken does not currently install, update, or remove third-party software on the endpoint.** Software lifecycle on user machines is the user's domain. Ken updates only itself. A future ADR could introduce a helper capability — for example, offering to apply a pending Windows Update after user confirmation — but third-party software management is not a current feature.

**T2-6. Ken's server does not currently export data to external systems.** The current version keeps all data on the family IT chief's hardware. A future ADR could introduce opt-in integrations — for example, a webhook for critical alerts, or a Prometheus metrics endpoint — but these would be explicit additions with their own consent and configuration semantics.

**T2-7. Ken's server does not currently support agent auto-enrollment.** Adding an endpoint is today an explicit, manual act by the family IT chief. Future convenience mechanisms — for example, one-time enrollment tokens, QR-code pairing, or local-network discovery with confirmation — are possible through their own ADRs, provided they preserve the "deliberate admission" principle of not accepting unknown agents silently.

### The process for changing Tier 2

A Tier 2 change is initiated by an ADR proposal. The proposal must:

1. Name the specific Tier 2 item being changed (e.g., "supersede T2-1 to allow Defender quick-scan triggering").
2. Describe the new capability in concrete terms — what it does, what it does not do, what inputs it accepts, what outputs it produces.
3. Define the consent mechanism that accompanies the new capability. If the capability is subject to Tier 1 invariants (for example, anything resembling a remote session falls under T1-4), the consent mechanism must be shown to comply with those invariants.
4. Define the audit trail — what is logged locally, what is logged in the server, what is visible to the user of the endpoint.
5. Explain the migration path for existing deployments: whether the new capability is opt-in, opt-out, off by default, or enabled by a configuration flag.

A Tier 2 change ADR cannot violate any Tier 1 invariant. If a proposed change would require loosening a Tier 1 item, the proposal is rejected on principle and no further discussion is required. Tier 1 is the floor; Tier 2 moves above it.

## Consequences

**Easier:**
- The trust story is honest about what is permanent and what is current. Users who want permanent guarantees know exactly where to find them. Users who want to see Ken grow know exactly what path that growth follows.
- Feature requests can be triaged cleanly. A request that touches Tier 1 is rejected on principle. A request that touches Tier 2 is a candidate for an ADR. Ad-hoc debate about whether a feature "fits the spirit of Ken" is replaced with a clear procedural question: which tier does it touch?
- The community can contribute to Ken's direction without needing to fork. The path from "I wish Ken did X" to "Ken now does X" exists and is documented.
- Legal exposure under DSGVO and similar frameworks is tightly bounded by Tier 1. No future commit can accidentally introduce telemetry to project maintainers, because the architecture does not support it and the invariant forbids it.

**Harder:**
- Writing a Tier 2 change ADR requires real work — it is not enough to say "we should do X." The author must specify consent, audit, migration, and compliance with Tier 1. This friction is intentional.
- Tier 1 invariants cannot be fixed if they turn out to be wrong. The architect accepts this risk deliberately: the value of an immovable floor is that it cannot be moved, and any mechanism that could move it in good cases could also move it in bad ones.

**Accepted:**
- Some legitimate-feeling feature requests will sit unresolved for long periods because no one has written the ADR to accept them. This is the correct failure mode. Features that nobody cares enough about to specify properly are features that should not ship.
- The Tier 2 list is not exhaustive. Capabilities that are neither listed nor obviously Tier 1 fall into an implicit "not currently in Ken, requires an ADR if added" state. Contributors who are unsure should open a discussion before implementation.

## Alternatives considered

**A single flat list of "will never do" items.** Rejected because it conflates architectural invariants with current scope, and makes the project look either too rigid (if all items are treated as permanent) or too loose (if all items are treated as negotiable). The tier split preserves the strength of each category.

**No explicit list at all, trust only through general principles.** Rejected because principles without enumeration do not survive contact with feature requests. Every surveillance product in history claimed to respect privacy. The value of this ADR is in the specific commitments it names, not in the abstract values it asserts.

**A capability list** (what Ken *does* do) instead of a boundary list. Rejected because capability lists are exhaustive only at the moment they are written and silently expand. A boundary list, especially one with a tier distinction, grows by explicit amendment and makes scope changes visible.

**Deferring this ADR until after implementation begins.** Rejected because the value of this document is highest before the first line of code is written. Retrofitting trust boundaries onto an existing codebase is possible but always feels like a compromise. Writing them first makes them constitutive rather than aspirational.
