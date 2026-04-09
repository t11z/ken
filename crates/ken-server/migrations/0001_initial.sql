-- Initial schema for the Ken server database.
-- All timestamps are stored as ISO-8601 strings (SQLite has no native datetime type).

CREATE TABLE IF NOT EXISTS endpoints (
    id TEXT PRIMARY KEY NOT NULL,
    hostname TEXT NOT NULL,
    os_version TEXT NOT NULL,
    agent_version TEXT NOT NULL,
    enrolled_at TEXT NOT NULL,
    last_heartbeat_at TEXT,
    certificate_pem TEXT NOT NULL,
    certificate_expires_at TEXT NOT NULL,
    revoked_at TEXT,
    display_name TEXT
);

CREATE TABLE IF NOT EXISTS enrollment_tokens (
    token TEXT PRIMARY KEY NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    consumed_at TEXT,
    display_name TEXT
);

CREATE TABLE IF NOT EXISTS heartbeats (
    id TEXT PRIMARY KEY NOT NULL,
    endpoint_id TEXT NOT NULL REFERENCES endpoints(id),
    received_at TEXT NOT NULL,
    sent_at TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    agent_version TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_heartbeats_endpoint ON heartbeats(endpoint_id, received_at);

CREATE TABLE IF NOT EXISTS status_snapshots (
    endpoint_id TEXT PRIMARY KEY NOT NULL REFERENCES endpoints(id),
    collected_at TEXT NOT NULL,
    snapshot_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS commands (
    id TEXT PRIMARY KEY NOT NULL,
    endpoint_id TEXT NOT NULL REFERENCES endpoints(id),
    issued_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    delivered_at TEXT,
    completed_at TEXT,
    outcome_json TEXT
);

CREATE INDEX IF NOT EXISTS idx_commands_endpoint ON commands(endpoint_id, delivered_at);

CREATE TABLE IF NOT EXISTS audit_events (
    id TEXT PRIMARY KEY NOT NULL,
    endpoint_id TEXT REFERENCES endpoints(id),
    occurred_at TEXT NOT NULL,
    source TEXT NOT NULL,
    kind TEXT NOT NULL,
    message TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audit_events_time ON audit_events(occurred_at);

CREATE TABLE IF NOT EXISTS admin_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    csrf_token TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS admin_secrets (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
