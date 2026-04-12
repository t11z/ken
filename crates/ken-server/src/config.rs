//! Server configuration loading and validation.
//!
//! Configuration is loaded from a TOML file at a path given via
//! `--config` on the command line, the `KEN_CONFIG` environment
//! variable, or the default `/etc/ken/ken.toml`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::AppError;

/// Top-level server configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Network listener and public URL settings.
    #[serde(default)]
    pub server: ServerConfig,

    /// Data storage location.
    #[serde(default)]
    pub storage: StorageConfig,

    /// TLS certificate file paths.
    #[serde(default)]
    pub tls: TlsConfig,

    /// Enrollment settings.
    #[serde(default)]
    pub enrollment: EnrollmentConfig,

    /// Logging configuration.
    #[serde(default)]
    pub logging: LoggingConfig,
}

/// Network listener configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Address for the agent-facing mTLS listener.
    #[serde(default = "default_agent_listen")]
    pub agent_listen_address: SocketAddr,

    /// Address for the admin UI and enrollment listener (server cert only).
    #[serde(default = "default_admin_listen")]
    pub admin_listen_address: SocketAddr,

    /// How agents should reach this server (used in enrollment responses).
    #[serde(default = "default_public_url")]
    pub public_url: String,

    /// How the admin UI (and enrollment URLs) should be reached from a browser.
    ///
    /// This is the public base URL of the admin listener. It is used when
    /// constructing the enrollment URL shown to the operator after creating a
    /// new enrollment token. Set this to the hostname or IP the operator's
    /// browser will use to reach the admin UI, e.g. `https://ken.local:8444`.
    #[serde(default = "default_admin_public_url")]
    pub admin_public_url: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            agent_listen_address: default_agent_listen(),
            admin_listen_address: default_admin_listen(),
            public_url: default_public_url(),
            admin_public_url: default_admin_public_url(),
        }
    }
}

fn default_agent_listen() -> SocketAddr {
    "0.0.0.0:8443".parse().unwrap()
}

fn default_admin_listen() -> SocketAddr {
    "0.0.0.0:8444".parse().unwrap()
}

fn default_public_url() -> String {
    "https://localhost:8443".to_string()
}

fn default_admin_public_url() -> String {
    "https://localhost:8444".to_string()
}

/// Storage configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    /// Root directory for all persistent data (`SQLite`, CA keys, state).
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
        }
    }
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("/var/lib/ken")
}

/// TLS certificate paths, resolved relative to `data_dir` if not absolute.
#[derive(Debug, Clone, Deserialize)]
#[allow(clippy::struct_field_names)] // All fields are paths by design
pub struct TlsConfig {
    /// Path to the root CA certificate PEM file.
    #[serde(default = "default_ca_cert")]
    pub ca_certificate_path: PathBuf,

    /// Path to the root CA private key PEM file.
    #[serde(default = "default_ca_key")]
    pub ca_key_path: PathBuf,

    /// Path to the server certificate PEM file.
    #[serde(default = "default_server_cert")]
    pub server_certificate_path: PathBuf,

    /// Path to the server private key PEM file.
    #[serde(default = "default_server_key")]
    pub server_key_path: PathBuf,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            ca_certificate_path: default_ca_cert(),
            ca_key_path: default_ca_key(),
            server_certificate_path: default_server_cert(),
            server_key_path: default_server_key(),
        }
    }
}

fn default_ca_cert() -> PathBuf {
    PathBuf::from("ca/root.crt")
}

fn default_ca_key() -> PathBuf {
    PathBuf::from("ca/root.key")
}

fn default_server_cert() -> PathBuf {
    PathBuf::from("server/server.crt")
}

fn default_server_key() -> PathBuf {
    PathBuf::from("server/server.key")
}

/// Enrollment settings.
#[derive(Debug, Clone, Deserialize)]
pub struct EnrollmentConfig {
    /// How long an enrollment token is valid, in seconds.
    #[serde(default = "default_token_lifetime")]
    pub token_lifetime_seconds: u64,

    /// How many days a client certificate is valid.
    #[serde(default = "default_cert_lifetime")]
    pub client_certificate_lifetime_days: u64,
}

impl Default for EnrollmentConfig {
    fn default() -> Self {
        Self {
            token_lifetime_seconds: default_token_lifetime(),
            client_certificate_lifetime_days: default_cert_lifetime(),
        }
    }
}

fn default_token_lifetime() -> u64 {
    900 // 15 minutes
}

fn default_cert_lifetime() -> u64 {
    365
}

/// Logging configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    /// Tracing filter level (e.g., "info", "debug", "`ken_server=debug`").
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Log output format.
    #[serde(default)]
    pub format: LogFormat,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: LogFormat::default(),
        }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

/// Log output format.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Structured JSON logs (default, suitable for production).
    #[default]
    Json,
    /// Human-readable text logs (for development).
    Text,
}

impl TlsConfig {
    /// Resolve all paths relative to the given data directory.
    /// Absolute paths are left unchanged.
    #[must_use]
    pub fn resolve_paths(&self, data_dir: &Path) -> Self {
        Self {
            ca_certificate_path: resolve(data_dir, &self.ca_certificate_path),
            ca_key_path: resolve(data_dir, &self.ca_key_path),
            server_certificate_path: resolve(data_dir, &self.server_certificate_path),
            server_key_path: resolve(data_dir, &self.server_key_path),
        }
    }
}

fn resolve(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

impl Config {
    /// Load configuration from a TOML file.
    ///
    /// The path is determined in order of precedence:
    /// 1. The `config_path` argument (from `--config` CLI flag)
    /// 2. The `KEN_CONFIG` environment variable
    /// 3. The default `/etc/ken/ken.toml`
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn load(config_path: Option<&Path>) -> Result<Self, AppError> {
        let path = if let Some(p) = config_path {
            p.to_path_buf()
        } else if let Ok(env_path) = std::env::var("KEN_CONFIG") {
            PathBuf::from(env_path)
        } else {
            PathBuf::from("/etc/ken/ken.toml")
        };

        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(
                    path = %path.display(),
                    "config file not found, using defaults"
                );
                String::new()
            }
            Err(e) => {
                return Err(AppError::Config(format!(
                    "failed to read config file at {}: {e}",
                    path.display()
                )));
            }
        };

        let config: Config = toml::from_str(&contents).map_err(|e| {
            AppError::Config(format!(
                "failed to parse config file at {}: {e}",
                path.display()
            ))
        })?;

        Ok(config)
    }

    /// Return the TLS config with all paths resolved against the data directory.
    #[must_use]
    pub fn resolved_tls(&self) -> TlsConfig {
        self.tls.resolve_paths(&self.storage.data_dir)
    }

    /// Log the resolved configuration, redacting secrets.
    pub fn log_summary(&self) {
        let tls = self.resolved_tls();
        tracing::info!(
            agent_listen = %self.server.agent_listen_address,
            admin_listen = %self.server.admin_listen_address,
            public_url = %self.server.public_url,
            admin_public_url = %self.server.admin_public_url,
            data_dir = %self.storage.data_dir.display(),
            ca_cert = %tls.ca_certificate_path.display(),
            ca_key = %tls.ca_key_path.display(),
            server_cert = %tls.server_certificate_path.display(),
            server_key = %tls.server_key_path.display(),
            token_lifetime_seconds = self.enrollment.token_lifetime_seconds,
            cert_lifetime_days = self.enrollment.client_certificate_lifetime_days,
            log_level = %self.logging.level,
            "resolved configuration"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_parses() {
        let config: Config = toml::from_str("").expect("empty config should parse");
        assert_eq!(
            config.server.agent_listen_address,
            "0.0.0.0:8443".parse::<SocketAddr>().unwrap()
        );
        assert_eq!(config.enrollment.token_lifetime_seconds, 900);
    }

    #[test]
    fn tls_paths_resolve_relative_to_data_dir() {
        let tls = TlsConfig::default();
        let resolved = tls.resolve_paths(Path::new("/var/lib/ken"));
        assert_eq!(
            resolved.ca_certificate_path,
            PathBuf::from("/var/lib/ken/ca/root.crt")
        );
    }

    #[test]
    fn tls_absolute_paths_unchanged() {
        let tls = TlsConfig {
            ca_certificate_path: PathBuf::from("/custom/ca.crt"),
            ..TlsConfig::default()
        };
        let resolved = tls.resolve_paths(Path::new("/var/lib/ken"));
        assert_eq!(
            resolved.ca_certificate_path,
            PathBuf::from("/custom/ca.crt")
        );
    }

    #[test]
    fn custom_config_parses() {
        let toml_str = r#"
[server]
agent_listen_address = "127.0.0.1:9443"
admin_listen_address = "127.0.0.1:9444"
public_url = "https://ken.home:9443"

[storage]
data_dir = "/tmp/ken-test"

[enrollment]
token_lifetime_seconds = 300
client_certificate_lifetime_days = 30
"#;
        let config: Config = toml::from_str(toml_str).expect("custom config should parse");
        assert_eq!(
            config.server.agent_listen_address,
            "127.0.0.1:9443".parse::<SocketAddr>().unwrap()
        );
        assert_eq!(config.enrollment.token_lifetime_seconds, 300);
    }
}
