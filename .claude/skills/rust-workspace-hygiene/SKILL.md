# Rust Workspace Hygiene

Load this skill when working on any crate in the Ken workspace. It covers the conventions for dependencies, features, lints, formatting, and testing that apply across all crates. Crate-specific conventions live in the respective `CLAUDE.md` and override what is here.

## Workspace structure

Ken is a Cargo workspace with three member crates: `ken-protocol`, `ken-agent`, `ken-server`. The workspace root owns:

- Shared dependency versions via `[workspace.dependencies]`
- Shared lint configuration via `[workspace.lints]`
- The Rust toolchain version via `rust-toolchain.toml`

Individual crates reference shared dependencies by name:

```toml
[dependencies]
serde = { workspace = true, features = ["derive"] }
```

This keeps versions in sync across the workspace. Do not pin a different version in an individual crate without a concrete reason documented in the pull request.

## Dependency discipline

New dependencies are not free. Every new crate brought in has a maintenance cost, a security-audit cost, a compile-time cost, and a binary-size cost. Before adding one, check whether:

- The standard library, an existing workspace dependency, or a small hand-written helper would work
- The crate is actively maintained (recent commits, recent releases, responsive maintainers)
- The crate has a reasonable dependency tree of its own (running `cargo tree` on a new dep should not reveal a forest)
- The crate is permissively licensed (MIT, Apache-2.0, BSD) — AGPL or GPL dependencies require explicit architect review because they interact with Ken's own AGPL-3.0 licensing in ways that need thought

When you add a dependency, add it to the workspace root `[workspace.dependencies]` with a version specifier, then reference it from the crate that needs it. This applies even if only one crate uses the dependency today; the workspace root is the canonical version registry.

Document any non-obvious dependency addition in the pull request description: why it was chosen, what alternatives were considered, and what it brings in transitively.

## Feature flags

Feature flags on dependencies are kept to the minimum needed. `serde = { workspace = true, features = ["derive"] }` is fine. `tokio = { workspace = true, features = ["full"] }` is lazy — enable only the features you use.

Feature flags on Ken's own crates are used sparingly. The default is that a crate has no feature flags and all code is always compiled. Introducing a feature flag is an architectural decision because it multiplies the set of build configurations the CI must test.

## Lints

The workspace root enables the following lints at the `warn` level:

- `clippy::all`
- `clippy::pedantic` (with a small set of explicit `allow` exceptions for lints that generate more noise than value)
- `rust_2018_idioms`
- `unused_imports`
- `unused_variables`

Any `#[allow(...)]` in source code must be accompanied by a comment explaining why. "Clippy is wrong here because X" is a valid comment. An `#[allow]` without a comment is a code smell and will be caught in review.

## Formatting

`cargo fmt` with default settings. No `rustfmt.toml` overrides unless there is a workspace-wide reason, documented in an ADR. Run `cargo fmt --check` in CI.

## Testing

Unit tests live in `src/` alongside the code, in `#[cfg(test)]` modules. Integration tests live in `crates/<crate>/tests/`.

Every test must:

- Be deterministic. No random values that are not seeded. No dependencies on wall-clock time unless the test explicitly pauses time with `tokio::time::pause`.
- Be hermetic. No dependencies on network, on specific file paths outside the test's own tempdir, on environment variables that might vary between CI and local runs.
- Be fast by default. Slow integration tests go behind a `#[ignore]` attribute with a comment explaining how to run them (e.g., `cargo test -- --ignored`).
- Assert one thing. A test named `test_everything_works` that has fifteen assertions is five tests trying to share a body.

Name tests after what they verify, not after what they do. `test_enrollment_rejects_unknown_agent` is good. `test_post_enrollment_endpoint` is not.

## Error handling

Use `thiserror` for library error types and `anyhow` for binary error contexts. Do not use `anyhow` in library code — it erases type information that callers need. Do not use raw `Box<dyn Error>` unless there is a specific reason not to use `thiserror`.

Every error variant in a `thiserror` enum has a descriptive message. Error messages are written for the operator reading the logs, not for the developer who wrote the code.

Do not use `.unwrap()` in production code. `.unwrap()` is acceptable in tests, in `const` initialization where the panic is proven to be unreachable, and in `main.rs` during startup before the tracing subscriber is initialized. Every other use should be `.expect("reason")` at minimum, and ideally propagated as an error.

## Async

`tokio` is the only async runtime in the workspace. Do not mix `async-std` or `smol`.

Use `tokio::spawn` for background tasks that are tied to the application lifecycle, not for parallelism of short-lived work (for that, use `tokio::join!` or `futures::future::try_join_all`). Every spawned task should have an associated cancellation mechanism — either a `CancellationToken` from `tokio-util` or a signal through a channel.

Never block the async runtime with a long CPU-bound task. If you have one, move it to `tokio::task::spawn_blocking`.

## Imports and module layout

Imports are grouped in three sections, separated by blank lines:

1. `std` imports
2. External crate imports
3. Crate-internal imports (`crate::`, `super::`, `self::`)

Within each group, imports are sorted. `rustfmt` does most of this automatically; verify with `cargo fmt`.

Modules are declared in `lib.rs` or `main.rs` near the top, grouped logically. Avoid deep module nesting — most Ken crates are flat enough that two levels of module depth is the maximum.

## Documentation

Every public item in every crate has a doc comment. The doc comment explains what the item does, not how it does it. If the item exists because of an ADR, the doc comment mentions the ADR by number.

Example:

```rust
/// Identifier for an enrolled endpoint.
///
/// Endpoints are enrolled manually by the family IT chief per
/// ADR-0001 T2-7; this type is the handle to a single enrolled agent
/// throughout the server.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EndpointId(String);
```

Run `cargo doc --no-deps` periodically to spot-check that the generated documentation is coherent.

## Build and verification

Before opening a pull request, verify:

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

CI runs all four. Passing them locally saves a round trip.
