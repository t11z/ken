# CLAUDE.md — ken-protocol

This crate defines the wire types exchanged between the Ken agent and the Ken server. It is the narrowest and most stable crate in the workspace. Read the root `CLAUDE.md` first; conventions there apply here unchanged unless refined below. Read `docs/adr/0001-trust-boundaries-and-current-scope.md` before deciding what belongs on the wire.

## Purpose

`ken-protocol` is the single source of truth for the shape of data flowing across the network boundary in Ken. Every message the agent sends to the server, every command the server sends back, every enrollment exchange, every audit event that travels over the wire — all of it is defined here, once.

The crate is deliberately small. It contains type definitions, serialization logic, schema version constants, and the minimum set of helpers required to produce and consume those types. It does not contain transport code, it does not contain I/O, it does not know about TLS, it does not know about HTTP. It is pure data.

## Why this matters

The agent and the server are separate binaries with separate release cycles, deployed on different hardware, owned by different concerns. The only thing keeping them compatible is the discipline of this crate. If a field is renamed without thought, agents in the field stop reporting. If a new variant is added to an enum without a compatibility strategy, every server older than the release stops parsing. The crate is small precisely because every line of it carries operational weight.

## Conventions

**Everything public has a doc comment** that explains what the type represents, what it is used for, and which ADR justifies its existence. The doc comment is part of the contract, not documentation afterthought.

**Every type that crosses the wire is `Serialize + Deserialize` via `serde`**, and every type is tested for round-trip equality. A type that serializes differently than it deserializes is a silent corruption bug waiting to happen.

**Schema versioning is explicit.** The crate exposes a `SCHEMA_VERSION` constant. When any wire type changes in a way that is not backward-compatible, the version is bumped and the change is recorded in an ADR. Backward-compatible changes (adding an optional field, adding a new enum variant with `#[serde(other)]` fallback) do not require a version bump but do require a test that proves old payloads still parse.

**Enums use `#[serde(rename_all = "snake_case")]`** by default. Variants are named in the Rust idiom (`CamelCase`) but serialized as `snake_case` for consistency with the rest of the wire format. Any deviation from this default is documented at the type definition.

**Timestamps are `OffsetDateTime` from the `time` crate**, serialized as RFC 3339. Never use `std::time::SystemTime` for wire types — it is opaque on the wire and its serialization is platform-dependent.

**Identifiers are typed.** An `EndpointId` is not a `String`, it is a newtype wrapping a `String` (or `Uuid`, depending on the type). This prevents the class of bugs where an endpoint ID is passed where a session ID was expected. The cost is a few lines of boilerplate per identifier type; the benefit is that the compiler catches mix-ups before they reach the wire.

**No `Option<Vec<T>>`** — an empty vector already encodes absence. No `Option<String>` where an empty string would carry meaning — use a dedicated type or a wrapper. Types in this crate are small, and the discipline of "make illegal states unrepresentable" is worth the extra effort here.

## What belongs on the wire

Per ADR-0001 T1-1 and T1-2, Ken does not talk to any server outside the deployment. This means the wire types defined here travel exclusively between one agent and one server within one family IT chief's deployment. There is no central schema registry, no public compatibility contract, no external consumer to worry about. The audience is one pair of binaries owned by the same architect.

This gives us freedom (we can iterate on the schema without breaking third parties) and responsibility (there is no one else to catch our mistakes). Tests and schema discipline replace what an external contract would otherwise enforce.

Data that must never travel on the wire, per ADR-0001:

- Keystrokes or input captures (T2-3 is a current boundary but the prohibition is clear today)
- File contents, clipboard contents, browser history (T2-2)
- Scheduled screenshots (T2-4)
- Any user data beyond what is strictly necessary for status reporting

If a task asks you to add a type that would carry any of these, stop and surface the question. A current Tier 2 boundary can be loosened through an ADR, but never silently through a new type in this crate.

## Dependencies

This crate has **no dependencies on `ken-agent` or `ken-server`**. The dependency direction is strictly one-way: both of those depend on this crate, and this crate depends on neither. If a task asks you to reach back from `ken-protocol` into `ken-server` or `ken-agent`, stop and surface the question to the architect — the request likely indicates a missing abstraction.

External dependencies are kept minimal. Allowed by default:

- `serde` and `serde_json` for serialization
- `time` for timestamps
- `uuid` for identifier generation when the protocol requires it

Anything beyond this set requires justification in the pull request description. Adding a dependency here has a large blast radius because both binaries inherit it.

## Testing

Every wire type has at least one round-trip test in `crates/ken-protocol/tests/`. The round-trip test:

1. Constructs an instance with realistic values
2. Serializes it to JSON
3. Deserializes it back
4. Asserts equality with the original

When a type is extended, the old payload format is added as a test case that must still parse into the new type. This is the compatibility gate: if old payloads stop parsing, the change is not backward-compatible and requires a schema version bump and an ADR.

Property-based testing with `proptest` is encouraged for types with non-trivial invariants, but not required.

## What this crate does not contain

- Network code (HTTP clients, TCP sockets, TLS setup) — lives in `ken-agent` and `ken-server`
- Database code (SQL, migrations, connection pools) — lives in `ken-server`
- Business logic (consent decisions, alerting rules, session lifecycle) — lives in `ken-agent` and `ken-server`
- Windows-specific code — lives in `ken-agent`
- Server configuration loading — lives in `ken-server`

If a task asks you to add any of the above to this crate, stop and verify with the architect. The crate is kept small on purpose.
