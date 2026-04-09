# ADR-0012: Kill switch architecture via state file and SYSTEM service self-stop

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

ADR-0001 T1-6 commits Ken to giving the user of an endpoint an always-available way to disable Ken locally, without the family IT chief's permission and without a network round-trip. This is the "kill switch" — the ultimate safeguard that makes the consent model meaningful, because trust is only real when withdrawal is possible.

The implementation question is non-trivial on Windows. The Ken agent runs as a SYSTEM service, which means stopping it via the Service Control Manager (`sc stop KenAgent`) requires administrative privileges. The user of an endpoint may or may not be a local administrator — in many family-IT contexts, the family IT chief is the only admin and the relatives use standard accounts. A kill-switch design that requires elevation would lock out exactly the people the kill switch is supposed to protect.

The realistic options are: have the tray app shell out to elevated `sc stop` (requires UAC prompt and admin credentials, fails for non-admin users), have the tray app signal the service via the existing IPC channel and let the service stop itself (works for any user, but requires the service to cooperate), or use a file-based marker that the service checks (works but introduces latency). Each of these has implications for the trust story and for the user experience.

The decision is forced now because the kill switch must exist before Phase 1 is considered complete — without it, the consent model in ADR-0001 T1-6 is aspirational.

## Decision

The kill switch is implemented as a **two-part mechanism**: a **state file at a well-known path** that the service checks on startup, plus a **service self-stop triggered via the existing Named Pipe IPC** when the kill switch is activated while the service is running.

The full flow:

1. The user activates the kill switch via the tray app (right-click tray icon → Kill switch → confirmation dialog → confirm).
2. The tray app writes a file at `%ProgramData%\Ken\state\kill-switch-requested` containing a JSON document with the timestamp, the requesting user's name, and a brief reason field. The file is written with permissions that allow the SYSTEM service to read it but do not require admin rights to write.
3. The tray app sends an `ActivateKillSwitch` message to the SYSTEM service via the Named Pipe IPC defined in ADR-0010.
4. The SYSTEM service receives the message, writes an audit log entry `KillSwitchActivated`, and initiates its own shutdown sequence: it finishes any in-flight work, flushes the audit log, closes the Named Pipe, and reports `ServiceState::Stopped` to the SCM.
5. Before reporting `Stopped`, the service also disables its own startup type by setting it to `SERVICE_DISABLED` via `ChangeServiceConfig`. This prevents the SCM from auto-restarting it.
6. On every subsequent service start attempt, the service checks for the existence of `%ProgramData%\Ken\state\kill-switch-requested`. If the file exists, the service immediately writes an audit log entry `KillSwitchStartupRefused`, sets its own startup type to `SERVICE_DISABLED` again (in case something re-enabled it), and exits without doing any work.

The kill switch is *reversible* but only by an explicit administrator action: deleting the state file and re-enabling the service via `sc config KenAgent start= auto` followed by `sc start KenAgent`. The tray app does not offer a "un-kill" button — un-killing requires going through the family IT chief or through an elevated command prompt. This asymmetry is deliberate: triggering the kill switch must be easy for the user, undoing it must be deliberate.

If the SYSTEM service is unresponsive when the tray app tries to send the IPC message (a degenerate case — the service has crashed but the tray app is still running), the tray app still writes the state file and then displays a message to the user explaining that the service must be stopped manually (with instructions for `services.msc` or `sc stop KenAgent` from an elevated prompt). The state file will prevent the service from restarting once stopped, regardless of how it was stopped.

## Consequences

**Easier:**
- The kill switch works for any user, regardless of their administrator status. This is essential to the trust story — the people who most need the kill switch are exactly the people who do not have admin rights on their own machines in most family-IT setups.
- The state-file check on service startup is a simple, robust failsafe. Even if Windows tries to restart the service after a crash, after a reboot, or after a system update, the file's presence stops it. The check is the first thing the service does, before any other initialization.
- The mechanism is auditable. Every kill-switch activation produces a state file (which the user can read to confirm what happened) and an audit log entry (which the family IT chief sees in the next heartbeat from any other agent, or never if the killed agent is the only one). The state file is the user's receipt; the audit log is the operator's notification.
- Un-killing is asymmetric, which is correct. A user who just hit the kill switch in alarm should not be able to undo it by accident. A user who hit it deliberately and now wants to undo it must take an explicit action that confirms the reversal.

**Harder:**
- The two-part mechanism (state file + IPC) is more code than a single mechanism would be. Both parts need to be tested, and both parts need to handle their own failure modes. The service must handle "state file exists but I just started anyway" (something rewrote startup config) and "kill-switch IPC message arrived but state file write failed" (disk full).
- Setting the service startup type to `SERVICE_DISABLED` from inside the running service is a Windows API call that must succeed before the service stops. If it fails (rare but possible), the SCM may try to restart the service, the state file check will catch it, and the audit log will record both events. This is acceptable behavior but worth being explicit about.
- Disk-write failures during kill-switch activation are a real concern. The state file write must be robust: write to a temporary path first, fsync, rename to the final path. The standard "atomic file write" pattern.

**Accepted:**
- A user who wants to permanently uninstall Ken (not just stop it) still needs the family IT chief or admin help to run the MSI uninstaller. The kill switch only stops the service and prevents it from restarting; it does not remove the binary, the configuration, or the audit log. This is correct: full removal is a deliberate cleanup operation, distinct from an emergency stop.
- The kill switch state file lives in `%ProgramData%\Ken\state\` and is part of what the MSI uninstaller cleans up. A reinstalled Ken on the same machine will not inherit a stale kill-switch state from before the uninstall.
- The tray app can fail to communicate with the service. In that case, the user sees the manual-stop instructions and the state file is still written. This is the most degraded path and is also documented in `docs/user/`.

## Alternatives considered

**Have the tray app shell out to `sc stop KenAgent` after a UAC elevation prompt.** Rejected because non-admin users cannot satisfy the UAC prompt at all, and even admin users would have to enter their credentials every time. The kill switch must work for non-admins.

**Have the service poll a file in the user's home directory.** Rejected because the polling latency is bad for a kill switch (the user expects immediate feedback) and because file ACLs in user-writable locations are weaker than the `ProgramData` location. The IPC-plus-state-file approach gives both immediate response and a reliable startup-time check.

**Use a Windows global event or named mutex as the signal.** Rejected because signaling primitives do not survive a service restart — if the service crashes and the SCM restarts it, the signal is gone, and the service has no record that it was supposed to stay stopped. The state file persists across restarts; the signal does not.

**Make the kill switch reversible from the tray app itself.** Rejected because reversibility from the same UI that triggered the kill switch makes it too easy to un-kill in a moment of doubt or by accident. The asymmetry (easy to activate, deliberate to undo) is part of the safeguard, not a usability flaw.

**Skip the kill switch entirely and trust the uninstall path.** Rejected because the uninstall path requires admin rights, and ADR-0001 T1-6 explicitly commits to a kill switch that does not. Skipping it would violate a Tier 1 invariant.
