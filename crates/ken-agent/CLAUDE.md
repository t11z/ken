# CLAUDE.md — `ken-agent`

This crate is the Windows endpoint binary. It is the most security-sensitive component of Ken because it runs with high privilege on machines belonging to people who trust the family IT chief. Read this entire file before touching anything in `src/`.

Read the root `CLAUDE.md` and ADR-0001 (`docs/adr/0001-what-ken-will-never-do.md`) first. Every line of code in this crate must be defensible against ADR-0001. When in doubt about whether a feature is allowed, the answer is no.

## What this binary is

A single Rust executable, packaged as an MSI, installed as a Windows service running under `LocalSystem`, with a companion Tray application running in the user session. The service and the Tray app communicate over a Named Pipe. The service speaks to the Ken server over mTLS. When a remote-control session is requested by the server and consented to by the user, the service hosts the session locally using embedded RustDesk crates.

## Component layout

The crate compiles to **two binary targets**:

- `ken-agent-svc` — the SYSTEM service. Runs as a Windows service. Owns all network communication with the server. Owns the embedded remote-session subsystem. Reads OS state. Writes the local audit log. Has no UI.
- `ken-agent-tray` — the user-mode Tray app. Runs in the interactive user session. Owns the consent dialog. Owns the kill switch. Talks to the service over a Named Pipe. Has no network access of its own.

This split is non-negotiable and has its own ADR (placeholder ADR-0006). The reason is simple: the service has privilege but no user context, the Tray app has user context but no privilege. Consent requires user context. Action requires privilege. The Named Pipe is the only place these two worlds meet, and the message types crossing it are deliberately small and well-typed.

## Module structure inside `src/`

```
src/
├── bin/
│   ├── svc.rs              entry point for ken-agent-svc
│   └── tray.rs             entry point for ken-agent-tray
├── lib.rs                  shared library used by both binaries
├── service/                SYSTEM service code
│   ├── mod.rs
│   ├── lifecycle.rs        Windows service lifecycle handlers
│   ├── client.rs           mTLS client to ken-server
│   ├── reporter.rs         polls OS state, builds StatusReport
│   └── command_handler.rs  dispatches ServerMessage variants
├── tray/                   user-mode UI code
│   ├── mod.rs
│   ├── consent_dialog.rs   the one and only consent surface
│   ├── status_view.rs      "Ken is running" status, audit log viewer
│   └── kill_switch.rs      the unconditional pause/uninstall flow
├── ipc/                    Named Pipe protocol between svc and tray
│   ├── mod.rs
│   ├── pipe.rs             low-level pipe handling
│   └── messages.rs         the small set of message types crossing the pipe
├── windows_state/          read-only OS state collection
│   ├── mod.rs
│   ├── defender.rs
│   ├── update.rs
│   ├── firewall.rs
│   ├── bitlocker.rs
│   └── events.rs           filtered Event Log reader
├── session/                embedded remote-session subsystem
│   ├── mod.rs
│   ├── capture.rs          screen capture wrapper
│   ├── codec.rs            video encoding wrapper
│   ├── signaling.rs        hbbs/hbbr client logic
│   ├── input.rs            input event routing
│   └── lifecycle.rs        session start/stop, consent enforcement
├── audit/
│   ├── mod.rs
│   └── log.rs              local user-readable audit log
└── updater/
    ├── mod.rs
    └── trigger.rs          checks for updates, schedules msiexec
```

This structure is the target. Claude Code is expected to create modules in this layout and not invent alternative organizations.

## Hard rules for code in this crate

These are concrete restatements of ADR-0001 in the form of code-level rules. Violating them is a build-blocking failure regardless of test outcomes.

1. **No code in this crate may read user files.** The only filesystem reads permitted are:
   - Ken's own config files under `%ProgramData%\Ken\`
   - Ken's own audit log
   - Windows API calls that incidentally touch the filesystem (BitLocker volume status, installed software registry, etc.) — but never the *contents* of user files

2. **No code in this crate may capture keyboard input outside an active, consented remote session.** The `tray::consent_dialog` module is the only place where user keystrokes are read at all (clicking "Allow"/"Deny" is implemented as button events, not key capture).

3. **No code in this crate may capture the screen outside an active, consented remote session.** `session::capture` only initializes after `session::lifecycle` has confirmed an in-session consent click within the last few seconds.

4. **No code in this crate may make network connections to any host other than the configured Ken server URL and the Ken server's signaling relay.** There is exactly one config-driven server endpoint. There is no fallback, no telemetry endpoint, no analytics, no crash reporter to a public URL.

5. **No code in this crate may modify Windows Defender, Windows Update, the firewall, BitLocker, scheduled tasks, services, registry keys outside Ken's own subtree, or any other OS configuration.** The `windows_state` module is read-only and named that way deliberately.

6. **No code in this crate may bypass the consent flow for remote sessions.** Specifically, the `session::lifecycle::start_session` function must not be callable without a fresh `ConsentGranted` message from the Tray app over the IPC channel. This is enforced by type: `start_session` takes a `ConsentToken` parameter that can only be constructed inside `tray::consent_dialog` after a user click.

7. **No code in this crate may write logs containing user data, screen contents, keystrokes, or session payloads.** The audit log records *what Ken did*, never *what Ken saw*.

## Dependencies

Permitted by default:

- `ken-protocol` (the shared crate)
- `windows` (the official `windows-rs` crate)
- `windows-service` (Windows service lifecycle helper)
- `tokio` with the `rt-multi-thread`, `net`, `io-util`, `time` features
- `rustls` and `tokio-rustls` for mTLS
- `serde` and `serde_json`
- `thiserror` and `anyhow`
- `tracing` and `tracing-subscriber`
- `clap` for command-line argument parsing in dev/debug builds
- The relevant RustDesk crates for the embedded remote-session subsystem (specific list to be pinned in an ADR)
- A Tray UI crate (TBD by ADR; the candidates are `tao`+`wry`, `egui`, or a minimal native window via `windows-rs` directly)

Anything else requires an ADR. Adding a dependency that pulls in a transitive HTTP client, telemetry library, or auto-update mechanism is a hard no — those capabilities must be implemented in Ken's own code or not at all.

## Tests

- Unit tests for `windows_state` modules can run on Linux CI by feature-gating the actual Windows API calls behind a `#[cfg(windows)]` block and providing a mock data path for non-Windows builds. The mock path exists for CI, never for production.
- The consent flow has a dedicated test suite in `tests/consent_enforcement.rs` that asserts `start_session` cannot be called without a valid `ConsentToken`. This test is the most important test in the entire crate.
- The IPC layer has a round-trip test for every message type, similar to `ken-protocol`.
- No test in this crate may make a real network connection. Network tests use a localhost loopback with a self-signed test cert.

## Build targets

Primary target: `x86_64-pc-windows-msvc`. Secondary target for development: `x86_64-pc-windows-gnu` (allows cross-compilation from Linux for compile-checks; production builds must use MSVC).

CI must build for `x86_64-pc-windows-msvc` on every PR. Release builds happen on a Windows runner with the official MSI signing certificate.

## What an LLM should do here

When given a prompt that touches this crate:

1. Read this file completely.
2. Read ADR-0001 again. Yes, again. The temptation to drift is highest in this crate.
3. Identify which module(s) the change affects.
4. Verify that every change satisfies all seven hard rules above.
5. If a change requires touching `session::lifecycle::start_session` or `tray::consent_dialog`, treat it as a security-critical change: extra care, extra tests, and the PR description must explicitly affirm that the consent flow remains intact.
6. Write the change. Build for Windows. Run the test suite. Open a PR.
7. If a change *cannot* be made without violating one of the hard rules, refuse and ask the architect to draft an ADR explicitly authorizing the exception. There may be cases where an exception is justified, but they are decided in writing, not in code.
