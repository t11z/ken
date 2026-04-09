# Ken component diagram

## Overview

Ken has two main components: the **agent** (Windows) and the **server** (Linux). They communicate over mTLS.

```
┌─────────────────────────────────────────────────┐
│                  WINDOWS PC                      │
│                                                  │
│  ┌────────────────────────────────────────────┐  │
│  │         Ken Agent (SYSTEM service)         │  │
│  │                                            │  │
│  │  ┌──────────┐  ┌──────────┐  ┌─────────┐  │  │
│  │  │ Observer  │  │ Heartbeat│  │ Command │  │  │
│  │  │ (WMI,    │  │   Loop   │  │Processor│  │  │
│  │  │ EventLog)│  │          │  │         │  │  │
│  │  └──────────┘  └──────────┘  └─────────┘  │  │
│  │         │            │            │        │  │
│  │         └────────────┼────────────┘        │  │
│  │                      │                     │  │
│  │              ┌───────┴───────┐             │  │
│  │              │  Named Pipe   │             │  │
│  │              │     IPC       │             │  │
│  │              └───────┬───────┘             │  │
│  └──────────────────────┼─────────────────────┘  │
│                         │                        │
│  ┌──────────────────────┴─────────────────────┐  │
│  │         Ken Tray App (user session)        │  │
│  │  ┌────────┐  ┌──────────┐  ┌───────────┐  │  │
│  │  │ Status │  │ Consent  │  │Kill Switch│  │  │
│  │  │Display │  │  Dialog  │  │  Button   │  │  │
│  │  └────────┘  └──────────┘  └───────────┘  │  │
│  └────────────────────────────────────────────┘  │
│                                                  │
│  ┌────────────────────────────────────────────┐  │
│  │  audit.log  (readable by user, T1-5)       │  │
│  └────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────┘
                         │
                    mTLS (port 8443)
                         │
┌──────────────────────────────────────────────────┐
│              RASPBERRY PI (Linux)                 │
│                                                   │
│  ┌─────────────────────────────────────────────┐  │
│  │              Ken Server                     │  │
│  │                                             │  │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  │  │
│  │  │ Agent API│  │ Admin UI │  │Enrollment│  │  │
│  │  │ (mTLS)   │  │ (htmx)  │  │ Endpoint │  │  │
│  │  │ :8443    │  │  :8444   │  │  :8444   │  │  │
│  │  └──────────┘  └──────────┘  └──────────┘  │  │
│  │        │             │             │        │  │
│  │  ┌─────┴─────────────┴─────────────┴─────┐  │  │
│  │  │           SQLite Database             │  │  │
│  │  │  endpoints | heartbeats | commands    │  │  │
│  │  │  status_snapshots | audit_events      │  │  │
│  │  └───────────────────────────────────────┘  │  │
│  │                                             │  │
│  │  ┌───────────────────────────────────────┐  │  │
│  │  │    Certificate Authority (rcgen)      │  │  │
│  │  │    Root CA + server cert + client     │  │  │
│  │  │    certs for enrolled endpoints       │  │  │
│  │  └───────────────────────────────────────┘  │  │
│  └─────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────┘
```

## Data flow

1. **Enrollment**: Family IT chief creates a one-time URL in the admin UI. The family member runs `ken-agent enroll --url <url>`. The agent receives its mTLS certificate and CA trust anchor.

2. **Heartbeat loop**: Every 60 seconds (with jitter), the agent collects OS status and sends a heartbeat to the server. The server responds with any pending commands.

3. **Commands**: The family IT chief issues commands (Ping, Refresh Status) through the admin UI. Commands are queued and delivered in the next heartbeat response.

4. **Remote sessions** (Phase 2): When the IT chief requests a remote session, the agent shows a consent dialog. Only after explicit approval does the session start.

## Trust boundaries

Per ADR-0001:

- The agent never communicates with any server other than the configured Ken server (T1-1, T1-2)
- Every remote session requires per-session consent (T1-4)
- The audit log is always readable by the endpoint user (T1-5)
- The kill switch is always available locally (T1-6)
