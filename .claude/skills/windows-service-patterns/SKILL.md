# Windows Service Patterns

Load this skill when working on `ken-agent`. It covers the Windows-specific idioms for running a service, talking to the user session, and reading OS state without fighting the platform.

## Service lifecycle with `windows-service`

Ken uses the `windows-service` crate to implement the service control handler. The shape of a correct service is:

1. `main()` decides whether we are running as a service, as a CLI tool (install, uninstall, start, stop), or as the Tray App, based on argv.
2. If we are the service, we register the control handler and enter the `run_service` function provided by `windows-service`.
3. The control handler responds to `ServiceControl::Stop` (and a few other signals) by setting a shutdown flag and returning quickly. The main service loop checks the shutdown flag between iterations.
4. When the shutdown flag is set, the service performs a clean shutdown: finishes any in-flight work, flushes the audit log, closes the Named Pipe, and reports `ServiceState::Stopped`.

Do not block in the control handler. The Service Control Manager has a short timeout for control handlers (typically 30 seconds), and if you miss it the SCM kills the process. The handler's job is to set a flag, not to do the shutdown work itself.

## Running as LocalSystem

The Ken service runs as LocalSystem. This is the default for `windows-service` and is correct for Ken because:

- LocalSystem has the privileges needed to read Defender state, Event Log, and WMI
- A custom service account adds installation complexity (password management, privilege grants) without meaningful security benefit in the family-IT threat model
- LocalSystem is the account Microsoft's own security products use for similar purposes

LocalSystem is powerful. Every line of code in the service runs with SYSTEM privileges. Act accordingly: never execute user-provided strings as commands, never load code from paths the user controls, never follow symlinks in paths you do not fully trust.

## Detecting the interactive user session

The Tray App runs in the interactive user session, not as SYSTEM. The service needs to know which session is currently interactive so it can launch the Tray App into that session. The Windows API for this is `WTSGetActiveConsoleSessionId` (for the console session) plus `WTSEnumerateSessions` (to find other interactive sessions on machines with multiple users).

Pattern:

1. The service starts and begins its main loop.
2. The service checks whether a Tray App is running in the active session. If not, it launches one via `CreateProcessAsUser` with the active session's token.
3. The service subscribes to session-change events via `SERVICE_ACCEPT_SESSIONCHANGE` in its `ServiceControl` registration. When the user logs off or a new user logs on, the service reacts by terminating the old Tray App (if any) and launching a new one in the new session.

Do not assume there is always an interactive session. A Windows machine with no one logged in still has the service running, and the service must continue to function — it just cannot display a consent dialog until someone logs in. Consent requests that arrive while no session is active are queued or rejected, depending on the command type, and logged to the audit log either way.

## Named Pipe IPC with restrictive ACLs

The service creates a Named Pipe at a well-known name (for example, `\\.\pipe\ken-agent`). The pipe ACL grants access only to the SID of the currently logged-in interactive user. Default ACLs are not acceptable — they are too permissive and would allow any process on the machine to connect.

Create the pipe with `CreateNamedPipeW` and an explicit `SECURITY_ATTRIBUTES` pointing to a `SECURITY_DESCRIPTOR` you construct. The descriptor has two entries: one granting `GENERIC_READ | GENERIC_WRITE` to the interactive user's SID, and one granting full control to LocalSystem (so the service itself can interact with the pipe). Deny everyone else.

The Tray App connects to the pipe with `CreateFileW` using the pipe name. If the connection fails (wrong user, pipe doesn't exist, pipe busy), the Tray App reports the failure prominently — it is not allowed to silently proceed, because a missing IPC means the consent dialog cannot function.

## Reading Defender state

Use the WMI root namespace `root\Microsoft\Windows\Defender` and query the `MSFT_MpComputerStatus` class. This is the same data source that `Get-MpComputerStatus` in PowerShell uses. It gives you:

- `AntivirusEnabled`
- `RealTimeProtectionEnabled`
- `AntivirusSignatureLastUpdated`
- `AntivirusSignatureAge`
- `OnAccessProtectionEnabled`
- `TamperProtectionSource`

Access WMI from Rust using the `wmi` crate, which provides a reasonably idiomatic wrapper around the COM interfaces. Do not shell out to `powershell.exe Get-MpComputerStatus` — spawning PowerShell for each query is slow, fragile, and creates an audit-log noise pattern that looks like malicious activity on the very Defender you are trying to observe.

Per ADR-0001, Ken **reads** Defender state. Ken does not **modify** Defender state. Any API call that would change Defender configuration is forbidden. This includes `Set-MpPreference` equivalents, exclusion-list modifications, and real-time-protection toggles. If a prompt asks you to modify Defender, stop and surface the question.

## Reading Event Log

The Windows Event Log is the primary source for security-relevant events. Use the `EvtQuery` family of APIs (not the older `ReadEventLog`, which only works with legacy logs). Rust wrappers exist in the `windows-rs` crate under the `Windows::Win32::System::EventLog` module.

For Ken's current scope, the events of interest are:

- Windows Defender events (log name: `Microsoft-Windows-Windows Defender/Operational`)
- Security log entries for successful and failed logons (log name: `Security`)
- Application crashes (log name: `Application`, source: `Application Error`)

Read in a streaming fashion with bookmarks, not by polling the whole log. Store the last-read bookmark per log in the local audit log directory so the service can resume after a restart.

## Update path with MSI

Agent updates are delivered as signed MSI files, downloaded from the Ken server. The service:

1. Periodically queries the server for the current version
2. If a newer version is available, downloads the MSI to a temporary path
3. Verifies the MSI's Authenticode signature against the expected signing certificate
4. Schedules the update for a maintenance window (nightly, configurable)
5. At the maintenance window, invokes `msiexec /i <path> /quiet /norestart`
6. `msiexec` handles the upgrade, including stopping the service, replacing the binary, and restarting the service
7. The new service verifies it came up cleanly and reports a heartbeat with the new version

If verification fails at any step, the downloaded MSI is deleted and the failure is logged and reported.

Do not write a custom updater that manipulates running `.exe` files. `msiexec` already handles the hard parts (atomic replacement, rollback on failure) and is trusted by Windows itself.

## What to avoid

- **Shelling out to `powershell.exe`.** Slow, fragile, and creates log noise.
- **Blocking the service control handler.** SCM will kill you.
- **Assuming an interactive session exists.** It often does not.
- **Using default ACLs on Named Pipes.** Too permissive.
- **Modifying Defender configuration.** Forbidden by ADR-0001.
- **Custom updaters.** Use `msiexec`.
- **Running as a non-SYSTEM account.** More complexity, no real benefit for Ken's threat model.
- **Reading the whole Event Log on every tick.** Use bookmarks and streaming reads.
