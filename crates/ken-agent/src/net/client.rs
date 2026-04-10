//! Typed HTTP client for the Ken agent-server API.
//!
//! Uses `reqwest` with `rustls-tls` for mTLS communication per
//! ADR-0002 and ADR-0008. The client is configured from enrolled
//! credentials (CA cert, client cert, client key) and the server URL.

use std::path::Path;
use std::time::Duration;

use ken_protocol::command::CommandOutcome;
use ken_protocol::heartbeat::{Heartbeat, HeartbeatAck};

use crate::config::EnrolledCredentials;

/// HTTP client for communicating with the Ken server over mTLS.
pub struct KenApiClient {
    client: reqwest::Client,
    server_url: String,
}

impl KenApiClient {
    /// Create a new client from enrolled credentials and server URL.
    ///
    /// Configures rustls with the Ken CA as the sole trusted root and
    /// the client certificate + key as the mTLS identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the certificates cannot be parsed or the
    /// reqwest client cannot be built.
    pub fn new(credentials: &EnrolledCredentials, server_url: &str) -> Result<Self, anyhow::Error> {
        let ca_cert = reqwest::Certificate::from_pem(credentials.ca_certificate_pem.as_bytes())
            .map_err(|e| anyhow::anyhow!("invalid CA certificate: {e}"))?;

        // reqwest::Identity wants a PEM bundle with cert + key.
        let identity_pem = format!(
            "{}\n{}",
            credentials.client_certificate_pem, credentials.client_private_key_pem,
        );
        let identity = reqwest::Identity::from_pem(identity_pem.as_bytes())
            .map_err(|e| anyhow::anyhow!("invalid client identity: {e}"))?;

        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .tls_built_in_root_certs(false)
            .add_root_certificate(ca_cert)
            .identity(identity)
            .timeout(Duration::from_secs(30))
            .user_agent(format!("ken-agent/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {e}"))?;

        Ok(Self {
            client,
            server_url: server_url.trim_end_matches('/').to_string(),
        })
    }

    /// Send a heartbeat to the server and receive an ack with pending
    /// commands.
    ///
    /// POSTs JSON to `{server_url}/api/v1/heartbeat`.
    pub async fn send_heartbeat(
        &self,
        heartbeat: &Heartbeat,
    ) -> Result<HeartbeatAck, anyhow::Error> {
        let url = format!("{}/api/v1/heartbeat", self.server_url);
        tracing::debug!(
            url = %url,
            endpoint_id = %heartbeat.endpoint_id,
            "sending heartbeat"
        );

        let response = self
            .client
            .post(&url)
            .json(heartbeat)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("heartbeat request failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("heartbeat returned {status}: {body}"));
        }

        let ack: HeartbeatAck = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("failed to parse heartbeat ack: {e}"))?;

        Ok(ack)
    }

    /// Report command outcomes to the server.
    ///
    /// POSTs JSON to `{server_url}/api/v1/command_outcomes` and expects
    /// 204 No Content.
    pub async fn report_command_outcomes(
        &self,
        outcomes: &[CommandOutcome],
    ) -> Result<(), anyhow::Error> {
        let url = format!("{}/api/v1/command_outcomes", self.server_url);
        tracing::debug!(count = outcomes.len(), "reporting command outcomes");

        let response = self
            .client
            .post(&url)
            .json(outcomes)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("command outcomes request failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "command outcomes returned {status}: {body}"
            ));
        }

        Ok(())
    }

    /// Retrieve the server's current time.
    ///
    /// GETs `{server_url}/api/v1/time` and parses the RFC 3339 timestamp.
    pub async fn server_time(&self) -> Result<time::OffsetDateTime, anyhow::Error> {
        let url = format!("{}/api/v1/time", self.server_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("time request failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!("time returned {status}"));
        }

        let body = response.text().await?;
        let ts = time::OffsetDateTime::parse(
            body.trim(),
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|e| anyhow::anyhow!("failed to parse server time: {e}"))?;

        Ok(ts)
    }

    /// Check for available updates.
    ///
    /// GETs `{server_url}/updates/latest.json` and parses the update info.
    pub async fn check_for_update(
        &self,
    ) -> Result<crate::updater::LatestUpdateInfo, anyhow::Error> {
        let url = format!("{}/updates/latest.json", self.server_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("update check failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!("update check returned {status}"));
        }

        let info: crate::updater::LatestUpdateInfo = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("failed to parse update info: {e}"))?;

        Ok(info)
    }

    /// Download an update MSI to the specified path.
    ///
    /// GETs the MSI URL and streams the response body to disk.
    pub async fn download_update(&self, msi_url: &str, dest: &Path) -> Result<(), anyhow::Error> {
        tracing::info!(url = %msi_url, dest = %dest.display(), "downloading update");

        let response = self
            .client
            .get(msi_url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("update download failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!("update download returned {status}"));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| anyhow::anyhow!("failed to read update body: {e}"))?;

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(dest, &bytes)?;

        tracing::info!(
            size = bytes.len(),
            dest = %dest.display(),
            "update downloaded"
        );

        Ok(())
    }
}
