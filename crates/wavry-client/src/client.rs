use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::Duration,
    fmt,
};
use uuid::Uuid;
use base64::{Engine as _, engine::general_purpose};

use anyhow::{anyhow, Result};
#[cfg(target_os = "linux")]
use evdev::{Device, EventType};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use rift_core::{
    decode_msg, encode_msg, PhysicalPacket, Codec as RiftCodec, ControlMessage as ProtoControl,
    FecPacket as ProtoFecPacket, Handshake, InputMessage as ProtoInputMessage, 
    Message as ProtoMessage, Role, Hello as ProtoHello, Ping as ProtoPing, 
    StatsReport as ProtoStatsReport, Resolution as ProtoResolution, RIFT_VERSION,
    relay::{RelayHeader, RelayPacketType, LeasePresentPayload, PeerRole, RELAY_HEADER_SIZE},
};
use rift_crypto::connection::{SecureClient};
use wavry_media::{Codec, DecodeConfig, Resolution as MediaResolution, Renderer};
#[cfg(target_os = "linux")]
use wavry_media::GstVideoRenderer as VideoRenderer;
#[cfg(not(target_os = "linux"))]
use wavry_media::DummyRenderer as VideoRenderer;
use tokio::{net::UdpSocket, sync::mpsc, time};
use bytes::Bytes;
use tracing::{debug, info, warn};

const FRAME_TIMEOUT_US: u64 = 50_000;
const MAX_FEC_CACHE: usize = 256;

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub connect_addr: Option<SocketAddr>,
    pub client_name: String,
    pub no_encrypt: bool,
    pub identity_key: Option<[u8; 32]>,
    pub relay_info: Option<RelayInfo>,
}

#[derive(Debug, Clone)]
pub struct RelayInfo {
    pub addr: SocketAddr,
    pub token: String,
    pub session_id: Uuid,
}

pub type RendererFactory = Box<dyn Fn(DecodeConfig) -> Result<Box<dyn Renderer + Send>> + Send>;

/// Crypto state for the client
enum CryptoState {
    /// No encryption (--no-encrypt mode)
    Disabled,
    /// Crypto handshake in progress
    Handshaking(SecureClient),
    /// Crypto established
    Established(SecureClient),
}

impl fmt::Debug for CryptoState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => write!(f, "Disabled"),
            Self::Handshaking(_) => write!(f, "Handshaking"),
            Self::Established(_) => write!(f, "Established"),
        }
    }
}

impl CryptoState {
    fn new(disabled: bool, identity_key: Option<[u8; 32]>) -> Result<Self> {
        if disabled {
            Ok(CryptoState::Disabled)
        } else if let Some(key) = identity_key {
            Ok(CryptoState::Handshaking(SecureClient::with_keypair(key)?))
        } else {
            Ok(CryptoState::Handshaking(SecureClient::new()?))
        }
    }
}

pub async fn discover_public_addr(socket: &UdpSocket) -> Result<SocketAddr> {
    use rift_core::stun::StunMessage;
    let stun_server = "stun.l.google.com:19302";
    let stun_msg = StunMessage::new_binding_request();
    let encoded = stun_msg.encode();

    socket.send_to(&encoded, stun_server).await?;

    let mut buf = [0u8; 1024];
    let (len, _) = time::timeout(Duration::from_secs(2), socket.recv_from(&mut buf)).await??;
    
    StunMessage::decode_address(&buf[..len])
}

async fn punch_hole(socket: &UdpSocket, target: SocketAddr) -> Result<()> {
    debug!("attempting UDP hole punch to {}", target);
    for _ in 0..3 {
        socket.send_to(&[0u8; 1], target).await?;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}

async fn present_relay_lease(socket: &UdpSocket, relay: &RelayInfo) -> Result<()> {
    let header = RelayHeader::new(RelayPacketType::LeasePresent, relay.session_id);
    let payload = LeasePresentPayload {
        peer_role: PeerRole::Client,
        lease_token: relay.token.as_bytes().to_vec(),
    };

    let mut buf = [0u8; 2048];
    header.encode(&mut buf).map_err(|e| anyhow!("header encode: {}", e))?;
    let p_len = payload.encode(&mut buf[RELAY_HEADER_SIZE..]).map_err(|e| anyhow!("payload encode: {}", e))?;
    
    socket.send_to(&buf[..RELAY_HEADER_SIZE + p_len], relay.addr).await?;
    info!("presented lease to relay at {}", relay.addr);
    Ok(())
}

pub async fn run_client(config: ClientConfig, renderer_factory: Option<RendererFactory>) -> Result<()> {
    // Note: Logging init removed, caller should init tracing

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    
    // 1. Determine connection strategy
    let p2p_target = match config.connect_addr {
        Some(addr) => Some(addr),
        None => discover_host(Duration::from_secs(1)).await.ok(),
    };

    let (connect_addr, relay_info) = if let Some(target) = p2p_target {
        info!("direct P2P target: {}", target);
        punch_hole(&socket, target).await.ok();
        (target, None)
    } else if let Some(ref relay) = config.relay_info {
        info!("no direct address, using relay: {}", relay.addr);
        present_relay_lease(&socket, relay).await?;
        (relay.addr, Some(relay))
    } else {
        return Err(anyhow!("no connection targets available"));
    };

    if config.no_encrypt {
        warn!("ENCRYPTION DISABLED - not for production use");
    }

    // Initialize crypto state
    let mut crypto = CryptoState::new(config.no_encrypt, config.identity_key)?;

    // Create input channel
    let (input_tx, mut input_rx) = mpsc::channel::<ProtoInputMessage>(128);
    spawn_input_threads(input_tx)?;

    // Perform crypto handshake if enabled
    if let CryptoState::Handshaking(ref mut client) = crypto {
        info!("starting crypto handshake with {}", connect_addr);
        
        // Send msg1
        let msg1_payload = client.start_handshake()
            .map_err(|e| anyhow!("crypto handshake: {}", e))?;
        
        let phys1 = PhysicalPacket {
            version: RIFT_VERSION,
            session_id: Some(0), // Handshake
            session_alias: None,
            packet_id: 0,
            payload: Bytes::copy_from_slice(&msg1_payload),
        };
        socket.send_to(&phys1.encode(), connect_addr).await?;
        debug!("sent crypto msg1");

        // Wait for msg2
        let mut buf_arr = [0u8; 4096];
        let (len, _) = time::timeout(Duration::from_secs(5), socket.recv_from(&mut buf_arr))
            .await
            .map_err(|_| anyhow!("crypto handshake timeout"))??;
        
        let phys2 = PhysicalPacket::decode(Bytes::copy_from_slice(&buf_arr[..len]))
            .map_err(|e| anyhow!("RIFT decode error in handshake: {}", e))?;
        
        debug!("received crypto msg2");

        // Process msg2 and send msg3
        let msg3_payload = client.process_server_response(&phys2.payload)
            .map_err(|e| anyhow!("crypto handshake error in msg3: {}", e))?;
        
        let phys3 = PhysicalPacket {
            version: RIFT_VERSION,
            session_id: None,
            session_alias: Some(1), // Dummy alias for msg3
            packet_id: 0,
            payload: Bytes::copy_from_slice(&msg3_payload),
        };
        socket.send_to(&phys3.encode(), connect_addr).await?;
        debug!("sent crypto msg3");

        info!("crypto handshake complete");
    }

    // Transition to established
    if let CryptoState::Handshaking(client) = crypto {
        crypto = CryptoState::Established(client);
    }

    // Now perform RIFT handshake
    let _ = Handshake::new(Role::Client);
    let hello = ProtoHello {
        client_name: config.client_name,
        platform: rift_core::Platform::Linux as i32,
        supported_codecs: vec![RiftCodec::Hevc as i32, RiftCodec::H264 as i32],
        max_resolution: Some(ProtoResolution { width: 1920, height: 1080 }),
        max_fps: 60,
        input_caps: 0xF, // All caps
        protocol_version: 1,
        public_addr: "".to_string(),
    };

    let msg = ProtoMessage {
        content: Some(rift_core::message::Content::Control(ProtoControl {
            content: Some(rift_core::control_message::Content::Hello(hello)),
        })),
    };

    let packet_counter = Arc::new(AtomicU64::new(1));
    let next_packet_id = || {
        packet_counter.fetch_add(1, Ordering::Relaxed)
    };

    send_rift_msg(&socket, &mut crypto, connect_addr, msg, None, next_packet_id(), relay_info).await?;
    info!("sent RIFT hello to {}", connect_addr);

    // Main recv loop
    let mut buf = vec![0u8; 64 * 1024];
    let mut ping_interval = time::interval(Duration::from_millis(500));
    let mut stats_interval = time::interval(Duration::from_millis(1000));
    
    let mut _session_id: Option<Vec<u8>> = None;
    let mut session_alias: Option<u32> = None;

    let mut last_packet_id: Option<u64> = None;
    let mut received_packets: u32 = 0;
    let mut lost_packets: u32 = 0;
    let mut last_rtt_us: u64 = 0;

    let mut renderer: Option<Box<dyn Renderer + Send>> = None;
    let mut audio_renderer: Option<Box<dyn Renderer + Send>> = None;
    let mut frames = FrameAssembler::new(FRAME_TIMEOUT_US);
    let mut fec_cache = FecCache::new();

    loop {
        tokio::select! {
            // Handle input from capture threads
            Some(input) = input_rx.recv() => {
                if let Some(alias) = session_alias {
                    let msg = ProtoMessage {
                        content: Some(rift_core::message::Content::Input(input)),
                    };
                    if let Err(e) = send_rift_msg(&socket, &mut crypto, connect_addr, msg, Some(alias), next_packet_id(), relay_info).await {
                        debug!("input send error: {}", e);
                    }
                }
            }

            // Ping interval
            _ = ping_interval.tick() => {
                if let Some(alias) = session_alias {
                    let ping = ProtoMessage {
                        content: Some(rift_core::message::Content::Control(ProtoControl {
                            content: Some(rift_core::control_message::Content::Ping(ProtoPing { timestamp_us: now_us() })),
                        })),
                    };
                    send_rift_msg(&socket, &mut crypto, connect_addr, ping, Some(alias), next_packet_id(), relay_info).await?;
                }
            }

            // Stats interval
            _ = stats_interval.tick() => {
                if let Some(alias) = session_alias {
                    let stats = ProtoStatsReport {
                        period_ms: 1000,
                        received_packets,
                        lost_packets,
                        rtt_us: last_rtt_us,
                        jitter_us: 0,
                    };
                    let msg = ProtoMessage {
                        content: Some(rift_core::message::Content::Control(ProtoControl {
                            content: Some(rift_core::control_message::Content::Stats(stats)),
                        })),
                    };
                    received_packets = 0;
                    lost_packets = 0;
                    send_rift_msg(&socket, &mut crypto, connect_addr, msg, Some(alias), next_packet_id(), relay_info).await?;
                }
            }

            // Receive packets
            recv = socket.recv_from(&mut buf) => {
                let (len, peer) = recv?;
                let mut raw = &buf[..len];

                if RelayHeader::quick_check(raw) {
                    if let Ok(relay_header) = RelayHeader::decode(raw) {
                        match relay_header.packet_type {
                            RelayPacketType::Forward => {
                                raw = &raw[RELAY_HEADER_SIZE..];
                            }
                            RelayPacketType::LeaseAck => {
                                info!("relay lease accepted");
                                continue;
                            }
                            RelayPacketType::LeaseReject => {
                                warn!("relay lease rejected");
                                continue;
                            }
                            _ => continue,
                        }
                    }
                }

                let phys = match PhysicalPacket::decode(Bytes::copy_from_slice(raw)) {
                    Ok(p) => p,
                    Err(e) => {
                        debug!("RIFT decode error from {}: {}", peer, e);
                        continue;
                    }
                };

                // Decrypt if needed
                let plaintext = match decrypt_packet(&mut crypto, &phys) {
                    Ok(p) => p,
                    Err(e) => {
                        debug!("decrypt error from {}: {}", peer, e);
                        continue;
                    }
                };

                if let Some(last_id) = last_packet_id {
                    if phys.packet_id > last_id + 1 {
                        lost_packets = lost_packets.saturating_add((phys.packet_id - last_id - 1) as u32);
                    }
                }
                last_packet_id = Some(phys.packet_id);
                received_packets = received_packets.saturating_add(1);

                let msg = match decode_msg(&plaintext) {
                    Ok(m) => m,
                    Err(err) => {
                        warn!("invalid proto msg from {}: {}", peer, err);
                        continue;
                    }
                };

                let content = match msg.content {
                    Some(c) => c,
                    None => continue,
                };

                match content {
                    rift_core::message::Content::Control(ctrl) => {
                        if let Some(ctrl_content) = ctrl.content {
                            match ctrl_content {
                                rift_core::control_message::Content::HelloAck(ack) => {
                                    if !ack.accepted {
                                        warn!("session rejected by {}", peer);
                                        continue;
                                    }
                                    info!("session established with {}", peer);
                                    _session_id = Some(ack.session_id.clone());
                                    session_alias = Some(ack.session_alias);
                                    
                                    if let Some(res) = ack.stream_resolution {
                                        let config = DecodeConfig {
                                            codec: match ack.selected_codec {
                                                c if c == RiftCodec::Hevc as i32 => Codec::Hevc,
                                                _ => Codec::H264,
                                            },
                                            resolution: MediaResolution {
                                                width: res.width as u16,
                                                height: res.height as u16,
                                            },
                                        };

                                        if let Some(factory) = &renderer_factory {
                                            renderer = Some(factory(config)?);
                                        } else {
                                            // Fallback to default platform renderer
                                            let r = VideoRenderer::new(config)?;
                                            renderer = Some(Box::new(r));

                                            #[cfg(target_os = "linux")]
                                            {
                                                let ar = wavry_media::GstAudioRenderer::new()?;
                                                audio_renderer = Some(Box::new(ar));
                                            }
                                        }
                                    }
                                }
                                rift_core::control_message::Content::Pong(pong) => {
                                    last_rtt_us = now_us().saturating_sub(pong.timestamp_us);
                                }
                                _ => {}
                            }
                        }
                    }
                    rift_core::message::Content::Media(media) => {
                        match media.content {
                            Some(rift_core::media_message::Content::Video(chunk)) => {
                                fec_cache.insert(phys.packet_id, plaintext.clone());
                                if let Some(frame) = frames.push(chunk) {
                                    if let Some(r) = renderer.as_mut() {
                                        r.render(&frame.data, frame.timestamp_us)?;
                                    }
                                }
                            }
                            Some(rift_core::media_message::Content::Audio(packet)) => {
                                fec_cache.insert(phys.packet_id, plaintext.clone());
                                if let Some(ar) = audio_renderer.as_mut() {
                                    ar.render(&packet.payload, packet.timestamp_us)?;
                                }
                            }
                            Some(rift_core::media_message::Content::Fec(fec)) => {
                                if let Some(recovered_plaintext) = fec_cache.try_recover(&fec) {
                                    if let Ok(recovered_msg) = decode_msg(&recovered_plaintext) {
                                        if let Some(rift_core::message::Content::Media(recovered_media)) = recovered_msg.content {
                                            match recovered_media.content {
                                                Some(rift_core::media_message::Content::Video(chunk)) => {
                                                    if let Some(frame) = frames.push(chunk) {
                                                        if let Some(r) = renderer.as_mut() {
                                                            r.render(&frame.data, frame.timestamp_us)?;
                                                        }
                                                    }
                                                }
                                                Some(rift_core::media_message::Content::Audio(packet)) => {
                                                    if let Some(ar) = audio_renderer.as_mut() {
                                                        ar.render(&packet.payload, packet.timestamp_us)?;
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn send_rift_msg(
    socket: &UdpSocket,
    crypto: &mut CryptoState,
    dest: SocketAddr,
    msg: ProtoMessage,
    alias: Option<u32>,
    packet_id: u64,
    relay: Option<&RelayInfo>,
) -> Result<()> {
    let plaintext = encode_msg(&msg);

    let payload = match crypto {
        CryptoState::Disabled => plaintext,
        CryptoState::Established(client) => client.encrypt(packet_id, &plaintext)
            .map_err(|e| anyhow!("encrypt failed: {}", e))?,
        CryptoState::Handshaking(_) => return Err(anyhow!("cannot send during crypto handshake")),
    };

    let phys = PhysicalPacket {
        version: RIFT_VERSION,
        session_id: None,
        session_alias: alias,
        packet_id,
        payload: Bytes::copy_from_slice(&payload),
    };

    let rift_bytes = phys.encode();

    if let Some(info) = relay {
        let header = RelayHeader::new(RelayPacketType::Forward, info.session_id);
        let mut buf = vec![0u8; RELAY_HEADER_SIZE + rift_bytes.len()];
        header.encode(&mut buf).map_err(|e| anyhow!("relay header encode: {}", e))?;
        buf[RELAY_HEADER_SIZE..].copy_from_slice(&rift_bytes);
        socket.send_to(&buf, info.addr).await?;
    } else {
        socket.send_to(&rift_bytes, dest).await?;
    }
    Ok(())
}

fn decrypt_packet(crypto: &mut CryptoState, phys: &PhysicalPacket) -> Result<Vec<u8>> {
    match crypto {
        CryptoState::Disabled => Ok(phys.payload.to_vec()),
        CryptoState::Established(client) => {
            client.decrypt(phys.packet_id, &phys.payload)
                .map_err(|e| anyhow!("decrypt failed: {}", e))
        }
        CryptoState::Handshaking(_) => Err(anyhow!("received data during crypto handshake")),
    }
}

// ============= Frame/FEC Types =============

struct FrameAssembler {
    timeout_us: u64,
    frames: HashMap<u64, FrameBuffer>,
}

struct FrameBuffer {
    first_seen_us: u64,
    timestamp_us: u64,
    #[allow(dead_code)]
    keyframe: bool,
    chunk_count: u32,
    chunks: Vec<Option<Vec<u8>>>,
}

struct AssembledFrame {
    timestamp_us: u64,
    data: Vec<u8>,
}

impl FrameAssembler {
    fn new(timeout_us: u64) -> Self {
        Self {
            timeout_us,
            frames: HashMap::new(),
        }
    }

    fn push(&mut self, chunk: rift_core::VideoChunk) -> Option<AssembledFrame> {
        let now = now_us();
        self.frames.retain(|_, frame| now.saturating_sub(frame.first_seen_us) < self.timeout_us);

        let entry = self.frames.entry(chunk.frame_id).or_insert_with(|| FrameBuffer {
            first_seen_us: now,
            timestamp_us: chunk.timestamp_us,
            keyframe: chunk.keyframe,
            chunk_count: chunk.chunk_count,
            chunks: vec![None; chunk.chunk_count as usize],
        });

        if chunk.chunk_index < entry.chunk_count {
            entry.chunks[chunk.chunk_index as usize] = Some(chunk.payload);
        }

        if entry.chunks.iter().all(|c| c.is_some()) {
            let mut assembled = Vec::new();
            for part in entry.chunks.iter_mut() {
                if let Some(bytes) = part.take() {
                    assembled.extend_from_slice(&bytes);
                }
            }
            let timestamp_us = entry.timestamp_us;
            self.frames.remove(&chunk.frame_id);
            return Some(AssembledFrame {
                timestamp_us,
                data: assembled,
            });
        }
        None
    }
}

struct FecCache {
    packets: HashMap<u64, Vec<u8>>,
}

impl FecCache {
    fn new() -> Self {
        Self {
            packets: HashMap::new(),
        }
    }

    #[allow(dead_code)]
    fn insert(&mut self, packet_id: u64, data: Vec<u8>) {
        if self.packets.len() >= MAX_FEC_CACHE {
            if let Some(min_id) = self.packets.keys().min().copied() {
                self.packets.remove(&min_id);
            }
        }
        self.packets.insert(packet_id, data);
    }

    fn try_recover(&self, fec: &ProtoFecPacket) -> Option<Vec<u8>> {
        let mut missing_id = None;
        let mut recovered_payload = fec.payload.clone();
        let mut present_count = 0;

        for offset in 0..(fec.shard_count - 1) {
            let pid = fec.first_packet_id + offset as u64;
            if let Some(p) = self.packets.get(&pid) {
                // XOR in the present packets
                for (i, b) in p.iter().enumerate() {
                    if i < recovered_payload.len() {
                        recovered_payload[i] ^= b;
                    }
                }
                present_count += 1;
            } else {
                if missing_id.is_some() {
                    // More than one missing, can't recover
                    return None;
                }
                missing_id = Some(pid);
            }
        }

        if present_count == (fec.shard_count - 2) {
            // Exactly one missing, we've XORed everything else into the parity
            debug!("FEC: Recovered packet {}", missing_id.unwrap());
            Some(recovered_payload)
        } else {
            None
        }
    }
}

async fn discover_host(timeout: Duration) -> Result<SocketAddr> {
    let handle = tokio::task::spawn_blocking(discover_host_blocking);
    let addr = time::timeout(timeout, handle).await??;
    addr
}

fn discover_host_blocking() -> Result<SocketAddr> {
    let daemon = ServiceDaemon::new()?;
    let receiver = daemon.browse("_wavry._udp.local.")?;
    for event in receiver {
        if let ServiceEvent::ServiceResolved(info) = event {
            if let Some(addr) = info.get_addresses().iter().next() {
                return Ok(SocketAddr::new(*addr, info.get_port()));
            }
        }
    }
    Err(anyhow!("no wavry hosts discovered"))
}

#[cfg(target_os = "linux")]
fn spawn_input_threads(input_tx: mpsc::Sender<ProtoInputMessage>) -> Result<()> {
    let keyboard = find_device(DeviceKind::Keyboard)?;
    let mouse = find_device(DeviceKind::Mouse)?;

    if let Some(mut keyboard) = keyboard {
        let tx = input_tx.clone();
        thread::spawn(move || loop {
            if let Ok(events) = keyboard.fetch_events() {
                for event in events {
                    if event.event_type() == EventType::KEY {
                        let keycode = event.code();
                        let pressed = event.value() != 0;
                        let input = ProtoInputMessage {
                            event: Some(rift_core::input_message::Event::Key(rift_core::Key { keycode: keycode as u32, pressed })),
                            timestamp_us: now_us(),
                        };
                        if tx.blocking_send(input).is_err() {
                            return;
                        }
                    }
                }
            }
        });
    }

    if let Some(mut mouse) = mouse {
        let _tx = input_tx;
        thread::spawn(move || {
            loop {
                if let Ok(events) = mouse.fetch_events() {
                    for _event in events {
                        // ... simple mouse handling ...
                    }
                }
            }
        });
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn spawn_input_threads(input_tx: mpsc::Sender<ProtoInputMessage>) -> Result<()> {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(2));
            let press = ProtoInputMessage {
                event: Some(rift_core::input_message::Event::Key(rift_core::Key { keycode: 30, pressed: true })),
                timestamp_us: now_us(),
            };
            let _ = input_tx.blocking_send(press);
            thread::sleep(Duration::from_millis(100));
            let release = ProtoInputMessage {
                event: Some(rift_core::input_message::Event::Key(rift_core::Key { keycode: 30, pressed: false })),
                timestamp_us: now_us(),
            };
            let _ = input_tx.blocking_send(release);
        }
    });
    Ok(())
}

#[cfg(target_os = "linux")]
enum DeviceKind { Keyboard, Mouse }

#[cfg(target_os = "linux")]
fn find_device(kind: DeviceKind) -> Result<Option<Device>> {
    for (_path, device) in evdev::enumerate() {
        match kind {
            DeviceKind::Keyboard => if device.supported_keys().is_some() { return Ok(Some(device)); }
            DeviceKind::Mouse => if device.supported_relative_axes().is_some() { return Ok(Some(device)); }
        }
    }
    Ok(None)
}

pub fn create_hello_base64(client_name: String, public_addr: Option<String>) -> Result<String> {
    let hello = ProtoHello {
        client_name,
        platform: if cfg!(target_os = "windows") { rift_core::Platform::Windows as i32 } 
                  else if cfg!(target_os = "macos") { rift_core::Platform::Macos as i32 }
                  else { rift_core::Platform::Linux as i32 },
        supported_codecs: vec![RiftCodec::Hevc as i32, RiftCodec::H264 as i32, RiftCodec::Av1 as i32],
        max_resolution: Some(ProtoResolution { width: 1920, height: 1080 }),
        max_fps: 60,
        input_caps: 0xF,
        protocol_version: RIFT_VERSION as u32,
        public_addr: public_addr.unwrap_or_default(),
    };
    let msg = ProtoMessage {
        content: Some(rift_core::message::Content::Control(ProtoControl {
            content: Some(rift_core::control_message::Content::Hello(hello)),
        })),
    };
    let bytes = encode_msg(&msg);
    Ok(general_purpose::STANDARD.encode(bytes))
}

pub fn create_hello_ack_base64(accepted: bool, session_id: [u8; 16], session_alias: u32, public_addr: Option<String>) -> Result<String> {
    let ack = rift_core::HelloAck {
        accepted,
        selected_codec: RiftCodec::Hevc as i32,
        stream_resolution: Some(ProtoResolution { width: 1920, height: 1080 }),
        fps: 60,
        initial_bitrate_kbps: 8000,
        keyframe_interval_ms: 2000,
        session_id: session_id.to_vec(),
        session_alias,
        public_addr: public_addr.unwrap_or_default(),
    };
    let msg = ProtoMessage {
        content: Some(rift_core::message::Content::Control(ProtoControl {
            content: Some(rift_core::control_message::Content::HelloAck(ack)),
        })),
    };
    let bytes = encode_msg(&msg);
    Ok(general_purpose::STANDARD.encode(bytes))
}

pub fn decode_hello_base64(b64: &str) -> Result<ProtoHello> {
    let bytes = general_purpose::STANDARD.decode(b64)?;
    let msg = decode_msg(&bytes)?;
    match msg.content {
        Some(rift_core::message::Content::Control(ctrl)) => {
            match ctrl.content {
                Some(rift_core::control_message::Content::Hello(h)) => Ok(h),
                _ => Err(anyhow!("Not a Hello message")),
            }
        }
        _ => Err(anyhow!("Not a Control message")),
    }
}

pub fn decode_hello_ack_base64(b64: &str) -> Result<rift_core::HelloAck> {
    let bytes = general_purpose::STANDARD.decode(b64)?;
    let msg = decode_msg(&bytes)?;
    match msg.content {
        Some(rift_core::message::Content::Control(ctrl)) => {
            match ctrl.content {
                Some(rift_core::control_message::Content::HelloAck(a)) => Ok(a),
                _ => Err(anyhow!("Not a HelloAck message")),
            }
        }
        _ => Err(anyhow!("Not a Control message")),
    }
}
fn now_us() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros() as u64
}
