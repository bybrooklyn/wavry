use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context};
use axum::http::HeaderValue;
use base64::{engine::general_purpose, Engine as _};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    Key, XChaCha20Poly1305, XNonce,
};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use tracing::warn;

const DEFAULT_ALLOWED_ORIGINS: [&str; 5] = [
    "http://localhost:1420",
    "http://127.0.0.1:1420",
    "http://localhost:3000",
    "http://127.0.0.1:3000",
    "tauri://localhost",
];

const TOTP_ENCRYPTED_PREFIX: &str = "enc:v1:";

#[derive(Clone, Copy)]
struct RateEntry {
    count: u32,
    window_start: Instant,
}

pub struct FixedWindowRateLimiter {
    max_requests: u32,
    window: Duration,
    max_keys: usize,
    entries: Mutex<HashMap<String, RateEntry>>,
}

impl FixedWindowRateLimiter {
    pub fn new(max_requests: u32, window: Duration, max_keys: usize) -> Self {
        Self {
            max_requests,
            window,
            max_keys,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn allow(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut guard = match self.entries.lock() {
            Ok(v) => v,
            Err(_) => return false,
        };

        if guard.len() > self.max_keys {
            guard.retain(|_, entry| now.duration_since(entry.window_start) < self.window);
            if guard.len() > self.max_keys {
                return false;
            }
        }

        let entry = guard.entry(key.to_string()).or_insert(RateEntry {
            count: 0,
            window_start: now,
        });

        if now.duration_since(entry.window_start) >= self.window {
            entry.count = 0;
            entry.window_start = now;
        }

        entry.count = entry.count.saturating_add(1);
        entry.count <= self.max_requests
    }
}

static AUTH_LIMITER: OnceLock<FixedWindowRateLimiter> = OnceLock::new();
static WEBRTC_LIMITER: OnceLock<FixedWindowRateLimiter> = OnceLock::new();
static WS_BIND_LIMITER: OnceLock<FixedWindowRateLimiter> = OnceLock::new();
static ALLOWED_ORIGINS: OnceLock<HashSet<String>> = OnceLock::new();

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(default)
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
}

fn normalize_origin(origin: &str) -> String {
    origin.trim().trim_end_matches('/').to_ascii_lowercase()
}

fn load_allowed_origins() -> HashSet<String> {
    let configured = std::env::var("WAVRY_ALLOWED_ORIGINS").unwrap_or_default();
    let mut set = HashSet::new();

    if configured.trim().is_empty() {
        for value in DEFAULT_ALLOWED_ORIGINS {
            set.insert(normalize_origin(value));
        }
        return set;
    }

    for value in configured.split(',') {
        let normalized = normalize_origin(value);
        if !normalized.is_empty() {
            set.insert(normalized);
        }
    }

    if set.is_empty() {
        for value in DEFAULT_ALLOWED_ORIGINS {
            set.insert(normalize_origin(value));
        }
    }

    set
}

pub fn cors_allow_any() -> bool {
    env_bool("WAVRY_CORS_ALLOW_ANY", false)
}

pub fn cors_origin_values() -> Vec<HeaderValue> {
    let set = ALLOWED_ORIGINS.get_or_init(load_allowed_origins);
    set.iter()
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect()
}

pub fn ws_origin_allowed(origin: Option<&str>) -> bool {
    let require_origin = env_bool("WAVRY_WS_REQUIRE_ORIGIN", true);
    let allow_missing = env_bool("WAVRY_WS_ALLOW_MISSING_ORIGIN", false);

    let Some(origin) = origin else {
        return !require_origin || allow_missing;
    };

    let normalized = normalize_origin(origin);
    let set = ALLOWED_ORIGINS.get_or_init(load_allowed_origins);
    set.contains(&normalized)
}

pub fn allow_auth_request(key: &str) -> bool {
    AUTH_LIMITER
        .get_or_init(|| {
            FixedWindowRateLimiter::new(
                env_u32("WAVRY_AUTH_RATE_LIMIT", 20),
                Duration::from_secs(env_u32("WAVRY_AUTH_RATE_WINDOW_SECS", 60).max(1) as u64),
                env_usize("WAVRY_AUTH_RATE_MAX_KEYS", 10_000),
            )
        })
        .allow(key)
}

pub fn allow_webrtc_request(key: &str) -> bool {
    WEBRTC_LIMITER
        .get_or_init(|| {
            FixedWindowRateLimiter::new(
                env_u32("WAVRY_WEBRTC_RATE_LIMIT", 120),
                Duration::from_secs(env_u32("WAVRY_WEBRTC_RATE_WINDOW_SECS", 60).max(1) as u64),
                env_usize("WAVRY_WEBRTC_RATE_MAX_KEYS", 50_000),
            )
        })
        .allow(key)
}

pub fn allow_ws_bind_request(key: &str) -> bool {
    WS_BIND_LIMITER
        .get_or_init(|| {
            FixedWindowRateLimiter::new(
                env_u32("WAVRY_WS_BIND_RATE_LIMIT", 10),
                Duration::from_secs(env_u32("WAVRY_WS_BIND_RATE_WINDOW_SECS", 60).max(1) as u64),
                env_usize("WAVRY_WS_BIND_RATE_MAX_KEYS", 50_000),
            )
        })
        .allow(key)
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();

    if a_bytes.len() != b_bytes.len() {
        return false;
    }

    let mut diff = 0u8;
    for (lhs, rhs) in a_bytes.iter().zip(b_bytes.iter()) {
        diff |= lhs ^ rhs;
    }
    diff == 0
}

pub fn is_valid_email(email: &str) -> bool {
    if email.len() > 254 || email.contains(char::is_whitespace) {
        return false;
    }
    let mut parts = email.split('@');
    let Some(local) = parts.next() else {
        return false;
    };
    let Some(domain) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

pub fn is_valid_username(username: &str) -> bool {
    let len = username.len();
    if !(3..=32).contains(&len) {
        return false;
    }
    username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

pub fn is_valid_display_name(display_name: &str) -> bool {
    let len = display_name.trim().len();
    (1..=64).contains(&len) && display_name.chars().all(|c| !c.is_control())
}

pub fn is_valid_password(password: &str) -> bool {
    let len = password.len();
    (12..=128).contains(&len)
}

pub fn is_valid_public_key_hex(key: &str) -> bool {
    let len = key.len();
    (64..=128).contains(&len) && key.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn is_valid_totp_code(code: &str) -> bool {
    code.len() == 6 && code.chars().all(|c| c.is_ascii_digit())
}

pub fn is_valid_session_token(token: &str) -> bool {
    let len = token.len();
    (32..=256).contains(&len)
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

fn insecure_totp_allowed() -> bool {
    if env_bool("WAVRY_ENVIRONMENT_PRODUCTION", false) || std::env::var("WAVRY_ENVIRONMENT").map(|v| v == "production").unwrap_or(false) {
        return false;
    }
    env_bool("WAVRY_ALLOW_INSECURE_TOTP", false)
}

fn load_totp_key() -> anyhow::Result<Option<[u8; 32]>> {
    let raw = std::env::var("WAVRY_TOTP_KEY_B64").unwrap_or_default();
    if raw.trim().is_empty() {
        if insecure_totp_allowed() {
            return Ok(None);
        }
        return Err(anyhow!(
            "WAVRY_TOTP_KEY_B64 is required unless WAVRY_ALLOW_INSECURE_TOTP=1"
        ));
    }

    let bytes = general_purpose::STANDARD
        .decode(raw.trim())
        .context("WAVRY_TOTP_KEY_B64 must be base64")?;
    if bytes.len() != 32 {
        return Err(anyhow!(
            "WAVRY_TOTP_KEY_B64 must decode to exactly 32 bytes"
        ));
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(Some(key))
}

pub fn encrypt_totp_secret(secret: &str) -> anyhow::Result<String> {
    let maybe_key = load_totp_key()?;
    let Some(key_bytes) = maybe_key else {
        warn!("storing plaintext TOTP secret due to WAVRY_ALLOW_INSECURE_TOTP=1");
        return Ok(secret.to_string());
    };

    let key = Key::from_slice(&key_bytes);
    let cipher = XChaCha20Poly1305::new(key);
    let mut nonce_bytes = [0u8; 24];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, secret.as_bytes())
        .map_err(|_| anyhow!("failed to encrypt TOTP secret"))?;

    let mut blob = Vec::with_capacity(24 + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    Ok(format!(
        "{}{}",
        TOTP_ENCRYPTED_PREFIX,
        general_purpose::STANDARD.encode(blob)
    ))
}

pub fn decrypt_totp_secret(stored: &str) -> anyhow::Result<String> {
    let Some(encoded) = stored.strip_prefix(TOTP_ENCRYPTED_PREFIX) else {
        if insecure_totp_allowed() {
            return Ok(stored.to_string());
        }
        return Err(anyhow!(
            "plaintext TOTP secret found; set WAVRY_ALLOW_INSECURE_TOTP=1 to migrate"
        ));
    };

    let key_bytes = load_totp_key()?
        .ok_or_else(|| anyhow!("WAVRY_TOTP_KEY_B64 required to decrypt stored TOTP secret"))?;
    let blob = general_purpose::STANDARD
        .decode(encoded)
        .context("stored TOTP secret is not valid base64")?;
    if blob.len() < 24 {
        return Err(anyhow!("stored TOTP secret blob is too short"));
    }

    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&blob[..24]);
    let ciphertext = &blob[24..];

    let key = Key::from_slice(&key_bytes);
    let cipher = XChaCha20Poly1305::new(key);
    let plaintext = cipher
        .decrypt(XNonce::from_slice(&nonce), ciphertext)
        .map_err(|_| anyhow!("failed to decrypt TOTP secret"))?;

    String::from_utf8(plaintext).context("decrypted TOTP secret is not utf-8")
}
