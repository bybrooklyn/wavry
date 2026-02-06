use base64::{engine::general_purpose, Engine as _};
use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    fmt,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

use anyhow::{anyhow, Result};
use bytes::Bytes;
#[cfg(target_os = "linux")]
use evdev::{Device, EventType};
use gilrs::{Event, EventType as GilrsEventType, Gilrs};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use rift_core::{
    decode_msg, encode_msg,
    relay::{LeasePresentPayload, PeerRole, RelayHeader, RelayPacketType, RELAY_HEADER_SIZE},
    Codec as RiftCodec, ControlMessage as ProtoControl, FecPacket as ProtoFecPacket, Handshake,
    Hello as ProtoHello, InputMessage as ProtoInputMessage, Message as ProtoMessage,
    PhysicalPacket, Ping as ProtoPing, Resolution as ProtoResolution, Role,
    StatsReport as ProtoStatsReport, RIFT_VERSION,
};
use rift_crypto::connection::SecureClient;
use socket2::SockRef;
use tokio::{net::UdpSocket, sync::mpsc, time};
use tracing::{debug, info, warn};
use wavry_vr::{VrAdapter, VrAdapterCallbacks};
use wavry_vr::types::{
    EncoderControl as VrEncoderControl, NetworkStats as VrNetworkStats, Pose as VrPose,
    StreamConfig as VrStreamConfig, VideoCodec as VrVideoCodec, VideoFrame as VrVideoFrame, VrTiming,
};
#[cfg(not(target_os = "linux"))]
use wavry_media::DummyRenderer as VideoRenderer;
#[cfg(target_os = "linux")]
use wavry_media::GstVideoRenderer as VideoRenderer;
use wavry_media::CapabilityProbe;
use wavry_media::{Codec, DecodeConfig, Renderer, Resolution as MediaResolution};

fn probe_supported_codecs() -> Vec<Codec> {
    #[cfg(target_os = "windows")]
    {
        return wavry_media::WindowsProbe
            .supported_decoders()
            .unwrap_or_else(|_| vec![Codec::H264]);
    }
    #[cfg(target_os = "macos")]
    {
        return wavry_media::MacProbe
            .supported_decoders()
            .unwrap_or_else(|_| vec![Codec::H264]);
    }
    #[cfg(target_os = "linux")]
    {
        return wavry_media::LinuxProbe
            .supported_decoders()
            .unwrap_or_else(|_| vec![Codec::H264]);
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        vec![Codec::H264]
    }
}

const FRAME_TIMEOUT_US: u64 = 50_000;
const MAX_FEC_CACHE: usize = 256;
const DSCP_EF: u32 = 0x2E;
const NACK_WINDOW_SIZE: u64 = 128;
const JITTER_GROW_THRESHOLD_US: f64 = 2_000.0;
const JITTER_SHRINK_THRESHOLD_US: f64 = 500.0;
const JITTER_MAX_BUFFER_US: u64 = 10_000;

#[derive(Clone)]
pub struct ClientConfig {
    pub connect_addr: Option<SocketAddr>,
    pub client_name: String,
    pub no_encrypt: bool,
    pub identity_key: Option<[u8; 32]>,
    pub relay_info: Option<RelayInfo>,
    pub max_resolution: Option<MediaResolution>,
    pub vr_adapter: Option<Arc<Mutex<dyn VrAdapter>>>,
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

enum VrOutbound {
    Pose(rift_core::PoseUpdate),
    Timing(rift_core::VrTiming),
    Gamepad(rift_core::InputMessage),
}

struct ClientVrCallbacks {
    tx: mpsc::Sender<VrOutbound>,
}

impl VrAdapterCallbacks for ClientVrCallbacks {
    fn on_video_frame(&self, _frame: VrVideoFrame, _timestamp_us: u64, _frame_id: u64) {
        // Client-side adapter should not originate video frames.
    }

    fn on_pose_update(&self, pose: VrPose, timestamp_us: u64) {
        let msg = rift_core::PoseUpdate {
            timestamp_us,
            position_x: pose.position[0],
            position_y: pose.position[1],
            position_z: pose.position[2],
            orientation_x: pose.orientation[0],
            orientation_y: pose.orientation[1],
            orientation_z: pose.orientation[2],
            orientation_w: pose.orientation[3],
        };
        let _ = self.tx.try_send(VrOutbound::Pose(msg));
    }

    fn on_vr_timing(&self, timing: VrTiming) {
        let msg = rift_core::VrTiming {
            refresh_hz: timing.refresh_hz,
            vsync_offset_us: timing.vsync_offset_us,
        };
        let _ = self.tx.try_send(VrOutbound::Timing(msg));
    }

    fn on_gamepad_input(&self, input: wavry_vr::types::GamepadInput) {
        let axes = input
            .axes
            .into_iter()
            .map(|axis| rift_core::GamepadAxis {
                axis: axis.axis,
                value: axis.value,
            })
            .collect();
        let buttons = input
            .buttons
            .into_iter()
            .map(|button| rift_core::GamepadButton {
                button: button.button,
                pressed: button.pressed,
            })
            .collect();
        let msg = rift_core::InputMessage {
            timestamp_us: input.timestamp_us,
            event: Some(rift_core::input_message::Event::Gamepad(
                rift_core::GamepadMessage {
                    gamepad_id: input.gamepad_id,
                    axes,
                    buttons,
                },
            )),
        };
        let _ = self.tx.try_send(VrOutbound::Gamepad(msg));
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
    header
        .encode(&mut buf)
        .map_err(|e| anyhow!("header encode: {}", e))?;
    let p_len = payload
        .encode(&mut buf[RELAY_HEADER_SIZE..])
        .map_err(|e| anyhow!("payload encode: {}", e))?;

    socket
        .send_to(&buf[..RELAY_HEADER_SIZE + p_len], relay.addr)
        .await?;
    info!("presented lease to relay at {}", relay.addr);
    Ok(())
}

pub async fn run_client(
    config: ClientConfig,
    renderer_factory: Option<RendererFactory>,
) -> Result<()> {
    // Note: Logging init removed, caller should init tracing

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    if let Err(e) = SockRef::from(&socket).set_tos_v4(DSCP_EF) {
        debug!("failed to set DSCP/TOS: {}", e);
    }

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

    // VR adapter wiring (optional)
    let (vr_tx, mut vr_rx) = mpsc::channel::<VrOutbound>(64);
    let vr_adapter: Option<Arc<Mutex<dyn VrAdapter>>> = if let Some(adapter) = config.vr_adapter.clone() {
        let cb = Arc::new(ClientVrCallbacks { tx: vr_tx });
        let start_ok = match adapter.lock() {
            Ok(mut guard) => match guard.start(cb) {
                Ok(()) => true,
                Err(e) => {
                    warn!("vr adapter start failed: {}", e);
                    false
                }
            },
            Err(e) => {
                warn!("vr adapter lock failed: {}", e);
                false
            }
        };
        if start_ok {
            Some(adapter)
        } else {
            None
        }
    } else {
        None
    };

    // Perform crypto handshake if enabled
    if let CryptoState::Handshaking(ref mut client) = crypto {
        info!("starting crypto handshake with {}", connect_addr);

        // Send msg1
        let msg1_payload = client
            .start_handshake()
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
        let msg3_payload = client
            .process_server_response(&phys2.payload)
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
    let supported_codecs = probe_supported_codecs();

    let supported_codecs: Vec<i32> = supported_codecs
        .into_iter()
        .map(|c| match c {
            Codec::Av1 => RiftCodec::Av1 as i32,
            Codec::Hevc => RiftCodec::Hevc as i32,
            Codec::H264 => RiftCodec::H264 as i32,
        })
        .collect();

    let hello = ProtoHello {
        client_name: config.client_name,
        platform: rift_core::Platform::Linux as i32,
        supported_codecs,
        max_resolution: config.max_resolution.map(|r| ProtoResolution {
            width: r.width as u32,
            height: r.height as u32,
        }),
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
    let next_packet_id = || packet_counter.fetch_add(1, Ordering::Relaxed);

    send_rift_msg(
        &socket,
        &mut crypto,
        connect_addr,
        msg,
        None,
        next_packet_id(),
        relay_info,
    )
    .await?;
    info!("sent RIFT hello to {}", connect_addr);

    // Main recv loop
    let mut buf = vec![0u8; 64 * 1024];
    let mut ping_interval = time::interval(Duration::from_millis(500));
    let mut stats_interval = time::interval(Duration::from_millis(1000));
    let mut jitter_interval = time::interval(Duration::from_millis(1));

    let mut _session_id: Option<Vec<u8>> = None;
    let mut session_alias: Option<u32> = None;

    let mut last_packet_id: Option<u64> = None;
    let mut received_packets: u32 = 0;
    let mut lost_packets: u32 = 0;
    let mut last_rtt_us: u64 = 0;
    let mut rtt_tracker = RttTracker::new();
    let mut arrival_jitter = ArrivalJitter::new();
    let mut nack_window = NackWindow::new(NACK_WINDOW_SIZE);
    let mut jitter_buffer = JitterBuffer::new();
    let mut last_skip_sent = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);

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

            // VR outbound (pose/timing)
            Some(out) = vr_rx.recv() => {
                if let Some(alias) = session_alias {
                    match out {
                        VrOutbound::Pose(pose) => {
                            let msg = ProtoMessage {
                                content: Some(rift_core::message::Content::Control(ProtoControl {
                                    content: Some(rift_core::control_message::Content::PoseUpdate(pose)),
                                })),
                            };
                            if let Err(e) = send_rift_msg(&socket, &mut crypto, connect_addr, msg, Some(alias), next_packet_id(), relay_info).await {
                                debug!("vr control send error: {}", e);
                            }
                        }
                        VrOutbound::Timing(timing) => {
                            let msg = ProtoMessage {
                                content: Some(rift_core::message::Content::Control(ProtoControl {
                                    content: Some(rift_core::control_message::Content::VrTiming(timing)),
                                })),
                            };
                            if let Err(e) = send_rift_msg(&socket, &mut crypto, connect_addr, msg, Some(alias), next_packet_id(), relay_info).await {
                                debug!("vr control send error: {}", e);
                            }
                        }
                        VrOutbound::Gamepad(input) => {
                            let msg = ProtoMessage {
                                content: Some(rift_core::message::Content::Input(input)),
                            };
                            if let Err(e) = send_rift_msg(&socket, &mut crypto, connect_addr, msg, Some(alias), next_packet_id(), relay_info).await {
                                debug!("vr input send error: {}", e);
                            }
                        }
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
                let stats_received = received_packets;
                let stats_lost = lost_packets;
                if let Some(alias) = session_alias {
                    let stats = ProtoStatsReport {
                        period_ms: 1000,
                        received_packets: stats_received,
                        lost_packets: stats_lost,
                        rtt_us: last_rtt_us,
                        jitter_us: arrival_jitter.jitter_us(),
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
                if let Some(adapter) = vr_adapter.as_ref() {
                    if let Ok(mut adapter) = adapter.lock() {
                        adapter.on_network_stats(VrNetworkStats {
                            rtt_us: last_rtt_us,
                            jitter_us: arrival_jitter.jitter_us(),
                            loss_ratio: if stats_received + stats_lost > 0 {
                                stats_lost as f32 / (stats_received + stats_lost) as f32
                            } else {
                                0.0
                            },
                        });
                    }
                }
            }

            // Jitter buffer drain
            _ = jitter_interval.tick() => {
                while let Some(ready) = jitter_buffer.pop_ready(now_us()) {
                    if let Some(adapter) = vr_adapter.as_ref() {
                        if let Ok(mut adapter) = adapter.lock() {
                            let frame = VrVideoFrame {
                                timestamp_us: ready.timestamp_us,
                                frame_id: ready.frame_id,
                                keyframe: ready.keyframe,
                                data: Bytes::from(ready.data),
                            };
                            let _ = adapter.submit_video(frame);
                        }
                    } else if let Some(r) = renderer.as_mut() {
                        r.render(&ready.data, ready.timestamp_us)?;
                    }
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

                let arrival_us = now_us();
                arrival_jitter.on_arrival(arrival_us);

                if let Some(alias) = session_alias {
                    let missing = nack_window.on_packet(phys.packet_id);
                    if !missing.is_empty() {
                        let nack = rift_core::Nack { packet_ids: missing };
                        let msg = ProtoMessage {
                            content: Some(rift_core::message::Content::Control(ProtoControl {
                                content: Some(rift_core::control_message::Content::Nack(nack)),
                            })),
                        };
                        if let Err(e) = send_rift_msg(&socket, &mut crypto, connect_addr, msg, Some(alias), next_packet_id(), relay_info).await {
                            debug!("nack send error: {}", e);
                        }
                    }
                }

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
                                        if vr_adapter.is_none() {
                                            let config = DecodeConfig {
                                                codec: match ack.selected_codec {
                                                    c if c == RiftCodec::Av1 as i32 => Codec::Av1,
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
                                                #[cfg(target_os = "macos")]
                                                {
                                                    let ar = wavry_media::MacAudioRenderer::new()?;
                                                    audio_renderer = Some(Box::new(ar));
                                                }
                                                #[cfg(target_os = "windows")]
                                                {
                                                    let ar = wavry_media::WindowsAudioRenderer::new()?;
                                                    audio_renderer = Some(Box::new(ar));
                                                }
                                            }
                                        }
                                    }
                                    if let Some(adapter) = vr_adapter.as_ref() {
                                        let codec = match ack.selected_codec {
                                            c if c == RiftCodec::Av1 as i32 => VrVideoCodec::Av1,
                                            c if c == RiftCodec::Hevc as i32 => VrVideoCodec::Hevc,
                                            _ => VrVideoCodec::H264,
                                        };
                                        let (width, height) = if let Some(res) = ack.stream_resolution {
                                            (res.width as u16, res.height as u16)
                                        } else if let Some(max) = config.max_resolution {
                                            (max.width, max.height)
                                        } else {
                                            (1280, 720)
                                        };
                                        if let Ok(mut adapter) = adapter.lock() {
                                            adapter.configure_stream(VrStreamConfig {
                                                codec,
                                                width,
                                                height,
                                            });
                                        }
                                    }
                                }
                                rift_core::control_message::Content::Pong(pong) => {
                                    let rtt_us = now_us().saturating_sub(pong.timestamp_us);
                                    last_rtt_us = rtt_us;
                                    let rtt_smooth = rtt_tracker.on_sample(rtt_us);
                                    if let Some(alias) = session_alias {
                                        if rtt_us as f64 > rtt_smooth + 30_000.0
                                            && last_skip_sent.elapsed() > Duration::from_millis(200)
                                        {
                                            let skip = if rtt_us as f64 > rtt_smooth + 50_000.0 { 2 } else { 1 };
                                            let msg = ProtoMessage {
                                                content: Some(rift_core::message::Content::Control(ProtoControl {
                                                    content: Some(rift_core::control_message::Content::EncoderControl(
                                                        rift_core::EncoderControl { skip_frames: skip },
                                                    )),
                                                })),
                                            };
                                            if let Err(e) = send_rift_msg(&socket, &mut crypto, connect_addr, msg, Some(alias), next_packet_id(), relay_info).await {
                                                debug!("encoder control send error: {}", e);
                                            } else {
                                                last_skip_sent = Instant::now();
                                            }
                                            if let Some(adapter) = vr_adapter.as_ref() {
                                                if let Ok(mut adapter) = adapter.lock() {
                                                    adapter.on_encoder_control(VrEncoderControl { skip_frames: skip });
                                                }
                                            }
                                        }
                                    }
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
                                    jitter_buffer.update(arrival_jitter.jitter_us_f64());
                                    jitter_buffer.push(frame, arrival_us);
                                    while let Some(ready) = jitter_buffer.pop_ready(now_us()) {
                                        if let Some(adapter) = vr_adapter.as_ref() {
                                            if let Ok(mut adapter) = adapter.lock() {
                                                let frame = VrVideoFrame {
                                                    timestamp_us: ready.timestamp_us,
                                                    frame_id: ready.frame_id,
                                                    keyframe: ready.keyframe,
                                                    data: Bytes::from(ready.data),
                                                };
                                                let _ = adapter.submit_video(frame);
                                            }
                                        } else if let Some(r) = renderer.as_mut() {
                                            r.render(&ready.data, ready.timestamp_us)?;
                                        }
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
                                                        jitter_buffer.update(arrival_jitter.jitter_us_f64());
                                                        jitter_buffer.push(frame, now_us());
                                                        while let Some(ready) = jitter_buffer.pop_ready(now_us()) {
                                                            if let Some(adapter) = vr_adapter.as_ref() {
                                                                if let Ok(mut adapter) = adapter.lock() {
                                                                    let frame = VrVideoFrame {
                                                                        timestamp_us: ready.timestamp_us,
                                                                        frame_id: ready.frame_id,
                                                                        keyframe: ready.keyframe,
                                                                        data: Bytes::from(ready.data),
                                                                    };
                                                                    let _ = adapter.submit_video(frame);
                                                                }
                                                            } else if let Some(r) = renderer.as_mut() {
                                                                r.render(&ready.data, ready.timestamp_us)?;
                                                            }
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
        CryptoState::Established(client) => client
            .encrypt(packet_id, &plaintext)
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
        header
            .encode(&mut buf)
            .map_err(|e| anyhow!("relay header encode: {}", e))?;
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
        CryptoState::Established(client) => client
            .decrypt(phys.packet_id, &phys.payload)
            .map_err(|e| anyhow!("decrypt failed: {}", e)),
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
    frame_id: u64,
    timestamp_us: u64,
    keyframe: bool,
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
        self.frames
            .retain(|_, frame| now.saturating_sub(frame.first_seen_us) < self.timeout_us);

        let entry = self
            .frames
            .entry(chunk.frame_id)
            .or_insert_with(|| FrameBuffer {
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
            let keyframe = entry.keyframe;
            let frame_id = chunk.frame_id;
            self.frames.remove(&chunk.frame_id);
            return Some(AssembledFrame {
                frame_id,
                timestamp_us,
                keyframe,
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
    // Spawn Gamepad Thread
    let tx_gamepad = input_tx.clone();
    thread::spawn(move || {
        let mut gilrs = Gilrs::new()
            .map_err(|e| anyhow!("gilrs init: {}", e))
            .unwrap();
        loop {
            while let Some(Event { id, event, .. }) = gilrs.next_event() {
                let gamepad_id = Into::<usize>::into(id) as u32;
                let mut msg = ProtoInputMessage {
                    timestamp_us: now_us(),
                    event: None,
                };
                match event {
                    GilrsEventType::ButtonPressed(button, _) => {
                        msg.event = Some(rift_core::input_message::Event::Gamepad(
                            rift_core::GamepadMessage {
                                gamepad_id,
                                buttons: vec![rift_core::GamepadButton {
                                    button: button as u32,
                                    pressed: true,
                                }],
                                axes: vec![],
                            },
                        ));
                    }
                    GilrsEventType::ButtonReleased(button, _) => {
                        msg.event = Some(rift_core::input_message::Event::Gamepad(
                            rift_core::GamepadMessage {
                                gamepad_id,
                                buttons: vec![rift_core::GamepadButton {
                                    button: button as u32,
                                    pressed: false,
                                }],
                                axes: vec![],
                            },
                        ));
                    }
                    GilrsEventType::AxisChanged(axis, value, _) => {
                        msg.event = Some(rift_core::input_message::Event::Gamepad(
                            rift_core::GamepadMessage {
                                gamepad_id,
                                axes: vec![rift_core::GamepadAxis {
                                    axis: axis as u32,
                                    value,
                                }],
                                buttons: vec![],
                            },
                        ));
                    }
                    _ => continue,
                }
                let _ = tx_gamepad.blocking_send(msg);
            }
            thread::sleep(Duration::from_millis(8));
        }
    });

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
                            event: Some(rift_core::input_message::Event::Key(rift_core::Key {
                                keycode: keycode as u32,
                                pressed,
                            })),
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
    // Spawn Gamepad Thread
    let tx_gamepad = input_tx.clone();
    thread::spawn(move || {
        let mut gilrs = Gilrs::new().unwrap();
        loop {
            while let Some(Event { id, event, .. }) = gilrs.next_event() {
                let gamepad_id = Into::<usize>::into(id) as u32;
                let mut msg = ProtoInputMessage {
                    timestamp_us: now_us(),
                    event: None,
                };
                match event {
                    GilrsEventType::ButtonPressed(button, _) => {
                        msg.event = Some(rift_core::input_message::Event::Gamepad(
                            rift_core::GamepadMessage {
                                gamepad_id,
                                buttons: vec![rift_core::GamepadButton {
                                    button: button as u32,
                                    pressed: true,
                                }],
                                axes: vec![],
                            },
                        ));
                    }
                    GilrsEventType::ButtonReleased(button, _) => {
                        msg.event = Some(rift_core::input_message::Event::Gamepad(
                            rift_core::GamepadMessage {
                                gamepad_id,
                                buttons: vec![rift_core::GamepadButton {
                                    button: button as u32,
                                    pressed: false,
                                }],
                                axes: vec![],
                            },
                        ));
                    }
                    GilrsEventType::AxisChanged(axis, value, _) => {
                        msg.event = Some(rift_core::input_message::Event::Gamepad(
                            rift_core::GamepadMessage {
                                gamepad_id,
                                axes: vec![rift_core::GamepadAxis {
                                    axis: axis as u32,
                                    value,
                                }],
                                buttons: vec![],
                            },
                        ));
                    }
                    _ => continue,
                }
                let _ = tx_gamepad.blocking_send(msg);
            }
            thread::sleep(Duration::from_millis(8));
        }
    });

    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(2));
        let press = ProtoInputMessage {
            event: Some(rift_core::input_message::Event::Key(rift_core::Key {
                keycode: 30,
                pressed: true,
            })),
            timestamp_us: now_us(),
        };
        let _ = input_tx.blocking_send(press);
        thread::sleep(Duration::from_millis(100));
        let release = ProtoInputMessage {
            event: Some(rift_core::input_message::Event::Key(rift_core::Key {
                keycode: 30,
                pressed: false,
            })),
            timestamp_us: now_us(),
        };
        let _ = input_tx.blocking_send(release);
    });
    Ok(())
}

#[cfg(target_os = "linux")]
enum DeviceKind {
    Keyboard,
    Mouse,
}

#[cfg(target_os = "linux")]
fn find_device(kind: DeviceKind) -> Result<Option<Device>> {
    for (_path, device) in evdev::enumerate() {
        match kind {
            DeviceKind::Keyboard => {
                if device.supported_keys().is_some() {
                    return Ok(Some(device));
                }
            }
            DeviceKind::Mouse => {
                if device.supported_relative_axes().is_some() {
                    return Ok(Some(device));
                }
            }
        }
    }
    Ok(None)
}

pub fn create_hello_base64(client_name: String, public_addr: Option<String>) -> Result<String> {
    let supported_codecs = probe_supported_codecs();

    let supported_codecs: Vec<i32> = supported_codecs
        .into_iter()
        .map(|c| match c {
            Codec::Av1 => RiftCodec::Av1 as i32,
            Codec::Hevc => RiftCodec::Hevc as i32,
            Codec::H264 => RiftCodec::H264 as i32,
        })
        .collect();

    let hello = ProtoHello {
        client_name,
        platform: if cfg!(target_os = "windows") {
            rift_core::Platform::Windows as i32
        } else if cfg!(target_os = "macos") {
            rift_core::Platform::Macos as i32
        } else {
            rift_core::Platform::Linux as i32
        },
        supported_codecs,
        max_resolution: Some(ProtoResolution {
            width: 1920,
            height: 1080,
        }),
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

pub fn create_hello_ack_base64(
    accepted: bool,
    session_id: [u8; 16],
    session_alias: u32,
    public_addr: Option<String>,
    width: u32,
    height: u32,
    selected_codec: RiftCodec,
) -> Result<String> {
    let ack = rift_core::HelloAck {
        accepted,
        selected_codec: selected_codec as i32,
        stream_resolution: Some(ProtoResolution { width, height }),
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
        Some(rift_core::message::Content::Control(ctrl)) => match ctrl.content {
            Some(rift_core::control_message::Content::Hello(h)) => Ok(h),
            _ => Err(anyhow!("Not a Hello message")),
        },
        _ => Err(anyhow!("Not a Control message")),
    }
}

pub fn decode_hello_ack_base64(b64: &str) -> Result<rift_core::HelloAck> {
    let bytes = general_purpose::STANDARD.decode(b64)?;
    let msg = decode_msg(&bytes)?;
    match msg.content {
        Some(rift_core::message::Content::Control(ctrl)) => match ctrl.content {
            Some(rift_core::control_message::Content::HelloAck(a)) => Ok(a),
            _ => Err(anyhow!("Not a HelloAck message")),
        },
        _ => Err(anyhow!("Not a Control message")),
    }
}
fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64
}

struct ArrivalJitter {
    last_arrival_us: Option<u64>,
    ia_avg_us: f64,
    jitter_us: f64,
}

impl ArrivalJitter {
    fn new() -> Self {
        Self {
            last_arrival_us: None,
            ia_avg_us: 0.0,
            jitter_us: 0.0,
        }
    }

    fn on_arrival(&mut self, arrival_us: u64) {
        if let Some(last) = self.last_arrival_us {
            let ia = arrival_us.saturating_sub(last) as f64;
            if self.ia_avg_us == 0.0 {
                self.ia_avg_us = ia;
            } else {
                self.ia_avg_us += (ia - self.ia_avg_us) / 16.0;
            }
            let deviation = (ia - self.ia_avg_us).abs();
            self.jitter_us += (deviation - self.jitter_us) / 16.0;
        }
        self.last_arrival_us = Some(arrival_us);
    }

    fn jitter_us(&self) -> u32 {
        self.jitter_us.max(0.0) as u32
    }

    fn jitter_us_f64(&self) -> f64 {
        self.jitter_us.max(0.0)
    }
}

struct RttTracker {
    smooth_us: f64,
}

impl RttTracker {
    fn new() -> Self {
        Self { smooth_us: 0.0 }
    }

    fn on_sample(&mut self, rtt_us: u64) -> f64 {
        if self.smooth_us == 0.0 {
            self.smooth_us = rtt_us as f64;
        } else {
            self.smooth_us = 0.875 * self.smooth_us + 0.125 * (rtt_us as f64);
        }
        self.smooth_us
    }
}

struct NackWindow {
    window: u64,
    highest: Option<u64>,
    received: BTreeSet<u64>,
    missing: BTreeSet<u64>,
}

impl NackWindow {
    fn new(window: u64) -> Self {
        Self {
            window,
            highest: None,
            received: BTreeSet::new(),
            missing: BTreeSet::new(),
        }
    }

    fn on_packet(&mut self, packet_id: u64) -> Vec<u64> {
        let mut newly_missing = Vec::new();
        if let Some(highest) = self.highest {
            if packet_id > highest + 1 {
                let gap_start = highest + 1;
                let gap_end = packet_id - 1;
                let min_start = packet_id.saturating_sub(self.window).max(gap_start);
                for id in min_start..=gap_end {
                    if !self.received.contains(&id) && !self.missing.contains(&id) {
                        self.missing.insert(id);
                        newly_missing.push(id);
                    }
                }
                self.highest = Some(packet_id);
            } else if packet_id > highest {
                self.highest = Some(packet_id);
            }
        } else {
            self.highest = Some(packet_id);
        }

        self.received.insert(packet_id);
        self.missing.remove(&packet_id);
        self.evict_old();
        newly_missing
    }

    fn evict_old(&mut self) {
        if let Some(highest) = self.highest {
            let cutoff = highest.saturating_sub(self.window);
            self.received = self.received.split_off(&cutoff);
            self.missing = self.missing.split_off(&cutoff);
        }
    }
}

struct JitterBuffer {
    target_delay_us: u64,
    queue: VecDeque<BufferedFrame>,
}

struct BufferedFrame {
    arrival_us: u64,
    frame: AssembledFrame,
}

impl JitterBuffer {
    fn new() -> Self {
        Self {
            target_delay_us: 0,
            queue: VecDeque::new(),
        }
    }

    fn update(&mut self, jitter_us: f64) {
        if jitter_us > JITTER_GROW_THRESHOLD_US {
            self.target_delay_us = (self.target_delay_us + 1_000).min(JITTER_MAX_BUFFER_US);
        } else if jitter_us < JITTER_SHRINK_THRESHOLD_US {
            self.target_delay_us = self.target_delay_us.saturating_sub(500);
        }
    }

    fn push(&mut self, frame: AssembledFrame, arrival_us: u64) {
        self.queue.push_back(BufferedFrame { arrival_us, frame });
    }

    fn pop_ready(&mut self, now_us: u64) -> Option<AssembledFrame> {
        if let Some(front) = self.queue.front() {
            if now_us.saturating_sub(front.arrival_us) >= self.target_delay_us {
                return self.queue.pop_front().map(|f| f.frame);
            }
        }
        None
    }
}
