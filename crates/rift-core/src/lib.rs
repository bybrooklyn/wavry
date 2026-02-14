//! Core RIFT protocol types, framing, and constants.
//!
//! This crate provides:
//! - Packet types for Control, Input, and Media channels
//! - Handshake state machine
//! - Video chunking and FEC for error correction
//! - Relay wire protocol for forwarding through relays

#![forbid(unsafe_code)]

pub mod relay;

// Removed unused serde imports

pub mod rift {
    include!(concat!(env!("OUT_DIR"), "/rift.rs"));
}

pub use rift::*;

pub const RIFT_VERSION: u16 = 1;
pub const UNASSIGNED_SESSION_ID: u128 = 0;

/// Physical Packet Header for Handshake (26 bytes)
/// [Magic (2B)][Version (2B)][SessionID (16B)][PacketID (8B)][Csum (2B)]
pub const HANDSHAKE_HEADER_SIZE: usize = 30;

/// Physical Packet Header for Transport (14 bytes)
/// [Magic (2B)][Version (2B)][SessionAlias (4B)][PacketID (8B)][Csum (2B)]
pub const TRANSPORT_HEADER_SIZE: usize = 18;

pub const RIFT_MAGIC: [u8; 2] = [0x52, 0x49]; // 'RI'

/// Maximum clipboard text size accepted from the network (1 MiB).
/// Prevents memory exhaustion from malformed or malicious ClipboardMessage payloads.
pub const MAX_CLIPBOARD_TEXT_BYTES: usize = 1024 * 1024;
/// Default maximum file size accepted over file-transfer messages (1 GiB).
pub const MAX_FILE_TRANSFER_BYTES: u64 = 1024 * 1024 * 1024;
/// Default chunk payload size for file transfer.
pub const DEFAULT_FILE_CHUNK_BYTES: usize = 900;

#[derive(Debug, thiserror::Error)]
pub enum RiftError {
    #[error("packet too short: {0}")]
    TooShort(usize),
    #[error("invalid magic: {0:?}")]
    InvalidMagic([u8; 2]),
    #[error("unsupported version: {0}")]
    UnsupportedVersion(u16),
    #[error("checksum mismatch")]
    ChecksumMismatch,
    #[error("protobuf encode error: {0}")]
    ProtoEncode(String),
    #[error("protobuf decode error: {0}")]
    ProtoDecode(String),
}
pub mod cc;
pub mod stun;

use bytes::{Buf, BufMut, Bytes, BytesMut};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalPacket {
    pub version: u16,
    pub session_id: Option<u128>,
    pub session_alias: Option<u32>,
    pub packet_id: u64,
    pub payload: Bytes,
}

impl PhysicalPacket {
    pub fn encode(&self) -> Bytes {
        let is_handshake = self.session_id.is_some();
        let header_size = if is_handshake {
            HANDSHAKE_HEADER_SIZE
        } else {
            TRANSPORT_HEADER_SIZE
        };

        let mut buf = BytesMut::with_capacity(header_size + self.payload.len());
        buf.put_slice(&RIFT_MAGIC);
        buf.put_u16(self.version);

        if let Some(id) = self.session_id {
            buf.put_u128(id);
        } else {
            buf.put_u32(self.session_alias.unwrap_or(0));
        }

        buf.put_u64(self.packet_id);

        // Placeholder for checksum
        let csum_pos = buf.len();
        buf.put_u16(0);

        buf.put_slice(&self.payload);

        // Compute checksum over everything except checksum field
        let mut state = crc16::State::<crc16::KERMIT>::new();
        state.update(&buf[..csum_pos]);
        let csum = state.get();

        // Fill checksum
        let mut buf_slice = &mut buf[csum_pos..csum_pos + 2];
        buf_slice.put_u16(csum);

        buf.freeze()
    }

    pub fn decode(bytes: Bytes) -> Result<Self, RiftError> {
        if bytes.len() < TRANSPORT_HEADER_SIZE {
            return Err(RiftError::TooShort(bytes.len()));
        }

        if bytes[0..2] != RIFT_MAGIC {
            return Err(RiftError::InvalidMagic([bytes[0], bytes[1]]));
        }

        let version = u16::from_be_bytes([bytes[2], bytes[3]]);
        if version != RIFT_VERSION {
            return Err(RiftError::UnsupportedVersion(version));
        }

        // Check if handshake
        let alias_test = u32::from_be_bytes(bytes[4..8].try_into().unwrap());

        if alias_test == 0 && bytes.len() >= HANDSHAKE_HEADER_SIZE {
            // Handshake (30 bytes)
            let session_id = u128::from_be_bytes(bytes[4..20].try_into().unwrap());
            let packet_id = u64::from_be_bytes(bytes[20..28].try_into().unwrap());
            let csum = u16::from_be_bytes([bytes[28], bytes[29]]);

            // Verify checksum
            let mut state = crc16::State::<crc16::KERMIT>::new();
            state.update(&bytes[..28]);
            if state.get() != csum {
                return Err(RiftError::ChecksumMismatch);
            }

            let mut payload = bytes;
            payload.advance(HANDSHAKE_HEADER_SIZE);

            Ok(Self {
                version,
                session_id: Some(session_id),
                session_alias: None,
                packet_id,
                payload,
            })
        } else {
            // Transport (18 bytes)
            let session_alias = alias_test;
            let packet_id = u64::from_be_bytes(bytes[8..16].try_into().unwrap());
            let csum = u16::from_be_bytes([bytes[16], bytes[17]]);

            // Verify checksum
            let mut state = crc16::State::<crc16::KERMIT>::new();
            state.update(&bytes[..16]);
            if state.get() != csum {
                return Err(RiftError::ChecksumMismatch);
            }

            let mut payload = bytes;
            payload.advance(TRANSPORT_HEADER_SIZE);

            Ok(Self {
                version,
                session_id: None,
                session_alias: Some(session_alias),
                packet_id,
                payload,
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketPriority {
    Control,
    Input,
    Video,
}

pub fn packet_priority(channel: Channel) -> PacketPriority {
    match channel {
        Channel::Control => PacketPriority::Control,
        Channel::Input => PacketPriority::Input,
        Channel::Media => PacketPriority::Video,
    }
}

pub fn chunk_video_payload(
    frame_id: u64,
    timestamp_us: u64,
    keyframe: bool,
    payload: &[u8],
    max_payload: usize,
    capture_us: u32,
    encode_us: u32,
) -> Result<Vec<VideoChunk>, ChunkError> {
    if max_payload == 0 {
        return Err(ChunkError::InvalidMaxPayload);
    }
    let chunk_count = payload.len().div_ceil(max_payload);
    if chunk_count > u32::MAX as usize {
        return Err(ChunkError::TooManyChunks);
    }
    let mut chunks = Vec::with_capacity(chunk_count);
    for (index, chunk) in payload.chunks(max_payload).enumerate() {
        chunks.push(VideoChunk {
            frame_id,
            chunk_index: index as u32,
            chunk_count: chunk_count as u32,
            timestamp_us,
            keyframe,
            payload: chunk.to_vec(),
            capture_us,
            encode_us,
        });
    }
    Ok(chunks)
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ChunkError {
    #[error("max payload must be non-zero")]
    InvalidMaxPayload,
    #[error("too many chunks for a single frame")]
    TooManyChunks,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FecError {
    #[error("shard count must be at least 2")]
    InvalidShardCount,
}

#[derive(Debug, Clone)]
pub struct FecBuilder {
    shard_count: u32,
    group_id: u64,
    first_packet_id: Option<u64>,
    payloads: Vec<Vec<u8>>,
    shard_lengths: Vec<u32>,
    max_payload_len: usize,
}

impl FecBuilder {
    pub fn new(shard_count: u32) -> Result<Self, FecError> {
        if shard_count < 2 {
            return Err(FecError::InvalidShardCount);
        }
        Ok(Self {
            shard_count,
            group_id: 0,
            first_packet_id: None,
            payloads: Vec::with_capacity(shard_count as usize),
            shard_lengths: Vec::with_capacity(shard_count as usize),
            max_payload_len: 0,
        })
    }

    pub fn push(&mut self, packet_id: u64, payload: &[u8]) -> Option<FecPacket> {
        if self.payloads.is_empty() {
            self.first_packet_id = Some(packet_id);
        }

        self.max_payload_len = self.max_payload_len.max(payload.len());
        self.shard_lengths.push(payload.len() as u32);
        self.payloads.push(payload.to_vec());

        if self.payloads.len() == (self.shard_count - 1) as usize {
            let parity_payload = xor_parity(&self.payloads, self.max_payload_len);
            let packet = FecPacket {
                group_id: self.group_id,
                first_packet_id: self.first_packet_id.unwrap_or(packet_id),
                shard_count: self.shard_count,
                parity_index: self.shard_count - 1,
                payload: parity_payload,
                shard_lengths: self.shard_lengths.clone(),
            };
            self.group_id = self.group_id.wrapping_add(1);
            self.reset();
            Some(packet)
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.payloads.clear();
        self.shard_lengths.clear();
        self.max_payload_len = 0;
        self.first_packet_id = None;
    }
}

fn xor_parity(payloads: &[Vec<u8>], max_len: usize) -> Vec<u8> {
    let mut parity = vec![0u8; max_len];
    for payload in payloads {
        xor_in_place(&mut parity, payload, max_len);
    }
    parity
}

fn xor_in_place(target: &mut [u8], payload: &[u8], max_len: usize) {
    let len = payload.len().min(max_len);
    for i in 0..len {
        target[i] ^= payload[i];
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeState {
    Init,
    HelloSent,
    HelloReceived,
    Established { session_id: Vec<u8> },
    Rejected { reason: HandshakeRejection },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeRejection {
    RemoteRejected,
    LocalRejected,
    ProtocolError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeEvent {
    SendHello,
    ReceiveHello,
    SendHelloAck,
    ReceiveHelloAck,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeTransition {
    pub from: HandshakeState,
    pub to: HandshakeState,
    pub event: HandshakeEvent,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HandshakeError {
    #[error("invalid state transition from {0:?} via {1:?}")]
    InvalidTransition(HandshakeState, HandshakeEvent),
    #[error("duplicate hello")]
    DuplicateHello,
    #[error("hello ack missing valid session_id")]
    InvalidSessionId,
}

#[derive(Debug, Clone)]
pub struct Handshake {
    role: Role,
    state: HandshakeState,
}

impl Handshake {
    pub fn new(role: Role) -> Self {
        Self {
            role,
            state: HandshakeState::Init,
        }
    }

    pub fn state(&self) -> &HandshakeState {
        &self.state
    }

    pub fn on_send_hello(&mut self) -> Result<HandshakeTransition, HandshakeError> {
        match (self.role, &self.state) {
            (Role::Client, HandshakeState::Init) => {
                self.transition(HandshakeEvent::SendHello, HandshakeState::HelloSent)
            }
            (_, HandshakeState::HelloSent) => Err(HandshakeError::DuplicateHello),
            _ => Err(HandshakeError::InvalidTransition(
                self.state.clone(),
                HandshakeEvent::SendHello,
            )),
        }
    }

    pub fn on_receive_hello(
        &mut self,
        _hello: &Hello,
    ) -> Result<HandshakeTransition, HandshakeError> {
        match (self.role, &self.state) {
            (Role::Host, HandshakeState::Init) => {
                self.transition(HandshakeEvent::ReceiveHello, HandshakeState::HelloReceived)
            }
            (_, HandshakeState::HelloReceived) => Err(HandshakeError::DuplicateHello),
            _ => Err(HandshakeError::InvalidTransition(
                self.state.clone(),
                HandshakeEvent::ReceiveHello,
            )),
        }
    }

    pub fn on_send_hello_ack(
        &mut self,
        ack: &HelloAck,
    ) -> Result<HandshakeTransition, HandshakeError> {
        match (self.role, &self.state) {
            (Role::Host, HandshakeState::HelloReceived) => {
                let next = if ack.accepted {
                    HandshakeState::Established {
                        session_id: ack.session_id.clone(),
                    }
                } else {
                    HandshakeState::Rejected {
                        reason: HandshakeRejection::LocalRejected,
                    }
                };
                self.transition(HandshakeEvent::SendHelloAck, next)
            }
            _ => Err(HandshakeError::InvalidTransition(
                self.state.clone(),
                HandshakeEvent::SendHelloAck,
            )),
        }
    }

    pub fn on_receive_hello_ack(
        &mut self,
        ack: &HelloAck,
    ) -> Result<HandshakeTransition, HandshakeError> {
        match (self.role, &self.state) {
            (Role::Client, HandshakeState::HelloSent) => {
                let next = if ack.accepted {
                    HandshakeState::Established {
                        session_id: ack.session_id.clone(),
                    }
                } else {
                    HandshakeState::Rejected {
                        reason: HandshakeRejection::RemoteRejected,
                    }
                };
                self.transition(HandshakeEvent::ReceiveHelloAck, next)
            }
            _ => Err(HandshakeError::InvalidTransition(
                self.state.clone(),
                HandshakeEvent::ReceiveHelloAck,
            )),
        }
    }

    fn transition(
        &mut self,
        event: HandshakeEvent,
        next: HandshakeState,
    ) -> Result<HandshakeTransition, HandshakeError> {
        let from = self.state.clone();
        let to = next;
        self.state = to.clone();
        Ok(HandshakeTransition { from, to, event })
    }
}

pub fn encode_msg(msg: &Message) -> Vec<u8> {
    use prost::Message as _;
    let mut buf = Vec::with_capacity(msg.encoded_len());
    msg.encode(&mut buf).unwrap();
    buf
}

pub fn decode_msg(bytes: &[u8]) -> Result<Message, RiftError> {
    use prost::Message as _;
    Message::decode(bytes).map_err(|err| RiftError::ProtoDecode(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    fn sample_hello() -> Hello {
        Hello {
            client_name: "test".to_string(),
            platform: Platform::Linux as i32,
            supported_codecs: vec![Codec::Av1 as i32, Codec::Hevc as i32, Codec::H264 as i32],
            max_resolution: Some(Resolution {
                width: 1920,
                height: 1080,
            }),
            max_fps: 60,
            input_caps: 1, // Keyboard
            protocol_version: 1,
            public_addr: "".to_string(),
        }
    }

    fn sample_ack(accepted: bool) -> HelloAck {
        HelloAck {
            accepted,
            selected_codec: Codec::Hevc as i32,
            stream_resolution: Some(Resolution {
                width: 1920,
                height: 1080,
            }),
            fps: 60,
            initial_bitrate_kbps: 8000,
            keyframe_interval_ms: 1000,
            session_id: vec![0u8; 16],
            session_alias: 42,
            public_addr: "".to_string(),
        }
    }

    #[test]
    fn client_happy_path() {
        let mut hs = Handshake::new(Role::Client);
        hs.on_send_hello().unwrap();
        let ack = sample_ack(true);
        let transition = hs.on_receive_hello_ack(&ack).unwrap();
        if let HandshakeState::Established { session_id } = transition.to {
            assert_eq!(session_id.len(), 16);
        } else {
            panic!("Not established");
        }
    }

    #[test]
    fn physical_packet_roundtrip() {
        let packet = PhysicalPacket {
            version: RIFT_VERSION,
            session_id: Some(12345),
            session_alias: None,
            packet_id: 10,
            payload: Bytes::from(vec![1, 2, 3, 4]),
        };
        let bytes = packet.encode();
        let decoded = PhysicalPacket::decode(bytes).unwrap();
        assert_eq!(packet.packet_id, decoded.packet_id);
        assert_eq!(packet.session_id, decoded.session_id);
    }

    #[test]
    fn physical_packet_handshake() {
        let packet = PhysicalPacket {
            version: RIFT_VERSION,
            session_id: Some(99999999999999999),
            session_alias: None,
            packet_id: 888888888,
            payload: Bytes::from(vec![7; 50]),
        };
        let encoded = packet.encode();
        assert!(encoded.len() >= HANDSHAKE_HEADER_SIZE);
        let decoded = PhysicalPacket::decode(encoded).unwrap();
        assert_eq!(decoded.session_id, Some(99999999999999999));
        assert_eq!(decoded.packet_id, 888888888);
        assert_eq!(decoded.payload.len(), 50);
    }

    #[test]
    fn physical_packet_transport() {
        let packet = PhysicalPacket {
            version: RIFT_VERSION,
            session_id: None,
            session_alias: Some(0x12345678),
            packet_id: 999,
            payload: Bytes::from(vec![42; 50]),
        };
        let encoded = packet.encode();
        assert!(encoded.len() >= TRANSPORT_HEADER_SIZE);
        let decoded = PhysicalPacket::decode(encoded).unwrap();
        assert_eq!(decoded.session_alias, Some(0x12345678));
        assert_eq!(decoded.session_id, None);
    }

    #[test]
    fn physical_packet_too_short() {
        let bytes = Bytes::from(vec![1, 2]);
        assert!(matches!(
            PhysicalPacket::decode(bytes),
            Err(RiftError::TooShort(_))
        ));
    }

    #[test]
    fn physical_packet_invalid_magic() {
        let bytes = Bytes::from(vec![
            0xFF, 0xFF, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]);
        assert!(matches!(
            PhysicalPacket::decode(bytes),
            Err(RiftError::InvalidMagic(_))
        ));
    }

    #[test]
    fn physical_packet_unsupported_version() {
        let mut bytes = vec![0x52, 0x49, 0xFF, 0xFF]; // Invalid version
        bytes.extend_from_slice(&[0; 14]);
        let bytes = Bytes::from(bytes);
        assert!(matches!(
            PhysicalPacket::decode(bytes),
            Err(RiftError::UnsupportedVersion(_))
        ));
    }

    #[test]
    fn chunk_video_payload_single_chunk() {
        let payload = vec![1, 2, 3, 4, 5];
        let chunks = chunk_video_payload(1, 1000, true, &payload, 1000, 0, 0).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_count, 1);
        assert_eq!(chunks[0].chunk_index, 0);
        assert!(chunks[0].keyframe);
    }

    #[test]
    fn chunk_video_payload_multiple_chunks() {
        let payload = vec![0; 1000];
        let chunks = chunk_video_payload(1, 1000, false, &payload, 300, 0, 0).unwrap();
        assert_eq!(chunks.len(), 4); // 1000 / 300 = 4 (rounded up)
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_count, 4);
            assert_eq!(chunk.chunk_index, i as u32);
            assert_eq!(chunk.frame_id, 1);
            assert_eq!(chunk.timestamp_us, 1000);
        }
    }

    #[test]
    fn chunk_video_payload_invalid_max_payload() {
        let payload = vec![1, 2, 3];
        let result = chunk_video_payload(1, 1000, true, &payload, 0, 0, 0);
        assert!(matches!(result, Err(ChunkError::InvalidMaxPayload)));
    }

    #[test]
    fn fec_builder_new() {
        let builder = FecBuilder::new(4);
        assert!(builder.is_ok());
        let builder = FecBuilder::new(2);
        assert!(builder.is_ok());
        let builder = FecBuilder::new(1);
        assert!(matches!(builder, Err(FecError::InvalidShardCount)));
    }

    #[test]
    fn packet_priority_mapping() {
        assert_eq!(packet_priority(Channel::Control), PacketPriority::Control);
        assert_eq!(packet_priority(Channel::Input), PacketPriority::Input);
        assert_eq!(packet_priority(Channel::Media), PacketPriority::Video);
    }
}
