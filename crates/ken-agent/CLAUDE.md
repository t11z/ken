# CLAUDE.md — ken-agent

This crate is the Windows endpoint binary. It is the most security-sensitive component of Ken because it runs with elevated privilege on machines belonging to people who trust the family IT chief. Every line of code in this crate must be defensible against ADR-0001.

Read the root `CLAUDE.md` and `docs/adr/0001-trust-boundaries-and-current-scope.md` before touching anything in `src/`. When in doubt about whether a capability is allowed, the answer is no — surface the question rather than making a judgment call.

## Components

The agent is composed of four distinct subsystems that share this crate but have different trust profiles and different runtime contexts:

**The SYSTEM service.** Runs as a Windows service under the LocalSystem account. Responsible for reading OS state (Defender, Event Log, WMI, Firewall, BitLocker, Update), reporting heartbeats and status to the server over mTLS, receiving commands, and hosting the embedded remote-session subsystem. This is the component with elevated privilege and the strictest audit requirements.

**The user-mode Tray App.** Runs in the interactive user session, not as SYSTEM. Its only job is to present the consent dialog when the SYSTEM service receives a remote-control request, and to offer the user-facing controls defined in ADR-0001 T1-5 and T1-6 (audit log viewer and kill switch). It has no privileges beyond those of the logged-in user and no direct access to any Ken data beyond what the service chooses to expose through the IPC channel.

**The Named Pipe IPC.** The bridge between the SYSTEM service and the Tray App. Uses Windows Named Pipes with strict ACLs restricting access to the current interactive user. The message format is defined in `ken-protocol` and is intentionally small: consent request, consent response, status query, audit log query, kill-switch trigger. Any expansion of this message set is an architectural decision and requires an ADR.

**The embedded remote-session subsystem.** The subsystem that handles screen capture (via `scrap`), codec (VP9), signaling and relay (via RustDesk crates), and input routing. This subsystem is only active when a consented session is in progress and is strictly gated by the consent check. It is the reason Ken embeds RustDesk crates rather than shelling out to an external client, per the architectural decision recorded in ADR-00XX (the RustDesk embedding ADR — number to be assigned when the ADR is drafted).

## Fail-safe defaults

The agent is built to fail toward the user's side, never toward the admin's side. Concretely:

- **If the Tray App crashes**, the SYSTEM service refuses to start a remote-control session. There is no "approve on the user's behalf because the dialog UI is broken" fallback.
- **If the Named Pipe cannot be established**, the service reports an error to the server but continues to run in read-only mode. It does not assume consent, does not retry with weaker verification, does not escalate.
- **If the server is unreachable**, the agent continues to operate locally. It buffers heartbeats and status reports for later delivery, and it never blocks user activity because of a network failure.
- **If an update fails to install**, the agent falls back to the previous version automatically and reports the failure on the next successful heartbeat. A partial update is never left in place.
- **If the service receives a command it does not understand**, it logs the command to the local audit log, reports the unknown command to the server, and ignores it. It does not guess, does not execute a "closest match," and does not fall back to a generic action.

These are not aspirational. They are properties the code must exhibit, and they must be covered by tests wherever possible.

## The consent gate

Any code path in this crate that leads to screen capture, input injection, or audio capture must pass through the consent gate. The consent gate is a single function that asks the Tray App to display the consent dialog and waits for an explicit positive response. It has no bypass, no "trusted admin" mode, no "remembered consent" cache.

When adding any feature that touches the remote-session subsystem, the first question is "does this route through the consent gate?" If the answer is not an unambiguous yes, the feature does not ship.

The consent gate is tested with a mock Tray App that can be configured to approve, deny, or time out. Every new code path that reaches the remote-session subsystem must include a test that the path is blocked when the mock denies or times out.

## The local audit log

Per ADR-0001 T1-5, the agent maintains a local audit log that is readable by the user of the endpoint. Everything the agent does is recorded there:

- Service start and stop
- Every heartbeat sent (timestamp and size, not content of user data since there is none)
- Every command received from the server (type, timestamp, outcome)
- Every consent dialog shown (type, timestamp, user response)
- Every remote-session start and end (timestamp, duration, initiating admin identity if known)
- Every update check and update application
- Every error and warning from the service

The log format is human-readable plain text or structured JSONL, and the log file is stored at a well-known path under `ProgramData\Ken\audit.log` with ACLs that grant read access to all local users. The Tray App provides a "view audit log" entry point that opens the log in the user's default text viewer.

Code that takes an action without logging it to the audit log is a bug. The log is not a diagnostic aid — it is the transparency mechanism that makes ADR-0001 T1-5 a live commitment.

## Windows-specific conventions

**Use `windows-rs` directly** for Windows API calls rather than wrapping each one in a custom abstraction. `windows-rs` is the canonical, Microsoft-maintained binding layer, and its shape is already idiomatic for the APIs Ken consumes.

**Use `windows-service`** for the service lifecycle (install, start, stop, status). Do not reimplement service control.

**Run in the LocalSystem account**, not in a custom service account. A custom account adds installation complexity without meaningful security benefit in the family-IT threat model.

**Tray App lives in its own binary** (`ken-tray.exe`), started by the service via the Windows session-switching APIs when the service detects an active user session. This is the cleanest separation between the privileged service and the user-mode UI. Both binaries live in this crate; the Cargo.toml defines two binary targets.

**Named Pipe ACLs** are set explicitly at pipe creation. The pipe grants access only to the SID of the interactive user whose Tray App is expected to connect. Default ACLs are not acceptable.

## Dependencies

This crate depends on `ken-protocol` for wire types. External dependencies beyond the core set (`tokio`, `serde`, `tracing`, `anyhow`, `thiserror`, `windows-rs`, `windows-service`, and the RustDesk crates) require justification in the pull request description. Adding a new external dependency here means more code running as LocalSystem on family PCs, and every addition expands the audit surface.

## Testing

Unit tests live alongside the code in `src/`, following Rust convention. Integration tests live in `crates/ken-agent/tests/`.

Tests that require Windows APIs are gated with `#[cfg(windows)]` and are run in CI on a Windows runner. Tests that exercise the consent gate use a mock Tray App implementation that is compiled into the test binary.

The remote-session subsystem is the hardest to test end-to-end, because it involves real screen capture and network I/O. The testing strategy is to verify the gating logic (consent check, session lifecycle, fail-safe paths) with unit tests, and to validate the capture/codec path with a small number of manual smoke tests documented in `docs/architecture/`.

## What this crate must not do

- Read user files, browser state, clipboard, or any user data (ADR-0001 T2-2)
- Log keystrokes or capture input outside an active session (ADR-0001 T2-3)
- Take silent or scheduled screenshots (ADR-0001 T2-4)
- Modify Defender configuration or any OS security settings (implied by T2-1 in combination with T1 invariants; the agent reads, it does not write)
- Install, update, or remove third-party software (T2-5)
- Accept commands from any server other than the configured Ken server (T1-1, T1-2)
- Perform any action without writing a corresponding entry to the local audit log (T1-5)
- Survive a user-initiated kill-switch request (T1-6)

If a prompt asks for any of the above, stop and surface the question before proceeding.
