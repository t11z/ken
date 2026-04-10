//! Main tray application entry point.
//!
//! Sets up the system tray icon, context menu, and dispatches menu
//! clicks to the appropriate windows or actions. A background thread
//! polls the SYSTEM service via Named Pipe IPC for pending consent
//! requests and forwards them to the UI.
//!
//! This module compiles only on Windows per ADR-0009.

#![cfg(all(windows, feature = "tray-app"))]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::TrayIconBuilder;

use crate::ipc::client::IpcClient;
use crate::ipc::PendingConsentInfo;

/// Messages from the IPC polling thread to the UI.
enum IpcMessage {
    /// A consent request arrived from the service.
    ConsentRequest(PendingConsentInfo),
}

/// Run the tray app event loop.
///
/// Creates the system tray icon with menu items, starts a background
/// IPC polling thread for consent requests, and runs the egui event
/// loop. The main window is hidden by default (tray-only mode).
pub fn run_tray_app() {
    tracing::info!("starting tray app");

    let menu = Menu::new();
    let item_status = MenuItem::new("Status", true, None);
    let item_audit = MenuItem::new("View audit log", true, None);
    let item_kill = MenuItem::new("Kill switch", true, None);
    let item_quit = MenuItem::new("Quit", true, None);

    let _ = menu.append(&item_status);
    let _ = menu.append(&item_audit);
    let _ = menu.append(&item_kill);
    let _ = menu.append(&item_quit);

    // Placeholder 16x16 RGBA icon.
    let icon = tray_icon::Icon::from_rgba(vec![0x33, 0x66, 0x99, 0xFF; 16 * 16], 16, 16)
        .expect("valid icon");

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Ken Agent")
        .with_icon(icon)
        .build()
        .expect("tray icon");

    let show_status = Arc::new(AtomicBool::new(false));
    let show_kill_confirm = Arc::new(AtomicBool::new(false));

    // Channel for IPC polling thread → UI
    let (ipc_tx, ipc_rx) = mpsc::channel::<IpcMessage>();

    // Menu event handler thread
    let show_status_clone = show_status.clone();
    let show_kill_clone = show_kill_confirm.clone();
    let status_id = item_status.id().clone();
    let audit_id = item_audit.id().clone();
    let kill_id = item_kill.id().clone();
    let quit_id = item_quit.id().clone();

    std::thread::spawn(move || {
        let receiver = MenuEvent::receiver();
        while let Ok(event) = receiver.recv() {
            if event.id == status_id {
                show_status_clone.store(true, Ordering::SeqCst);
            } else if event.id == audit_id {
                let data_dir = crate::config::data_dir();
                let paths = crate::config::DataPaths::new(&data_dir);
                let _ = std::process::Command::new("notepad.exe")
                    .arg(&paths.audit_log)
                    .spawn();
            } else if event.id == kill_id {
                show_kill_clone.store(true, Ordering::SeqCst);
            } else if event.id == quit_id {
                std::process::exit(0);
            }
        }
    });

    // IPC consent polling thread — polls every 500ms when no dialog is showing.
    let consent_dialog_active = Arc::new(AtomicBool::new(false));
    let consent_dialog_active_clone = consent_dialog_active.clone();

    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));

            // Skip polling while a consent dialog is already showing.
            if consent_dialog_active_clone.load(Ordering::SeqCst) {
                continue;
            }

            let pending = match IpcClient::connect() {
                Ok(mut client) => client.get_pending_consent(),
                Err(_) => continue, // service not reachable, try again later
            };

            if let Ok(Some(info)) = pending {
                let _ = ipc_tx.send(IpcMessage::ConsentRequest(info));
            }
        }
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_visible(false),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "Ken Agent",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(TrayApp {
                show_status,
                show_kill_confirm,
                kill_state: KillSwitchState::Idle,
                consent_dialog: None,
                consent_dialog_active,
                ipc_rx: Mutex::new(ipc_rx),
            }) as Box<dyn eframe::App>)
        }),
    );
}

/// State machine for the kill-switch confirmation flow.
enum KillSwitchState {
    /// No kill switch action in progress.
    Idle,
    /// Kill switch was activated successfully.
    Confirmed,
    /// Kill switch activation failed via IPC.
    Failed(String),
}

struct TrayApp {
    show_status: Arc<AtomicBool>,
    show_kill_confirm: Arc<AtomicBool>,
    kill_state: KillSwitchState,
    consent_dialog: Option<super::consent_dialog::ConsentDialog>,
    consent_dialog_active: Arc<AtomicBool>,
    ipc_rx: Mutex<mpsc::Receiver<IpcMessage>>,
}

impl eframe::App for TrayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for incoming IPC messages (non-blocking).
        if let Ok(rx) = self.ipc_rx.lock() {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    IpcMessage::ConsentRequest(info) => {
                        if self.consent_dialog.is_none() {
                            self.consent_dialog_active.store(true, Ordering::SeqCst);
                            self.consent_dialog = Some(super::consent_dialog::ConsentDialog::new(
                                info.admin_name,
                                info.session_description,
                                info.command_id,
                            ));
                        }
                    }
                }
            }
        }

        // Show consent dialog if active.
        if let Some(ref mut dialog) = self.consent_dialog {
            if let Some(outcome) = dialog.show(ctx) {
                let granted = matches!(outcome, crate::ipc::ConsentOutcome::Granted);
                let command_id = dialog.command_id;

                // Submit the consent response via IPC.
                if let Ok(mut client) = IpcClient::connect() {
                    let _ = client.submit_consent_response(&command_id, granted);
                }

                self.consent_dialog = None;
                self.consent_dialog_active.store(false, Ordering::SeqCst);
            }
        }

        // Status window.
        if self.show_status.load(Ordering::SeqCst) {
            super::status_window::show(ctx, &self.show_status);
        }

        // Kill switch confirmation.
        if self.show_kill_confirm.load(Ordering::SeqCst) {
            match &self.kill_state {
                KillSwitchState::Idle => {
                    egui::Window::new("Kill switch")
                        .collapsible(false)
                        .resizable(false)
                        .show(ctx, |ui| {
                            ui.label(
                                "Ken wirklich stoppen? Das deaktiviert Ken auf diesem PC \
                                 bis ein Administrator den Service wieder einschaltet.",
                            );
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if ui.button("Ja, stoppen").clicked() {
                                    match IpcClient::connect()
                                        .and_then(|mut c| c.activate_kill_switch())
                                    {
                                        Ok(()) => {
                                            self.kill_state = KillSwitchState::Confirmed;
                                        }
                                        Err(e) => {
                                            // IPC failed — fall back to local activation
                                            let data_dir = crate::config::data_dir();
                                            let paths = crate::config::DataPaths::new(&data_dir);
                                            let user =
                                                std::env::var("USERNAME").unwrap_or_default();
                                            if crate::killswitch::activate(
                                                &paths.kill_switch_file,
                                                "user requested via tray app (IPC fallback)",
                                                &user,
                                            )
                                            .is_ok()
                                            {
                                                self.kill_state = KillSwitchState::Confirmed;
                                            } else {
                                                self.kill_state =
                                                    KillSwitchState::Failed(e.to_string());
                                            }
                                        }
                                    }
                                }
                                if ui.button("Abbrechen").clicked() {
                                    self.show_kill_confirm.store(false, Ordering::SeqCst);
                                }
                            });
                        });
                }
                KillSwitchState::Confirmed => {
                    egui::Window::new("Ken gestoppt")
                        .collapsible(false)
                        .resizable(false)
                        .show(ctx, |ui| {
                            ui.label(
                                "Ken wurde gestoppt und wird beim nächsten Start \
                                 nicht wieder ausgeführt.",
                            );
                            if ui.button("OK").clicked() {
                                self.kill_state = KillSwitchState::Idle;
                                self.show_kill_confirm.store(false, Ordering::SeqCst);
                            }
                        });
                }
                KillSwitchState::Failed(ref msg) => {
                    let msg = msg.clone();
                    egui::Window::new("Fehler")
                        .collapsible(false)
                        .resizable(false)
                        .show(ctx, |ui| {
                            ui.label(format!("Kill-Switch konnte nicht aktiviert werden: {msg}"));
                            ui.add_space(5.0);
                            ui.label(
                                "Manuell: Datei 'kill-switch-requested' \
                                 im Ken-Datenverzeichnis erstellen, dann Service stoppen.",
                            );
                            if ui.button("OK").clicked() {
                                self.kill_state = KillSwitchState::Idle;
                                self.show_kill_confirm.store(false, Ordering::SeqCst);
                            }
                        });
                }
            }
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}
