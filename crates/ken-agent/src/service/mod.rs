//! Windows service implementation for the Ken agent.
//!
//! The service runs under `LocalSystem` and is responsible for collecting
//! OS status, sending heartbeats, processing commands, and managing
//! the Named Pipe IPC with the Tray App.
//!
//! On non-Windows platforms, this module provides stubs that allow the
//! crate to compile and tests to run.

pub mod lifecycle;
