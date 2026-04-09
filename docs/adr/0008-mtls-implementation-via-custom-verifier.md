# ADR-0008: mTLS implementation via rustls custom client verifier

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

ADR-0004 establishes that the Ken server runs two TLS listeners: an admin listener with server-cert-only TLS, and an agent listener that requires client certificates and verifies them against the Ken root CA and the database. The architectural shape of the split is settled. The remaining question is the *technical mechanism* for the agent listener: how exactly does rustls verify the client certificate, how does the verifier reach the database to check enrollment status, and how does the verified endpoint identity reach the request handlers.

This question is forced now because the server's `main.rs` currently runs both listeners as plain `tokio::net::TcpListener` with no TLS at all. The plain-HTTP state was honest as a Phase 1 starting point but is unacceptable as a long-term answer — every Tier 1 commitment in ADR-0001 about agent authentication and trust depends on this layer working. The TLS work cannot start until the implementation strategy is decided.

There are three realistic paths: use `axum-server` with its built-in rustls integration, write a custom TLS acceptor that sits between the TCP listener and axum, or front the whole thing with an external reverse proxy that terminates TLS. The third was already rejected in ADR-0004. The choice between the first two is what this ADR settles.

## Decision

The Ken agent listener uses **`axum-server` with `axum_server::tls_rustls::RustlsConfig`** as the TLS acceptor. The `RustlsConfig` is constructed from a manually-built `rustls::ServerConfig` that has a **custom `ClientCertVerifier`** implementing the Ken-specific enrollment check.

The custom verifier is a struct that holds an `Arc<Storage>` and an `Arc<Ca>`. It implements the `rustls::server::danger::ClientCertVerifier` trait. The `verify_client_cert` method:

1. Verifies the certificate chain against the Ken root CA loaded from `Ca`. If the chain is invalid, the handshake is rejected.
2. Extracts the subject Common Name from the leaf certificate. The CN is the `EndpointId` as a string. If parsing fails, the handshake is rejected.
3. Calls `Storage::get_endpoint(&endpoint_id)` to look up the endpoint. If the endpoint does not exist, the handshake is rejected.
4. Checks the endpoint's `revoked_at` field. If non-null, the endpoint is revoked and the handshake is rejected.
5. Checks the endpoint's certificate `expires_at` field against the current time. If expired, the handshake is rejected.
6. On success, returns `Ok(ClientCertVerified::assertion())`.

The verifier's `Storage` lookup is synchronous from rustls's perspective but the `Storage` methods are async. The verifier wraps the lookup in a `tokio::runtime::Handle::current().block_on(...)` call. This is acceptable because the verifier runs once per TLS handshake, not once per request, and TLS handshakes are infrequent enough relative to request volume that the brief blocking does not matter. If profiling later shows this is a bottleneck, the lookup can be replaced with an in-memory cache that is invalidated when endpoints are added, removed, or revoked.

The verified `EndpointId` is propagated into request handlers via a tower middleware that runs after TLS termination. The middleware reads the peer certificate from the connection extensions (axum-server makes the rustls peer info available) and inserts the parsed `EndpointId` into the request extensions. Handlers extract it via `Extension<EndpointId>`.

## Consequences

**Easier:**
- `axum-server` is a maintained crate specifically designed for TLS-terminated axum applications. It handles the rustls integration, the connection lifecycle, and the graceful shutdown. We do not write our own TLS acceptor or our own connection-loop machinery.
- The custom verifier is a small, focused, testable struct. Its only job is "is this client certificate valid for this Ken deployment". Unit tests construct certificates with `rcgen`, hand them to the verifier, and assert the expected accept/reject outcomes.
- Revocation is enforced at handshake time. A removed or revoked endpoint cannot establish a connection at all, regardless of whether its certificate is still cryptographically valid. There is no window where a revoked endpoint could continue making requests.
- The cryptographic chain check is performed by rustls itself, using its standard `WebPkiClientVerifier` as a foundation that the custom verifier wraps. We do not reimplement chain verification.

**Harder:**
- The `block_on` inside the verifier is mildly ugly. It exists because rustls's `ClientCertVerifier` trait is synchronous, but our database is async. The pattern is well-known and not actually problematic at Ken's scale, but it is the kind of code that future readers will look at and ask "is this safe?". The answer is yes, with a comment explaining why.
- `axum-server`'s peer-certificate extraction has changed shape between versions. The code that reads the verified `EndpointId` from the request extensions is coupled to `axum-server`'s public surface in a way that may need adjustment when upgrading. This is a known maintenance cost.
- Tests that exercise the full mTLS round-trip require constructing a client certificate signed by a test CA, configuring `reqwest` (or another HTTP client) to use it, and pointing it at an in-process server. This is several dozen lines of test setup per test. The integration test file `crates/ken-server/tests/http_api.rs` will grow accordingly.

**Accepted:**
- We are tying ourselves to `axum-server` as the TLS acceptor. If `axum-server` becomes unmaintained, we would need to switch to either `hyper-rustls` directly with our own connection loop, or to a different acceptor crate. Both are migrations of bounded scope, but they are real costs.
- The synchronous block in the verifier means each handshake briefly occupies a tokio worker thread for the duration of the database lookup. At Ken's scale this is invisible. At a hypothetical scale of thousands of concurrent enrollments, it would matter. We accept the limitation.

## Alternatives considered

**Write a custom TLS acceptor on top of `tokio_rustls::TlsAcceptor` and bridge to axum manually.** Rejected because it reinvents what `axum-server` already provides, and the connection-loop and shutdown-handling code that we would have to write is exactly the kind of non-domain-specific boilerplate that is expensive to get right and easy to get subtly wrong. The marginal flexibility of a custom acceptor is not worth the maintenance burden.

**Use `axum-server` with the built-in `WebPkiClientVerifier` only, and check enrollment status in handler middleware instead of the verifier.** Rejected because it weakens the trust model: a client with a still-cryptographically-valid certificate from a removed endpoint would be able to establish a TLS connection, and the rejection would happen at the application layer. This is observable behavior: the connection succeeds, the request is parsed, the database is queried, and only then the request is rejected. The handshake-time rejection in the chosen design closes that gap completely — the removed endpoint cannot even connect.

**Use a JWT or session token in the agent's HTTP requests instead of mTLS.** Rejected because tokens require their own issuance, rotation, and revocation infrastructure, and they shift the burden of agent authentication from the cryptographic layer to the application layer. mTLS is the simpler and more rigorous design for a single-tenant deployment where the server already runs its own CA. ADR-0004 already commits to mTLS structurally; this ADR is only about the implementation mechanism.

**Use an external reverse proxy (nginx, Caddy) for TLS termination and pass the verified client cert info to axum via headers.** Already rejected in ADR-0004 for the operational reason that it adds an external component to a single-binary deployment story.
