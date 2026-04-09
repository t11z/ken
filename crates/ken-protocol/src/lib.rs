//! Wire types for the Ken agent-server protocol.
//!
//! This crate defines every type that crosses the network boundary between
//! the Ken agent (Windows) and the Ken server (Linux). It is the single
//! source of truth for the protocol schema and is depended on by both
//! `ken-agent` and `ken-server`.
//!
//! All types are `Serialize + Deserialize` with round-trip stability
//! guaranteed by tests. See ADR-0001 for the trust boundaries that
//! constrain what data may appear on the wire.
