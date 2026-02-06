//! Encrypted session management.
//!
//! This module provides a high-level API for encrypted RIFT sessions,
//! combining Noise encryption with sequence number tracking for replay protection.

use crate::noise::{NoiseError, NoiseSession};
use crate::seq_window::SequenceWindow;
use anyhow::Result;
use thiserror::Error;

/// Session encryption errors.
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("encryption failed: {0}")]
    Encryption(String),

    #[error("decryption failed: {0}")]
    Decryption(String),

    #[error("replay detected: sequence {0}")]
    Replay(u64),

    #[error("session not established")]
    NotEstablished,

    #[error("noise error: {0}")]
    Noise(#[from] NoiseError),
}

/// Encrypted RIFT session with replay protection.
///
/// Wraps a Noise session with:
/// - Sequence number tracking
/// - Replay window for incoming packets
/// - Automatic nonce management
pub struct EncryptedSession {
    /// Noise transport session
    noise: NoiseSession,

    /// Outgoing sequence number
    tx_seq: u64,

    /// Sequence window for incoming packets
    rx_window: SequenceWindow,

    /// Remote peer's public key (for identification)
    remote_public_key: [u8; 32],
}

impl EncryptedSession {
    /// Create from an established Noise session.
    pub fn new(noise: NoiseSession) -> Result<Self, SessionError> {
        let remote_public_key = noise.remote_static().ok_or(SessionError::NotEstablished)?;

        Ok(Self {
            noise,
            tx_seq: 0,
            rx_window: SequenceWindow::new(),
            remote_public_key,
        })
    }

    /// Get the remote peer's public key.
    pub fn remote_public_key(&self) -> &[u8; 32] {
        &self.remote_public_key
    }

    /// Encrypt a message for sending.
    ///
    /// Returns (sequence_number, ciphertext).
    /// The caller should include the sequence number in the packet header.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<(u64, Vec<u8>), SessionError> {
        let seq = self.tx_seq;
        self.tx_seq = self.tx_seq.wrapping_add(1);

        let ciphertext = self
            .noise
            .encrypt(plaintext)
            .map_err(|e| SessionError::Encryption(e.to_string()))?;

        Ok((seq, ciphertext))
    }

    /// Decrypt a received message with replay protection.
    ///
    /// # Arguments
    /// * `seq` - Sequence number from packet header
    /// * `ciphertext` - Encrypted payload
    ///
    /// # Errors
    /// Returns `SessionError::Replay` if the sequence number was already seen.
    pub fn decrypt(&mut self, seq: u64, ciphertext: &[u8]) -> Result<Vec<u8>, SessionError> {
        // Check replay window BEFORE decryption (fail fast)
        if !self.rx_window.check(seq) {
            return Err(SessionError::Replay(seq));
        }

        // Attempt decryption
        let plaintext = self
            .noise
            .decrypt(ciphertext)
            .map_err(|e| SessionError::Decryption(e.to_string()))?;

        // Only update window after successful decryption
        // (prevents DoS via bogus sequence numbers)
        self.rx_window.check_and_update(seq);

        Ok(plaintext)
    }

    /// Get the next outgoing sequence number (without incrementing).
    pub fn next_tx_seq(&self) -> u64 {
        self.tx_seq
    }

    /// Get the highest received sequence number.
    pub fn highest_rx_seq(&self) -> u64 {
        self.rx_window.highest()
    }
}

/// Session builder for constructing encrypted sessions.
pub struct SessionBuilder {
    noise_private_key: [u8; 32],
}

impl SessionBuilder {
    /// Create a new session builder with the given private key.
    pub fn new(noise_private_key: [u8; 32]) -> Self {
        Self { noise_private_key }
    }

    /// Create an initiator (client) for handshake.
    pub fn build_initiator(&self) -> Result<crate::noise::NoiseInitiator> {
        crate::noise::NoiseInitiator::new(&self.noise_private_key)
    }

    /// Create a responder (server) for handshake.
    pub fn build_responder(&self) -> Result<crate::noise::NoiseResponder> {
        crate::noise::NoiseResponder::new(&self.noise_private_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::noise::{generate_noise_keypair, NoiseInitiator, NoiseResponder};

    fn create_session_pair() -> (EncryptedSession, EncryptedSession) {
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

        let client = EncryptedSession::new(initiator.into_session().unwrap()).unwrap();
        let server = EncryptedSession::new(responder.into_session().unwrap()).unwrap();

        (client, server)
    }

    #[test]
    fn test_encrypted_session() {
        let (mut client, mut server) = create_session_pair();

        // Client sends to server
        let data = b"hello from client";
        let (seq, ciphertext) = client.encrypt(data).unwrap();
        let plaintext = server.decrypt(seq, &ciphertext).unwrap();
        assert_eq!(plaintext, data);

        // Server sends to client
        let data = b"hello from server";
        let (seq, ciphertext) = server.encrypt(data).unwrap();
        let plaintext = client.decrypt(seq, &ciphertext).unwrap();
        assert_eq!(plaintext, data);
    }

    #[test]
    fn test_replay_protection() {
        let (mut client, mut server) = create_session_pair();

        let data = b"important message";
        let (seq, ciphertext) = client.encrypt(data).unwrap();

        // First receipt should succeed
        let _ = server.decrypt(seq, &ciphertext).unwrap();

        // Replay should fail
        let result = server.decrypt(seq, &ciphertext);
        assert!(matches!(result, Err(SessionError::Replay(_))));
    }

    #[test]
    fn test_sequence_numbers() {
        let (mut client, _server) = create_session_pair();

        let (seq1, _) = client.encrypt(b"msg1").unwrap();
        let (seq2, _) = client.encrypt(b"msg2").unwrap();
        let (seq3, _) = client.encrypt(b"msg3").unwrap();

        assert_eq!(seq1, 0);
        assert_eq!(seq2, 1);
        assert_eq!(seq3, 2);
    }

    #[test]
    fn test_in_order_delivery() {
        // NOTE: Noise transport encrypts with an internal nonce counter that
        // MUST be in sync between sender and receiver. This means decryption
        // MUST happen in-order. For UDP with potential packet reordering, we
        // would need to buffer and reorder packets before decryption, or use
        // a different encryption scheme (like DTLS or explicit nonces).
        //
        // The sequence numbers we track in EncryptedSession are for REPLAY
        // PROTECTION, not for reordering decryption.
        let (mut client, mut server) = create_session_pair();

        // Client sends three messages
        let (seq1, ct1) = client.encrypt(b"first").unwrap();
        let (seq2, ct2) = client.encrypt(b"second").unwrap();
        let (seq3, ct3) = client.encrypt(b"third").unwrap();

        assert_eq!(seq1, 0);
        assert_eq!(seq2, 1);
        assert_eq!(seq3, 2);

        // In-order decryption works
        let p1 = server.decrypt(seq1, &ct1).unwrap();
        let p2 = server.decrypt(seq2, &ct2).unwrap();
        let p3 = server.decrypt(seq3, &ct3).unwrap();

        assert_eq!(p1, b"first");
        assert_eq!(p2, b"second");
        assert_eq!(p3, b"third");
    }
}
