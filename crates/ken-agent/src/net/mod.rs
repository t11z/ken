//! HTTP client for communicating with the Ken server over mTLS.
//!
//! The client is built from enrolled credentials (CA cert, client cert,
//! client key) and the server URL. It uses `reqwest` with rustls as
//! the TLS backend.

pub mod client;
