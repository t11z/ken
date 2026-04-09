//! Types for the one-time agent enrollment exchange.
//!
//! Enrollment is the single moment when an agent joins a Ken deployment.
//! The family IT chief creates a one-time enrollment URL in the admin UI,
//! shares it with the family member, and the agent uses it to register
//! with the server and receive its mTLS credentials.
//!
//! See ADR-0001 T2-7: enrollment is always an explicit, manual act by
//! the family IT chief.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::ids::EndpointId;

/// The agent's enrollment request, sent to the server's one-time
/// enrollment endpoint.
///
/// Submitted as JSON to `POST /enroll/:token` on the admin listener.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnrollmentRequest {
    /// Protocol schema version the agent speaks.
    pub schema_version: u32,

    /// The one-time enrollment token from the URL.
    pub enrollment_token: String,

    /// Agent binary version (semver).
    pub agent_version: String,

    /// Windows version string (e.g., "Windows 11 24H2").
    pub os_version: String,

    /// The agent's local hostname, used for display only.
    pub hostname: String,

    /// When the agent initiated the enrollment request.
    #[serde(with = "time::serde::rfc3339")]
    pub requested_at: OffsetDateTime,
}

/// The server's enrollment response, containing the agent's mTLS
/// credentials and the CA certificate to trust.
///
/// The client private key travels in this response, which is a
/// deliberate trade-off: generating the key server-side allows a
/// single enrollment round-trip. This is acceptable because:
///
/// 1. The enrollment URL is single-use and short-lived (default 15 min).
/// 2. The enrollment channel is HTTPS-protected.
/// 3. The URL is delivered out-of-band on a trusted local channel
///    (the family IT chief gives it directly to the family member).
///
/// After enrollment, all future communication uses mTLS with the
/// credentials established here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnrollmentResponse {
    /// Server-assigned stable identifier for this endpoint.
    pub endpoint_id: EndpointId,

    /// PEM-encoded Ken root CA certificate for the agent to pin.
    pub ca_certificate_pem: String,

    /// PEM-encoded client certificate signed by the Ken CA.
    pub client_certificate_pem: String,

    /// PEM-encoded private key for the client certificate.
    pub client_private_key_pem: String,

    /// The server URL the agent should use for all future communication.
    pub server_url: String,

    /// When the credentials were issued.
    #[serde(with = "time::serde::rfc3339")]
    pub issued_at: OffsetDateTime,

    /// When the client certificate expires.
    #[serde(with = "time::serde::rfc3339")]
    pub certificate_expires_at: OffsetDateTime,
}
