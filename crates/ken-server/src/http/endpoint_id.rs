//! Middleware to bridge the verified `EndpointId` from the mTLS
//! connection into request extensions.
//!
//! The `KenAcceptor` in `tls.rs` wraps each connection's service in
//! `AddEndpointId<S>`, which stores the `EndpointId` extracted from
//! the peer certificate. On every HTTP request, `AddEndpointId`
//! inserts the `EndpointId` into the request extensions so handlers
//! can use `Extension<EndpointId>`.
//!
//! The `require_endpoint_id` middleware is mounted on the agent router
//! as defense-in-depth: if a request somehow arrives without the
//! extension (indicating a wiring bug where the agent router was
//! served without `KenAcceptor`), the middleware returns 500 and logs
//! the condition as a server bug.
//!
//! See ADR-0008 for the verifier design, ADR-0016 for the single-source
//! identity rule, and ADR-0017 for the bridge architecture.

use std::task::{Context, Poll};

use axum::extract::Request;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use ken_protocol::ids::EndpointId;

/// A tower service wrapper that injects a connection-level `EndpointId`
/// into every request's extensions.
///
/// Created by `KenAcceptor` after the TLS handshake completes. The
/// `EndpointId` is parsed from the verified peer certificate's CN and
/// lives for the duration of the connection. Each request on the
/// connection receives a clone in its extensions.
///
/// This type contains no parsing logic and performs no security checks.
/// It is purely a value-routing layer between the acceptor (which
/// extracts the identity) and the handlers (which consume it).
#[derive(Clone, Debug)]
pub struct AddEndpointId<S> {
    inner: S,
    endpoint_id: EndpointId,
}

impl<S> AddEndpointId<S> {
    /// Wrap a service with a verified `EndpointId` that will be
    /// injected into every request's extensions.
    pub fn new(inner: S, endpoint_id: EndpointId) -> Self {
        Self { inner, endpoint_id }
    }
}

impl<S, B> tower::Service<axum::http::Request<B>> for AddEndpointId<S>
where
    S: tower::Service<axum::http::Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: axum::http::Request<B>) -> Self::Future {
        req.extensions_mut().insert(self.endpoint_id);
        self.inner.call(req)
    }
}

/// Middleware that verifies every request on the agent listener carries
/// a verified `EndpointId` in its extensions.
///
/// Under correct wiring (agent listener served via `KenAcceptor`), the
/// `EndpointId` is always present because `AddEndpointId` inserts it
/// for every request on the connection. This middleware exists as
/// defense-in-depth: if the extension is absent, it means the acceptor
/// and verifier are unsynchronized or the agent router was mounted on
/// the wrong listener — both are server bugs that must surface loudly.
///
/// Mount this on the agent router via `axum::middleware::from_fn`.
pub async fn require_endpoint_id(request: Request, next: axum::middleware::Next) -> Response {
    if request.extensions().get::<EndpointId>().is_none() {
        tracing::error!(
            "agent listener received a request without a verified EndpointId \
             — this indicates the acceptor and verifier are unsynchronized"
        );
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    next.run(request).await
}
