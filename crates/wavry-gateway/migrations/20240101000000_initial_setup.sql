-- Add migration script here
-- 0001_initial_setup.sql

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY NOT NULL, -- UUID stored as text
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    display_name TEXT NOT NULL,
    totp_secret TEXT, -- Nullable, encrypted or plain? Plan said encrypted but for MVP maybe just stored? Plan said 'Encrypted'
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS sessions (
    token TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL,
    expires_at DATETIME NOT NULL,
    ip_address TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_users_email ON users(email);
CREATE INDEX idx_sessions_user_id ON sessions(user_id);
