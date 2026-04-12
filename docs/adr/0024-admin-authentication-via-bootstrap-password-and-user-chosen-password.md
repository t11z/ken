# ADR-0024: Admin authentication via bootstrap password and user-chosen password

- **Status:** Accepted
- **Date:** 2026-04-12
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

ADR-0004 settles the shape of admin access: the admin listener serves the UI over server-cert-only TLS, and the admin authenticates via a session cookie issued by the UI's own login flow rather than via mTLS. What ADR-0004 deliberately leaves open is the mechanism that seeds that login flow on a fresh server — the "later" in the Alternatives note that the admin UI "carries the access token (and later, session cookies)." The Ken server currently has no Accepted decision on how an admin first authenticates against an empty database, and the `docs/user/install.md` text describing a log-printed access token is an undocumented choice rather than a ratified one.

The problem is symmetric to the agent enrollment story that ADR-0001 T2-7 and the `mtls-with-rustls` skill already treat explicitly: both flows need a way to hand the first credential across a boundary before a stronger credential exists, both are single-use and short-lived by design, and both rely on the deployment being on a trusted local network at that moment. The agent side is settled; the admin side is not, and the first real server run will implicitly settle it if this ADR does not.

The decision is forced now because the server has not yet run successfully once. Any implementation work on the admin login path — current code, test code, or the first production deployment — would otherwise write the decision into behavior before it is recorded in an ADR.

## Decision

The Ken server authenticates the admin through a two-stage model.

**Stage 1 — Bootstrap password.** On first startup, when no admin password is set in the database, the server generates a cryptographically random bootstrap password and writes it to the process log at startup, clearly marked. The admin retrieves it from the log (`docker compose logs ken-server | grep` or equivalent) and uses it once to log in. The bootstrap password is consumed on first successful login; further attempts to use it are rejected.

**Stage 2 — User-chosen password.** Immediately after the first successful bootstrap login, the admin UI requires the admin to set a permanent password before any other action is permitted. From that point on, all admin logins use that password, and the server issues a session cookie valid until the admin explicitly logs out, until the session expires, or until the password is reset. The session cookie is the credential presented on every subsequent request.

**Recovery.** If the admin loses access to their session and cannot authenticate with the user-chosen password, recovery is a shell-side operation at the Raspberry Pi: `ken-server admin reset-password` invalidates the stored password and all active sessions, generates a new bootstrap password, and writes it to the log. The next login proceeds as a fresh first-login, with Stage 2 reoccurring.

**User model.** Ken recognizes exactly one admin identity. The concepts of multiple admin users, role differentiation, or shared accounts are out of scope. Expanding the user model would require its own future ADR.

Session persistence follows ADR-0005: sessions live in the server's SQLite database, which makes both session expiry and on-demand revocation at reset time straightforward.

## Consequences

**Easier:**
- The first-login path has no configuration: the admin starts the server and reads one log line. No environment variables at compose time, no pre-deployment file staging, no shell-exec into a running container for account setup. This matches the "start and go" expectation the README sets for the Pi deployment.
- The long-lived credential is a password the admin chose and that their password manager can hold. Admins who work in incognito sessions or across multiple devices re-authenticate with a credential they control, not one they had to retrieve from a log a second time.
- Recovery uses the same mechanism as first-login. One code path, two use cases. The operator's mental model is "if I have shell access to the Pi, I have a way back in," which matches the physical-trust assumption underlying the entire self-hosted deployment.
- Session invalidation at password reset is exact and cheap: a `DELETE` against the sessions table. No key rotation, no grace period, no partial revocation.

**Harder:**
- The bootstrap password is exposed through the server log at the moment it is generated. Any process or human with read access to the container's stdout in that window sees the password. In practice the window is seconds to minutes and the password is immediately replaced by a user-chosen one, but the exposure is real and documented rather than denied.
- An admin who ignores the password log line and never logs in leaves the bootstrap password sitting in the log and in the database indefinitely. The server does not time it out on its own; the only invalidation trigger is successful first login or explicit reset. Operators who do not complete first login promptly accept a longer exposure window.
- Admin recovery requires shell access to the Pi. An admin who has lost both their session and their SSH access to their own server has no further in-band path. This is accepted: Ken's trust model already assumes the operator controls the hardware.

**Accepted:**
- Log-printed credentials are a well-worn pattern in the self-hosted ecosystem (Rancher, Vaultwarden, many smaller projects use the same approach). Admins in Ken's target audience are familiar with it. The objection that logs are a "bad credential channel" is weakened by the credential being transient rather than long-lived.
- The single-admin model excludes legitimate future scenarios (two family members equally comfortable administering the server). Those scenarios are not being denied, only not being addressed in this ADR. A later ADR can introduce multi-admin support cleanly because the storage layer and session model are already relational.
- No passwordless option (magic link, TOTP-only, device binding) is offered. The design choice is uniformity and familiarity over novelty; a family IT chief should not have to learn a Ken-specific auth mental model to use the tool.

## Alternatives considered

**Bootstrap via environment variables set at compose time** (the Keycloak and Authentik pattern: `KC_BOOTSTRAP_ADMIN_PASSWORD` / `AUTHENTIK_BOOTSTRAP_PASSWORD`). Rejected because it forces the admin to choose and handle a password before the server has even started, and then either leave it committed in the compose file or remove it after first boot. Both outcomes are worse than a transient log entry: the first leaves the credential in version control or in filesystem backups, the second requires extra operator discipline that the log-printed pattern does not require.

**Bootstrap via a one-time URL file written to the data directory** (the shape the README originally implied, before this ADR). Rejected because it adds filesystem-permission handling inside the container (UID mapping, volume mount sharing with the host), and because the user-flow advantage over the log pattern — clicking a URL versus copying a password — is small once Stage 2 requires a real password anyway. The additional complexity would not pay for itself.

**Bootstrap via an explicit CLI subcommand** (`ken-server admin init`, the Passbolt pattern). Rejected as the default first-login mechanism because it breaks the "starting the server does everything needed" expectation of a Docker Compose deployment, and because asking the admin to exec a command inside a container before anything works adds friction for a tool that otherwise aims to be undramatic to install. The same CLI shape is kept for the recovery path, where the explicit-intent property is actually desirable.

**Skip the user-chosen password stage; keep the bootstrap password as the permanent credential.** Rejected because the bootstrap password's only affordance is that it exists once in the log. Reusing it as the permanent credential means either the admin stores it in a password manager manually — in which case a user-chosen password would have been fine and clearer — or the admin loses it and has to reset, which treats every login as a first-login. The two-stage model is worth the small extra UI surface.

**Admin-only mTLS instead of a password.** Rejected because browsers cannot present client certificates without significant operator friction (certificate import into the OS or browser trust store, per-device provisioning), and because the whole point of ADR-0004 splitting the admin listener off the agent listener was to keep the admin path on a standard browser flow. Reintroducing client certificates on the admin side would defeat that split.
