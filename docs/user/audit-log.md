# Reading the audit log

Ken keeps a record of everything it does on your computer. This is part of Ken's commitment to transparency (ADR-0001 T1-5).

## Where is the log?

The audit log is at:

```
C:\ProgramData\Ken\audit.log
```

You can open this file with Notepad or any text editor. You do not need administrator rights to read it.

## What's in it?

Each line is a JSON entry recording one action. The key fields are:

- **occurred_at** — when it happened
- **kind** — what type of action (e.g., `service_started`, `heartbeat_sent`, `consent_requested`)
- **message** — a human-readable description

## Example entries

```json
{"event_id":"...","occurred_at":"2024-01-15T10:00:00Z","kind":{"kind":"service_started"},"message":"service started"}
{"event_id":"...","occurred_at":"2024-01-15T10:00:05Z","kind":{"kind":"heartbeat_sent"},"message":"heartbeat sent"}
{"event_id":"...","occurred_at":"2024-01-15T10:05:00Z","kind":{"kind":"consent_requested"},"message":"remote session consent requested"}
{"event_id":"...","occurred_at":"2024-01-15T10:05:10Z","kind":{"kind":"consent_denied"},"message":"user denied remote session"}
```

## Log rotation

When the log file grows beyond 10 MB, it is rotated: the current file is renamed to `audit.log.1` and a new `audit.log` is started. Only one rotated file is kept.

## Viewing from the Tray App

You can also view the audit log by right-clicking the Ken tray icon and selecting "View audit log." This opens the file in your default text editor.
