//! Agent enrollment flow.
//!
//! Enrollment is a one-time process where the agent connects to the
//! Ken server using a short-lived enrollment URL, receives mTLS
//! credentials, and writes them to disk.

use std::path::Path;

use ken_protocol::enrollment::{EnrollmentRequest, EnrollmentResponse};

use crate::config::DataPaths;

/// Parse an enrollment URL to extract the base URL and token.
///
/// The URL format is: `https://<host>:<port>/enroll/<token>`
///
/// # Errors
///
/// Returns an error if the URL does not contain `/enroll/`.
pub fn parse_enrollment_url(url: &str) -> Result<(String, String), anyhow::Error> {
    let parts: Vec<&str> = url.rsplitn(2, "/enroll/").collect();
    if parts.len() != 2 {
        anyhow::bail!("invalid enrollment URL: must contain /enroll/<token>");
    }
    let base_url = parts[1].to_string();
    let token = parts[0].to_string();
    Ok((base_url, token))
}

/// Write enrollment credentials to disk.
///
/// Creates the credentials directory and writes CA cert, client cert,
/// client key, endpoint ID, and updates the config file.
///
/// # Errors
///
/// Returns an error if any file operation fails.
pub fn write_credentials(
    paths: &DataPaths,
    response: &EnrollmentResponse,
    server_url: &str,
) -> Result<(), anyhow::Error> {
    // Create directories
    create_parent_dirs(&paths.ca_cert)?;
    create_parent_dirs(&paths.endpoint_id_file)?;

    // Write credentials
    std::fs::write(&paths.ca_cert, &response.ca_certificate_pem)?;
    std::fs::write(&paths.client_cert, &response.client_certificate_pem)?;
    std::fs::write(&paths.client_key, &response.client_private_key_pem)?;

    // Write endpoint ID
    std::fs::write(&paths.endpoint_id_file, response.endpoint_id.to_string())?;

    // Update config with server URL
    let config_content = format!(
        "[server]\nurl = \"{server_url}\"\n\n[heartbeat]\ninterval_seconds = 60\njitter_seconds = 10\n"
    );
    std::fs::write(&paths.config_file, config_content)?;

    tracing::info!(
        endpoint_id = %response.endpoint_id,
        "credentials written to disk"
    );

    Ok(())
}

/// Build an `EnrollmentRequest` from current host information.
#[must_use]
pub fn build_request(token: &str) -> EnrollmentRequest {
    let hostname = hostname();
    let os_version = os_version();

    EnrollmentRequest {
        schema_version: ken_protocol::SCHEMA_VERSION,
        enrollment_token: token.to_string(),
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
        os_version,
        hostname,
        requested_at: time::OffsetDateTime::now_utc(),
    }
}

fn hostname() -> String {
    #[cfg(windows)]
    {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "UNKNOWN".to_string())
    }
    #[cfg(not(windows))]
    {
        std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("HOST"))
            .unwrap_or_else(|_| "unknown".to_string())
    }
}

fn os_version() -> String {
    #[cfg(windows)]
    {
        "Windows".to_string()
    }
    #[cfg(not(windows))]
    {
        "Linux (development)".to_string()
    }
}

fn create_parent_dirs(path: &Path) -> Result<(), anyhow::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_enrollment_url_valid() {
        let (base, token) = parse_enrollment_url("https://ken.home:8444/enroll/abc-123").unwrap();
        assert_eq!(base, "https://ken.home:8444");
        assert_eq!(token, "abc-123");
    }

    #[test]
    fn parse_enrollment_url_invalid() {
        assert!(parse_enrollment_url("https://ken.home:8444/something").is_err());
    }

    #[test]
    fn build_request_sets_version() {
        let req = build_request("test-token");
        assert_eq!(req.enrollment_token, "test-token");
        assert_eq!(req.agent_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn write_credentials_to_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::new(dir.path());

        let response = EnrollmentResponse {
            endpoint_id: ken_protocol::EndpointId::new(),
            ca_certificate_pem: "test-ca".to_string(),
            client_certificate_pem: "test-cert".to_string(),
            client_private_key_pem: "test-key".to_string(),
            server_url: "https://test:8443".to_string(),
            issued_at: time::OffsetDateTime::now_utc(),
            certificate_expires_at: time::OffsetDateTime::now_utc(),
        };

        write_credentials(&paths, &response, "https://test:8443").unwrap();

        assert_eq!(std::fs::read_to_string(&paths.ca_cert).unwrap(), "test-ca");
        assert_eq!(
            std::fs::read_to_string(&paths.endpoint_id_file).unwrap(),
            response.endpoint_id.to_string()
        );
    }
}
