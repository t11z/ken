//! Main tray application entry point.
//!
//! Sets up the system tray icon, context menu, and dispatches menu
//! clicks to the appropriate windows or actions.

#![cfg(windows)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

/// Run the tray app event loop.
///
/// Creates the system tray icon with menu items and starts the egui
/// event loop. The main window is hidden by default (tray-only mode)
/// and windows are shown on demand from the tray menu.
pub fn run_tray_app() {
    tracing::info!("starting tray app");

    let menu = Menu::new();
    let item_status = MenuItem::new("Status", true, None);
    let item_audit = MenuItem::new("View audit log", true, None);
    let item_kill = MenuItem::new("Kill switch", true, None);
    let item_about = MenuItem::new("About", true, None);
    let item_quit = MenuItem::new("Quit", true, None);

    menu.append(&item_status).ok();
    menu.append(&item_audit).ok();
    menu.append(&item_kill).ok();
    menu.append(&item_about).ok();
    menu.append(&item_quit).ok();

    // Build tray icon — uses a placeholder 16x16 RGBA icon.
    // A proper .ico file would be loaded from resources/ in production.
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

    // Menu event handling runs in a thread; egui runs on the main thread.
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
                // Open the audit log in the user's default text viewer
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

    // Run the egui event loop. The main window is hidden; we only show
    // windows when triggered by tray menu clicks.
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
                kill_confirmed: false,
            }))
        }),
    );
}

struct TrayApp {
    show_status: Arc<AtomicBool>,
    show_kill_confirm: Arc<AtomicBool>,
    kill_confirmed: bool,
}

impl eframe::App for TrayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Status window
        if self.show_status.load(Ordering::SeqCst) {
            super::status_window::show(ctx, &self.show_status);
        }

        // Kill switch confirmation
        if self.show_kill_confirm.load(Ordering::SeqCst) && !self.kill_confirmed {
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
                            let data_dir = crate::config::data_dir();
                            let paths = crate::config::DataPaths::new(&data_dir);
                            let user = std::env::var("USERNAME").unwrap_or_default();
                            match crate::killswitch::activate(
                                &paths.kill_switch_file,
                                "user requested via tray app",
                                &user,
                            ) {
                                Ok(()) => {
                                    self.kill_confirmed = true;
                                }
                                Err(e) => {
                                    tracing::error!(error = %e, "kill switch activation failed");
                                }
                            }
                        }
                        if ui.button("Abbrechen").clicked() {
                            self.show_kill_confirm.store(false, Ordering::SeqCst);
                        }
                    });
                });
        }

        // Kill switch success message
        if self.kill_confirmed {
            egui::Window::new("Ken gestoppt")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(
                        "Ken wurde gestoppt und wird beim nächsten Start \
                         nicht wieder ausgeführt.",
                    );
                    if ui.button("OK").clicked() {
                        self.kill_confirmed = false;
                        self.show_kill_confirm.store(false, Ordering::SeqCst);
                    }
                });
        }

        // Repaint periodically for status polling
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }
}
