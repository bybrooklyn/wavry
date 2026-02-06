use anyhow::{Context, Result};
use log::{info, warn};
use once_cell::sync::Lazy;
use rift_crypto::noise::generate_noise_keypair;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

// Global Identity Storage
pub static IDENTITY: Lazy<Mutex<Option<[u8; 32]>>> = Lazy::new(|| Mutex::new(None));

pub fn init_identity(storage_path: &str) -> Result<[u8; 32]> {
    let path = PathBuf::from(storage_path).join("identity.key");

    // 1. Try to load
    if path.exists() {
        match fs::read(&path) {
            Ok(bytes) => {
                if bytes.len() == 32 {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&bytes);
                    *IDENTITY.lock().unwrap() = Some(key);
                    info!("Loaded identity from {:?}", path);
                    return Ok(key);
                } else {
                    warn!("Invalid key file length: {}", bytes.len());
                }
            }
            Err(e) => warn!("Failed to read key file: {}", e),
        }
    }

    // 2. Generate new
    let (priv_key, _) = generate_noise_keypair();

    // 3. Save
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create identity dir")?;
    }

    fs::write(&path, priv_key).context("Failed to write identity file")?;
    *IDENTITY.lock().unwrap() = Some(priv_key);
    info!("Generated new identity at {:?}", path);

    Ok(priv_key)
}

pub fn get_private_key() -> Option<[u8; 32]> {
    *IDENTITY.lock().unwrap()
}

pub fn get_public_key() -> Option<[u8; 32]> {
    if let Some(priv_key) = get_private_key() {
        let secret = x25519_dalek::StaticSecret::from(priv_key);
        let public = x25519_dalek::PublicKey::from(&secret);
        Some(*public.as_bytes())
    } else {
        None
    }
}
