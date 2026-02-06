//! Ed25519 identity keys and Wavry ID.
//!
//! A **Wavry ID** is the base64url-encoded Ed25519 public key (32 bytes → 43 characters).
//! This provides a stable, cryptographic identity that users control.
//!
//! # Example
//!
//! ```
//! use rift_crypto::identity::IdentityKeypair;
//!
//! // Generate a new keypair
//! let keypair = IdentityKeypair::generate();
//!
//! // Get the Wavry ID (base64url-encoded public key)
//! let wavry_id = keypair.wavry_id();
//! println!("Wavry ID: {}", wavry_id);
//!
//! // Sign a challenge
//! let challenge = b"random challenge bytes";
//! let signature = keypair.sign(challenge);
//!
//! // Verify the signature
//! assert!(keypair.verify(challenge, &signature));
//! ```

use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use zeroize::Zeroize;

/// Wavry ID: base64url-encoded Ed25519 public key.
///
/// This is the primary user identifier in the Wavry system.
/// 32 bytes encoded as 43 characters (no padding).
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WavryId(String);

impl WavryId {
    /// Create a Wavry ID from raw public key bytes.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(URL_SAFE_NO_PAD.encode(bytes))
    }

    /// Parse a Wavry ID from its string representation.
    pub fn parse(s: &str) -> Result<Self> {
        let bytes = URL_SAFE_NO_PAD
            .decode(s)
            .context("invalid base64url encoding")?;

        if bytes.len() != 32 {
            anyhow::bail!(
                "invalid Wavry ID length: expected 32 bytes, got {}",
                bytes.len()
            );
        }

        Ok(Self(s.to_string()))
    }

    /// Get the raw public key bytes.
    pub fn to_bytes(&self) -> Result<[u8; 32]> {
        let bytes = URL_SAFE_NO_PAD
            .decode(&self.0)
            .context("invalid base64url encoding")?;

        bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid key length"))
    }

    /// Get the string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WavryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for WavryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WavryId({})", self.0)
    }
}

/// Ed25519 identity keypair.
///
/// Contains both the signing key (private) and verifying key (public).
/// The signing key is zeroized on drop for security.
pub struct IdentityKeypair {
    signing_key: SigningKey,
}

impl IdentityKeypair {
    /// Generate a new random keypair using the OS CSPRNG.
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self { signing_key }
    }

    /// Create from raw signing key bytes.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(bytes);
        Self { signing_key }
    }

    /// Get the Wavry ID (base64url-encoded public key).
    pub fn wavry_id(&self) -> WavryId {
        WavryId::from_bytes(self.signing_key.verifying_key().as_bytes())
    }

    /// Get the public key as a verifying key.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Get the public key bytes.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        *self.signing_key.verifying_key().as_bytes()
    }

    /// Get the private key bytes.
    ///
    /// # Security
    /// Handle with care! These bytes can recreate the identity.
    pub fn private_key_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// Sign a message with this identity.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing_key.sign(message).to_bytes()
    }

    /// Verify a signature against this identity's public key.
    pub fn verify(&self, message: &[u8], signature: &[u8; 64]) -> bool {
        let sig = match Signature::from_slice(signature) {
            Ok(s) => s,
            Err(_) => return false,
        };
        self.signing_key
            .verifying_key()
            .verify(message, &sig)
            .is_ok()
    }

    /// Save keypair to files.
    ///
    /// Private key is saved with restricted permissions (0600 on Unix).
    pub fn save(&self, private_path: &str, public_path: &str) -> Result<()> {
        let private_bytes = self.private_key_bytes();
        let public_bytes = self.public_key_bytes();

        // Save private key
        fs::write(private_path, private_bytes)?;

        // Restrict permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(private_path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(private_path, perms)?;
        }

        // Save public key
        fs::write(public_path, public_bytes)?;

        Ok(())
    }

    /// Load keypair from private key file.
    pub fn load(private_path: &str) -> Result<Self> {
        let bytes = fs::read(private_path).context("failed to read private key")?;

        if bytes.len() != 32 {
            anyhow::bail!("invalid private key length: expected 32 bytes");
        }

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&bytes);

        let keypair = Self::from_bytes(&key_bytes);
        key_bytes.zeroize();

        Ok(keypair)
    }

    /// Load only the public key (for verification).
    pub fn load_public(public_path: &str) -> Result<PublicIdentity> {
        let bytes = fs::read(public_path).context("failed to read public key")?;

        if bytes.len() != 32 {
            anyhow::bail!("invalid public key length: expected 32 bytes");
        }

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&bytes);

        PublicIdentity::from_bytes(&key_bytes)
    }
}

impl Drop for IdentityKeypair {
    fn drop(&mut self) {
        // SigningKey from ed25519-dalek implements Zeroize,
        // but we ensure cleanup anyway
    }
}

/// Public identity (verifying key only).
///
/// Used when you only need to verify signatures, not create them.
pub struct PublicIdentity {
    verifying_key: VerifyingKey,
}

impl PublicIdentity {
    /// Create from raw public key bytes.
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self> {
        let verifying_key = VerifyingKey::from_bytes(bytes).context("invalid public key")?;
        Ok(Self { verifying_key })
    }

    /// Get the Wavry ID.
    pub fn wavry_id(&self) -> WavryId {
        WavryId::from_bytes(self.verifying_key.as_bytes())
    }

    /// Verify a signature.
    pub fn verify(&self, message: &[u8], signature: &[u8; 64]) -> bool {
        let sig = match Signature::from_slice(signature) {
            Ok(s) => s,
            Err(_) => return false,
        };
        self.verifying_key.verify(message, &sig).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let keypair = IdentityKeypair::generate();
        let wavry_id = keypair.wavry_id();

        // Wavry ID should be 43 characters (32 bytes base64url without padding)
        assert_eq!(wavry_id.as_str().len(), 43);
    }

    #[test]
    fn test_sign_verify() {
        let keypair = IdentityKeypair::generate();
        let message = b"hello wavry";

        let signature = keypair.sign(message);
        assert!(keypair.verify(message, &signature));

        // Wrong message should fail
        assert!(!keypair.verify(b"wrong message", &signature));
    }

    #[test]
    fn test_wavry_id_roundtrip() {
        let keypair = IdentityKeypair::generate();
        let wavry_id = keypair.wavry_id();

        let parsed = WavryId::parse(wavry_id.as_str()).unwrap();
        assert_eq!(wavry_id, parsed);
    }

    #[test]
    fn test_keypair_bytes_roundtrip() {
        let keypair = IdentityKeypair::generate();
        let private_bytes = keypair.private_key_bytes();
        let public_bytes = keypair.public_key_bytes();

        let restored = IdentityKeypair::from_bytes(&private_bytes);
        assert_eq!(restored.public_key_bytes(), public_bytes);
    }
}
