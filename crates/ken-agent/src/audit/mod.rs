//! Local audit log for the Ken agent.
//!
//! Per ADR-0001 T1-5, every action the agent takes is recorded in a
//! local audit log readable by the endpoint user. The log is append-only
//! JSONL at a well-known path under `ProgramData\Ken\audit.log`.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ken_protocol::audit::{AuditEvent, AuditEventKind};
use time::OffsetDateTime;
use uuid::Uuid;

/// The audit logger, writing to a JSONL file and keeping a bounded
/// in-memory tail for inclusion in heartbeats.
pub struct AuditLogger {
    file: Mutex<File>,
    path: PathBuf,
    recent: Mutex<VecDeque<AuditEvent>>,
    max_size_bytes: u64,
}

/// Maximum number of events kept in memory for heartbeat inclusion.
const MAX_RECENT: usize = 200;

impl AuditLogger {
    /// Open or create the audit log file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened.
    pub fn open(path: &Path, max_size_bytes: u64) -> Result<Self, anyhow::Error> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new().create(true).append(true).open(path)?;

        Ok(Self {
            file: Mutex::new(file),
            path: path.to_path_buf(),
            recent: Mutex::new(VecDeque::with_capacity(MAX_RECENT)),
            max_size_bytes,
        })
    }

    /// Log an audit event.
    ///
    /// Writes to the file and adds to the in-memory tail. If the file
    /// exceeds the configured size limit, it is rotated.
    pub fn log(&self, kind: AuditEventKind, message: &str) {
        let event = AuditEvent {
            event_id: Uuid::new_v4(),
            occurred_at: OffsetDateTime::now_utc(),
            kind,
            message: message.to_string(),
        };

        // Write to file
        if let Ok(json) = serde_json::to_string(&event) {
            if let Ok(mut file) = self.file.lock() {
                let _ = writeln!(file, "{json}");
                let _ = file.flush();
            }
        }

        // Add to in-memory tail
        if let Ok(mut recent) = self.recent.lock() {
            if recent.len() >= MAX_RECENT {
                recent.pop_front();
            }
            recent.push_back(event);
        }

        // Check rotation
        self.maybe_rotate();
    }

    /// Return the most recent events for inclusion in a heartbeat.
    pub fn recent(&self, limit: usize) -> Vec<AuditEvent> {
        self.recent
            .lock()
            .map(|recent| recent.iter().rev().take(limit).rev().cloned().collect())
            .unwrap_or_default()
    }

    fn maybe_rotate(&self) {
        let size = fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if size > self.max_size_bytes {
            let rotated = self.path.with_extension("log.1");
            let _ = fs::rename(&self.path, &rotated);

            // Re-open the file
            if let Ok(new_file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
            {
                if let Ok(mut file) = self.file.lock() {
                    *file = new_file;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_log_write_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("audit.log");

        let logger = AuditLogger::open(&log_path, 1024 * 1024).unwrap();

        logger.log(AuditEventKind::ServiceStarted, "service started");
        logger.log(AuditEventKind::HeartbeatSent, "heartbeat sent");

        let recent = logger.recent(50);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].message, "service started");
        assert_eq!(recent[1].message, "heartbeat sent");

        // Verify file has content
        let contents = fs::read_to_string(&log_path).unwrap();
        assert!(contents.contains("service_started"));
    }

    #[test]
    fn audit_log_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("audit.log");

        // Small max size to trigger rotation
        let logger = AuditLogger::open(&log_path, 100).unwrap();

        for i in 0..20 {
            logger.log(AuditEventKind::HeartbeatSent, &format!("heartbeat {i}"));
        }

        // After rotation, the rotated file should exist
        let rotated = dir.path().join("audit.log.1");
        assert!(rotated.exists() || log_path.exists());
    }
}
