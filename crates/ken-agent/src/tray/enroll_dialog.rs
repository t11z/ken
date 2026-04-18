//! Enrollment dialog for the Ken tray app (ADR-0009).
//!
//! Lets the user paste a one-time enrollment URL received from the family
//! IT chief, then contacts the Ken server, receives mTLS credentials, and
//! writes them to disk. The HTTP round-trip runs in a background thread so
//! the egui frame loop is never blocked.

#![cfg(all(windows, feature = "tray-app"))]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;

/// Lifecycle state for the enrollment flow.
enum EnrollState {
    Idle,
    InProgress,
    Success,
    Failed(String),
}

/// Enrollment dialog shown in an independent OS viewport.
pub struct EnrollDialog {
    url_input: String,
    state: Arc<Mutex<EnrollState>>,
}

impl EnrollDialog {
    #[must_use]
    pub fn new() -> Self {
        Self {
            url_input: String::new(),
            state: Arc::new(Mutex::new(EnrollState::Idle)),
        }
    }

    /// Render inside a deferred viewport. Sets `visible` to `false` and sends
    /// `ViewportCommand::Close` when the dialog should dismiss. ADR-0009.
    pub fn show_in_viewport(&mut self, ctx: &egui::Context, visible: &Arc<AtomicBool>) {
        if ctx.input(|i| i.viewport().close_requested()) {
            visible.store(false, Ordering::SeqCst);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Snapshot state outside the UI closure to avoid holding the guard
        // while egui borrows the widget tree.
        let (is_idle, is_in_progress, is_success, error_msg) = {
            let s = self.state.lock().unwrap();
            match &*s {
                EnrollState::Idle => (true, false, false, None),
                EnrollState::InProgress => (false, true, false, None),
                EnrollState::Success => (false, false, true, None),
                EnrollState::Failed(msg) => (false, false, false, Some(msg.clone())),
            }
        };

        let mut submit_url: Option<String> = None;
        let mut should_close = false;

        egui::CentralPanel::default().show(ctx, |ui| {
            if is_idle || error_msg.is_some() {
                if let Some(ref msg) = error_msg {
                    ui.colored_label(egui::Color32::RED, format!("Fehler: {msg}"));
                    ui.add_space(5.0);
                }
                ui.label("Enrollment-URL:");
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.url_input)
                        .hint_text("https://ken.example:8444/enroll/…")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let can_submit = !self.url_input.trim().is_empty();
                    if ui
                        .add_enabled(can_submit, egui::Button::new("Einschreiben"))
                        .clicked()
                    {
                        submit_url = Some(self.url_input.trim().to_string());
                    }
                    if ui.button("Abbrechen").clicked() {
                        should_close = true;
                    }
                });
            } else if is_in_progress {
                ui.spinner();
                ui.label("Enrollment läuft…");
            } else if is_success {
                ui.label(
                    "Enrollment erfolgreich! \
                     Der Ken Agent ist jetzt mit dem Server verbunden.",
                );
                ui.add_space(10.0);
                if ui.button("OK").clicked() {
                    should_close = true;
                }
            }
        });

        if let Some(url) = submit_url {
            let state_arc = self.state.clone();
            {
                let mut s = state_arc.lock().unwrap();
                *s = EnrollState::InProgress;
            }
            std::thread::spawn(move || {
                let result = do_enrollment(&url);
                let mut s = state_arc.lock().unwrap();
                match result {
                    Ok(()) => *s = EnrollState::Success,
                    Err(e) => *s = EnrollState::Failed(e.to_string()),
                }
            });
        }

        if should_close {
            visible.store(false, Ordering::SeqCst);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        if is_in_progress {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
}

/// POST the enrollment request to the Ken server and write credentials to disk.
fn do_enrollment(url: &str) -> Result<(), anyhow::Error> {
    let (base_url, token) = crate::enroll::parse_enrollment_url(url)?;
    let request_body = crate::enroll::build_request(&token);
    let enrollment_url = format!("{base_url}/enroll/{token}");

    let rt = tokio::runtime::Runtime::new()?;
    let response = rt.block_on(async {
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(format!("ken-agent/{}", env!("CARGO_PKG_VERSION")))
            .build()?;

        let resp = client
            .post(&enrollment_url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Verbindung zum Server fehlgeschlagen: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Server antwortete mit {status}: {body}"));
        }

        resp.json::<ken_protocol::enrollment::EnrollmentResponse>()
            .await
            .map_err(|e| anyhow::anyhow!("Ungültige Server-Antwort: {e}"))
    })?;

    let data_dir = crate::config::data_dir();
    let paths = crate::config::DataPaths::new(&data_dir);
    crate::enroll::write_credentials(&paths, &response, &response.server_url)?;

    tracing::info!("enrollment completed via tray app");
    Ok(())
}
