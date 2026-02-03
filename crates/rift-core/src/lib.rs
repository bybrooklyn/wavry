#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

pub const RIFT_VERSION: u16 = 1;
pub const UNASSIGNED_SESSION_ID: u128 = 0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Channel {
    Control,
    Input,
    Media,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Role {
    Host,
    Client,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Codec {
    Hevc,
    H264,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Windows,
    Macos,
    Freebsd,
    Openbsd,
    Netbsd,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Resolution {
    pub width: u16,
    pub height: u16,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct InputCaps: u32 {
        const KEYBOARD = 0b0001;
        const MOUSE_BUTTONS = 0b0010;
        const MOUSE_RELATIVE = 0b0100;
        const MOUSE_ABSOLUTE = 0b1000;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hello {
    pub client_name: String,
    pub platform: Platform,
    pub supported_codecs: Vec<Codec>,
    pub max_resolution: Resolution,
    pub max_fps: u16,
    pub input_caps: InputCaps,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelloAck {
    pub accepted: bool,
    pub selected_codec: Codec,
    pub stream_resolution: Resolution,
    pub fps: u16,
    pub initial_bitrate_kbps: u32,
    pub keyframe_interval_ms: u32,
    pub session_id: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ping {
    pub timestamp_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Pong {
    pub timestamp_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatsReport {
    pub period_ms: u32,
    pub received_packets: u32,
    pub lost_packets: u32,
    pub rtt_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InputEvent {
    Key { keycode: u32, pressed: bool },
    MouseButton { button: u8, pressed: bool },
    MouseMotion { dx: i32, dy: i32 },
    MouseAbsolute { x: i32, y: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InputMessage {
    pub event: InputEvent,
    pub timestamp_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VideoChunk {
    pub frame_id: u64,
    pub chunk_index: u16,
    pub chunk_count: u16,
    pub timestamp_us: u64,
    pub keyframe: bool,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FecPacket {
    pub group_id: u64,
    pub first_packet_id: u64,
    pub shard_count: u8,
    pub max_payload_len: u16,
    pub payload_sizes: Vec<u16>,
    pub parity_payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ControlMessage {
    Hello(Hello),
    HelloAck(HelloAck),
    Ping(Ping),
    Pong(Pong),
    Stats(StatsReport),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Message {
    Control(ControlMessage),
    Input(InputMessage),
    Video(VideoChunk),
    Fec(FecPacket),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Packet {
    pub version: u16,
    pub session_id: u128,
    pub packet_id: u64,
    pub channel: Channel,
    pub message: Message,
}

#[derive(Debug, thiserror::Error)]
pub enum RiftError {
    #[error("packet decode failed: {0}")]
    Decode(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketPriority {
    Control,
    Input,
    Video,
}

pub fn packet_priority(packet: &Packet) -> PacketPriority {
    match packet.channel {
        Channel::Control => PacketPriority::Control,
        Channel::Input => PacketPriority::Input,
        Channel::Media => PacketPriority::Video,
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ChunkError {
    #[error("max payload must be non-zero")]
    InvalidMaxPayload,
    #[error("too many chunks for a single frame")]
    TooManyChunks,
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
    let chunk_count = (payload.len() + max_payload - 1) / max_payload;
    if chunk_count > u16::MAX as usize {
        return Err(ChunkError::TooManyChunks);
    }
    let mut chunks = Vec::with_capacity(chunk_count);
    for (index, chunk) in payload.chunks(max_payload).enumerate() {
        chunks.push(VideoChunk {
            frame_id,
            chunk_index: index as u16,
            chunk_count: chunk_count as u16,
            timestamp_us,
            keyframe,
            payload: chunk.to_vec(),
        });
    }
    Ok(chunks)
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FecError {
    #[error("payload size list does not match shard count")]
    SizeMismatch,
    #[error("shard count must be at least 2")]
    InvalidShardCount,
    #[error("no missing shard to recover")]
    NoMissingShard,
    #[error("too many missing shards to recover")]
    TooManyMissingShards,
}

#[derive(Debug, Clone)]
pub struct FecBuilder {
    shard_count: u8,
    group_id: u64,
    first_packet_id: Option<u64>,
    payloads: Vec<Vec<u8>>,
    sizes: Vec<u16>,
    max_payload_len: usize,
}

impl FecBuilder {
    pub fn new(shard_count: u8) -> Result<Self, FecError> {
        if shard_count < 2 {
            return Err(FecError::InvalidShardCount);
        }
        Ok(Self {
            shard_count,
            group_id: 0,
            first_packet_id: None,
            payloads: Vec::with_capacity(shard_count as usize),
            sizes: Vec::with_capacity(shard_count as usize),
            max_payload_len: 0,
        })
    }

    pub fn push(&mut self, packet_id: u64, payload: &[u8]) -> Option<FecPacket> {
        if self.payloads.is_empty() {
            self.first_packet_id = Some(packet_id);
        } else if let Some(first) = self.first_packet_id {
            let expected = first + self.payloads.len() as u64;
            if packet_id != expected {
                self.reset();
                self.first_packet_id = Some(packet_id);
            }
        }

        self.max_payload_len = self.max_payload_len.max(payload.len());
        self.payloads.push(payload.to_vec());
        self.sizes.push(payload.len() as u16);

        if self.payloads.len() == self.shard_count as usize {
            let parity_payload = xor_parity(&self.payloads, self.max_payload_len);
            let packet = FecPacket {
                group_id: self.group_id,
                first_packet_id: self.first_packet_id.unwrap_or(packet_id),
                shard_count: self.shard_count,
                max_payload_len: self.max_payload_len as u16,
                payload_sizes: self.sizes.clone(),
                parity_payload,
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
        self.sizes.clear();
        self.max_payload_len = 0;
        self.first_packet_id = None;
    }
}

pub fn recover_missing_shard(
    fec: &FecPacket,
    shards: &mut [Option<Vec<u8>>],
) -> Result<Vec<u8>, FecError> {
    if shards.len() != fec.shard_count as usize || fec.payload_sizes.len() != fec.shard_count as usize
    {
        return Err(FecError::SizeMismatch);
    }

    let mut missing_index = None;
    for (index, shard) in shards.iter().enumerate() {
        if shard.is_none() {
            if missing_index.is_some() {
                return Err(FecError::TooManyMissingShards);
            }
            missing_index = Some(index);
        }
    }

    let missing_index = missing_index.ok_or(FecError::NoMissingShard)?;
    let mut recovered = fec.parity_payload.clone();
    for (index, shard) in shards.iter().enumerate() {
        if index == missing_index {
            continue;
        }
        if let Some(bytes) = shard {
            xor_in_place(&mut recovered, bytes, fec.max_payload_len as usize);
        }
    }

    let size = fec.payload_sizes[missing_index] as usize;
    recovered.truncate(size);
    Ok(recovered)
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
    Established { session_id: u128 },
    Rejected { reason: HandshakeRejection },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeRejection {
    RemoteRejected,
    LocalRejected,
    ProtocolError,
    InvalidSessionId,
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
    #[error("invalid role for this action")]
    InvalidRole,
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
                if ack.accepted && ack.session_id == UNASSIGNED_SESSION_ID {
                    return Err(HandshakeError::InvalidSessionId);
                }
                let next = if ack.accepted {
                    HandshakeState::Established {
                        session_id: ack.session_id,
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
                if ack.accepted && ack.session_id == UNASSIGNED_SESSION_ID {
                    return Err(HandshakeError::InvalidSessionId);
                }
                let next = if ack.accepted {
                    HandshakeState::Established {
                        session_id: ack.session_id,
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

pub fn encode_packet(packet: &Packet) -> Vec<u8> {
    bincode::serialize(packet).unwrap_or_default()
}

pub fn decode_packet(bytes: &[u8]) -> Result<Packet, RiftError> {
    bincode::deserialize(bytes).map_err(|err| RiftError::Decode(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_hello() -> Hello {
        Hello {
            client_name: "test".to_string(),
            platform: Platform::Linux,
            supported_codecs: vec![Codec::Hevc],
            max_resolution: Resolution { width: 1920, height: 1080 },
            max_fps: 60,
            input_caps: InputCaps::KEYBOARD,
        }
    }

    fn sample_ack(accepted: bool, session_id: u128) -> HelloAck {
        HelloAck {
            accepted,
            selected_codec: Codec::Hevc,
            stream_resolution: Resolution { width: 1920, height: 1080 },
            fps: 60,
            initial_bitrate_kbps: 8000,
            keyframe_interval_ms: 1000,
            session_id,
        }
    }

    #[test]
    fn client_happy_path() {
        let mut hs = Handshake::new(Role::Client);
        hs.on_send_hello().unwrap();
        let ack = sample_ack(true, 42);
        let transition = hs.on_receive_hello_ack(&ack).unwrap();
        assert_eq!(
            transition.to,
            HandshakeState::Established { session_id: 42 }
        );
    }

    #[test]
    fn host_happy_path() {
        let mut hs = Handshake::new(Role::Host);
        let hello = sample_hello();
        hs.on_receive_hello(&hello).unwrap();
        let ack = sample_ack(true, 77);
        let transition = hs.on_send_hello_ack(&ack).unwrap();
        assert_eq!(
            transition.to,
            HandshakeState::Established { session_id: 77 }
        );
    }

    #[test]
    fn client_rejects_invalid_order() {
        let mut hs = Handshake::new(Role::Client);
        let ack = sample_ack(true, 1);
        let err = hs.on_receive_hello_ack(&ack).unwrap_err();
        assert_eq!(
            err,
            HandshakeError::InvalidTransition(HandshakeState::Init, HandshakeEvent::ReceiveHelloAck)
        );
    }

    #[test]
    fn host_duplicate_hello_is_error() {
        let mut hs = Handshake::new(Role::Host);
        let hello = sample_hello();
        hs.on_receive_hello(&hello).unwrap();
        let err = hs.on_receive_hello(&hello).unwrap_err();
        assert_eq!(err, HandshakeError::DuplicateHello);
    }

    #[test]
    fn ack_requires_session_id_when_accepted() {
        let mut hs = Handshake::new(Role::Client);
        hs.on_send_hello().unwrap();
        let ack = sample_ack(true, UNASSIGNED_SESSION_ID);
        let err = hs.on_receive_hello_ack(&ack).unwrap_err();
        assert_eq!(err, HandshakeError::InvalidSessionId);
    }

    #[test]
    fn packet_priority_matches_channel() {
        let packet = Packet {
            version: RIFT_VERSION,
            session_id: 1,
            packet_id: 10,
            channel: Channel::Media,
            message: Message::Video(VideoChunk {
                frame_id: 1,
                chunk_index: 0,
                chunk_count: 1,
                timestamp_us: 0,
                keyframe: true,
                payload: vec![0, 1],
            }),
        };
        assert_eq!(packet_priority(&packet), PacketPriority::Video);
    }

    #[test]
    fn chunk_video_payload_splits() {
        let payload = vec![1u8; 10];
        let chunks = chunk_video_payload(5, 123, false, &payload, 4).unwrap();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].chunk_count, 3);
        assert_eq!(chunks[2].payload.len(), 2);
    }

    #[test]
    fn fec_recovers_single_missing() {
        let mut builder = FecBuilder::new(3).unwrap();
        let p0 = vec![1u8, 2, 3];
        let p1 = vec![4u8, 5, 6];
        let p2 = vec![7u8, 8, 9];
        let mut fec = builder.push(10, &p0);
        if fec.is_none() {
            fec = builder.push(11, &p1);
        }
        if fec.is_none() {
            fec = builder.push(12, &p2);
        }
        let fec = fec.expect("fec packet should be emitted after 3 shards");

        let mut shards = vec![Some(p0), None, Some(p2)];
        let recovered = recover_missing_shard(&fec, &mut shards).unwrap();
        assert_eq!(recovered, p1);
    }
}
