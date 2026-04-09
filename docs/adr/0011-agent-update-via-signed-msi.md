# ADR-0011: Agent update mechanism via signed MSI and msiexec

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

The Ken agent runs as a SYSTEM service on Windows endpoints distributed across many family members' homes. When a new agent version is released, the running agents need to update themselves to it: discover that an update exists, download it, verify its authenticity, install it, restart the service, and confirm the new version is healthy. Doing this wrong has visible consequences — a botched update breaks the agent in a place where the family IT chief cannot reach it, and an unverified update is the obvious channel for someone to push malicious code onto every machine in the deployment.

The realistic update mechanisms for a Windows service binary are: a custom updater that downloads a new `.exe` and replaces the running one, an MSI-based update via `msiexec`, an MSIX package via the Microsoft Store or sideload, or a third-party updater framework like Squirrel.Windows or WinSparkle. Each has trade-offs for atomicity, signing, rollback, and integration with the Windows service control manager.

The decision is forced now because Phase 1 includes a stub update checker that needs to know what shape the actual install action will take, and because the MSI build pipeline (Phase 2) cannot be set up without committing to MSI as the format.

## Decision

The Ken agent updates itself by **downloading a signed MSI from the Ken server, verifying the Authenticode signature, and invoking `msiexec /i <path> /quiet /norestart` at a configurable maintenance window**.

The flow is:

1. The agent's update checker periodically (default: once every 24 hours) requests `GET /updates/latest.json` from the Ken server. The endpoint lives on the agent listener, so the request is mTLS-authenticated like every other agent-server interaction.
2. The response is a small JSON document: `{"version": "0.2.0", "url": "https://server/updates/ken-agent-0.2.0.msi", "signature_thumbprint": "..."}`. The version field is parsed as semver. If the version is not strictly greater than the running agent's version, the update check ends here.
3. If a newer version is available, the agent downloads the MSI to `%ProgramData%\Ken\updates\incoming.msi` over the same mTLS channel.
4. The agent verifies the MSI's Authenticode signature via `WinVerifyTrust` (called through `windows-rs`), checking that the signature is valid, that the certificate chains to a trusted root, and that the signing certificate's thumbprint matches the expected value from the JSON response.
5. If verification fails at any step, the downloaded file is deleted, the failure is logged to the local audit log, and the failure is reported to the server in the next heartbeat. No installation is attempted.
6. If verification succeeds, the agent schedules the install for the next maintenance window. The maintenance window is configured in the agent's config file (default: between 03:00 and 05:00 local time). At the maintenance window, the agent invokes `msiexec /i %ProgramData%\Ken\updates\incoming.msi /quiet /norestart` via `std::process::Command`.
7. `msiexec` handles the rest: stopping the running service, replacing the binary, registering the new binary with the SCM, starting the new service. If the new service fails to start, `msiexec` rolls back automatically, leaving the old binary in place.
8. The new agent (or the old, if rollback occurred) reports the version it is running in its next heartbeat. The server compares against the expected version and surfaces any discrepancy in the admin UI.

The Ken agent does **not** write its own updater that manipulates the running `.exe` file. The Ken agent does **not** attempt to bypass the maintenance window for "urgent" updates. The Ken agent does **not** auto-update on user logoff or any other "convenient" trigger that could surprise the user.

For Phase 1, the update checker is implemented as a stub that always reports "no update available" — the actual MSI build and signing is Phase 2 work. The trait boundary is in place so that the Phase 2 work is additive.

## Consequences

**Easier:**
- `msiexec` is a part of every Windows installation, has been the standard installer mechanism for decades, and is trusted by Windows itself. Atomic replacement of a running binary, rollback on failure, service registration, uninstall — all of these are handled by `msiexec`, not by Ken's code. We do not write any of the hard parts.
- Authenticode signature verification is the standard Windows mechanism for trusting installer payloads. Family IT chiefs and their family members are likely to recognize the mechanism even if they do not know the term, because it is what every legitimate Windows installer uses.
- The maintenance window keeps updates predictable. The agent does not vanish in the middle of someone's work day. If a family member is using their PC at 4 AM, that is unusual enough that the brief service interruption is acceptable.
- Rollback is automatic. If the new version is broken, `msiexec` puts the old version back without Ken having to implement any rollback logic of its own. The next heartbeat reports the version, the server sees the older version, and the admin can investigate.
- The MSI format is auditable: every change between versions is in the MSI's payload, and tools like `Orca` or `lessmsi` can inspect what the installer actually does.

**Harder:**
- Building MSIs in CI requires a Windows runner with WiX Toolset installed, and the WiX configuration files (`.wxs`) are an XML dialect with their own learning curve. The MSI build is non-trivial to set up the first time.
- Code signing requires a certificate. For Phase 1, a self-signed certificate is acceptable (the family IT chief installs the cert as trusted on each endpoint manually). For a future "real" release, an OV or EV code-signing certificate is needed, and EV certificates are hardware-bound (HSM or USB token) which complicates CI signing. This is acknowledged but is a Phase 2 decision, not a blocker for Phase 1.
- The maintenance-window scheduling means an update can sit on disk for up to 24 hours before being installed. For non-urgent updates this is correct. For a security-relevant emergency update, the family IT chief would have to communicate out-of-band. There is no "force install now" path, and adding one would require an ADR because it would change the agent's autonomy story.
- `WinVerifyTrust` is a Windows API with a slightly awkward Rust binding via `windows-rs`. The verification code needs careful unit tests with intentionally-broken signatures to confirm rejection.

**Accepted:**
- Updates are not instantaneous. A family member who reports a bug today may not be running the fix until tomorrow morning. For Ken's deployment context — family PCs with patient users — this is fine.
- We rely on the Windows trust store and Authenticode infrastructure being intact. If a future Windows update breaks `WinVerifyTrust` or changes the signature format, Ken would need to react. The risk is bounded because Authenticode is foundational to Windows itself.
- The Ken server hosts the MSI. There is no CDN, no torrent, no Microsoft Store distribution. The server's bandwidth and uptime determine update reliability for the deployment. For a single-household deployment, this is correct.

## Alternatives considered

**A custom updater that downloads `.exe` files and replaces the binary directly.** Rejected because atomic replacement of a running binary on Windows is genuinely hard (file locks, service control, partial-write windows), and rollback is even harder. Reinventing what `msiexec` already does correctly is a poor use of effort and a fertile source of bugs that only manifest in deployment.

**MSIX and the Microsoft Store.** Rejected because MSIX is designed for store-distributed apps and assumes a relationship with Microsoft's infrastructure that Ken does not have. Sideloaded MSIX is possible but adds complexity (manifest, certificates, package identity) without solving any problem MSI does not already solve. MSIX is also weaker for SYSTEM services than MSI.

**Squirrel.Windows or a similar self-update framework.** Rejected because Squirrel is designed for user-installed desktop applications, not for SYSTEM services, and its update model assumes the application can restart itself. SYSTEM services have a different lifecycle (the SCM controls them) and Squirrel does not handle that gracefully. Squirrel also pulls in a JavaScript or .NET dependency.

**WinGet for distribution.** Rejected because WinGet is designed for end-user package management and does not have the trust model Ken needs. Family members are not going to run `winget install` on their own PCs, and the family IT chief cannot push updates via WinGet to remote machines. WinGet might be a useful *additional* distribution channel for the initial install in the future, but it cannot replace the in-agent update mechanism.

**No auto-update at all; require manual reinstall for every update.** Rejected because the family IT chief cannot physically visit every endpoint to reinstall, and the whole point of Ken is to make remote help possible without physical presence. An agent that requires manual reinstall to update would defeat the project's purpose within a few months.
