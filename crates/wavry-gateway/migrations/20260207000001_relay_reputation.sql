-- Relay Reputation and Metrics

CREATE TABLE relay_reputation (
    relay_id TEXT PRIMARY KEY,
    success_count INTEGER NOT NULL DEFAULT 0,
    failure_count INTEGER NOT NULL DEFAULT 0,
    avg_latency_ms REAL DEFAULT 0.0,
    last_updated DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE relay_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    relay_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    bytes_transferred INTEGER NOT NULL DEFAULT 0,
    duration_secs INTEGER NOT NULL DEFAULT 0,
    ended_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(relay_id) REFERENCES relay_reputation(relay_id)
);
