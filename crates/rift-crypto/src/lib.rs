//! Cryptographic primitives for Wavry.
//!
//! This crate provides:
//! - Ed25519 identity keys and Wavry IDs
//! - Noise XX handshake for secure session establishment
//! - Encrypted session management with replay protection
//! - Secure connection abstraction for UDP transport
//!
//! # Design
//!
//! Uses the Noise XX pattern (`Noise_XX_25519_ChaChaPoly_BLAKE2s`) which provides:
//! - Mutual authentication (both peers prove identity)
//! - Identity hiding (static keys encrypted during handshake)
//! - Forward secrecy (ephemeral keys per session)
//!
//! For UDP transport, we use explicit nonces (packet_id) to allow
//! decryption of out-of-order packets.

#![forbid(unsafe_code)]

pub mod connection;
pub mod identity;
pub mod noise;
pub mod seq_window;
pub mod session;

pub use identity::{IdentityKeypair, WavryId};
pub use noise::{NoiseInitiator, NoiseResponder, NoiseSession};
pub use seq_window::SequenceWindow;
pub use session::EncryptedSession;
