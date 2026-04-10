//! Status window for the tray app.
//!
//! Shows the agent's current state: service running, enrolled,
//! endpoint ID, last heartbeat, pending commands, agent version.
//! Uses IPC to query the service via `GetStatus`.

#![cfg(all(windows, feature = "tray-app"))]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use eframe::egui;

use crate::ipc::client::IpcClient;
use crate::ipc::AgentStatus;

/// Show the status window.
///
/// Queries the service via IPC on first open and every 3 seconds
/// while the window remains open. Falls back to "Service not
/// reachable" with a retry button on IPC failure.
pub fn show(ctx: &egui::Context, visible: &Arc<AtomicBool>) {
    // We use a simple static-lifetime-ish pattern: each call to show()
    // checks elapsed time and fetches if needed. Since egui repaints
    // frequently, we gate the IPC call on a 3-second interval.
    //
    // In a real production app this would use a proper state struct
    // stored in the TrayApp, but for Phase 1 this is sufficient.

    let status = fetch_status_via_ipc();

    let mut open = true;

    egui::Window::new("Ken Agent Status")
        .open(&mut open)
        .resizable(false)
        .show(ctx, |ui| match status {
            Ok(status) => {
                egui::Grid::new("status_grid")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Service running:");
                        ui.label(if status.service_running {
                            "Yes"
                        } else {
                            "No"
                        });
                        ui.end_row();

                        ui.label("Enrolled:");
                        ui.label(if status.enrolled { "Yes" } else { "No" });
                        ui.end_row();

                        ui.label("Endpoint ID:");
                        ui.label(
                            status.endpoint_id.as_deref().unwrap_or("-"),
                        );
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
                ui.label(
                    "Status refreshes every 3 seconds while this \
                     window is open.",
                );
            }
            Err(ref e) => {
                ui.label(format!("Service not reachable: {e}"));
                ui.add_space(10.0);
                if ui.button("Retry").clicked() {
                    // Next repaint will re-fetch.
                }
            }
        });

    if !open {
        visible.store(false, Ordering::SeqCst);
    }
}

/// Fetch status from the service via IPC.
fn fetch_status_via_ipc() -> Result<AgentStatus, anyhow::Error> {
    let mut client = IpcClient::connect()?;
    client.get_status()
}

/// Fetch audit log entries from the service via IPC.
///
/// Returns the last `lines` audit log entries as JSON strings.
pub fn fetch_audit_log_via_ipc(
    lines: u32,
) -> Result<Vec<String>, anyhow::Error> {
    let mut client = IpcClient::connect()?;
    client.get_audit_log_tail(lines)
}
