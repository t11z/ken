//! Tray application for the Ken agent per ADR-0009.
//!
//! The tray app runs in the interactive user session (not as SYSTEM).
//! It provides:
//! - A system tray icon with a context menu
//! - A status window showing agent state
//! - The consent dialog for remote session requests (ADR-0001 T1-4)
//! - The kill switch activation UI (ADR-0001 T1-6)
//! - An audit log viewer entry point
//!
//! Built on `egui` with `eframe` as the windowing framework and
//! `tray-icon` for the system tray icon. Windows-only, gated behind
//! the `tray-app` cargo feature.

#[cfg(all(windows, feature = "tray-app"))]
pub mod app;
#[cfg(all(windows, feature = "tray-app"))]
pub mod consent_dialog;
#[cfg(all(windows, feature = "tray-app"))]
pub mod enroll_dialog;
#[cfg(all(windows, feature = "tray-app"))]
pub mod status_window;

/// Run the tray app. Called from the `ken-agent.exe tray` subcommand.
///
/// On Windows with the `tray-app` feature: initializes the system tray
/// icon and the egui event loop.
/// Otherwise: prints a message and returns.
pub fn run() {
    #[cfg(all(windows, feature = "tray-app"))]
    {
        app::run_tray_app();
    }
    #[cfg(not(all(windows, feature = "tray-app")))]
    {
        eprintln!("The tray app requires the 'tray-app' feature on Windows.");
        eprintln!("Build with: cargo build --features tray-app");
    }
}
