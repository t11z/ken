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
/// loop. The root eframe window is invisible and zero-sized; all
/// visible UI is opened as independent OS-level viewports (ADR-0009).
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

    // Placeholder 16x16 RGBA icon (solid blue, one pixel repeated).
    let icon = tray_icon::Icon::from_rgba([0x33u8, 0x66, 0x99, 0xFF].repeat(16 * 16), 16, 16)
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

    // Root window: invisible, zero-sized, no decorations, no taskbar entry.
    // All visible surfaces are opened as independent OS-level viewports below.
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1.0, 1.0])
            .with_visible(false)
            .with_decorations(false)
            .with_taskbar(false),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "Ken Agent",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(TrayApp {
                show_status,
                show_kill_confirm,
                kill_state: Arc::new(Mutex::new(KillSwitchState::Idle)),
                consent_dialog: Arc::new(Mutex::new(None)),
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
    kill_state: Arc<Mutex<KillSwitchState>>,
    consent_dialog: Arc<Mutex<Option<super::consent_dialog::ConsentDialog>>>,
    consent_dialog_active: Arc<AtomicBool>,
    ipc_rx: Mutex<mpsc::Receiver<IpcMessage>>,
}

impl eframe::App for TrayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Keep the root shell window hidden — with_visible(false) in NativeOptions is not
        // reliable on Windows with eframe 0.31 and the window can flash on first paint.
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));

        // Check for incoming IPC messages (non-blocking).
        if let Ok(rx) = self.ipc_rx.lock() {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    IpcMessage::ConsentRequest(info) => {
                        let mut dialog = self.consent_dialog.lock().unwrap();
                        if dialog.is_none() {
                            self.consent_dialog_active.store(true, Ordering::SeqCst);
                            *dialog = Some(super::consent_dialog::ConsentDialog::new(
                                info.admin_name,
                                info.session_description,
                                info.command_id,
                            ));
                        }
                    }
                }
            }
        }

        // Consent dialog — always-on-top OS window per ADR-0009.
        if self.consent_dialog_active.load(Ordering::SeqCst) {
            let consent_dialog = self.consent_dialog.clone();
            let consent_dialog_active = self.consent_dialog_active.clone();
            ctx.show_viewport_deferred(
                egui::ViewportId::from_hash_of("consent"),
                egui::ViewportBuilder::default()
                    .with_title("Ken \u{2014} Fernsteuerungs-Anfrage")
                    .with_always_on_top()
                    .with_inner_size([400.0, 220.0])
                    .with_resizable(false),
                move |ctx, _class| {
                    let mut dialog_lock = consent_dialog.lock().unwrap();
                    if let Some(ref mut dialog) = *dialog_lock {
                        if let Some(outcome) = dialog.show_in_viewport(ctx) {
                            let granted = matches!(outcome, crate::ipc::ConsentOutcome::Granted);
                            let command_id = dialog.command_id;
                            if let Ok(mut client) = IpcClient::connect() {
                                let _ = client.submit_consent_response(&command_id, granted);
                            }
                            *dialog_lock = None;
                            consent_dialog_active.store(false, Ordering::SeqCst);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    } else {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                },
            );
        }

        // Status window — independent OS window.
        if self.show_status.load(Ordering::SeqCst) {
            let show_status = self.show_status.clone();
            ctx.show_viewport_deferred(
                egui::ViewportId::from_hash_of("status"),
                egui::ViewportBuilder::default()
                    .with_title("Ken Agent Status")
                    .with_inner_size([450.0, 300.0])
                    .with_resizable(false),
                move |ctx, _class| {
                    super::status_window::show_in_viewport(ctx, &show_status);
                },
            );
        }

        // Kill switch confirmation — independent OS window.
        if self.show_kill_confirm.load(Ordering::SeqCst) {
            let kill_state = self.kill_state.clone();
            let show_kill_confirm = self.show_kill_confirm.clone();
            ctx.show_viewport_deferred(
                egui::ViewportId::from_hash_of("kill_switch"),
                egui::ViewportBuilder::default()
                    .with_title("Kill Switch")
                    .with_inner_size([420.0, 180.0])
                    .with_resizable(false),
                move |ctx, _class| {
                    if ctx.input(|i| i.viewport().close_requested()) {
                        show_kill_confirm.store(false, Ordering::SeqCst);
                        return;
                    }

                    let close = egui::CentralPanel::default()
                        .show(ctx, |ui| {
                            let mut state = kill_state.lock().unwrap();
                            let mut new_state: Option<KillSwitchState> = None;
                            let mut close = false;

                            match &*state {
                                KillSwitchState::Idle => {
                                    ui.label(
                                        "Ken wirklich stoppen? Das deaktiviert Ken auf \
                                         diesem PC bis ein Administrator den Service \
                                         wieder einschaltet.",
                                    );
                                    ui.add_space(10.0);
                                    let mut ja = false;
                                    let mut abbrechen = false;
                                    ui.horizontal(|ui| {
                                        ja = ui.button("Ja, stoppen").clicked();
                                        abbrechen = ui.button("Abbrechen").clicked();
                                    });
                                    if ja {
                                        match IpcClient::connect()
                                            .and_then(|mut c| c.activate_kill_switch())
                                        {
                                            Ok(()) => {
                                                new_state = Some(KillSwitchState::Confirmed);
                                            }
                                            Err(e) => {
                                                let data_dir = crate::config::data_dir();
                                                let paths =
                                                    crate::config::DataPaths::new(&data_dir);
                                                let user =
                                                    std::env::var("USERNAME").unwrap_or_default();
                                                if crate::killswitch::activate(
                                                    &paths.kill_switch_file,
                                                    "user requested via tray app (IPC fallback)",
                                                    &user,
                                                )
                                                .is_ok()
                                                {
                                                    new_state = Some(KillSwitchState::Confirmed);
                                                } else {
                                                    new_state = Some(KillSwitchState::Failed(
                                                        e.to_string(),
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                    if abbrechen {
                                        close = true;
                                    }
                                }
                                KillSwitchState::Confirmed => {
                                    ui.label(
                                        "Ken wurde gestoppt und wird beim nächsten Start \
                                         nicht wieder ausgeführt.",
                                    );
                                    if ui.button("OK").clicked() {
                                        new_state = Some(KillSwitchState::Idle);
                                        close = true;
                                    }
                                }
                                KillSwitchState::Failed(ref msg) => {
                                    let msg = msg.clone();
                                    ui.label(format!(
                                        "Kill-Switch konnte nicht aktiviert werden: {msg}"
                                    ));
                                    ui.add_space(5.0);
                                    ui.label(
                                        "Manuell: Datei 'kill-switch-requested' \
                                         im Ken-Datenverzeichnis erstellen, dann Service stoppen.",
                                    );
                                    if ui.button("OK").clicked() {
                                        new_state = Some(KillSwitchState::Idle);
                                        close = true;
                                    }
                                }
                            }

                            if let Some(ns) = new_state {
                                *state = ns;
                            }
                            close
                        })
                        .inner;

                    if close {
                        show_kill_confirm.store(false, Ordering::SeqCst);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                },
            );
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}
