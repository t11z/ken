//! Status window for the tray app.
//!
//! Shows the agent's current state: service running, enrolled,
//! endpoint ID, last heartbeat, pending commands, agent version.
//! Uses IPC to query the service via `GetStatus`.

#![cfg(all(windows, feature = "tray-app"))]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::ipc::client::IpcClient;
use crate::ipc::AgentStatus;

/// Render the status window inside an OS-level viewport (via `show_viewport_deferred`).
///
/// Reads from `cached_status`, which is populated every 2 s by a background
/// thread in `TrayApp` — no blocking IPC call is made on the render thread.
/// Sets `visible` to `false` when the OS close button is clicked. ADR-0009.
pub fn show_in_viewport(
    ctx: &egui::Context,
    visible: &Arc<AtomicBool>,
    cached_status: &Arc<Mutex<Option<Result<AgentStatus, String>>>>,
) {
    if ctx.input(|i| i.viewport().close_requested()) {
        visible.store(false, Ordering::SeqCst);
        return;
    }

    let status = cached_status.lock().unwrap();

    egui::CentralPanel::default().show(ctx, |ui| match &*status {
        None => {
            ui.spinner();
            ui.label("Connecting to service…");
        }
        Some(Ok(status)) => {
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
            ui.label("Status refreshes every 2 seconds while this window is open.");
        }
        Some(Err(ref e)) => {
            ui.label(format!("Service not reachable: {e}"));
            ui.add_space(10.0);
            ui.label("Retrying every 2 seconds.");
        }
    });
}

/// Fetch audit log entries from the service via IPC.
///
/// Returns the last `lines` audit log entries as JSON strings.
pub fn fetch_audit_log_via_ipc(lines: u32) -> Result<Vec<String>, anyhow::Error> {
    let mut client = IpcClient::connect()?;
    client.get_audit_log_tail(lines)
}
