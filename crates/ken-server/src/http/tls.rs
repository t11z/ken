//! TLS configuration and custom client certificate verifier for mTLS.
//!
//! The agent listener uses a custom `ClientCertVerifier` that validates
//! client certificates against the Ken root CA and checks enrollment
//! status in the database. See ADR-0008 for the full specification.

use std::sync::Arc;

use rustls::crypto::ring::default_provider;
use rustls::pki_types::{CertificateDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::server::WebPkiClientVerifier;
use rustls::{DistinguishedName, RootCertStore, SignatureScheme};

use ken_protocol::ids::EndpointId;

use crate::ca::Ca;
use crate::storage::Storage;

/// Custom client certificate verifier that checks the Ken CA chain
/// and verifies the endpoint is enrolled and not revoked.
///
/// This verifier is used on the agent listener only. The admin listener
/// uses server-only TLS with no client certificate requirement.
pub struct KenClientCertVerifier {
    storage: Storage,
    inner: Arc<dyn ClientCertVerifier>,
}

impl KenClientCertVerifier {
    /// Build a new verifier that trusts only the given Ken root CA
    /// and checks enrollment status via storage.
    ///
    /// # Errors
    ///
    /// Returns an error if the CA certificate cannot be parsed.
    pub fn new(storage: Storage, ca: &Ca) -> Result<Self, crate::error::AppError> {
        let mut root_store = RootCertStore::empty();
        let ca_pem = ca.root_certificate_pem();
        let ca_der = pem_to_der(ca_pem)?;
        root_store.add(ca_der).map_err(|e| {
            crate::error::AppError::Tls(format!("failed to add CA to root store: {e}"))
        })?;

        let inner = WebPkiClientVerifier::builder_with_provider(
            Arc::new(root_store),
            Arc::new(default_provider()),
        )
        .build()
        .map_err(|e| {
            crate::error::AppError::Tls(format!("failed to build WebPki verifier: {e}"))
        })?;

        Ok(Self { storage, inner })
    }
}

impl std::fmt::Debug for KenClientCertVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KenClientCertVerifier").finish()
    }
}

impl ClientCertVerifier for KenClientCertVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        self.inner.root_hint_subjects()
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        // Step 1: Delegate chain verification to the built-in WebPki verifier.
        self.inner
            .verify_client_cert(end_entity, intermediates, now)?;

        // Step 2: Extract the subject CN from the leaf certificate.
        let cn = extract_cn(end_entity).map_err(|_| {
            rustls::Error::General("failed to extract CN from client certificate".to_string())
        })?;

        // Step 3: Parse the CN as an EndpointId.
        let endpoint_id = EndpointId::parse(&cn).map_err(|_| {
            rustls::Error::General(format!(
                "client certificate CN is not a valid EndpointId: {cn}"
            ))
        })?;

        // Step 4: Check enrollment status in the database.
        // The rustls verifier trait is synchronous; wrap the async storage
        // call in block_on. This is safe because it runs once per TLS
        // handshake (not once per request) and at Ken's scale (~10
        // endpoints) this is negligible.
        let endpoint = tokio::runtime::Handle::current()
            .block_on(self.storage.get_endpoint(&endpoint_id))
            .map_err(|e| {
                rustls::Error::General(format!("database lookup failed during TLS handshake: {e}"))
            })?;

        // Step 5: Enrollment checks.
        let endpoint = endpoint.ok_or_else(|| {
            rustls::Error::General(format!("endpoint not enrolled: {endpoint_id}"))
        })?;

        if endpoint.revoked_at.is_some() {
            return Err(rustls::Error::General(format!(
                "endpoint revoked: {endpoint_id}"
            )));
        }

        // Check certificate expiry from the database record.
        let expires = time::OffsetDateTime::parse(
            &endpoint.certificate_expires_at,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|e| rustls::Error::General(format!("invalid cert expiry in database: {e}")))?;

        if time::OffsetDateTime::now_utc() > expires {
            return Err(rustls::Error::General(format!(
                "endpoint certificate expired: {endpoint_id}"
            )));
        }

        // Step 6: Success.
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }

    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }
}

/// Build a `rustls::ServerConfig` for a listener.
///
/// When `client_verifier` is `Some`, the config requires client certificates
/// (agent listener). When `None`, only server TLS is configured (admin listener).
///
/// # Errors
///
/// Returns an error if the server certificate or key cannot be parsed.
pub fn build_server_tls_config(
    server_cert_pem: &str,
    server_key_pem: &str,
    client_verifier: Option<Arc<dyn ClientCertVerifier>>,
) -> Result<rustls::ServerConfig, crate::error::AppError> {
    let cert_chain = pem_chain_to_der(server_cert_pem)?;
    let key = rustls::pki_types::PrivateKeyDer::try_from(pem_key_to_der(server_key_pem)?)
        .map_err(|e| crate::error::AppError::Tls(format!("invalid server private key: {e}")))?;

    let config = if let Some(verifier) = client_verifier {
        rustls::ServerConfig::builder_with_provider(Arc::new(default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| {
                crate::error::AppError::Tls(format!("failed to set protocol versions: {e}"))
            })?
            .with_client_cert_verifier(verifier)
            .with_single_cert(cert_chain, key)
            .map_err(|e| {
                crate::error::AppError::Tls(format!("failed to configure server cert: {e}"))
            })?
    } else {
        rustls::ServerConfig::builder_with_provider(Arc::new(default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| {
                crate::error::AppError::Tls(format!("failed to set protocol versions: {e}"))
            })?
            .with_no_client_auth()
            .with_single_cert(cert_chain, key)
            .map_err(|e| {
                crate::error::AppError::Tls(format!("failed to configure server cert: {e}"))
            })?
    };

    Ok(config)
}

/// Extract the Common Name (CN) from a DER-encoded certificate.
fn extract_cn(cert_der: &CertificateDer<'_>) -> Result<String, String> {
    let (_, cert) = x509_parser::parse_x509_certificate(cert_der.as_ref())
        .map_err(|e| format!("failed to parse X.509 certificate: {e}"))?;

    for rdn in cert.subject().iter() {
        for attr in rdn.iter() {
            if attr.attr_type() == &x509_parser::oid_registry::OID_X509_COMMON_NAME {
                return attr
                    .as_str()
                    .map(String::from)
                    .map_err(|e| format!("CN is not valid UTF-8: {e}"));
            }
        }
    }

    Err("no CN found in certificate subject".to_string())
}

/// Parse PEM certificate data to a single DER certificate.
fn pem_to_der(pem: &str) -> Result<CertificateDer<'static>, crate::error::AppError> {
    let certs = pem_chain_to_der(pem)?;
    certs
        .into_iter()
        .next()
        .ok_or_else(|| crate::error::AppError::Tls("no certificates found in PEM data".to_string()))
}

/// Parse PEM certificate chain data to DER certificates.
fn pem_chain_to_der(pem: &str) -> Result<Vec<CertificateDer<'static>>, crate::error::AppError> {
    let mut reader = std::io::BufReader::new(pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| crate::error::AppError::Tls(format!("failed to parse PEM certs: {e}")))?;

    if certs.is_empty() {
        return Err(crate::error::AppError::Tls(
            "no certificates found in PEM data".to_string(),
        ));
    }
    Ok(certs)
}

/// Parse a PEM private key to DER bytes.
fn pem_key_to_der(pem: &str) -> Result<Vec<u8>, crate::error::AppError> {
    let mut reader = std::io::BufReader::new(pem.as_bytes());

    // Try PKCS8 first, then RSA, then EC
    for item in std::iter::from_fn(|| rustls_pemfile::read_one(&mut reader).transpose()) {
        match item {
            Ok(rustls_pemfile::Item::Pkcs8Key(key)) => return Ok(key.secret_pkcs8_der().to_vec()),
            Ok(rustls_pemfile::Item::Pkcs1Key(key)) => {
                return Ok(key.secret_pkcs1_der().to_vec());
            }
            Ok(rustls_pemfile::Item::Sec1Key(key)) => return Ok(key.secret_sec1_der().to_vec()),
            Ok(_) => {}
            Err(e) => {
                return Err(crate::error::AppError::Tls(format!(
                    "failed to parse PEM key: {e}"
                )));
            }
        }
    }

    Err(crate::error::AppError::Tls(
        "no private key found in PEM data".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_cn_and_parse_pem_chain() {
        let ca = crate::ca::Ca::generate_ca_for_test();
        let endpoint_id = EndpointId::new();
        let signed = ca
            .sign_client_certificate(&endpoint_id, 365)
            .expect("sign should succeed");

        let chain = pem_chain_to_der(&signed.certificate_pem).expect("should parse");
        assert_eq!(chain.len(), 1);

        let cn = extract_cn(&chain[0]).expect("should extract CN");
        assert_eq!(cn, endpoint_id.to_string());
    }

    #[test]
    fn cn_extraction_fails_for_invalid_cert() {
        let bad_der = CertificateDer::from(vec![0u8; 10]);
        assert!(extract_cn(&bad_der).is_err());
    }
}
