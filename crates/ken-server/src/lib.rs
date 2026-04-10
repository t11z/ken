//! Ken server library.
//!
//! This module re-exports the server's internals so that integration
//! tests can exercise the real router, acceptor, and storage without
//! hand-rolling duplicates. The binary entry point in `main.rs`
//! imports from this library.

pub mod ca;
pub mod config;
pub mod error;
pub mod http;
pub mod state;
pub mod storage;
