# ADR-0004: Two-listener TLS architecture for the server

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

The Ken server has two distinct audiences: the family IT chief, who reaches the admin web UI through a browser, and the enrolled agents, which reach the API through mTLS-authenticated requests. These two audiences have incompatible TLS requirements. The admin browser cannot present a client certificate (browsers do not have one and would not know which one to present if they did). The agent must present its client certificate on every request, and the server must verify it against the database before accepting any heartbeat or command outcome.

A single TLS listener cannot serve both audiences cleanly. If the listener requires client certificates, the browser is locked out. If the listener does not require them, the agent's authentication becomes optional and the entire mTLS trust model collapses to a soft convention. The standard "require client cert on a path prefix" trick does not work either, because TLS client certificate verification happens at handshake time, before any HTTP path is parsed.

The decision is forced now because the server's `main.rs` currently binds plain `TcpListener` for both audiences with an honest comment that mTLS is deferred. That comment cannot stand as the long-term answer. Some structural decision is needed before the TLS layer is built.

## Decision

The Ken server runs **two TLS listeners on two distinct TCP ports**, each with its own `rustls::ServerConfig`. Both listeners use the same server certificate (issued by the Ken root CA at server first-start). They differ only in client certificate requirements:

- **Admin listener (default port 8444).** Server certificate only. No client certificate required. Serves the admin web UI, the enrollment endpoint (`POST /enroll/:token`), and the public download page for the one-time enrollment URL. The admin authenticates via a session cookie issued by the admin UI's own login flow, not via TLS.

- **Agent listener (default port 8443).** Server certificate plus required client certificate. The client certificate must be signed by the Ken root CA and the corresponding endpoint must be enrolled and not revoked in the database. A custom `rustls::server::danger::ClientCertVerifier` runs the cryptographic chain check and then queries the `Storage` handle for the endpoint's enrollment status. Failure at either check rejects the handshake before any HTTP request is processed. Serves only the agent API: heartbeat, command outcomes, server time.

Both ports are configurable in `ken.toml` via `server.admin_listen_address` and `server.agent_listen_address`. The default values are 8444 and 8443 respectively. Both default to binding all interfaces (`0.0.0.0`) so the server is reachable from the local network without further configuration.

The verified `EndpointId` from the client certificate is propagated into the agent listener's request handlers via a request extension, set by middleware that runs after TLS termination. Handlers extract it via an axum `Extension<EndpointId>` extractor.

## Consequences

**Easier:**
- Each listener has a single, simple TLS configuration. No conditional client cert verification, no per-path TLS magic, no need to convince a browser to behave like an mTLS client.
- The trust boundary between agents and admins is structurally enforced at the network layer, not just at the application layer. An admin browser literally cannot reach the agent API even if it tried, because the handshake would fail with "no client certificate provided".
- The custom client cert verifier runs once per handshake, not once per request, which is the cheapest place to enforce enrollment-status checks.
- Operationally, the two ports map cleanly to different firewall and logging postures. The admin port can be locked down to a single LAN subnet; the agent port can be left open within the home network.

**Harder:**
- Two ports to configure, two ports to expose in the Docker Compose example, two ports to remember when troubleshooting. The documentation has to explain the split clearly to family IT chiefs who may not have encountered split TLS before.
- The server binary holds two `axum::serve` futures in a `tokio::select!`. Either listener failing brings down the whole process, which is the right behavior but is worth knowing when reading the main loop.
- The CA's server certificate must include both ports' hostnames (or IP) in its SAN, or the SAN must be flexible enough to cover both. In practice the SAN covers the `public_url` hostname and `localhost`, and both listeners share that certificate, so this is automatic — but it is a constraint to remember when generating certificates.

**Accepted:**
- Two ports is mildly inconvenient compared to one. We accept the inconvenience because the alternative — collapsing the two audiences onto a single port — requires either weakening the agent's authentication or making the admin UI inaccessible from a normal browser. Neither is acceptable.
- Future expansions of the admin UI cannot reach the agent API directly via JavaScript (because the browser cannot mTLS into the agent listener). Any cross-cutting feature that needs both must be brokered server-side. This is the correct shape for Ken — it forces all admin actions to flow through code that runs under server-side authentication, not under whatever the browser happens to send.

## Alternatives considered

**Single TLS listener on one port, with optional client certs and per-path enforcement.** Rejected because rustls's client certificate verification runs at handshake time, before any HTTP path exists. Making the verification "optional" means the agent authentication is no longer enforced — the server has to trust HTTP-level claims instead, which collapses the entire mTLS model. There is no clean way to require client certs on `/api/*` and not on `/admin/*` within a single rustls listener.

**Use a reverse proxy (nginx, Caddy) in front of two upstreams.** Rejected because it adds an external component to a self-hosted single-binary deployment story. The whole point of shipping a single Rust server binary on a Raspberry Pi is to avoid the operational tax of running a separate proxy. Caddy's automatic HTTPS is appealing but irrelevant — Ken uses its own CA, not Let's Encrypt.

**Use TLS SNI to route to two virtual hosts on the same port.** Rejected because SNI-based routing still requires a proxy or a custom acceptor in front of the two `axum::serve` instances, which is roughly the same complexity as two ports without the operational clarity of two ports. Two ports are visible in `netstat`, in the firewall config, and in logs. SNI routing is invisible and harder to debug.

**Run the admin UI without TLS at all on `http://`.** Rejected because the admin UI carries the access token (and later, session cookies) in cleartext. Family IT chiefs who run the server on a home LAN may be tempted to skip TLS for convenience, but the server must not encourage this. TLS on the admin port is mandatory and uses the same self-signed cert the server already generates.
