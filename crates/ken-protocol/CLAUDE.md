# CLAUDE.md — `ken-protocol`

This crate defines the wire types shared between `ken-agent` and `ken-server`. It is the contract at the boundary, and like all contracts its value comes from being stable and explicit.

Read the root `CLAUDE.md` first if you have not already. The rules there apply here unless this file refines them.

## Purpose

`ken-protocol` is a small, dependency-light crate that contains:

- Message types exchanged over the agent ↔ server connection (heartbeat, status report, command, command acknowledgment, consent response)
- Enums for endpoint state (Defender status, BitLocker state, update state, firewall profile state, etc.)
- Versioning primitives so that agent and server can negotiate protocol compatibility
- Serialization and deserialization via `serde`, with the wire format documented in this file

It does **not** contain:

- Network code (no HTTP, no TLS, no socket logic)
- Business logic (no decisions about *what* to do with a message, only *what a message looks like*)
- Platform-specific code (no Windows APIs, no Linux APIs)
- Persistence logic (no database types, no SQLite-specific concerns)

If you find yourself wanting to add any of those things to this crate, the answer is no. Add them in `ken-agent` or `ken-server` and import the relevant `ken-protocol` types.

## Why this crate exists at all

Two reasons.

First, the agent and the server are written in the same language and live in the same workspace, but they are deployed independently and will run different versions in production. Without a shared crate, the wire types would be duplicated, drift apart, and create silent compatibility breaks. With a shared crate, breaking the wire format produces a compile error in both binaries, which is the correct failure mode.

Second, the wire format is the most important architectural surface in Ken. A future contributor who wants to understand "what does Ken actually exchange between agent and server" can read this single crate and have a complete, authoritative answer. No other crate has that property.

## Conventions

### Type naming

- Messages from the agent to the server: `AgentMessage` enum with variants like `Heartbeat`, `StatusReport`, `ConsentResponse`.
- Messages from the server to the agent: `ServerMessage` enum with variants like `RequestRemoteSession`, `RequestStatusRefresh`.
- State enums use the form `<Subject>State`, e.g., `DefenderState`, `BitLockerState`, `UpdateState`. All variants are explicit; no `Other(String)` escape hatches without an ADR justifying them.
- All public types derive `Debug`, `Clone`, `serde::Serialize`, `serde::Deserialize`, `PartialEq`, and `Eq` where the contained types allow it.

### Serialization

The wire format is **JSON over mTLS** for now. This is documented in an ADR (placeholder ADR-0004, to be written). If a future ADR moves the format to something binary (CBOR, Protobuf, MessagePack), the change happens here, in this crate, and is invisible to the rest of the codebase.

`serde` is the only serialization framework used. No hand-rolled `Serialize` implementations without an ADR.

### Versioning

Every top-level message includes a `protocol_version` field of type `ProtocolVersion`, a small struct with `major` and `minor` fields. The compatibility rule is:

- Same `major`: agent and server must interoperate.
- Different `major`: agent and server must refuse to interoperate, log a clear error, and surface the version mismatch in the audit log and the server UI.

`ProtocolVersion` is defined as a constant in this crate. Bumping it is an architectural decision and requires an ADR.

### No optional fields without a default

Every field in every message either is required or has an explicit `#[serde(default)]` with a documented default value. The reason: agents and servers will run mixed versions, and a missing field on one side must produce a predictable value on the other side, never a deserialization error. If a field is genuinely optional, it is `Option<T>` and the absence of value has documented semantic meaning.

## Dependencies

Permitted in this crate:

- `serde` and `serde_derive`
- `serde_json` (only if used in this crate's own tests; production serialization happens at the call site)
- `thiserror` for error types
- `time` or `chrono` for timestamps (TBD by ADR; pick one and stick with it)

Forbidden in this crate without an ADR:

- Any crate that pulls in a runtime (`tokio`, `async-std`, `smol`)
- Any crate with platform-specific code (`windows-rs`, `nix`, etc.)
- Any HTTP, TLS, or network crate
- Any database crate

The point is that `ken-protocol` should compile in milliseconds, have minimal dependencies, and be trivially auditable. If a dependency makes either of those properties worse, it does not belong here.

## Tests

- Every message type has a round-trip serialization test: `serialize → deserialize → assert_eq` against a representative instance.
- Every message type has a "wire format snapshot" test that asserts the exact JSON output for a known input. These snapshots live in `tests/snapshots/` and are committed. When a snapshot changes, the change is visible in the diff and must be justified in the PR description.
- Cross-version compatibility tests: when the protocol version is bumped, the old version's snapshots must continue to deserialize correctly until the next major bump.

## What an LLM should do here

When asked to add a new message type:

1. Identify which enum it belongs in (`AgentMessage`, `ServerMessage`, or a new enum if neither fits — but adding a new enum requires an ADR).
2. Add the variant with full documentation, including which ADR motivates its existence.
3. Write the round-trip test and the snapshot test.
4. Update the relevant `CLAUDE.md` in the agent and server crates if the new message changes how they should behave.
5. Open a PR that names the prompt file and the relevant ADR.

When asked to change an existing message type:

1. **Stop.** Changing an existing wire type is a compatibility-breaking change. Unless the change is purely additive (new optional field with a documented default), this requires an ADR and a protocol version bump.
2. If the change is additive, proceed with the same steps as above.
3. If it is not additive, refuse and ask the architect to draft an ADR.
