#![allow(dead_code)]

#[allow(unused_imports)]
use std::collections::{BTreeMap, VecDeque};
#[allow(unused_imports)]
use std::net::SocketAddr;
#[allow(unused_imports)]
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
#[allow(unused_imports)]
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
#[allow(unused_imports)]
use bytes::Bytes;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot};
use tokio::time;

// Imports
use wavry_media::{Codec, EncodeConfig, EncodedFrame, Renderer, Resolution};

#[cfg(target_os = "macos")]
use wavry_media::{MacAudioCapturer, MacScreenEncoder, MacVideoRenderer as PlatformVideoRenderer};

#[cfg(target_os = "android")]
use wavry_media::AndroidVideoRenderer as PlatformVideoRenderer;

#[cfg(target_os = "macos")]
use rift_core::cc::{DeltaCC, DeltaConfig};
#[allow(unused_imports)]
use rift_core::{
    chunk_video_payload, decode_msg, encode_msg, Codec as RiftCodec,
    CongestionControl as ProtoCongestion, ControlMessage as ProtoControl, Handshake,
    Hello as ProtoHello, HelloAck as ProtoHelloAck, Message as ProtoMessage, PhysicalPacket,
    Pong as ProtoPong, Resolution as ProtoResolution, Role, RIFT_MAGIC, RIFT_VERSION,
};
use rift_crypto::connection::SecureServer;
use wavry_client::{
    run_client as run_rift_client, ClientConfig, ClientRuntimeStats, RelayInfo, RendererFactory,
};
#[cfg(not(any(target_os = "macos", target_os = "android")))]
use wavry_media::DummyRenderer as PlatformVideoRenderer;

#[allow(dead_code)]
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_DATAGRAM_SIZE: usize = 1200;
const NACK_HISTORY: usize = 512;
const PACER_MIN_US: u64 = 20;
const PACER_MAX_US: u64 = 500;
const PACER_BASE_US: f64 = 30.0;

#[derive(Debug)]
struct SendHistory {
    capacity: usize,
    order: VecDeque<u64>,
    packets: BTreeMap<u64, Bytes>,
}

impl SendHistory {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::with_capacity(capacity),
            packets: BTreeMap::new(),
        }
    }

    fn insert(&mut self, packet_id: u64, payload: Bytes) {
        if !self.packets.contains_key(&packet_id) {
            self.order.push_back(packet_id);
        }
        self.packets.insert(packet_id, payload);
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.packets.remove(&oldest);
            }
        }
    }

    fn get(&self, packet_id: u64) -> Option<Bytes> {
        self.packets.get(&packet_id).cloned()
    }
}

#[derive(Debug)]
struct Pacer {
    next_send: time::Instant,
    interval_us: u64,
    rtt_smooth_us: f64,
    rtt_min_us: u64,
    jitter_smooth_us: f64,
    last_packet_bytes: usize,
}

impl Pacer {
    fn new() -> Self {
        Self {
            next_send: time::Instant::now(),
            interval_us: PACER_BASE_US as u64,
            rtt_smooth_us: 0.0,
            rtt_min_us: u64::MAX,
            jitter_smooth_us: 0.0,
            last_packet_bytes: 1200,
        }
    }

    fn on_stats(&mut self, rtt_us: u64, jitter_us: u32, bitrate_kbps: u32) {
        if self.rtt_smooth_us == 0.0 {
            self.rtt_smooth_us = rtt_us as f64;
        } else {
            self.rtt_smooth_us = 0.875 * self.rtt_smooth_us + 0.125 * (rtt_us as f64);
        }
        self.rtt_min_us = self.rtt_min_us.min(rtt_us);
        if self.jitter_smooth_us == 0.0 {
            self.jitter_smooth_us = jitter_us as f64;
        } else {
            self.jitter_smooth_us = 0.75 * self.jitter_smooth_us + 0.25 * (jitter_us as f64);
        }
        self.recompute_interval(bitrate_kbps);
    }

    fn note_packet_bytes(&mut self, bytes: usize, bitrate_kbps: u32) {
        self.last_packet_bytes = bytes.max(1);
        self.recompute_interval(bitrate_kbps);
    }

    fn recompute_interval(&mut self, bitrate_kbps: u32) {
        let bitrate_factor = (20_000.0 / bitrate_kbps.max(1) as f64).clamp(0.5, 2.0);
        let size_factor = (self.last_packet_bytes as f64 / 1200.0).clamp(0.5, 2.0);
        let base_interval = PACER_BASE_US * bitrate_factor * size_factor;

        let rtt_base = if self.rtt_min_us == u64::MAX {
            self.rtt_smooth_us.max(1.0)
        } else {
            self.rtt_min_us as f64
        };
        let rtt_increase = ((self.rtt_smooth_us - rtt_base).max(0.0) / rtt_base).clamp(0.0, 2.0);
        let jitter_norm = (self.jitter_smooth_us / 2000.0).clamp(0.0, 3.0);

        let mut congestion = 1.0 + rtt_increase * 1.5 + jitter_norm * 0.5;
        if rtt_increase < 0.02 && jitter_norm < 0.2 {
            congestion *= 0.8;
        }

        let interval =
            (base_interval * congestion).clamp(PACER_MIN_US as f64, PACER_MAX_US as f64) as u64;
        self.interval_us = interval.max(PACER_MIN_US);
    }

    async fn wait(&mut self) {
        let now = time::Instant::now();
        if self.next_send <= now {
            self.next_send = now;
        }
        let target = self.next_send;
        self.next_send += Duration::from_micros(self.interval_us);
        time::sleep_until(target).await;
    }
}

enum CryptoState {
    Disabled,
    Handshaking(SecureServer),
    Established(SecureServer),
}

impl CryptoState {
    fn is_established(&self) -> bool {
        matches!(self, CryptoState::Established(_))
    }

    fn decrypt(&mut self, packet_id: u64, payload: &[u8]) -> Result<Vec<u8>> {
        match self {
            CryptoState::Disabled => Ok(payload.to_vec()),
            CryptoState::Established(server) => server
                .decrypt(packet_id, payload)
                .map_err(|e| anyhow!("decrypt failed: {}", e)),
            CryptoState::Handshaking(_) => Err(anyhow!("crypto handshake not complete")),
        }
    }

    fn encrypt(&mut self, packet_id: u64, payload: &[u8]) -> Result<Vec<u8>> {
        match self {
            CryptoState::Disabled => Ok(payload.to_vec()),
            CryptoState::Established(server) => server
                .encrypt(packet_id, payload)
                .map_err(|e| anyhow!("encrypt failed: {}", e)),
            CryptoState::Handshaking(_) => Err(anyhow!("crypto handshake not complete")),
        }
    }
}

struct PeerState {
    session_alias: u32,
    session_id: Option<Vec<u8>>,
    pending_crypto_msg2: Option<Bytes>,
    crypto: CryptoState,
    handshake: Handshake,
    next_packet_id: u64,
    frame_id: u64,
    send_history: SendHistory,
    pacer: Pacer,
}

impl PeerState {
    fn new() -> Result<Self> {
        let crypto = SecureServer::new().map_err(|e| anyhow!("crypto init failed: {}", e))?;
        Ok(Self {
            session_alias: rand::random::<u32>().max(1),
            session_id: None,
            pending_crypto_msg2: None,
            crypto: CryptoState::Handshaking(crypto),
            handshake: Handshake::new(Role::Host),
            next_packet_id: 1,
            frame_id: 0,
            send_history: SendHistory::new(NACK_HISTORY),
            pacer: Pacer::new(),
        })
    }
}

// Stats shared with FFI
#[derive(Debug, Default)]
pub struct SessionStats {
    pub connected: AtomicBool,
    pub fps: AtomicU32,
    pub rtt_ms: AtomicU32,
    pub bitrate_kbps: AtomicU32,
    pub frames_encoded: AtomicU64,
    pub frames_decoded: AtomicU64,
}

pub struct SessionHandle {
    pub stop_tx: Option<oneshot::Sender<()>>,
    pub monitor_tx: Option<mpsc::UnboundedSender<u32>>,
    pub stats: Arc<SessionStats>,
}

impl SessionHandle {
    pub fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HostRuntimeConfig {
    pub codec: Codec,
    pub width: u16,
    pub height: u16,
    pub fps: u16,
    pub bitrate_kbps: u32,
    pub keyframe_interval_ms: u32,
    pub display_id: Option<u32>,
}

impl Default for HostRuntimeConfig {
    fn default() -> Self {
        Self {
            codec: Codec::H264,
            width: 1920,
            height: 1080,
            fps: 60,
            bitrate_kbps: 8000,
            keyframe_interval_ms: 2000,
            display_id: None,
        }
    }
}

fn select_codec_for_hello(hello: &ProtoHello, encoder_codec: Codec) -> Option<RiftCodec> {
    let desired = match encoder_codec {
        Codec::Av1 => RiftCodec::Av1,
        Codec::Hevc => RiftCodec::Hevc,
        Codec::H264 => RiftCodec::H264,
    };
    if hello.supported_codecs.contains(&(desired as i32)) {
        Some(desired)
    } else {
        None
    }
}

fn stream_resolution_from_config(config: &EncodeConfig) -> ProtoResolution {
    ProtoResolution {
        width: config.resolution.width as u32,
        height: config.resolution.height as u32,
    }
}

async fn send_rift_msg(
    socket: &UdpSocket,
    peer_state: &mut PeerState,
    peer: SocketAddr,
    msg: ProtoMessage,
) -> Result<()> {
    let plaintext = encode_msg(&msg);
    let packet_id = peer_state.next_packet_id;
    peer_state.next_packet_id = peer_state.next_packet_id.wrapping_add(1);

    let payload = peer_state.crypto.encrypt(packet_id, &plaintext)?;
    let phys = PhysicalPacket {
        version: RIFT_VERSION,
        session_id: None,
        session_alias: Some(peer_state.session_alias),
        packet_id,
        payload: Bytes::from(payload),
    };

    let bytes = phys.encode();
    peer_state.send_history.insert(packet_id, bytes.clone());
    socket.send_to(&bytes, peer).await?;
    Ok(())
}

async fn send_video_frame(
    socket: &UdpSocket,
    peer_state: &mut PeerState,
    peer: SocketAddr,
    frame: EncodedFrame,
    bitrate_kbps: u32,
) -> Result<()> {
    let chunks = chunk_video_payload(
        peer_state.frame_id,
        frame.timestamp_us,
        frame.keyframe,
        &frame.data,
        MAX_DATAGRAM_SIZE,
        frame.capture_duration_us,
        frame.encode_duration_us,
    )
    .map_err(|e| anyhow!("chunking error: {}", e))?;
    peer_state.frame_id = peer_state.frame_id.wrapping_add(1);

    for chunk in chunks {
        let packet_bytes = chunk.payload.len() + 64;
        let msg = ProtoMessage {
            content: Some(rift_core::message::Content::Media(
                rift_core::MediaMessage {
                    content: Some(rift_core::media_message::Content::Video(chunk)),
                },
            )),
        };
        peer_state
            .pacer
            .note_packet_bytes(packet_bytes, bitrate_kbps);
        peer_state.pacer.wait().await;
        send_rift_msg(socket, peer_state, peer, msg).await?;
    }
    Ok(())
}

async fn send_audio_packet(
    socket: &UdpSocket,
    peer_state: &mut PeerState,
    peer: SocketAddr,
    packet: EncodedFrame,
) -> Result<()> {
    let msg = ProtoMessage {
        content: Some(rift_core::message::Content::Media(
            rift_core::MediaMessage {
                content: Some(rift_core::media_message::Content::Audio(
                    rift_core::AudioPacket {
                        timestamp_us: packet.timestamp_us,
                        payload: packet.data,
                    },
                )),
            },
        )),
    };
    send_rift_msg(socket, peer_state, peer, msg).await?;
    Ok(())
}

pub async fn run_host(
    port: u16,
    host_config: HostRuntimeConfig,
    stats: Arc<SessionStats>,
    #[allow(unused_mut)] mut stop_rx: oneshot::Receiver<()>,
    init_tx: oneshot::Sender<Result<u16>>,
) -> Result<()> {
    #![allow(unused_variables)]
    // 1. Setup UDP
    let addr = format!("0.0.0.0:{}", port);

    let socket = match std::net::UdpSocket::bind(&addr) {
        Ok(s) => {
            let _ = s.set_nonblocking(true);
            match UdpSocket::from_std(s) {
                Ok(ts) => Arc::new(ts),
                Err(e) => {
                    let _ = init_tx.send(Err(anyhow!("Failed to convert socket: {}", e)));
                    return Err(e.into());
                }
            }
        }
        Err(e) => {
            let _ = init_tx.send(Err(anyhow!("Failed to bind UDP: {}", e)));
            return Err(e.into());
        }
    };
    let bound_port = socket.local_addr().map(|addr| addr.port()).unwrap_or(port);
    log::info!(
        "Host listening on {} (requested port {}, bound port {})",
        addr,
        port,
        bound_port
    );

    // 2. Setup Encoder
    let config = EncodeConfig {
        codec: host_config.codec,
        resolution: Resolution {
            width: host_config.width,
            height: host_config.height,
        },
        fps: host_config.fps,
        bitrate_kbps: host_config.bitrate_kbps,
        keyframe_interval_ms: host_config.keyframe_interval_ms,
        display_id: host_config.display_id,
        enable_10bit: false,
        enable_hdr: false,
    };

    #[cfg(target_os = "macos")]
    {
        // 2. Setup Encoder (Mac Only)
        let mut encoder = match MacScreenEncoder::new(config).await {
            Ok(enc) => enc,
            Err(e) => {
                let _ = init_tx.send(Err(anyhow!("Failed to create encoder: {}", e)));
                return Err(e);
            }
        };

        // 2b. Setup Audio (Mac Only)
        let mut audio_capturer = match MacAudioCapturer::new().await {
            Ok(ac) => Some(ac),
            Err(e) => {
                log::warn!("Failed to create audio capturer: {}", e);
                None
            }
        };

        // Signal Init Success
        let _ = init_tx.send(Ok(bound_port));

        // Notify Signaling Layer
        crate::signaling_ffi::set_hosting(bound_port);

        // 3. Client state
        let mut client_addr: Option<SocketAddr> = None;
        let mut peer_state: Option<PeerState> = None;

        // 4. DELTA Congestion Control
        let mut cc = DeltaCC::new(
            DeltaConfig::default(),
            config.bitrate_kbps,
            config.fps as u32,
        );
        let mut last_target_bitrate = config.bitrate_kbps;

        // Loop
        let mut fps_counter = 0;
        let mut last_fps_time = std::time::Instant::now();
        let mut last_packet_time = std::time::Instant::now(); // To track client activity
        let mut bytes_sent: u64 = 0;

        loop {
            // Enforce timeout
            if client_addr.is_some() && last_packet_time.elapsed() > CONNECTION_TIMEOUT {
                log::warn!("Client timed out");
                client_addr = None;
                peer_state = None;
                stats.connected.store(false, Ordering::Relaxed);
            }

            tokio::select! {
                _ = &mut stop_rx => {
                    log::info!("Host session stopped");
                    stats.connected.store(false, Ordering::Relaxed);
                    crate::signaling_ffi::clear_hosting();
                    break;
                }

                // Check for incoming packets (Control/Keepalive/Handshake)
                res = async {
                    let mut buf = [0u8; 2048];
                    socket.recv_from(&mut buf).await.map(|(len, src)| (buf, len, src))
                } => {
                    let handled: Result<()> = async {
                        let (buf, len, src) = res?;
                        if client_addr.is_some() && client_addr != Some(src) {
                            // Only one active client for now.
                            return Ok(());
                        }

                        if client_addr.is_none() {
                            client_addr = Some(src);
                            peer_state = Some(PeerState::new()?);
                            log::info!("Client connected from {}", src);
                        }

                        last_packet_time = std::time::Instant::now();

                        if len < 2 || buf[0..2] != RIFT_MAGIC {
                            return Ok(());
                        }

                        let phys = match PhysicalPacket::decode(Bytes::copy_from_slice(&buf[..len])) {
                            Ok(p) => p,
                            Err(e) => {
                                log::warn!("RIFT decode error: {}", e);
                                return Ok(());
                            }
                        };

                        let state = match peer_state.as_mut() {
                            Some(s) => s,
                            None => return Ok(()),
                        };

                        if let CryptoState::Handshaking(server) = &mut state.crypto {
                            if let Some(session_id) = phys.session_id {
                                if session_id == 0 {
                                    let msg2 = if let Some(cached) = state.pending_crypto_msg2.clone() {
                                        log::debug!("resending cached crypto msg2 to {}", src);
                                        cached
                                    } else {
                                        let msg2 = server.process_client_hello(&phys.payload)
                                            .map_err(|e| anyhow!("crypto msg1 error: {}", e))?;
                                        let cached = Bytes::copy_from_slice(&msg2);
                                        state.pending_crypto_msg2 = Some(cached.clone());
                                        cached
                                    };
                                    let resp = PhysicalPacket {
                                        version: RIFT_VERSION,
                                        session_id: Some(0),
                                        session_alias: None,
                                        packet_id: 0,
                                        payload: msg2,
                                    };
                                    let _ = socket.send_to(&resp.encode(), src).await;
                                }
                            } else if phys.session_alias.is_some() {
                                let mut server = match std::mem::replace(&mut state.crypto, CryptoState::Disabled) {
                                    CryptoState::Handshaking(server) => server,
                                    other => {
                                        state.crypto = other;
                                        return Ok(());
                                    }
                                };
                                if let Err(e) = server.process_client_finish(&phys.payload) {
                                    state.crypto = CryptoState::Handshaking(server);
                                    return Err(anyhow!("crypto msg3 error: {}", e));
                                }
                                state.crypto = CryptoState::Established(server);
                                state.pending_crypto_msg2 = None;
                                log::info!("crypto established with {}", src);
                            }
                            return Ok(());
                        }

                        let plaintext = match state.crypto.decrypt(phys.packet_id, &phys.payload) {
                            Ok(p) => p,
                            Err(e) => {
                                log::warn!("decrypt failed: {}", e);
                                return Ok(());
                            }
                        };
                        let msg = match decode_msg(&plaintext) {
                            Ok(m) => m,
                            Err(e) => {
                                log::warn!("RIFT proto decode error: {}", e);
                                return Ok(());
                            }
                        };

                        if let Some(rift_core::message::Content::Control(ctrl)) = msg.content {
                            match ctrl.content {
                                Some(rift_core::control_message::Content::Hello(hello)) => {
                                    if !state.crypto.is_established() {
                                        return Ok(());
                                    }
                                    let selected = select_codec_for_hello(&hello, config.codec);
                                    let accepted = selected.is_some();
                                    let ack = ProtoHelloAck {
                                        accepted,
                                        selected_codec: selected.map(|c| c as i32).unwrap_or(0),
                                        stream_resolution: Some(stream_resolution_from_config(&config)),
                                        fps: config.fps as u32,
                                        initial_bitrate_kbps: config.bitrate_kbps,
                                        keyframe_interval_ms: config.keyframe_interval_ms,
                                        session_id: if accepted {
                                            let sid = rand::random::<[u8; 16]>().to_vec();
                                            state.session_id = Some(sid.clone());
                                            sid
                                        } else {
                                            vec![0u8; 16]
                                        },
                                        session_alias: state.session_alias,
                                        public_addr: String::new(),
                                    };

                                    if accepted {
                                        if let Err(e) = state.handshake.on_receive_hello(&hello) {
                                            log::warn!("handshake error: {}", e);
                                        }
                                        if let Err(e) = state.handshake.on_send_hello_ack(&ack) {
                                            log::warn!("handshake ack error: {}", e);
                                        }
                                        stats.connected.store(true, Ordering::Relaxed);
                                    }

                                    let ack_msg = ProtoMessage {
                                        content: Some(rift_core::message::Content::Control(ProtoControl {
                                            content: Some(rift_core::control_message::Content::HelloAck(ack)),
                                        })),
                                    };
                                    let _ = send_rift_msg(socket.as_ref(), state, src, ack_msg).await;
                                }
                                Some(rift_core::control_message::Content::Ping(ping)) => {
                                    let pong = ProtoMessage {
                                        content: Some(rift_core::message::Content::Control(ProtoControl {
                                            content: Some(rift_core::control_message::Content::Pong(ProtoPong {
                                                timestamp_us: ping.timestamp_us,
                                            })),
                                        })),
                                    };
                                    let _ = send_rift_msg(socket.as_ref(), state, src, pong).await;
                                }
                                Some(rift_core::control_message::Content::Stats(report)) => {
                                    let loss_ratio = if report.received_packets > 0 {
                                        report.lost_packets as f32 / (report.received_packets + report.lost_packets) as f32
                                    } else {
                                        0.0
                                    };
                                    cc.on_rtt_sample(report.rtt_us, loss_ratio, report.jitter_us);
                                    state.pacer.on_stats(report.rtt_us, report.jitter_us, last_target_bitrate);
                                    stats.rtt_ms.store((report.rtt_us / 1000) as u32, Ordering::Relaxed);

                                    let new_bitrate = cc.target_bitrate_kbps();
                                    if new_bitrate != last_target_bitrate {
                                        if let Err(e) = encoder.set_bitrate(new_bitrate) {
                                            log::warn!("Failed to set encoder bitrate: {}", e);
                                        }
                                        last_target_bitrate = new_bitrate;

                                        let cc_msg = ProtoMessage {
                                            content: Some(rift_core::message::Content::Control(ProtoControl {
                                                content: Some(rift_core::control_message::Content::Congestion(ProtoCongestion {
                                                    target_bitrate_kbps: new_bitrate,
                                                    target_fps: cc.target_fps(),
                                                })),
                                            })),
                                        };
                                        let _ = send_rift_msg(socket.as_ref(), state, src, cc_msg).await;
                                    }
                                }
                                Some(rift_core::control_message::Content::Nack(nack)) => {
                                    for packet_id in nack.packet_ids {
                                        if let Some(payload) = state.send_history.get(packet_id) {
                                            let _ = socket.send_to(&payload, src).await;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        Ok(())
                    }.await;

                    if let Err(e) = handled {
                        log::warn!("recv handler error: {}", e);
                    }
                }

                // Encode next frame
                res = encoder.next_frame_async() => {
                    match res {
                        Ok(frame) => {
                            if let (Some(addr), Some(state)) = (client_addr, peer_state.as_mut()) {
                                let ready = state.crypto.is_established() &&
                                    matches!(state.handshake.state(), rift_core::HandshakeState::Established { .. });
                                if ready {
                                    let frame_bytes = frame.data.len();
                                    if let Err(e) = send_video_frame(socket.as_ref(), state, addr, frame, last_target_bitrate).await {
                                        log::warn!("send frame error: {}", e);
                                    }

                                    stats.frames_encoded.fetch_add(1, Ordering::Relaxed);
                                    bytes_sent = bytes_sent.saturating_add(frame_bytes as u64);
                                    fps_counter += 1;
                                    if last_fps_time.elapsed() >= std::time::Duration::from_secs(1) {
                                        stats.fps.store(fps_counter, Ordering::Relaxed);
                                        stats.bitrate_kbps.store((bytes_sent as u32 * 8) / 1000, Ordering::Relaxed);
                                        fps_counter = 0;
                                        bytes_sent = 0;
                                        last_fps_time = std::time::Instant::now();
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Encoder error: {}", e);
                            break;
                        }
                    }
                }

                // Audio packets
                res = async {
                    if let Some(ac) = audio_capturer.as_mut() {
                        ac.next_packet_async().await
                    } else {
                        std::future::pending::<Result<EncodedFrame>>().await
                    }
                } => {
                    if let Ok(packet) = res {
                        if let (Some(addr), Some(state)) = (client_addr, peer_state.as_mut()) {
                             let ready = state.crypto.is_established() &&
                                matches!(state.handshake.state(), rift_core::HandshakeState::Established { .. });
                            if ready {
                                if let Err(e) = send_audio_packet(socket.as_ref(), state, addr, packet).await {
                                    log::warn!("send audio packet error: {}", e);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        // On non-macOS, we just error out immediately.
        let _ = init_tx.send(Err(anyhow!("Hosting only supported on macOS")));
        anyhow::bail!("Hosting only supported on macOS");
    }
}

struct SharedRenderer(Arc<Mutex<Option<Box<PlatformVideoRenderer>>>>);
impl Renderer for SharedRenderer {
    fn render(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        if let Ok(mut g) = self.0.lock() {
            if let Some(r) = g.as_mut() {
                return r.render(payload, timestamp_us);
            }
        }
        Ok(())
    }
}

pub struct ClientSessionParams {
    pub direct_target: Option<(String, u16)>,
    pub relay_info: Option<RelayInfo>,
    pub client_name: String,
    pub renderer_handle: Arc<std::sync::Mutex<Option<Box<PlatformVideoRenderer>>>>,
    pub stats: Arc<SessionStats>,
    pub stop_rx: oneshot::Receiver<()>,
    pub init_tx: oneshot::Sender<Result<()>>,
    pub monitor_rx: mpsc::UnboundedReceiver<u32>,
}

pub async fn run_client(params: ClientSessionParams) -> Result<()> {
    let ClientSessionParams {
        direct_target,
        relay_info,
        client_name,
        renderer_handle,
        stats,
        mut stop_rx,
        init_tx,
        monitor_rx,
    } = params;
    let mut init_tx = Some(init_tx);
    let connect_addr = match direct_target.as_ref() {
        Some((host_ip, port)) => match format!("{}:{}", host_ip, port).parse::<SocketAddr>() {
            Ok(a) => Some(a),
            Err(e) => {
                if let Some(tx) = init_tx.take() {
                    let _ = tx.send(Err(anyhow!("Invalid address: {}", e)));
                }
                return Err(anyhow!("Invalid address: {}", e));
            }
        },
        None => None,
    };
    if connect_addr.is_none() && relay_info.is_none() {
        if let Some(tx) = init_tx.take() {
            let _ = tx.send(Err(anyhow!("No client targets available")));
        }
        return Err(anyhow!("No client targets available"));
    }

    let target_label = if let Some(addr) = connect_addr {
        addr.to_string()
    } else if let Some(relay) = relay_info.as_ref() {
        format!("relay {}", relay.addr)
    } else {
        "unknown target".to_string()
    };

    let runtime_stats = Arc::new(ClientRuntimeStats::default());

    // Config for lib
    let config = ClientConfig {
        connect_addr,
        client_name,
        no_encrypt: false,
        identity_key: crate::identity::get_private_key(),
        relay_info,
        master_url: None, // FFI layer currently doesn't pass master_url
        max_resolution: None,
        gamepad_enabled: true,
        gamepad_deadzone: 0.1,
        vr_adapter: None,
        runtime_stats: Some(runtime_stats.clone()),
        recorder_config: None,
        send_files: Vec::new(),
        file_out_dir: std::path::PathBuf::from("received-files"),
        file_max_bytes: wavry_common::file_transfer::DEFAULT_MAX_FILE_BYTES,
        file_command_bus: None,
    };

    // Factory
    let factory: RendererFactory = Box::new(move |_config| {
        // Return a new SharedRenderer wrapper
        Ok(Box::new(SharedRenderer(renderer_handle.clone())))
    });

    log::info!(
        "Starting Wavry Client (Refactored) connecting to {}",
        target_label
    );
    let mut started = false;
    let startup_deadline = Instant::now() + Duration::from_secs(12);
    let mut stats_tick = time::interval(Duration::from_millis(250));

    let client_fut = run_rift_client(config, Some(factory), Some(monitor_rx));
    tokio::pin!(client_fut);

    loop {
        tokio::select! {
            res = &mut client_fut => {
                stats.connected.store(false, Ordering::Relaxed);
                match res {
                    Ok(_) => {
                        if !started {
                            let err = anyhow!("Connection ended before handshake completed");
                            if let Some(tx) = init_tx.take() {
                                let _ = tx.send(Err(anyhow!(err.to_string())));
                            }
                            return Err(err);
                        }
                        log::info!("Client finished normally");
                        return Ok(());
                    }
                    Err(e) => {
                        if !started {
                            if let Some(tx) = init_tx.take() {
                                let _ = tx.send(Err(anyhow!("Failed to connect: {}", e)));
                            }
                        }
                        return Err(anyhow!("Client returned error: {}", e));
                    }
                }
            }
            _ = &mut stop_rx => {
                stats.connected.store(false, Ordering::Relaxed);
                if !started {
                    if let Some(tx) = init_tx.take() {
                        let _ = tx.send(Err(anyhow!("Client startup canceled")));
                    }
                }
                log::info!("Client stopped via FFI");
                return Ok(());
            }
            _ = stats_tick.tick() => {
                let connected = runtime_stats.connected.load(Ordering::Relaxed);
                stats.connected.store(connected, Ordering::Relaxed);
                stats.frames_decoded.store(
                    runtime_stats.frames_decoded.load(Ordering::Relaxed),
                    Ordering::Relaxed,
                );

                if connected && !started {
                    started = true;
                    if let Some(tx) = init_tx.take() {
                        let _ = tx.send(Ok(()));
                    }
                } else if !started && Instant::now() >= startup_deadline {
                    let err = anyhow!("Timed out waiting for host acknowledgment at {}", target_label);
                    if let Some(tx) = init_tx.take() {
                        let _ = tx.send(Err(anyhow!(err.to_string())));
                    }
                    return Err(err);
                }
            }
        }
    }
}
