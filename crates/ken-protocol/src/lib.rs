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

pub mod audit;
pub mod command;
pub mod enrollment;
pub mod heartbeat;
pub mod ids;
pub mod status;
pub mod version;

// Re-export the most commonly used types at the crate root for convenience.
pub use ids::{CommandId, EndpointId, HeartbeatId, SessionId};
pub use version::SCHEMA_VERSION;
