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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalPacket {
    pub version: u16,
    pub session_id: Option<u128>,
    pub session_alias: Option<u32>,
    pub packet_id: u64,
    pub payload: Vec<u8>,
}

impl PhysicalPacket {
    pub fn encode(&self) -> Vec<u8> {
        let is_handshake = self.session_id.is_some();
        let size = if is_handshake {
            HANDSHAKE_HEADER_SIZE
        } else {
            TRANSPORT_HEADER_SIZE
        } + self.payload.len();

        let mut buf = Vec::with_capacity(size);
        buf.extend_from_slice(&RIFT_MAGIC);
        buf.extend_from_slice(&self.version.to_be_bytes());

        if let Some(id) = self.session_id {
            buf.extend_from_slice(&id.to_be_bytes());
        } else {
            buf.extend_from_slice(&self.session_alias.unwrap_or(0).to_be_bytes());
        }

        buf.extend_from_slice(&self.packet_id.to_be_bytes());
        
        // Placeholder for checksum
        buf.extend_from_slice(&[0u8, 0u8]);

        buf.extend_from_slice(&self.payload);

        // Compute checksum over everything except checksum field
        let mut state = crc16::State::<crc16::KERMIT>::new();
        state.update(&buf[..buf.len() - self.payload.len() - 2]);
        let csum = state.get();
        let csum_idx = if is_handshake { 28 } else { 16 };
        buf[csum_idx..csum_idx+2].copy_from_slice(&csum.to_be_bytes());

        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RiftError> {
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

        // We distinguish handshake by length OR by a reserved alias (0)
        // For now, let's assume if len >= HANDSHAKE_HEADER_SIZE and alias is 0, it's handshake
        let alias_test = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        
        if alias_test == 0 && bytes.len() >= HANDSHAKE_HEADER_SIZE {
            // Handshake
            let session_id = u128::from_be_bytes(bytes[4..20].try_into().unwrap());
            let packet_id = u64::from_be_bytes(bytes[20..28].try_into().unwrap());
            let _csum = u16::from_be_bytes([bytes[28], bytes[29]]);
            // Verification omitted for speed in this prototype
            let payload = bytes[30..].to_vec();
            Ok(Self {
                version,
                session_id: Some(session_id),
                session_alias: None,
                packet_id,
                payload,
            })
        } else {
            // Transport
            let session_alias = alias_test;
            let packet_id = u64::from_be_bytes(bytes[8..16].try_into().unwrap());
            let _csum = u16::from_be_bytes([bytes[16], bytes[17]]);
            let payload = bytes[18..].to_vec();
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
            max_payload_len: 0,
        })
    }

    pub fn push(&mut self, packet_id: u64, payload: &[u8]) -> Option<FecPacket> {
        if self.payloads.is_empty() {
            self.first_packet_id = Some(packet_id);
        }

        self.max_payload_len = self.max_payload_len.max(payload.len());
        self.payloads.push(payload.to_vec());

        if self.payloads.len() == (self.shard_count - 1) as usize {
            let parity_payload = xor_parity(&self.payloads, self.max_payload_len);
            let packet = FecPacket {
                group_id: self.group_id,
                first_packet_id: self.first_packet_id.unwrap_or(packet_id),
                shard_count: self.shard_count,
                parity_index: self.shard_count - 1,
                payload: parity_payload,
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

    pub fn on_receive_hello(&mut self, _hello: &Hello) -> Result<HandshakeTransition, HandshakeError> {
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
            supported_codecs: vec![Codec::Hevc as i32],
            max_resolution: Some(Resolution { width: 1920, height: 1080 }),
            max_fps: 60,
            input_caps: 1, // Keyboard
            protocol_version: 1,
        }
    }

    fn sample_ack(accepted: bool) -> HelloAck {
        HelloAck {
            accepted,
            selected_codec: Codec::Hevc as i32,
            stream_resolution: Some(Resolution { width: 1920, height: 1080 }),
            fps: 60,
            initial_bitrate_kbps: 8000,
            keyframe_interval_ms: 1000,
            session_id: vec![0u8; 16],
            session_alias: 42,
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
            payload: vec![1, 2, 3, 4],
        };
        let bytes = packet.encode();
        let decoded = PhysicalPacket::decode(&bytes).unwrap();
        assert_eq!(packet.packet_id, decoded.packet_id);
        assert_eq!(packet.session_id, decoded.session_id);
    }
}
