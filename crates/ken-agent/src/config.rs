//! Agent configuration loading.
//!
//! The agent has two kinds of configuration:
//! - **Bundled defaults** compiled into the binary
//! - **Enrolled state** written to disk during enrollment
//!
//! The enrolled state lives under `%ProgramData%\Ken\` on Windows
//! with restricted ACLs (only `LocalSystem` and Administrators can read).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Top-level agent configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Server connection settings.
    #[serde(default)]
    pub server: ServerConfig,

    /// Heartbeat timing settings.
    #[serde(default)]
    pub heartbeat: HeartbeatConfig,

    /// Audit log settings.
    #[serde(default)]
    pub audit: AuditConfig,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            heartbeat: HeartbeatConfig::default(),
            audit: AuditConfig::default(),
        }
    }
}

/// Server connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Server URL. Empty until enrollment.
    #[serde(default)]
    pub url: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
        }
    }
}

/// Heartbeat timing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    /// Base interval between heartbeats in seconds.
    #[serde(default = "default_heartbeat_interval")]
    pub interval_seconds: u32,

    /// Random jitter added to the interval (0 to this value).
    #[serde(default = "default_jitter")]
    pub jitter_seconds: u32,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            interval_seconds: default_heartbeat_interval(),
            jitter_seconds: default_jitter(),
        }
    }
}

fn default_heartbeat_interval() -> u32 {
    60
}

fn default_jitter() -> u32 {
    10
}

/// Audit log configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    /// Maximum log file size in bytes before rotation.
    #[serde(default = "default_max_log_size")]
    pub max_log_size_bytes: u64,

    /// How many days of audit entries to retain.
    #[serde(default = "default_retention")]
    pub retention_days: u32,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            max_log_size_bytes: default_max_log_size(),
            retention_days: default_retention(),
        }
    }
}

fn default_max_log_size() -> u64 {
    10 * 1024 * 1024 // 10 MiB
}

fn default_retention() -> u32 {
    30
}

/// Credentials and identity established during enrollment.
#[derive(Debug, Clone)]
pub struct EnrolledCredentials {
    /// The server-assigned endpoint identifier.
    pub endpoint_id: ken_protocol::EndpointId,
    /// PEM-encoded root CA certificate.
    pub ca_certificate_pem: String,
    /// PEM-encoded client certificate.
    pub client_certificate_pem: String,
    /// PEM-encoded client private key.
    pub client_private_key_pem: String,
}

/// Return the base data directory for Ken on the current platform.
///
/// On Windows: `%ProgramData%\Ken`
/// On other platforms (for development): `./ken-data`
#[must_use]
pub fn data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let program_data =
            std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".to_string());
        PathBuf::from(program_data).join("Ken")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("./ken-data")
    }
}

/// Paths within the data directory.
pub struct DataPaths {
    pub config_file: PathBuf,
    pub ca_cert: PathBuf,
    pub client_cert: PathBuf,
    pub client_key: PathBuf,
    pub endpoint_id_file: PathBuf,
    pub audit_log: PathBuf,
    pub kill_switch_file: PathBuf,
}

impl DataPaths {
    /// Resolve all paths relative to the given data directory.
    #[must_use]
    pub fn new(base: &Path) -> Self {
        Self {
            config_file: base.join("config.toml"),
            ca_cert: base.join("credentials").join("ca.pem"),
            client_cert: base.join("credentials").join("client.crt"),
            client_key: base.join("credentials").join("client.key"),
            endpoint_id_file: base.join("state").join("endpoint_id"),
            audit_log: base.join("audit.log"),
            kill_switch_file: base.join("state").join("kill-switch-requested"),
        }
    }
}

impl AgentConfig {
    /// Load configuration from the default config file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn load(config_path: &Path) -> Result<Self, anyhow::Error> {
        let contents = match std::fs::read_to_string(config_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "failed to read config at {}: {e}",
                    config_path.display()
                ));
            }
        };

        let config: Self = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Check whether the agent has been enrolled (server URL is set).
    #[must_use]
    pub fn is_enrolled(&self) -> bool {
        !self.server.url.is_empty()
    }
}

impl EnrolledCredentials {
    /// Load enrolled credentials from disk.
    ///
    /// # Errors
    ///
    /// Returns an error if any credential file is missing or unreadable.
    pub fn load(paths: &DataPaths) -> Result<Self, anyhow::Error> {
        let endpoint_id_str = std::fs::read_to_string(&paths.endpoint_id_file)
            .map_err(|e| anyhow::anyhow!("failed to read endpoint ID: {e}"))?;
        let endpoint_id = ken_protocol::EndpointId::parse(endpoint_id_str.trim())
            .map_err(|e| anyhow::anyhow!("invalid endpoint ID: {e}"))?;

        let ca_certificate_pem = std::fs::read_to_string(&paths.ca_cert)
            .map_err(|e| anyhow::anyhow!("failed to read CA cert: {e}"))?;
        let client_certificate_pem = std::fs::read_to_string(&paths.client_cert)
            .map_err(|e| anyhow::anyhow!("failed to read client cert: {e}"))?;
        let client_private_key_pem = std::fs::read_to_string(&paths.client_key)
            .map_err(|e| anyhow::anyhow!("failed to read client key: {e}"))?;

        Ok(Self {
            endpoint_id,
            ca_certificate_pem,
            client_certificate_pem,
            client_private_key_pem,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_roundtrip() {
        let config = AgentConfig::default();
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: AgentConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.heartbeat.interval_seconds, 60);
        assert_eq!(parsed.heartbeat.jitter_seconds, 10);
    }

    #[test]
    fn empty_server_url_means_not_enrolled() {
        let config = AgentConfig::default();
        assert!(!config.is_enrolled());
    }

    #[test]
    fn data_paths_resolve_correctly() {
        let paths = DataPaths::new(Path::new("/var/lib/ken"));
        assert_eq!(paths.config_file, PathBuf::from("/var/lib/ken/config.toml"));
        assert_eq!(
            paths.ca_cert,
            PathBuf::from("/var/lib/ken/credentials/ca.pem")
        );
    }
}
