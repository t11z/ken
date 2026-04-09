# mTLS with rustls

Load this skill when working on the mTLS layer in either `ken-server` or `ken-agent`. It covers how Ken issues its own certificates, how both sides authenticate, and the concrete `rustls` patterns to use.

## The trust model

Ken operates as a closed PKI: the server is its own certificate authority, issues certificates to enrolled agents, and is the only thing agents trust. There is no public CA in the loop. This is correct for Ken because:

- There is no public discovery — the agent is told at install time exactly which server to trust, and that server's root certificate is pinned
- A public CA would add nothing (the server has no public DNS name, no third-party audience) and would introduce a trust dependency on a third party
- Certificate issuance and revocation are operations the family IT chief already performs when enrolling or removing endpoints; making them part of Ken rather than delegating them keeps the whole trust story in one place

The implication is that the Ken server is both a TLS server and a CA. These are different concerns and must be kept separate in the code. The TLS server uses `rustls` to terminate connections; the CA is a separate module that handles certificate generation, signing, and persistence.

## Certificate generation and storage

On first startup, the server generates:

- A root CA key pair and self-signed certificate (long-lived, e.g., 10 years)
- A server certificate signed by the root CA, with a SAN for the server's configured hostname or IP

Both are stored in the configured data directory with strict file permissions (0600 for the key, 0644 for the certificate). The root CA private key is the crown jewel of the deployment — if it leaks, the entire trust chain collapses. Treat it accordingly: never log it, never expose it through the admin API, never include it in any export.

Use the `rcgen` crate for certificate generation. `rcgen` is maintained, supports the features Ken needs (custom subject, SAN, key usage, extended key usage), and produces DER output that `rustls` can consume directly.

## Enrollment flow

When the family IT chief enrolls a new endpoint, the server:

1. Generates a new key pair for the endpoint
2. Builds a certificate signing request (CSR) on the server side (since the server controls both ends)
3. Signs the CSR with the root CA, producing an endpoint certificate
4. Bundles the endpoint certificate, its private key, and the root CA certificate into an enrollment package
5. Delivers the package to the endpoint through a one-time, short-lived enrollment URL that is shown in the admin UI

The endpoint retrieves the package, extracts it to the agent's configuration directory with strict permissions, and begins using the certificate for all subsequent communication. The one-time URL is invalidated after first use.

The enrollment URL is the only part of the flow that is not mTLS-protected, because the agent does not yet have a certificate. It uses a one-time token in the URL path plus HTTPS with the server's own certificate (trusted because the admin manually transcribes it or uses a short-lived LAN connection). This is the weakest point in the trust chain and should be treated as such: the token is single-use, the URL expires quickly, and the admin is instructed to perform enrollment on a trusted local network.

## rustls server setup

The server uses `rustls` with client certificate verification required:

```rust
let server_config = rustls::ServerConfig::builder()
    .with_client_cert_verifier(Arc::new(ken_client_verifier))
    .with_single_cert(server_cert_chain, server_key)?;
```

The `ken_client_verifier` is a custom `ClientCertVerifier` implementation that:

1. Verifies the client certificate chain against the Ken root CA
2. Extracts the endpoint ID from the certificate's common name or subject alternative name
3. Checks the endpoint is still enrolled in the database (revocation check)
4. Returns the verified endpoint ID for use by the handler

The custom verifier is important because `rustls`'s built-in verifier does not know about Ken's database. Every request must pass both the cryptographic check (signature chain) and the enrollment check (is this endpoint still admitted?). An endpoint removed from the database must be immediately unable to connect, even if its certificate is still cryptographically valid.

## rustls client setup (agent)

The agent uses `rustls` as a client:

```rust
let root_store = rustls::RootCertStore::empty();
root_store.add(&ken_root_ca)?;

let client_config = rustls::ClientConfig::builder()
    .with_root_certificates(root_store)
    .with_client_auth_cert(endpoint_cert_chain, endpoint_key)?;
```

The root store contains only the Ken root CA certificate. The agent never trusts any other CA, public or private. If the server presents a certificate not signed by the Ken root CA, the TLS handshake fails. This is by design per ADR-0001 T1-1.

The agent's own certificate and key are loaded from the enrollment package at startup. They are stored on disk with strict permissions and never logged.

## Revocation

Ken does not use CRLs or OCSP. Revocation is enforced at the server by the custom client verifier: when the admin removes an endpoint from the database, the next connection from that endpoint is rejected at the verifier step.

This works because the server is the only thing that verifies agent certificates. There is no third party that needs to check revocation. The database row is the authoritative revocation list.

Rotation is the mechanism for dealing with compromised agent keys: revoke the old endpoint, re-enroll with a new certificate. The old certificate becomes unusable the moment the database row is removed, even if the certificate is still within its validity period.

## Cipher suites and TLS version

TLS 1.3 only. Explicitly disable TLS 1.2 and below in the `rustls` configuration. TLS 1.3 is mandatory in all modern browsers and in `rustls` by default; forcing it removes a whole class of downgrade concerns.

Cipher suites are the `rustls` defaults. Do not attempt to customize them — the defaults are conservative, actively maintained, and appropriate for Ken's threat model.

## Key rotation

The root CA key is long-lived (10 years) and is not rotated in normal operation. Rotating it would require re-enrolling every endpoint, which is operationally expensive.

Server and agent certificates are rotated on a shorter schedule (a year by default, configurable). The server automatically generates new server certificates before expiry and hot-reloads them. Agent certificates are rotated by the admin explicitly through the admin UI; this causes the agent to re-enroll on its next connection attempt with a new certificate provided by the server.

## What to avoid

- **Self-signed server certificates the agent accepts because "just this once."** Either the certificate is signed by the Ken root CA or the handshake fails. There is no middle ground.
- **Storing keys in version control.** Obvious but worth stating.
- **Logging certificates or keys.** Log the subject and fingerprint if you need to debug, never the material itself.
- **Trusting any CA other than the Ken root CA.** The agent's trust store is one certificate, not a bundle.
- **Shipping test certificates in the release build.** Tests use in-memory certificates generated per test. Production certificates come from the server's own CA at deployment time.
- **Disabling certificate verification "temporarily."** If a test or a development workflow needs to disable verification, it does not need mTLS at all — use plain HTTP in dev and add an explicit feature flag that panics in release builds if enabled.
