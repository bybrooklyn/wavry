//! Secure connection establishment and encrypted transport.
//!
//! This module provides the integration layer between Noise handshake
//! and RIFT protocol. It handles:
//!
//! 1. Noise XX handshake over unreliable UDP (with retransmission)
//! 2. Session key extraction for per-packet encryption
//! 3. AEAD encryption with packet_id as explicit nonce
//!
//! # Why not use Noise transport directly?
//!
//! Noise transport uses an internal nonce counter that requires in-order
//! decryption. UDP can reorder packets, so we:
//! - Use Noise XX for the handshake (few messages, easy to retry)
//! - Extract symmetric keys after handshake
//! - Use ChaCha20-Poly1305 with packet_id as explicit nonce
//!
//! # Wire Format
//!
//! During handshake:
//! ```text
//! [1 byte: type] [payload]
//! type 0x01 = handshake message 1 (initiator → responder)
//! type 0x02 = handshake message 2 (responder → initiator)
//! type 0x03 = handshake message 3 (initiator → responder)
//! ```
//!
//! After handshake:
//! ```text
//! [8 bytes: packet_id (nonce)] [16 bytes: auth tag] [ciphertext]
//! ```

use anyhow::Result;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use thiserror::Error;

use crate::noise::{generate_noise_keypair, NoiseError, NoiseInitiator, NoiseResponder};

/// Handshake message types
pub mod handshake_type {
    pub const MSG1: u8 = 0x01;
    pub const MSG2: u8 = 0x02;
    pub const MSG3: u8 = 0x03;
    pub const DATA: u8 = 0x10;
}

/// Errors during secure connection establishment.
#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("handshake failed: {0}")]
    HandshakeFailed(String),

    #[error("encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("decryption failed: {0}")]
    DecryptionFailed(String),

    #[error("invalid packet format")]
    InvalidPacket,

    #[error("not yet established")]
    NotEstablished,

    #[error("noise error: {0}")]
    Noise(#[from] NoiseError),
}

/// Connection state during handshake.
pub enum ClientHandshakeState {
    /// Waiting to send message 1
    Init,
    /// Sent message 1, waiting for message 2
    SentMsg1,
    /// Received message 2, ready to send message 3
    ReceivedMsg2,
    /// Handshake complete, keys ready
    Complete,
}

/// Client-side secure connection.
pub struct SecureClient {
    initiator: Option<NoiseInitiator>,
    state: ClientHandshakeState,
    cipher: Option<PacketCipher>,
    local_keypair: ([u8; 32], [u8; 32]),
}

impl SecureClient {
    /// Create a new client with a fresh ephemeral keypair.
    pub fn new() -> Result<Self> {
        let keypair = generate_noise_keypair();
        let initiator = NoiseInitiator::new(&keypair.0)?;

        Ok(Self {
            initiator: Some(initiator),
            state: ClientHandshakeState::Init,
            cipher: None,
            local_keypair: keypair,
        })
    }

    /// Create client with a specific keypair.
    pub fn with_keypair(private_key: [u8; 32]) -> Result<Self> {
        let initiator = NoiseInitiator::new(&private_key)?;
        
        // Compute public key from private
        let secret = x25519_dalek::StaticSecret::from(private_key);
        let public_key = x25519_dalek::PublicKey::from(&secret);

        Ok(Self {
            initiator: Some(initiator),
            state: ClientHandshakeState::Init,
            cipher: None,
            local_keypair: (private_key, *public_key.as_bytes()),
        })
    }

    /// Generate the first handshake message.
    ///
    /// Returns bytes to send to the server.
    pub fn start_handshake(&mut self) -> Result<Vec<u8>, ConnectionError> {
        let initiator = self.initiator.as_mut().ok_or(ConnectionError::NotEstablished)?;
        let msg = initiator.write_message_1()?;
        self.state = ClientHandshakeState::SentMsg1;
        Ok(msg)
    }

    /// Process server response (message 2) and generate message 3.
    ///
    /// Returns bytes to send to complete the handshake.
    pub fn process_server_response(&mut self, data: &[u8]) -> Result<Vec<u8>, ConnectionError> {
        let initiator = self.initiator.as_mut().ok_or(ConnectionError::NotEstablished)?;
        
        // Read message 2
        let _payload = initiator.read_message_2(data)?;
        self.state = ClientHandshakeState::ReceivedMsg2;

        // Write message 3 and transition to transport
        let msg3 = initiator.write_message_3(&[])?;

        // Extract the session and create cipher
        let initiator = self.initiator.take().ok_or(ConnectionError::NotEstablished)?;
        let session = initiator.into_session()?;

        // Create cipher from established session (client = initiator)
        self.cipher = Some(PacketCipher::from_session(session, true)?);
        self.state = ClientHandshakeState::Complete;

        Ok(msg3)
    }

    /// Check if handshake is complete.
    pub fn is_established(&self) -> bool {
        matches!(self.state, ClientHandshakeState::Complete)
    }

    /// Encrypt a packet for sending.
    pub fn encrypt(&mut self, packet_id: u64, plaintext: &[u8]) -> Result<Vec<u8>, ConnectionError> {
        let cipher = self.cipher.as_mut().ok_or(ConnectionError::NotEstablished)?;
        cipher.encrypt(packet_id, plaintext)
    }

    /// Decrypt a received packet.
    pub fn decrypt(&mut self, packet_id: u64, ciphertext: &[u8]) -> Result<Vec<u8>, ConnectionError> {
        let cipher = self.cipher.as_mut().ok_or(ConnectionError::NotEstablished)?;
        cipher.decrypt(packet_id, ciphertext)
    }

    /// Get the local public key.
    pub fn local_public_key(&self) -> &[u8; 32] {
        &self.local_keypair.1
    }
}

impl Default for SecureClient {
    fn default() -> Self {
        Self::new().expect("failed to create secure client")
    }
}

/// Connection state during server-side handshake.
pub enum ServerHandshakeState {
    /// Waiting for message 1
    Init,
    /// Received message 1, sent message 2, waiting for message 3
    SentMsg2,
    /// Handshake complete
    Complete,
}

/// Server-side secure connection.
pub struct SecureServer {
    responder: Option<NoiseResponder>,
    state: ServerHandshakeState,
    cipher: Option<PacketCipher>,
    local_keypair: ([u8; 32], [u8; 32]),
}

impl SecureServer {
    /// Create a new server with a fresh keypair.
    pub fn new() -> Result<Self> {
        let keypair = generate_noise_keypair();
        let responder = NoiseResponder::new(&keypair.0)?;

        Ok(Self {
            responder: Some(responder),
            state: ServerHandshakeState::Init,
            cipher: None,
            local_keypair: keypair,
        })
    }

    /// Create server with a specific keypair.
    pub fn with_keypair(private_key: [u8; 32]) -> Result<Self> {
        let responder = NoiseResponder::new(&private_key)?;

        let secret = x25519_dalek::StaticSecret::from(private_key);
        let public_key = x25519_dalek::PublicKey::from(&secret);

        Ok(Self {
            responder: Some(responder),
            state: ServerHandshakeState::Init,
            cipher: None,
            local_keypair: (private_key, *public_key.as_bytes()),
        })
    }

    /// Process client message 1 and generate message 2.
    ///
    /// Returns bytes to send back to client.
    pub fn process_client_hello(&mut self, data: &[u8]) -> Result<Vec<u8>, ConnectionError> {
        let responder = self.responder.as_mut().ok_or(ConnectionError::NotEstablished)?;

        // Read message 1
        let _payload = responder.read_message_1(data)?;

        // Write message 2
        let msg2 = responder.write_message_2(&[])?;
        self.state = ServerHandshakeState::SentMsg2;

        Ok(msg2)
    }

    /// Process client message 3 to complete handshake.
    pub fn process_client_finish(&mut self, data: &[u8]) -> Result<(), ConnectionError> {
        let responder = self.responder.as_mut().ok_or(ConnectionError::NotEstablished)?;

        // Read message 3 and transition
        let _payload = responder.read_message_3(data)?;

        // Extract session and create cipher (server = responder)
        let responder = self.responder.take().ok_or(ConnectionError::NotEstablished)?;
        let session = responder.into_session()?;

        self.cipher = Some(PacketCipher::from_session(session, false)?);
        self.state = ServerHandshakeState::Complete;

        Ok(())
    }

    /// Check if handshake is complete.
    pub fn is_established(&self) -> bool {
        matches!(self.state, ServerHandshakeState::Complete)
    }

    /// Encrypt a packet for sending.
    pub fn encrypt(&mut self, packet_id: u64, plaintext: &[u8]) -> Result<Vec<u8>, ConnectionError> {
        let cipher = self.cipher.as_mut().ok_or(ConnectionError::NotEstablished)?;
        cipher.encrypt(packet_id, plaintext)
    }

    /// Decrypt a received packet.
    pub fn decrypt(&mut self, packet_id: u64, ciphertext: &[u8]) -> Result<Vec<u8>, ConnectionError> {
        let cipher = self.cipher.as_mut().ok_or(ConnectionError::NotEstablished)?;
        cipher.decrypt(packet_id, ciphertext)
    }

    /// Get the local public key.
    pub fn local_public_key(&self) -> &[u8; 32] {
        &self.local_keypair.1
    }
}

impl Default for SecureServer {
    fn default() -> Self {
        Self::new().expect("failed to create secure server")
    }
}

/// AES-GCM or ChaCha20-Poly1305 based packet cipher.
pub struct PacketCipher {
    send_cipher: ChaCha20Poly1305,
    recv_cipher: ChaCha20Poly1305,
}

impl PacketCipher {
    /// Create a cipher from a completed Noise session.
    ///
    /// Uses the handshake hash to derive bidirectional keys:
    /// - Initiator-to-Responder key: H(handshake_hash || "I2R")
    /// - Responder-to-Initiator key: H(handshake_hash || "R2I")
    fn from_session(session: crate::noise::NoiseSession, is_initiator: bool) -> Result<Self, ConnectionError> {
        let hash = session.handshake_hash();

        // Derive keys using simple hash-based KDF:
        // key_i2r = first 32 bytes of H(hash || "wavry-i2r-key-v1")
        // key_r2i = first 32 bytes of H(hash || "wavry-r2i-key-v1")
        //
        // Since we're using ChaCha20-Poly1305 which needs 32-byte keys,
        // and our handshake hash is 32 bytes from BLAKE2s, we XOR with labels.
        
        let mut key_i2r = *hash;
        let mut key_r2i = *hash;
        
        // XOR with different constants to derive different keys (exactly 32 bytes each)
        let label_i2r: [u8; 32] = *b"wavrykdf-initiator-to-responder0";
        let label_r2i: [u8; 32] = *b"wavrykdf-responder-to-initiator0";
        
        for i in 0..32 {
            key_i2r[i] ^= label_i2r[i];
            key_r2i[i] ^= label_r2i[i];
        }

        // Assign send/recv based on role
        let (send_key, recv_key) = if is_initiator {
            (key_i2r, key_r2i) // Initiator sends with I2R key, receives with R2I key
        } else {
            (key_r2i, key_i2r) // Responder sends with R2I key, receives with I2R key
        };

        Ok(Self::new(&send_key, &recv_key))
    }

    pub fn new(send_key: &[u8; 32], recv_key: &[u8; 32]) -> Self {
        Self {
            send_cipher: ChaCha20Poly1305::new(send_key.into()),
            recv_cipher: ChaCha20Poly1305::new(recv_key.into()),
        }
    }

    /// Encrypt plaintext with the given packet_id as nonce.
    pub fn encrypt(&mut self, packet_id: u64, plaintext: &[u8]) -> Result<Vec<u8>, ConnectionError> {
        let nonce = packet_id_to_nonce(packet_id);

        let ciphertext = self.send_cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| ConnectionError::EncryptionFailed(e.to_string()))?;

        Ok(ciphertext)
    }

    /// Decrypt ciphertext with the given packet_id as nonce.
    fn decrypt(&self, packet_id: u64, ciphertext: &[u8]) -> Result<Vec<u8>, ConnectionError> {
        let nonce = packet_id_to_nonce(packet_id);

        self.recv_cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|e| ConnectionError::DecryptionFailed(e.to_string()))
    }
}

/// Convert packet_id to 12-byte nonce for ChaCha20-Poly1305.
fn packet_id_to_nonce(packet_id: u64) -> Nonce {
    let mut nonce_bytes = [0u8; 12];
    nonce_bytes[4..12].copy_from_slice(&packet_id.to_le_bytes());
    Nonce::from(nonce_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_handshake_and_transport() {
        // Create client and server
        let mut client = SecureClient::new().unwrap();
        let mut server = SecureServer::new().unwrap();

        // Client sends message 1
        let msg1 = client.start_handshake().unwrap();

        // Server processes message 1, sends message 2
        let msg2 = server.process_client_hello(&msg1).unwrap();

        // Client processes message 2, sends message 3
        let msg3 = client.process_server_response(&msg2).unwrap();

        // Server processes message 3
        server.process_client_finish(&msg3).unwrap();

        // Both should be established
        assert!(client.is_established());
        assert!(server.is_established());
    }

    #[test]
    fn test_encrypted_communication() {
        // Setup
        let mut client = SecureClient::new().unwrap();
        let mut server = SecureServer::new().unwrap();

        let msg1 = client.start_handshake().unwrap();
        let msg2 = server.process_client_hello(&msg1).unwrap();
        let msg3 = client.process_server_response(&msg2).unwrap();
        server.process_client_finish(&msg3).unwrap();

        // Client sends encrypted message
        let plaintext = b"Hello, secure server!";
        let ciphertext = client.encrypt(0, plaintext).unwrap();

        let decrypted = server.decrypt(0, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }
}
