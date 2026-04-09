# Consent model

Ken is built on the principle that the person using the computer always has the final say.

## How consent works

When your family IT helper wants to connect to your computer remotely, Ken shows a dialog on your screen asking for permission. You can choose:

- **Allow** — the remote session starts. Your IT helper can see your screen and help you.
- **Deny** — nothing happens. Your IT helper is told you declined.

If you don't respond within 60 seconds, the request is automatically denied.

Every consent request and your response is recorded in the local audit log. You can review this log at any time (see [audit-log.md](audit-log.md)).

## What Ken cannot do without your consent

- See your screen
- Control your mouse or keyboard
- Access your microphone or camera
- Take screenshots

These actions are **only** possible during an active remote session that you explicitly approved.

## What Ken does without asking

Ken passively reports the health of your computer to your family IT helper:

- Whether Windows Defender is enabled and up to date
- Whether the Windows Firewall is active
- Whether BitLocker encryption is on
- Whether Windows Updates are pending

This is like a dashboard light on a car — it tells your IT helper if something needs attention, but it does not give them access to your files, messages, or browsing history.

## The kill switch

If you ever want to stop Ken completely:

1. Right-click the Ken icon in your system tray
2. Select "Kill switch"
3. Confirm

Ken will stop immediately and will not restart until you (or your IT helper) explicitly re-enable it. This is your emergency off switch.
