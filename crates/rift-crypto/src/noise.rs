//! Noise Protocol handshake implementation.
//!
//! Uses Noise XX pattern: `Noise_XX_25519_ChaChaPoly_BLAKE2s`
//!
//! # Why XX?
//!
//! The XX pattern provides:
//! - **Mutual authentication**: Both peers prove their identity
//! - **Identity hiding**: Static keys are encrypted during handshake
//! - **Forward secrecy**: Ephemeral keys per session
//!
//! In Wavry, peers may connect for the first time without prior knowledge
//! of each other's identity. XX allows this while still hiding identities
//! from passive observers (relays, network sniffers).
//!
//! # Handshake Flow
//!
//! ```text
//! Initiator (Client)                    Responder (Server)
//!     |                                       |
//!     |  -> e                                 |  ephemeral key
//!     |-------------------------------------->|
//!     |                                       |
//!     |  <- e, ee, s, es                      |  ephemeral, static
//!     |<--------------------------------------|
//!     |                                       |
//!     |  -> s, se                             |  static key
//!     |-------------------------------------->|
//!     |                                       |
//!     [     Session keys established          ]
//! ```
//!
//! After handshake, both sides have symmetric keys for encryption.

use anyhow::{Context, Result};
use snow::{Builder, HandshakeState, TransportState};
use thiserror::Error;

/// Noise protocol pattern (XX with X25519, ChaCha20-Poly1305, BLAKE2s)
const NOISE_PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";

/// Maximum message size for Noise handshake
const MAX_HANDSHAKE_MSG_SIZE: usize = 65535;

/// Noise handshake errors
#[derive(Debug, Error)]
pub enum NoiseError {
    #[error("handshake not complete")]
    HandshakeNotComplete,

    #[error("handshake already complete")]
    HandshakeAlreadyComplete,

    #[error("invalid handshake message")]
    InvalidMessage,

    #[error("encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("decryption failed: {0}")]
    DecryptionFailed(String),

    #[error("snow error: {0}")]
    Snow(#[from] snow::Error),
}

/// Noise handshake initiator (client side).
pub struct NoiseInitiator {
    state: InitiatorState,
    handshake_hash: Option<[u8; 32]>,
}

enum InitiatorState {
    Handshake(Box<HandshakeState>),
    Transport(TransportState),
    Invalid,
}

impl NoiseInitiator {
    /// Create a new initiator with a local static keypair.
    ///
    /// # Arguments
    /// * `local_private_key` - 32-byte X25519 private key
    pub fn new(local_private_key: &[u8; 32]) -> Result<Self> {
        let builder = Builder::new(NOISE_PATTERN.parse()?);

        let state = builder
            .local_private_key(local_private_key)
            .build_initiator()
            .context("failed to build noise initiator")?;

        Ok(Self {
            state: InitiatorState::Handshake(Box::new(state)),
            handshake_hash: None,
        })
    }

    /// Generate the first handshake message (-> e).
    ///
    /// Returns the message to send to the responder.
    pub fn write_message_1(&mut self) -> Result<Vec<u8>, NoiseError> {
        let state = match &mut self.state {
            InitiatorState::Handshake(s) => s,
            _ => return Err(NoiseError::HandshakeAlreadyComplete),
        };

        let mut buf = vec![0u8; MAX_HANDSHAKE_MSG_SIZE];
        let len = state.write_message(&[], &mut buf)?;
        buf.truncate(len);
        Ok(buf)
    }

    /// Process the second handshake message (<- e, ee, s, es).
    ///
    /// Returns any payload included by the responder.
    pub fn read_message_2(&mut self, message: &[u8]) -> Result<Vec<u8>, NoiseError> {
        let state = match &mut self.state {
            InitiatorState::Handshake(s) => s,
            _ => return Err(NoiseError::HandshakeAlreadyComplete),
        };

        let mut buf = vec![0u8; MAX_HANDSHAKE_MSG_SIZE];
        let len = state.read_message(message, &mut buf)?;
        buf.truncate(len);
        Ok(buf)
    }

    /// Generate the third handshake message (-> s, se).
    ///
    /// This completes the handshake on the initiator side.
    /// Returns the message to send and transitions to transport mode.
    pub fn write_message_3(&mut self, payload: &[u8]) -> Result<Vec<u8>, NoiseError> {
        // Take ownership of state to transition
        let old_state = std::mem::replace(&mut self.state, InitiatorState::Invalid);

        let mut handshake = match old_state {
            InitiatorState::Handshake(s) => s,
            other => {
                self.state = other;
                return Err(NoiseError::HandshakeAlreadyComplete);
            }
        };

        let mut buf = vec![0u8; MAX_HANDSHAKE_MSG_SIZE];
        let len = handshake.write_message(payload, &mut buf)?;
        buf.truncate(len);

        // Capture handshake hash before transition
        let handshake_hash: [u8; 32] = handshake.get_handshake_hash().try_into()
            .map_err(|_| NoiseError::InvalidMessage)?;

        // Transition to transport mode
        let transport = handshake.into_transport_mode()?;
        self.state = InitiatorState::Transport(transport);
        self.handshake_hash = Some(handshake_hash);

        Ok(buf)
    }

    /// Check if the handshake is complete and ready for transport.
    pub fn is_handshake_complete(&self) -> bool {
        matches!(self.state, InitiatorState::Transport(_))
    }

    /// Get the responder's static public key (after handshake).
    pub fn get_remote_static(&self) -> Option<[u8; 32]> {
        match &self.state {
            InitiatorState::Transport(t) => {
                t.get_remote_static().map(|s| {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(s);
                    arr
                })
            }
            _ => None,
        }
    }

    /// Convert to a NoiseSession for encrypted transport.
    pub fn into_session(self) -> Result<NoiseSession, NoiseError> {
        let hash = self.handshake_hash.ok_or(NoiseError::HandshakeNotComplete)?;
        match self.state {
            InitiatorState::Transport(t) => Ok(NoiseSession { 
                transport: t, 
                handshake_hash: hash,
            }),
            _ => Err(NoiseError::HandshakeNotComplete),
        }
    }
}

/// Noise handshake responder (server side).
pub struct NoiseResponder {
    state: ResponderState,
    handshake_hash: Option<[u8; 32]>,
}

enum ResponderState {
    Handshake(Box<HandshakeState>),
    Transport(TransportState),
    Invalid,
}

impl NoiseResponder {
    /// Create a new responder with a local static keypair.
    ///
    /// # Arguments
    /// * `local_private_key` - 32-byte X25519 private key
    pub fn new(local_private_key: &[u8; 32]) -> Result<Self> {
        let builder = Builder::new(NOISE_PATTERN.parse()?);

        let state = builder
            .local_private_key(local_private_key)
            .build_responder()
            .context("failed to build noise responder")?;

        Ok(Self {
            state: ResponderState::Handshake(Box::new(state)),
            handshake_hash: None,
        })
    }

    /// Process the first handshake message (-> e).
    ///
    /// Returns any payload included by the initiator (usually empty).
    pub fn read_message_1(&mut self, message: &[u8]) -> Result<Vec<u8>, NoiseError> {
        let state = match &mut self.state {
            ResponderState::Handshake(s) => s,
            _ => return Err(NoiseError::HandshakeAlreadyComplete),
        };

        let mut buf = vec![0u8; MAX_HANDSHAKE_MSG_SIZE];
        let len = state.read_message(message, &mut buf)?;
        buf.truncate(len);
        Ok(buf)
    }

    /// Generate the second handshake message (<- e, ee, s, es).
    ///
    /// Returns the message to send to the initiator.
    pub fn write_message_2(&mut self, payload: &[u8]) -> Result<Vec<u8>, NoiseError> {
        let state = match &mut self.state {
            ResponderState::Handshake(s) => s,
            _ => return Err(NoiseError::HandshakeAlreadyComplete),
        };

        let mut buf = vec![0u8; MAX_HANDSHAKE_MSG_SIZE];
        let len = state.write_message(payload, &mut buf)?;
        buf.truncate(len);
        Ok(buf)
    }

    /// Process the third handshake message (-> s, se).
    ///
    /// This completes the handshake on the responder side.
    /// Returns any payload from the initiator and transitions to transport mode.
    pub fn read_message_3(&mut self, message: &[u8]) -> Result<Vec<u8>, NoiseError> {
        // Take ownership of state to transition
        let old_state = std::mem::replace(&mut self.state, ResponderState::Invalid);

        let mut handshake = match old_state {
            ResponderState::Handshake(s) => s,
            other => {
                self.state = other;
                return Err(NoiseError::HandshakeAlreadyComplete);
            }
        };

        let mut buf = vec![0u8; MAX_HANDSHAKE_MSG_SIZE];
        let len = handshake.read_message(message, &mut buf)?;
        buf.truncate(len);

        // Capture handshake hash before transition
        let handshake_hash: [u8; 32] = handshake.get_handshake_hash().try_into()
            .map_err(|_| NoiseError::InvalidMessage)?;

        // Transition to transport mode
        let transport = handshake.into_transport_mode()?;
        self.state = ResponderState::Transport(transport);
        self.handshake_hash = Some(handshake_hash);

        Ok(buf)
    }

    /// Check if the handshake is complete and ready for transport.
    pub fn is_handshake_complete(&self) -> bool {
        matches!(self.state, ResponderState::Transport(_))
    }

    /// Get the initiator's static public key (after handshake).
    pub fn get_remote_static(&self) -> Option<[u8; 32]> {
        match &self.state {
            ResponderState::Transport(t) => {
                t.get_remote_static().map(|s| {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(s);
                    arr
                })
            }
            _ => None,
        }
    }

    /// Convert to a NoiseSession for encrypted transport.
    pub fn into_session(self) -> Result<NoiseSession, NoiseError> {
        let hash = self.handshake_hash.ok_or(NoiseError::HandshakeNotComplete)?;
        match self.state {
            ResponderState::Transport(t) => Ok(NoiseSession { 
                transport: t, 
                handshake_hash: hash,
            }),
            _ => Err(NoiseError::HandshakeNotComplete),
        }
    }
}

/// Established Noise session for encrypted transport.
///
/// After handshake completion, use this to encrypt/decrypt messages.
pub struct NoiseSession {
    transport: TransportState,
    handshake_hash: [u8; 32],
}

impl NoiseSession {
    /// Get the handshake hash for key derivation.
    pub fn handshake_hash(&self) -> &[u8; 32] {
        &self.handshake_hash
    }
    /// Encrypt a message.
    ///
    /// Returns ciphertext (plaintext + 16-byte auth tag).
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, NoiseError> {
        // Output buffer: plaintext + 16 bytes for AEAD tag
        let mut buf = vec![0u8; plaintext.len() + 16];

        let len = self
            .transport
            .write_message(plaintext, &mut buf)
            .map_err(|e| NoiseError::EncryptionFailed(e.to_string()))?;

        buf.truncate(len);
        Ok(buf)
    }

    /// Decrypt a message.
    ///
    /// Returns plaintext.
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, NoiseError> {
        if ciphertext.len() < 16 {
            return Err(NoiseError::DecryptionFailed("ciphertext too short".into()));
        }

        // Output buffer: ciphertext - 16 bytes for AEAD tag
        let mut buf = vec![0u8; ciphertext.len()];

        let len = self
            .transport
            .read_message(ciphertext, &mut buf)
            .map_err(|e| NoiseError::DecryptionFailed(e.to_string()))?;

        buf.truncate(len);
        Ok(buf)
    }

    /// Get the remote peer's static public key.
    pub fn remote_static(&self) -> Option<[u8; 32]> {
        self.transport.get_remote_static().map(|s| {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(s);
            arr
        })
    }
}

/// Generate a random X25519 keypair for Noise.
///
/// Returns (private_key, public_key).
pub fn generate_noise_keypair() -> ([u8; 32], [u8; 32]) {
    use rand::RngCore;

    let mut private = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut private);

    // Compute public key using x25519-dalek
    let secret = x25519_dalek::StaticSecret::from(private);
    let public = x25519_dalek::PublicKey::from(&secret);

    (private, *public.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_handshake() {
        // Generate keypairs
        let (client_private, _) = generate_noise_keypair();
        let (server_private, _) = generate_noise_keypair();

        // Create handshake states
        let mut initiator = NoiseInitiator::new(&client_private).unwrap();
        let mut responder = NoiseResponder::new(&server_private).unwrap();

        // Message 1: client -> server
        let msg1 = initiator.write_message_1().unwrap();
        responder.read_message_1(&msg1).unwrap();

        // Message 2: server -> client
        let msg2 = responder.write_message_2(&[]).unwrap();
        initiator.read_message_2(&msg2).unwrap();

        // Message 3: client -> server
        let msg3 = initiator.write_message_3(&[]).unwrap();
        responder.read_message_3(&msg3).unwrap();

        // Both should be complete
        assert!(initiator.is_handshake_complete());
        assert!(responder.is_handshake_complete());

        // Both should know each other's public key
        assert!(initiator.get_remote_static().is_some());
        assert!(responder.get_remote_static().is_some());
    }

    #[test]
    fn test_encrypted_transport() {
        // Setup handshake
        let (client_private, _) = generate_noise_keypair();
        let (server_private, _) = generate_noise_keypair();

        let mut initiator = NoiseInitiator::new(&client_private).unwrap();
        let mut responder = NoiseResponder::new(&server_private).unwrap();

        let msg1 = initiator.write_message_1().unwrap();
        responder.read_message_1(&msg1).unwrap();

        let msg2 = responder.write_message_2(&[]).unwrap();
        initiator.read_message_2(&msg2).unwrap();

        let msg3 = initiator.write_message_3(&[]).unwrap();
        responder.read_message_3(&msg3).unwrap();

        // Get transport sessions
        let mut client_session = initiator.into_session().unwrap();
        let mut server_session = responder.into_session().unwrap();

        // Test client -> server encryption
        let plaintext = b"hello from client";
        let ciphertext = client_session.encrypt(plaintext).unwrap();
        let decrypted = server_session.decrypt(&ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);

        // Test server -> client encryption
        let plaintext = b"hello from server";
        let ciphertext = server_session.encrypt(plaintext).unwrap();
        let decrypted = client_session.decrypt(&ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_ciphertext_tamper_detection() {
        // Setup handshake
        let (client_private, _) = generate_noise_keypair();
        let (server_private, _) = generate_noise_keypair();

        let mut initiator = NoiseInitiator::new(&client_private).unwrap();
        let mut responder = NoiseResponder::new(&server_private).unwrap();

        let msg1 = initiator.write_message_1().unwrap();
        responder.read_message_1(&msg1).unwrap();
        let msg2 = responder.write_message_2(&[]).unwrap();
        initiator.read_message_2(&msg2).unwrap();
        let msg3 = initiator.write_message_3(&[]).unwrap();
        responder.read_message_3(&msg3).unwrap();

        let mut client_session = initiator.into_session().unwrap();
        let mut server_session = responder.into_session().unwrap();

        // Encrypt a message
        let plaintext = b"sensitive data";
        let mut ciphertext = client_session.encrypt(plaintext).unwrap();

        // Tamper with ciphertext
        ciphertext[0] ^= 0xff;

        // Decryption should fail
        assert!(server_session.decrypt(&ciphertext).is_err());
    }
}
