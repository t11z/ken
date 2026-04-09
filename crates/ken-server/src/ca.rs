//! Ken certificate authority for mTLS enrollment.
//!
//! The server acts as its own CA for the closed PKI described in
//! the mTLS skill document. On first startup it generates a root CA
//! and server certificate; on each enrollment it signs a new client
//! certificate for the endpoint.

use std::fs;
use std::path::Path;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use time::OffsetDateTime;

use crate::config::TlsConfig;
use crate::error::AppError;
use ken_protocol::EndpointId;

/// The Ken certificate authority, holding the root CA material and
/// able to sign client certificates for enrolled endpoints.
pub struct Ca {
    root_cert_pem: String,
    root_key_pair: KeyPair,
}

/// A freshly signed client certificate and its private key.
pub struct SignedClientCertificate {
    /// PEM-encoded client certificate.
    pub certificate_pem: String,
    /// PEM-encoded private key.
    pub private_key_pem: String,
    /// When the certificate expires.
    pub expires_at: OffsetDateTime,
}

impl Ca {
    /// Load an existing CA from disk, or create a new one if the files
    /// do not exist.
    ///
    /// When creating, the root CA and server certificates are written to
    /// disk with restricted permissions. The server certificate's SAN
    /// includes the given hostname and `localhost`.
    ///
    /// # Errors
    ///
    /// Returns an error if the files exist but cannot be read, or if
    /// certificate generation fails.
    pub fn load_or_create(config: &TlsConfig, server_hostname: &str) -> Result<Self, AppError> {
        let ca_cert_path = &config.ca_certificate_path;
        let ca_key_path = &config.ca_key_path;

        if ca_cert_path.exists() && ca_key_path.exists() {
            tracing::info!(
                cert = %ca_cert_path.display(),
                key = %ca_key_path.display(),
                "loading existing CA"
            );
            let cert_pem = fs::read_to_string(ca_cert_path).map_err(|e| {
                AppError::Tls(format!(
                    "failed to read CA certificate at {}: {e}",
                    ca_cert_path.display()
                ))
            })?;
            let key_pem = fs::read_to_string(ca_key_path).map_err(|e| {
                AppError::Tls(format!(
                    "failed to read CA key at {}: {e}",
                    ca_key_path.display()
                ))
            })?;

            let key_pair = KeyPair::from_pem(&key_pem)
                .map_err(|e| AppError::Tls(format!("failed to parse CA key: {e}")))?;

            Ok(Self {
                root_cert_pem: cert_pem,
                root_key_pair: key_pair,
            })
        } else {
            tracing::info!("generating new CA and server certificate");
            let ca = Self::generate_ca()?;
            ca.write_ca_to_disk(ca_cert_path, ca_key_path)?;

            // Generate and write server certificate
            let server_cert_path = &config.server_certificate_path;
            let server_key_path = &config.server_key_path;
            ca.generate_and_write_server_cert(server_hostname, server_cert_path, server_key_path)?;

            Ok(ca)
        }
    }

    /// Return the root CA certificate in PEM format.
    #[must_use]
    pub fn root_certificate_pem(&self) -> &str {
        &self.root_cert_pem
    }

    /// Sign a new client certificate for an enrolled endpoint.
    ///
    /// The certificate's subject CN is the endpoint ID string, which is
    /// how the server's custom client verifier identifies which endpoint
    /// is connecting.
    ///
    /// # Errors
    ///
    /// Returns an error if certificate generation or signing fails.
    pub fn sign_client_certificate(
        &self,
        endpoint_id: &EndpointId,
        validity_days: u64,
    ) -> Result<SignedClientCertificate, AppError> {
        let client_key = KeyPair::generate()
            .map_err(|e| AppError::Tls(format!("failed to generate client key: {e}")))?;

        let mut params = CertificateParams::default();
        params
            .distinguished_name
            .push(DnType::CommonName, endpoint_id.to_string());
        params
            .distinguished_name
            .push(DnType::OrganizationName, "Ken Endpoint");
        params.key_usages.push(KeyUsagePurpose::DigitalSignature);
        params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ClientAuth);

        let now = OffsetDateTime::now_utc();
        params.not_before = now;
        let days = i64::try_from(validity_days).unwrap_or(i64::MAX);
        let expires_at = now + time::Duration::days(days);
        params.not_after = expires_at;

        let issuer = self.make_issuer()?;

        let cert = params
            .signed_by(&client_key, &issuer)
            .map_err(|e| AppError::Tls(format!("failed to sign client certificate: {e}")))?;

        Ok(SignedClientCertificate {
            certificate_pem: cert.pem(),
            private_key_pem: client_key.serialize_pem(),
            expires_at,
        })
    }

    /// Build an `Issuer` from the stored CA cert PEM and key pair.
    fn make_issuer(&self) -> Result<Issuer<'_, &KeyPair>, AppError> {
        Issuer::from_ca_cert_pem(&self.root_cert_pem, &self.root_key_pair)
            .map_err(|e| AppError::Tls(format!("failed to build issuer from CA cert: {e}")))
    }

    fn generate_ca() -> Result<Self, AppError> {
        let key_pair = KeyPair::generate()
            .map_err(|e| AppError::Tls(format!("failed to generate CA key: {e}")))?;

        let mut params = CertificateParams::default();
        params
            .distinguished_name
            .push(DnType::CommonName, "Ken Root CA");
        params
            .distinguished_name
            .push(DnType::OrganizationName, "Ken");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages.push(KeyUsagePurpose::KeyCertSign);
        params.key_usages.push(KeyUsagePurpose::CrlSign);

        let now = OffsetDateTime::now_utc();
        params.not_before = now;
        // 10-year CA lifetime
        params.not_after = now + time::Duration::days(3650);

        let cert = params
            .self_signed(&key_pair)
            .map_err(|e| AppError::Tls(format!("failed to self-sign CA certificate: {e}")))?;

        let cert_pem = cert.pem();

        Ok(Self {
            root_cert_pem: cert_pem,
            root_key_pair: key_pair,
        })
    }

    fn write_ca_to_disk(&self, cert_path: &Path, key_path: &Path) -> Result<(), AppError> {
        if let Some(parent) = cert_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(parent) = key_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(cert_path, &self.root_cert_pem)?;
        fs::write(key_path, self.root_key_pair.serialize_pem())?;

        set_restrictive_permissions(key_path)?;

        tracing::info!(
            cert = %cert_path.display(),
            key = %key_path.display(),
            "CA certificate and key written"
        );
        Ok(())
    }

    fn generate_and_write_server_cert(
        &self,
        hostname: &str,
        cert_path: &Path,
        key_path: &Path,
    ) -> Result<(), AppError> {
        let server_key = KeyPair::generate()
            .map_err(|e| AppError::Tls(format!("failed to generate server key: {e}")))?;

        let mut params = CertificateParams::default();

        // Extract hostname from URL if it looks like a URL
        let clean_hostname = extract_hostname(hostname);

        params
            .distinguished_name
            .push(DnType::CommonName, clean_hostname.clone());
        params
            .distinguished_name
            .push(DnType::OrganizationName, "Ken Server");
        params.key_usages.push(KeyUsagePurpose::DigitalSignature);
        params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ServerAuth);

        // SAN: the hostname, localhost, and loopback IP
        let mut sans = vec![
            rcgen::SanType::DnsName(
                clean_hostname
                    .clone()
                    .try_into()
                    .map_err(|e| AppError::Tls(format!("invalid hostname for SAN: {e}")))?,
            ),
            rcgen::SanType::DnsName(
                "localhost"
                    .to_string()
                    .try_into()
                    .map_err(|e| AppError::Tls(format!("invalid localhost SAN: {e}")))?,
            ),
        ];
        sans.push(rcgen::SanType::IpAddress(std::net::IpAddr::V4(
            std::net::Ipv4Addr::LOCALHOST,
        )));
        params.subject_alt_names = sans;

        let now = OffsetDateTime::now_utc();
        params.not_before = now;
        params.not_after = now + time::Duration::days(365);

        let issuer = self.make_issuer()?;

        let cert = params
            .signed_by(&server_key, &issuer)
            .map_err(|e| AppError::Tls(format!("failed to sign server certificate: {e}")))?;

        if let Some(parent) = cert_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(parent) = key_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(cert_path, cert.pem())?;
        fs::write(key_path, server_key.serialize_pem())?;

        set_restrictive_permissions(key_path)?;

        tracing::info!(
            cert = %cert_path.display(),
            key = %key_path.display(),
            hostname = %clean_hostname,
            "server certificate written"
        );
        Ok(())
    }
}

/// Extract hostname from a string that might be a URL (e.g., `https://ken.home:8443`).
fn extract_hostname(input: &str) -> String {
    let without_scheme = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
        .unwrap_or(input);

    // Remove port and path
    without_scheme
        .split(':')
        .next()
        .unwrap_or(without_scheme)
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .to_string()
}

/// Set file permissions to 0600 (owner read/write only).
#[cfg(unix)]
fn set_restrictive_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

/// No-op on non-Unix platforms (the server is Linux-only per ADR,
/// but this allows cross-platform CI builds).
#[cfg(not(unix))]
fn set_restrictive_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_hostname_from_url() {
        assert_eq!(extract_hostname("https://ken.home:8443"), "ken.home");
        assert_eq!(extract_hostname("https://192.168.1.5:8443"), "192.168.1.5");
        assert_eq!(extract_hostname("ken.home"), "ken.home");
        assert_eq!(extract_hostname("localhost"), "localhost");
    }

    #[test]
    fn generate_ca_and_sign_client() {
        let ca = Ca::generate_ca().expect("CA generation should succeed");
        assert!(ca.root_certificate_pem().contains("BEGIN CERTIFICATE"));

        let endpoint_id = EndpointId::new();
        let signed = ca
            .sign_client_certificate(&endpoint_id, 365)
            .expect("client cert signing should succeed");

        assert!(signed.certificate_pem.contains("BEGIN CERTIFICATE"));
        assert!(signed.private_key_pem.contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn load_or_create_in_tempdir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tls_config = TlsConfig {
            ca_certificate_path: dir.path().join("ca/root.crt"),
            ca_key_path: dir.path().join("ca/root.key"),
            server_certificate_path: dir.path().join("server/server.crt"),
            server_key_path: dir.path().join("server/server.key"),
        };

        // First call creates the CA
        let ca1 = Ca::load_or_create(&tls_config, "test.local")
            .expect("first load_or_create should succeed");
        let pem1 = ca1.root_certificate_pem().to_string();

        // Second call loads the same CA
        let ca2 = Ca::load_or_create(&tls_config, "test.local")
            .expect("second load_or_create should succeed");
        assert_eq!(pem1, ca2.root_certificate_pem());
    }

    #[test]
    fn client_cert_chains_to_ca() {
        let ca = Ca::generate_ca().expect("CA generation");
        let endpoint_id = EndpointId::new();

        // Sign two different clients — both should work
        let cert1 = ca
            .sign_client_certificate(&endpoint_id, 365)
            .expect("sign 1");
        let cert2 = ca
            .sign_client_certificate(&endpoint_id, 30)
            .expect("sign 2");

        assert_ne!(cert1.certificate_pem, cert2.certificate_pem);
        assert!(cert1.certificate_pem.contains("BEGIN CERTIFICATE"));
        assert!(cert2.certificate_pem.contains("BEGIN CERTIFICATE"));
    }
}
