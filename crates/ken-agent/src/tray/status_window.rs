//! Status window for the tray app.
//!
//! Shows the agent's current state: service running, enrolled,
//! endpoint ID, last heartbeat, pending commands, agent version.
//! Polls for updates every 3 seconds while the window is open.

#![cfg(windows)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui;

use crate::ipc::AgentStatus;

/// Show the status window.
///
/// In Phase 1 (IPC not yet implemented), shows a placeholder status
/// constructed from local state. When IPC is wired up (after #3 is
/// resolved), this will query the service via `IpcRequest::GetStatus`.
pub fn show(ctx: &egui::Context, visible: &Arc<AtomicBool>) {
    let mut open = true;

    egui::Window::new("Ken Agent Status")
        .open(&mut open)
        .resizable(false)
        .show(ctx, |ui| {
            // Phase 1: construct status from local file state since
            // IPC pipe is not yet implemented (blocked on #3).
            let status = local_status();

            egui::Grid::new("status_grid").striped(true).show(ui, |ui| {
                ui.label("Service running:");
                ui.label(if status.service_running { "Yes" } else { "No" });
                ui.end_row();

                ui.label("Enrolled:");
                ui.label(if status.enrolled { "Yes" } else { "No" });
                ui.end_row();

                ui.label("Endpoint ID:");
                ui.label(status.endpoint_id.as_deref().unwrap_or("-"));
                ui.end_row();

                ui.label("Last heartbeat:");
                ui.label(
                    status
                        .last_heartbeat
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "never".to_string()),
                );
                ui.end_row();

                ui.label("Pending commands:");
                ui.label(status.pending_commands.to_string());
                ui.end_row();

                ui.label("Agent version:");
                ui.label(&status.agent_version);
                ui.end_row();
            });

            ui.add_space(10.0);
            ui.label("Status refreshes every 3 seconds while this window is open.");
        });

    if !open {
        visible.store(false, Ordering::SeqCst);
    }
}

/// Construct a status from local file state (Phase 1 fallback).
fn local_status() -> AgentStatus {
    let data_dir = crate::config::data_dir();
    let paths = crate::config::DataPaths::new(&data_dir);

    let endpoint_id = std::fs::read_to_string(&paths.endpoint_id_file)
        .ok()
        .map(|s| s.trim().to_string());

    let enrolled = endpoint_id.is_some();

    AgentStatus {
        service_running: true, // Assume running if tray app is alive
        enrolled,
        endpoint_id,
        last_heartbeat: None,
        pending_commands: 0,
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}
