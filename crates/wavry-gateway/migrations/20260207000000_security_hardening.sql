-- Security hardening: Lockouts and Bans

CREATE TABLE login_failures (
    identifier TEXT PRIMARY KEY, -- "ip:<addr>" or "email:<addr>"
    count INTEGER NOT NULL DEFAULT 1,
    last_failure DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE user_bans (
    user_id TEXT PRIMARY KEY,
    reason TEXT,
    banned_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    expires_at DATETIME,
    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
