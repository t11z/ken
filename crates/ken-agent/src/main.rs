//! Ken agent — Windows endpoint observability and consent-gated remote access.
//!
//! The agent runs as a Windows service under `LocalSystem` and reports
//! passive OS state (Defender, firewall, `BitLocker`, Windows Update,
//! security events) to the Ken server. A user-mode Tray App provides
//! visibility and the consent gate for remote sessions.

fn main() {
    println!("ken-agent: not yet implemented");
}
