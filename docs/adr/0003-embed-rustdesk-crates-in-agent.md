# ADR-0003: Embed RustDesk crates in the agent

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

Ken's consent-gated remote-control feature requires a working remote-desktop subsystem: screen capture, video codec, signaling, relay, input injection, session lifecycle. Building any one of these from scratch would be a multi-month engineering effort, and building all of them would put the remote-control feature out of reach for a small project. The realistic question is not whether to use existing components but which existing components and in what relationship.

RustDesk is the obvious candidate. It is mature, actively maintained, written in Rust, AGPL-licensed (compatible with Ken's own AGPL-3.0), and its protocol crates (`hbb_common`, `scrap`, `enigo`, the codec wrappers) are designed to be reusable. The remaining question is the *shape* of the integration: does Ken embed the crates as Cargo dependencies and link them into the agent binary, or does Ken control an external RustDesk client process via IPC?

This decision is a current `ADR-00XX` placeholder in `crates/ken-agent/CLAUDE.md` and needs to be resolved before the remote-session subsystem is built (Phase 2). It is recorded now so the agent's architecture has a stable place for the backend to slot in, and so the wire protocol is stable.

## Decision

The Ken agent embeds the RustDesk protocol crates as direct Cargo dependencies and links them into the agent binary. There is no external RustDesk client process, no IPC bridge to a separate executable, no configuration that "points Ken at an existing RustDesk install". The remote-session subsystem is part of `ken-agent.exe`.

The integration point is a `RemoteSessionBackend` trait defined in `crates/ken-agent/src/remote_session/`. Phase 1 ships a `NoOpBackend` that refuses all session requests with `RemoteSessionError::NotImplemented`. Phase 2 introduces a `RustDeskBackend` that implements the same trait using the embedded crates. The trait boundary exists so that the consent flow, the wire protocol, and the command processing are all testable end-to-end before the real backend lands, and so the Phase 2 work is additive rather than restructuring.

The crates Ken depends on are taken from a pinned RustDesk commit, vendored or referenced by exact version. Ken does not track RustDesk's `main` branch — every upgrade is a deliberate act with a corresponding pull request and release note.

## Consequences

**Easier:**
- Single binary, single installer, single update path. The user installs Ken and the remote-control capability comes with it. No "also install RustDesk" step.
- The consent gate is enforced at the language level: every code path that reaches screen capture or input injection passes through `RemoteSessionBackend::start_session`, which is called only after the consent flow returns granted. There is no way to invoke the capture subsystem from outside the agent's own process boundary.
- Audit log integration is direct: the embedded subsystem can write to Ken's local audit log via the same logger the rest of the agent uses, without IPC marshaling.
- The trust story is honest. Users can audit one binary and one source tree. There is no "and also this other process Ken talks to" footnote.

**Harder:**
- Ken's binary is significantly larger than it would be without the embedded crates. The codec libraries alone add several megabytes. For a service binary on a modern Windows machine this is acceptable, but it is a real cost.
- Ken inherits RustDesk's compile-time complexity. The codec crates have C dependencies that need to be available at build time on both the development machine and the CI runner. The Windows build pipeline becomes more involved.
- Upgrading RustDesk crates is now Ken's responsibility. If RustDesk fixes a security issue, Ken's release process must roll the dependency forward and ship a new version. There is no "Ken stays the same and RustDesk updates underneath" path.
- The AGPL-3.0 license of RustDesk crates is compatible with Ken's own AGPL-3.0, but it does mean Ken cannot relicense to a more permissive license in the future without removing the embedded crates first. This is not a real constraint because ADR-0001 T1-7 already forbids permissive relicensing, but it is worth naming.

**Accepted:**
- We are tying Ken's release cadence to RustDesk's. When RustDesk publishes a security-relevant update, Ken must respond. This is a maintenance burden a small project must take seriously.
- Some RustDesk features that Ken does not need (user interface chrome, account management, device discovery) are not used by Ken, but the code paths exist in the dependency graph and are part of the audit surface. We accept this in exchange for not maintaining a fork.

## Alternatives considered

**Control an external RustDesk client process via IPC.** Rejected because it multiplies the trust surface (now there are two binaries, two update mechanisms, two places where the consent gate could be bypassed) and breaks the single-binary distribution story. It also requires the user to install and update RustDesk separately, which defeats the "small, focused tool" positioning.

**Maintain a hard fork of RustDesk inside the Ken repository.** Rejected because forking immediately doubles the maintenance work and forfeits the ability to absorb upstream security fixes without manual merging. A fork makes sense only when the upstream is unmaintained or when the project's needs diverge structurally from the upstream's. Neither is the case here.

**Build the remote-control subsystem from scratch.** Rejected because the engineering cost (screen capture across DXGI generations, video codec integration, NAT traversal, relay protocol, input injection, session lifecycle management) is multi-month at minimum and the result would almost certainly be inferior to RustDesk in every measurable dimension. The project does not have the headcount or the rationale for this work.

**Use a different remote-control library, such as noVNC or a commercial SDK.** Rejected because no other actively-maintained Rust remote-desktop library exists at RustDesk's level of maturity, and bringing in a non-Rust component (noVNC's Python or JavaScript stack, for example) would violate ADR-0002.
