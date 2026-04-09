//! Middleware to extract the authenticated `EndpointId` from the mTLS
//! client certificate and inject it into request extensions.
//!
//! Per ADR-0008, the agent listener requires client certificates. The
//! `KenClientCertVerifier` validates the certificate chain and checks
//! enrollment status during the TLS handshake. This middleware extracts
//! the verified `EndpointId` from the peer certificate so handlers can
//! access it via `Extension<EndpointId>`.
//!
//! # Current limitation
//!
//! `axum-server` 0.8 does not expose peer certificates in request
//! extensions. Until upstream support is added or we switch to a
//! different TLS integration layer, the `EndpointId` in the heartbeat
//! body is trusted after the mTLS handshake succeeds. The handshake
//! itself validates the CN against enrolled endpoints, so an
//! unenrolled or revoked agent cannot reach the handler at all.
//!
//! A future enhancement (tracked below) will add the cross-check:
//! the handler will verify that `heartbeat.endpoint_id` matches the
//! CN extracted from the peer certificate, preventing an enrolled
//! agent from impersonating another enrolled agent.

// TODO(#5): Add peer cert extraction when axum-server exposes
// client certificates in request extensions, or migrate to a TLS
// integration layer that does.
//
// The cross-check logic would be:
//
// ```ignore
// pub async fn verify_endpoint_id(
//     Extension(verified_id): Extension<EndpointId>,
//     Json(heartbeat): Json<Heartbeat>,
// ) -> Result<..., AppError> {
//     if heartbeat.endpoint_id != verified_id {
//         return Err(AppError::Forbidden("endpoint ID mismatch"));
//     }
//     // ...
// }
// ```
