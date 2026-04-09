# ADR-0010: Named Pipe IPC between SYSTEM service and tray app

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

The Ken agent on Windows is split into two processes that must communicate: the SYSTEM service (privileged, runs as LocalSystem, does the actual observability and remote-session work) and the user-mode tray app (runs in the interactive user's session, displays the consent dialog and the status UI). The two processes have different security contexts and different lifetimes — the service runs continuously, the tray app starts when the user logs in and stops when they log out.

The communication between them is small in volume but high in stakes. The most security-critical message is the consent request: when the SYSTEM service receives a `RequestRemoteSession` command from the Ken server, it asks the tray app to display the consent dialog and waits for the user's answer. The integrity of that exchange is the load-bearing trust mechanism for ADR-0001 T1-4. If the IPC channel can be impersonated, the consent gate is bypassable.

The realistic IPC mechanisms on Windows are: Named Pipes with explicit ACLs, local TCP sockets with some other authentication mechanism, COM/RPC, shared memory with synchronization primitives, or files in a well-known directory polled by both processes. Each has trade-offs for security, complexity, and reliability.

## Decision

The Ken SYSTEM service and tray app communicate via a **Windows Named Pipe with an explicit security descriptor**. The pipe is created by the service at a per-session path: `\\.\pipe\ken-agent-<session_id>`, where `<session_id>` is the Windows session ID of the interactive user. The service obtains the session ID via `WTSGetActiveConsoleSessionId` and `WTSEnumerateSessions`, and creates one pipe per active interactive session.

The pipe is created with `CreateNamedPipeW` and an explicit `SECURITY_ATTRIBUTES` pointing to a `SECURITY_DESCRIPTOR` constructed in Rust via `windows-rs`. The descriptor has exactly two access control entries:

- **Allow** `GENERIC_READ | GENERIC_WRITE` to the SID of the user in the target session.
- **Allow** full control to `SYSTEM` (so the service itself can interact with the pipe).

There is no entry for `Everyone`, no entry for `Administrators`, and no entry for any other principal. Default ACLs are explicitly not used; the descriptor is built from scratch every time.

The tray app connects with `CreateFileW` using the pipe name appropriate to its own session. If the pipe does not exist (service not running) or if access is denied (wrong user, wrong session), the tray app reports the error prominently — it does not silently degrade or retry indefinitely.

The wire format on the pipe is **length-prefixed JSON**: a 4-byte little-endian length, then that many bytes of UTF-8 JSON. The message types are an enum defined in `ken-protocol::ipc` (or in the agent crate if they are not strictly wire types — to be decided during implementation; this ADR does not pre-commit). The Phase 1 message set is small:

- `GetStatus` / `Status(AgentStatus)` — tray app polls service for current state
- `RequestConsent { session_description, admin_name }` / `ConsentGranted` | `ConsentDenied` — service asks tray app to display the consent dialog
- `GetAuditLogTail { lines }` / `AuditLogTail(Vec<String>)` — tray app fetches the most recent audit log entries
- `ActivateKillSwitch` / `KillSwitchActivated` — tray app triggers the kill switch (see ADR-0012)

Each request/response is one exchange. The pipe is not used as a long-lived streaming channel. The service accepts one connection at a time per session — the tray app is the only legitimate client.

## Consequences

**Easier:**
- Windows enforces the ACL at the kernel level. A process running as a different user cannot open the pipe regardless of what the application code does. The OS is the authentication mechanism, not the application.
- The pipe lifetime is tied to the service's lifetime, which is the right scope. When the service stops, the pipe is gone. When a new user logs in, the service creates a new pipe for that session. There is no lingering state to clean up.
- Length-prefixed JSON is dead-simple to parse and debug. The message format is human-readable in a packet capture or in a logging dump (with sensitive fields redacted).
- The consent gate's wire format becomes visible and testable. Unit tests construct `RequestConsent` messages, send them to a mock pipe, and verify the tray-app side renders the dialog correctly.
- The service can drop a connection cleanly if the tray app misbehaves, and the next connection attempt creates a fresh exchange with no carry-over state.

**Harder:**
- Building a `SECURITY_DESCRIPTOR` in Rust via `windows-rs` is verbose. The code involves several `windows::Win32::Security` API calls and explicit handling of SID buffers and ACL structures. It is mechanical but tedious, and it must be reviewed carefully — a mistake in the descriptor can either lock out the legitimate tray app or open the pipe to other processes.
- The per-session pipe naming requires the service to track which sessions are active and to react to `WTS_SESSION_LOGON` and `WTS_SESSION_LOGOFF` events. The service registers for `SERVICE_ACCEPT_SESSIONCHANGE` in its control handler, and the session-management code is non-trivial. This is the price of doing user-context-aware service work on Windows correctly.
- The consent dialog's latency is bounded by the pipe round-trip plus the user's response time. The pipe round-trip is microseconds, so the bottleneck is the human. Tests of the consent flow must mock the human side.
- Concurrent users (Fast User Switching, Remote Desktop with multiple sessions) require multiple pipes in parallel. The service must handle this. The implementation cost is real but bounded.

**Accepted:**
- We rely on Windows' Named Pipe ACL enforcement being correct. This is a reasonable trust assumption — Named Pipes have been a foundational Windows IPC mechanism for decades and the security model is well-tested. If the OS itself is compromised at the kernel level, no IPC mechanism we choose would help.
- The consent dialog cannot be displayed if no interactive user is logged in. This is correct behavior: a consent request that arrives while no one is at the machine is queued or rejected, depending on the policy in the command processor (Phase 1: rejected with `Rejected { reason: "no active user session" }`). The audit log records the rejection.

## Alternatives considered

**Local TCP socket on `127.0.0.1` with a shared secret.** Rejected because TCP sockets on `localhost` are reachable by every local user and every local process, and the shared secret would have to live somewhere both processes can read — typically a file with restrictive ACLs, which is exactly the same security model as a Named Pipe but with more moving parts. Named Pipes give us the same isolation with less code.

**COM / RPC.** Rejected because COM in Rust is a swamp. The `windows-rs` COM bindings exist but the boilerplate of registering interfaces, marshaling parameters, and handling lifetimes is substantial. COM also has a long history of subtle ACL issues that are hard to diagnose. For two endpoints inside one project that we control, Named Pipes are dramatically simpler.

**Shared memory with a mutex and signal.** Rejected because shared memory has no built-in message framing, no built-in flow control, and no built-in authentication. We would have to add all three, which gets us back to roughly the same place as Named Pipes but without the ACL story.

**A file in a well-known directory polled by both processes.** Rejected because polling adds latency to the consent dialog (the user would see the dialog half a second after the request arrives, which is noticeable), and because file ACLs are weaker than pipe ACLs (a process running as the same user could read files that the tray app reads, even if it has no business doing so). The "I think a file is here, let me read it" model also has obvious race conditions that pipes do not.

**Inheriting a pipe handle from a child-process spawn instead of opening one by name.** Rejected because the tray app is not a child process of the service — it runs in the user's session, started by the service via `CreateProcessAsUser`, but it has its own process tree. Handle inheritance does not work cleanly across the session boundary. The named pipe with explicit ACLs is the standard solution to exactly this problem.
