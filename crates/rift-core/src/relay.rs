//! Relay wire protocol types.
//!
//! This module defines the binary protocol used between peers and relays.
//! The protocol is designed for:
//! - Minimal overhead (20-byte header)
//! - Fast path for forwarding (magic byte check, then lookup)
//! - Lease-based authentication
//!
//! # Packet Format
//!
//! All packets share a common header:
//!
//! ```text
//!  0                   1                   2                   3
//!  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |  Magic (0x57) |    Version    |     Type      |    Flags      |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                         Session ID                            |
//! |                         (16 bytes)                            |
//! |                                                               |
//! |                                                               |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Magic byte identifying Wavry relay protocol packets.
pub const RELAY_MAGIC: u8 = 0x57; // 'W' for Wavry

/// Current relay protocol version.
pub const RELAY_VERSION: u8 = 1;

/// Minimum packet size (header only).
pub const RELAY_HEADER_SIZE: usize = 20;

/// Maximum packet size for relay forwarding.
pub const RELAY_MAX_PACKET_SIZE: usize = 1500;

/// Maximum payload size (packet - header).
pub const RELAY_MAX_PAYLOAD_SIZE: usize = RELAY_MAX_PACKET_SIZE - RELAY_HEADER_SIZE;

/// Relay packet types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum RelayPacketType {
    /// Peer presenting a lease to the relay.
    LeasePresent = 0x01,
    /// Relay acknowledging lease acceptance.
    LeaseAck = 0x02,
    /// Relay rejecting a lease.
    LeaseReject = 0x03,
    /// Peer renewing an existing lease.
    LeaseRenew = 0x04,
    /// Forwarded data packet.
    Forward = 0x10,
}

impl TryFrom<u8> for RelayPacketType {
    type Error = RelayError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Self::LeasePresent),
            0x02 => Ok(Self::LeaseAck),
            0x03 => Ok(Self::LeaseReject),
            0x04 => Ok(Self::LeaseRenew),
            0x10 => Ok(Self::Forward),
            _ => Err(RelayError::UnknownPacketType(value)),
        }
    }
}

/// Peer role in a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum PeerRole {
    /// Initiator (client).
    Client = 0,
    /// Responder (server/host).
    Server = 1,
}

impl TryFrom<u8> for PeerRole {
    type Error = RelayError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Client),
            1 => Ok(Self::Server),
            _ => Err(RelayError::InvalidPeerRole(value)),
        }
    }
}

/// Reasons for lease rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
pub enum LeaseRejectReason {
    /// Lease has expired.
    Expired = 0x0001,
    /// Token signature verification failed.
    InvalidSignature = 0x0002,
    /// Lease not for this relay.
    WrongRelay = 0x0003,
    /// Relay at capacity.
    SessionFull = 0x0004,
    /// Peer is banned.
    Banned = 0x0005,
    /// Too many requests from this source.
    RateLimited = 0x0006,
}

impl TryFrom<u16> for LeaseRejectReason {
    type Error = RelayError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0x0001 => Ok(Self::Expired),
            0x0002 => Ok(Self::InvalidSignature),
            0x0003 => Ok(Self::WrongRelay),
            0x0004 => Ok(Self::SessionFull),
            0x0005 => Ok(Self::Banned),
            0x0006 => Ok(Self::RateLimited),
            _ => Err(RelayError::UnknownRejectReason(value)),
        }
    }
}

/// Relay protocol errors.
#[derive(Debug, Error)]
pub enum RelayError {
    #[error("packet too short: {0} bytes, minimum {1}")]
    TooShort(usize, usize),

    #[error("invalid magic byte: 0x{0:02x}, expected 0x{1:02x}")]
    InvalidMagic(u8, u8),

    #[error("unsupported version: {0}, expected {1}")]
    UnsupportedVersion(u8, u8),

    #[error("unknown packet type: 0x{0:02x}")]
    UnknownPacketType(u8),

    #[error("invalid peer role: {0}")]
    InvalidPeerRole(u8),

    #[error("unknown reject reason: 0x{0:04x}")]
    UnknownRejectReason(u16),

    #[error("malformed packet: {0}")]
    Malformed(String),
}

/// Relay packet header (20 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayHeader {
    /// Protocol version.
    pub version: u8,
    /// Packet type.
    pub packet_type: RelayPacketType,
    /// Reserved flags.
    pub flags: u8,
    /// Session identifier.
    pub session_id: Uuid,
}

impl RelayHeader {
    /// Create a new header.
    pub fn new(packet_type: RelayPacketType, session_id: Uuid) -> Self {
        Self {
            version: RELAY_VERSION,
            packet_type,
            flags: 0,
            session_id,
        }
    }

    /// Encode header to bytes.
    pub fn encode(&self, buf: &mut [u8]) -> Result<usize, RelayError> {
        if buf.len() < RELAY_HEADER_SIZE {
            return Err(RelayError::TooShort(buf.len(), RELAY_HEADER_SIZE));
        }

        buf[0] = RELAY_MAGIC;
        buf[1] = self.version;
        buf[2] = self.packet_type as u8;
        buf[3] = self.flags;
        buf[4..20].copy_from_slice(self.session_id.as_bytes());

        Ok(RELAY_HEADER_SIZE)
    }

    /// Decode header from bytes.
    pub fn decode(buf: &[u8]) -> Result<Self, RelayError> {
        if buf.len() < RELAY_HEADER_SIZE {
            return Err(RelayError::TooShort(buf.len(), RELAY_HEADER_SIZE));
        }

        if buf[0] != RELAY_MAGIC {
            return Err(RelayError::InvalidMagic(buf[0], RELAY_MAGIC));
        }

        let version = buf[1];
        if version != RELAY_VERSION {
            return Err(RelayError::UnsupportedVersion(version, RELAY_VERSION));
        }

        let packet_type = RelayPacketType::try_from(buf[2])?;
        let flags = buf[3];

        let mut session_bytes = [0u8; 16];
        session_bytes.copy_from_slice(&buf[4..20]);
        let session_id = Uuid::from_bytes(session_bytes);

        Ok(Self {
            version,
            packet_type,
            flags,
            session_id,
        })
    }

    /// Quick check if a buffer might be a valid relay packet.
    ///
    /// This is a fast pre-check before full parsing.
    pub fn quick_check(buf: &[u8]) -> bool {
        buf.len() >= RELAY_HEADER_SIZE && buf[0] == RELAY_MAGIC && buf[1] == RELAY_VERSION
    }
}

/// LEASE_PRESENT packet payload.
#[derive(Debug, Clone)]
pub struct LeasePresentPayload {
    /// Peer's role in the session.
    pub peer_role: PeerRole,
    /// PASETO lease token.
    pub lease_token: Vec<u8>,
}

impl LeasePresentPayload {
    /// Encode to bytes.
    pub fn encode(&self, buf: &mut [u8]) -> Result<usize, RelayError> {
        let token_len = self.lease_token.len();
        let total_len = 1 + 2 + token_len;

        if buf.len() < total_len {
            return Err(RelayError::TooShort(buf.len(), total_len));
        }

        buf[0] = self.peer_role as u8;
        buf[1..3].copy_from_slice(&(token_len as u16).to_be_bytes());
        buf[3..3 + token_len].copy_from_slice(&self.lease_token);

        Ok(total_len)
    }

    /// Decode from bytes.
    pub fn decode(buf: &[u8]) -> Result<Self, RelayError> {
        if buf.len() < 3 {
            return Err(RelayError::TooShort(buf.len(), 3));
        }

        let peer_role = PeerRole::try_from(buf[0])?;
        let token_len = u16::from_be_bytes([buf[1], buf[2]]) as usize;

        if buf.len() < 3 + token_len {
            return Err(RelayError::TooShort(buf.len(), 3 + token_len));
        }

        let lease_token = buf[3..3 + token_len].to_vec();

        Ok(Self {
            peer_role,
            lease_token,
        })
    }
}

/// LEASE_ACK packet payload.
#[derive(Debug, Clone, Copy)]
pub struct LeaseAckPayload {
    /// Lease expiration (Unix timestamp milliseconds).
    pub expires_ms: u64,
    /// Soft rate limit (kbps).
    pub soft_limit_kbps: u32,
    /// Hard rate limit (kbps).
    pub hard_limit_kbps: u32,
}

impl LeaseAckPayload {
    /// Encoded size in bytes.
    pub const SIZE: usize = 16;

    /// Encode to bytes.
    pub fn encode(&self, buf: &mut [u8]) -> Result<usize, RelayError> {
        if buf.len() < Self::SIZE {
            return Err(RelayError::TooShort(buf.len(), Self::SIZE));
        }

        buf[0..8].copy_from_slice(&self.expires_ms.to_be_bytes());
        buf[8..12].copy_from_slice(&self.soft_limit_kbps.to_be_bytes());
        buf[12..16].copy_from_slice(&self.hard_limit_kbps.to_be_bytes());

        Ok(Self::SIZE)
    }

    /// Decode from bytes.
    pub fn decode(buf: &[u8]) -> Result<Self, RelayError> {
        if buf.len() < Self::SIZE {
            return Err(RelayError::TooShort(buf.len(), Self::SIZE));
        }

        let expires_ms = u64::from_be_bytes(buf[0..8].try_into().unwrap());
        let soft_limit_kbps = u32::from_be_bytes(buf[8..12].try_into().unwrap());
        let hard_limit_kbps = u32::from_be_bytes(buf[12..16].try_into().unwrap());

        Ok(Self {
            expires_ms,
            soft_limit_kbps,
            hard_limit_kbps,
        })
    }
}

/// LEASE_REJECT packet payload.
#[derive(Debug, Clone, Copy)]
pub struct LeaseRejectPayload {
    /// Rejection reason.
    pub reason: LeaseRejectReason,
}

impl LeaseRejectPayload {
    /// Encoded size in bytes.
    pub const SIZE: usize = 2;

    /// Encode to bytes.
    pub fn encode(&self, buf: &mut [u8]) -> Result<usize, RelayError> {
        if buf.len() < Self::SIZE {
            return Err(RelayError::TooShort(buf.len(), Self::SIZE));
        }

        buf[0..2].copy_from_slice(&(self.reason as u16).to_be_bytes());
        Ok(Self::SIZE)
    }

    /// Decode from bytes.
    pub fn decode(buf: &[u8]) -> Result<Self, RelayError> {
        if buf.len() < Self::SIZE {
            return Err(RelayError::TooShort(buf.len(), Self::SIZE));
        }

        let reason_code = u16::from_be_bytes([buf[0], buf[1]]);
        let reason = LeaseRejectReason::try_from(reason_code)?;

        Ok(Self { reason })
    }
}

/// FORWARD packet payload header.
#[derive(Debug, Clone, Copy)]
pub struct ForwardPayloadHeader {
    /// Sequence number for replay protection.
    pub sequence: u64,
}

impl ForwardPayloadHeader {
    /// Encoded size in bytes.
    pub const SIZE: usize = 8;

    /// Encode to bytes.
    pub fn encode(&self, buf: &mut [u8]) -> Result<usize, RelayError> {
        if buf.len() < Self::SIZE {
            return Err(RelayError::TooShort(buf.len(), Self::SIZE));
        }

        buf[0..8].copy_from_slice(&self.sequence.to_be_bytes());
        Ok(Self::SIZE)
    }

    /// Decode from bytes.
    pub fn decode(buf: &[u8]) -> Result<Self, RelayError> {
        if buf.len() < Self::SIZE {
            return Err(RelayError::TooShort(buf.len(), Self::SIZE));
        }

        let sequence = u64::from_be_bytes(buf[0..8].try_into().unwrap());
        Ok(Self { sequence })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_roundtrip() {
        let session_id = Uuid::new_v4();
        let header = RelayHeader::new(RelayPacketType::Forward, session_id);

        let mut buf = [0u8; 32];
        header.encode(&mut buf).unwrap();

        let decoded = RelayHeader::decode(&buf).unwrap();
        assert_eq!(decoded.version, RELAY_VERSION);
        assert_eq!(decoded.packet_type, RelayPacketType::Forward);
        assert_eq!(decoded.session_id, session_id);
    }

    #[test]
    fn test_quick_check() {
        let mut buf = [0u8; 32];
        buf[0] = RELAY_MAGIC;
        buf[1] = RELAY_VERSION;

        assert!(RelayHeader::quick_check(&buf));
        assert!(!RelayHeader::quick_check(&[0x00; 32])); // Wrong magic
        assert!(!RelayHeader::quick_check(&[0x57, 0x99])); // Wrong version
        assert!(!RelayHeader::quick_check(&[0x57])); // Too short
    }

    #[test]
    fn test_lease_present_payload() {
        let payload = LeasePresentPayload {
            peer_role: PeerRole::Client,
            lease_token: b"test.token.here".to_vec(),
        };

        let mut buf = [0u8; 256];
        let len = payload.encode(&mut buf).unwrap();

        let decoded = LeasePresentPayload::decode(&buf[..len]).unwrap();
        assert_eq!(decoded.peer_role, PeerRole::Client);
        assert_eq!(decoded.lease_token, b"test.token.here");
    }

    #[test]
    fn test_lease_ack_payload() {
        let payload = LeaseAckPayload {
            expires_ms: 1_000_000_000,
            soft_limit_kbps: 50_000,
            hard_limit_kbps: 100_000,
        };

        let mut buf = [0u8; 16];
        payload.encode(&mut buf).unwrap();

        let decoded = LeaseAckPayload::decode(&buf).unwrap();
        assert_eq!(decoded.expires_ms, 1_000_000_000);
        assert_eq!(decoded.soft_limit_kbps, 50_000);
        assert_eq!(decoded.hard_limit_kbps, 100_000);
    }

    #[test]
    fn test_forward_payload_header() {
        let header = ForwardPayloadHeader { sequence: 42 };

        let mut buf = [0u8; 8];
        header.encode(&mut buf).unwrap();

        let decoded = ForwardPayloadHeader::decode(&buf).unwrap();
        assert_eq!(decoded.sequence, 42);
    }
}
