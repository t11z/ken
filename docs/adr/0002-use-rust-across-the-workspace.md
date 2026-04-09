# ADR-0002: Use Rust across the entire workspace

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

Ken consists of three components that must work together: a Windows endpoint agent that runs as a SYSTEM service and reads OS state, a Linux server that runs on a Raspberry Pi and presents the admin UI, and a wire-protocol crate shared between the two. Each of these has different platform constraints and different operational profiles. The choice of implementation language for each one is the foundational technical decision the project rests on, and choosing differently across components would multiply the maintenance surface, the dependency story, and the build pipeline complexity.

The realistic candidates were Rust everywhere, Go for the server with C# or Rust for the agent, or a mixed-language setup where each component picks its idiomatic ecosystem. The decision is forced now because every other technical decision (HTTP framework, TLS library, observability libraries, build tooling) inherits from it.

## Decision

All three crates in the Ken workspace are written in Rust. There are no exceptions, no second language anywhere in the source tree, and no FFI shims to other-language components. The agent uses `windows-rs` and `windows-service` to interact with Windows. The server uses `axum`, `sqlx`, `rustls`, and `askama` to serve the admin UI and the agent API. The shared `ken-protocol` crate uses `serde` for wire types and depends on neither the agent nor the server.

This is a Cargo workspace with three members, a single `rust-toolchain.toml` at the root, shared lints and dependency versions in `[workspace.dependencies]`, and a single CI matrix that builds all three crates on the appropriate target platforms.

## Consequences

**Easier:**
- One language to learn, one toolchain to install, one set of conventions across the codebase. Anyone who can read one crate can read the others.
- Memory safety as a structural property of the entire system, including the privileged Windows service. For a tool whose value proposition depends on trustworthiness, this is not a small benefit — a use-after-free in the agent would be a security incident in the literal sense.
- A single dependency graph. Workspace-wide `cargo audit`, workspace-wide `cargo deny`, workspace-wide license review.
- Sharing types across the wire boundary is trivial: `ken-protocol` is a normal Cargo dependency, not a code-generation step or an IDL compiler.
- The build pipeline is one `cargo build --workspace`. CI is one matrix. Release artifacts are one set.

**Harder:**
- Rust on Windows still has rougher edges than Rust on Linux. Some `windows-rs` APIs are verbose and require more boilerplate than the equivalent C# or PowerShell would. The agent will pay this cost in code volume.
- Cross-compilation (Linux ARM64 server from x86_64 dev machines) requires `cross` or a Docker-based build, both of which add CI complexity compared to a native-language alternative on each platform.
- Rust compile times are longer than Go compile times, especially for the agent's dependency closure (windows-rs is heavy at compile time). CI runs on Windows runners are noticeably slower than on Linux runners.

**Accepted:**
- The agent's binary is larger than the equivalent in C# would be, because Rust statically links most of its dependencies. For a service binary distributed via MSI, this is a fair trade — no .NET runtime to install, no version conflicts with whatever .NET is already on the user's machine.
- We forgo the Go ecosystem's strengths for server work (fast builds, generous standard library, ubiquitous Linux deployment) in exchange for the consistency benefit. The server's needs are modest enough that Rust's heavier ergonomics are not a real obstacle.

## Alternatives considered

**Go for the server, Rust for the agent.** Rejected because the wire-protocol crate would have to be maintained twice — once in Rust (for the agent), once in Go (for the server) — or generated from a third-party IDL. Both options reintroduce exactly the kind of cross-language friction the workspace structure is meant to eliminate. The marginal benefit of Go's faster builds and standard library does not offset the cost of maintaining two protocol implementations in lockstep.

**C# for the agent, Rust for the server.** Rejected because C# on Windows brings the .NET runtime as a dependency, adds installer complexity for the MSI, and creates a culture gap inside a small project — half of the codebase would feel idiomatically alien to anyone working on the other half. The agent's interaction with Windows APIs is well-served by `windows-rs`, which is Microsoft's own Rust binding layer and is actively maintained.

**A single-language mixed-platform language like Kotlin/JVM or .NET-everywhere.** Rejected because Ken is a self-hosted tool aimed at family IT chiefs running Raspberry Pis. Bringing a JVM or a .NET runtime onto a Pi is technically possible but operationally heavier than a static Rust binary, and the value proposition of "small, fast, observable" suffers for it.
