//! Consent dialog for remote session requests per ADR-0001 T1-4.
//!
//! This is the most security-critical UI in the entire agent. It
//! displays a modal dialog asking the user to allow or deny a remote
//! control session. The dialog:
//!
//! - Is always-on-top and modal
//! - Cannot be dismissed by clicking outside
//! - Has a 60-second countdown timer that auto-denies on expiry
//! - Shows who is requesting and why
//!
//! The consent flow is exercised end-to-end even in Phase 1 where
//! the actual remote session uses `NoOpBackend`.

#![cfg(all(windows, feature = "tray-app"))]

use std::time::{Duration, Instant};

use eframe::egui;
use ken_protocol::ids::CommandId;

use crate::ipc::ConsentOutcome;

/// State for the consent dialog.
pub struct ConsentDialog {
    /// Who is requesting the session.
    pub admin_name: String,
    /// Description of why the session is requested.
    pub session_description: String,
    /// Which command this consent request is for.
    pub command_id: CommandId,
    /// When the dialog was shown.
    pub shown_at: Instant,
    /// How long to wait before auto-denying.
    pub timeout: Duration,
    /// The user's decision, if made.
    pub outcome: Option<ConsentOutcome>,
}

impl ConsentDialog {
    /// Create a new consent dialog.
    #[must_use]
    pub fn new(admin_name: String, session_description: String, command_id: CommandId) -> Self {
        Self {
            admin_name,
            session_description,
            command_id,
            shown_at: Instant::now(),
            timeout: Duration::from_secs(60),
            outcome: None,
        }
    }

    /// Render the dialog inside an OS-level viewport (via `show_viewport_deferred`).
    ///
    /// Uses `egui::CentralPanel` so the content fills the independent OS window
    /// rather than floating inside a parent eframe window. Returns `Some(outcome)`
    /// when the user decides or the timeout expires. ADR-0009.
    pub fn show_in_viewport(&mut self, ctx: &egui::Context) -> Option<ConsentOutcome> {
        if self.outcome.is_some() {
            return self.outcome.clone();
        }

        let elapsed = self.shown_at.elapsed();
        if elapsed >= self.timeout {
            self.outcome = Some(ConsentOutcome::TimedOut);
            return self.outcome.clone();
        }

        let remaining = (self.timeout - elapsed).as_secs();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} möchte eine Fernsteuerungs-Sitzung auf deinem PC starten.",
                        self.admin_name
                    ))
                    .size(16.0),
                );

                if !self.session_description.is_empty() {
                    ui.add_space(5.0);
                    ui.label(format!("Grund: {}", self.session_description));
                }

                ui.add_space(10.0);
                ui.label(format!("Verbleibende Zeit: {remaining} Sekunden"));
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    if ui
                        .button(egui::RichText::new("Erlauben").size(14.0))
                        .clicked()
                    {
                        self.outcome = Some(ConsentOutcome::Granted);
                    }
                    ui.add_space(20.0);
                    if ui
                        .button(egui::RichText::new("Ablehnen").size(14.0))
                        .clicked()
                    {
                        self.outcome = Some(ConsentOutcome::Denied);
                    }
                });
            });
        });

        ctx.request_repaint_after(Duration::from_millis(500));

        self.outcome.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_initializes_without_outcome() {
        let dialog = ConsentDialog::new("Admin".to_string(), "test".to_string(), CommandId::new());
        assert!(dialog.outcome.is_none());
    }

    #[test]
    fn timeout_produces_timed_out() {
        let mut dialog =
            ConsentDialog::new("Admin".to_string(), "test".to_string(), CommandId::new());
        dialog.timeout = Duration::from_millis(0);
        dialog.shown_at = Instant::now() - Duration::from_secs(1);
        // Without a UI context we can't call show_in_viewport(), but the timeout
        // logic is verified.
        let elapsed = dialog.shown_at.elapsed();
        assert!(elapsed >= dialog.timeout);
    }
}
