use anyhow::{anyhow, Result};
use bytes::Bytes;
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot};
use tokio::time;
use tracing::{debug, info, warn};

use rift_core::{
    decode_msg, encode_msg,
    relay::{LeasePresentPayload, PeerRole, RelayHeader, RelayPacketType, RELAY_HEADER_SIZE},
    Codec as RiftCodec, ControlMessage as ProtoControl, Handshake, Hello as ProtoHello,
    Message as ProtoMessage, PhysicalPacket, Ping as ProtoPing, Resolution as ProtoResolution,
    Role, StatsReport as ProtoStatsReport, RIFT_VERSION,
};
use socket2::SockRef;

use crate::helpers::{env_bool, local_platform, now_us};
use crate::input::spawn_input_threads;
use crate::media::{
    ArrivalJitter, FecCache, FrameAssembler, JitterBuffer, NackWindow, RttTracker,
    FRAME_TIMEOUT_US, NACK_WINDOW_SIZE,
};
use crate::types::{
    ClientConfig, ClientRuntimeStats, CryptoState, RelayInfo, RendererFactory, VrOutbound,
};

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
use wavry_media::CapabilityProbe;
#[cfg(not(target_os = "linux"))]
use wavry_media::DummyRenderer as VideoRenderer;
#[cfg(target_os = "linux")]
use wavry_media::DummyRenderer as LinuxFallbackRenderer;
#[cfg(target_os = "linux")]
use wavry_media::GstVideoRenderer as VideoRenderer;
use wavry_media::{Codec, DecodeConfig, Renderer, Resolution as MediaResolution};
use wavry_vr::types::{
    EncoderControl as VrEncoderControl, HandPose as VrHandPose, NetworkStats as VrNetworkStats,
    Pose as VrPose, StreamConfig as VrStreamConfig, VideoCodec as VrVideoCodec,
    VideoFrame as VrVideoFrame, VrTiming,
};
use wavry_vr::{VrAdapter, VrAdapterCallbacks};

const CRYPTO_HANDSHAKE_ATTEMPTS: u32 = 6;
const CRYPTO_HANDSHAKE_STEP_TIMEOUT: Duration = Duration::from_secs(2);
const DSCP_EF: u32 = 0x2E;

fn probe_supported_codecs() -> Vec<Codec> {
    #[cfg(target_os = "windows")]
    {
        return wavry_media::WindowsProbe
            .supported_decoders()
            .unwrap_or_else(|_| vec![Codec::H264]);
    }
    #[cfg(target_os = "macos")]
    {
        wavry_media::MacProbe
            .supported_decoders()
            .unwrap_or_else(|_| vec![Codec::H264])
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

#[cfg(target_os = "linux")]
fn linux_has_display() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
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

    fn on_hand_pose_update(&self, hand_pose: VrHandPose, timestamp_us: u64) {
        let msg = rift_core::HandPoseUpdate {
            timestamp_us,
            hand_id: hand_pose.hand_id,
            position_x: hand_pose.pose.position[0],
            position_y: hand_pose.pose.position[1],
            position_z: hand_pose.pose.position[2],
            orientation_x: hand_pose.pose.orientation[0],
            orientation_y: hand_pose.pose.orientation[1],
            orientation_z: hand_pose.pose.orientation[2],
            orientation_w: hand_pose.pose.orientation[3],
            linear_velocity_x: hand_pose.linear_velocity[0],
            linear_velocity_y: hand_pose.linear_velocity[1],
            linear_velocity_z: hand_pose.linear_velocity[2],
            angular_velocity_x: hand_pose.angular_velocity[0],
            angular_velocity_y: hand_pose.angular_velocity[1],
            angular_velocity_z: hand_pose.angular_velocity[2],
        };
        let _ = self.tx.try_send(VrOutbound::HandPose(msg));
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

struct RuntimeStatsGuard {
    stats: Option<Arc<ClientRuntimeStats>>,
}

impl RuntimeStatsGuard {
    fn new(stats: Option<Arc<ClientRuntimeStats>>) -> Self {
        if let Some(s) = stats.as_ref() {
            s.connected.store(false, Ordering::Relaxed);
            s.frames_decoded.store(0, Ordering::Relaxed);
        }
        Self { stats }
    }
}

impl Drop for RuntimeStatsGuard {
    fn drop(&mut self) {
        if let Some(stats) = self.stats.as_ref() {
            stats.connected.store(false, Ordering::Relaxed);
        }
    }
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
    monitor_rx: Option<mpsc::UnboundedReceiver<u32>>,
) -> Result<()> {
    run_client_inner(config, renderer_factory, None, monitor_rx).await
}

pub async fn run_client_with_shutdown(
    config: ClientConfig,
    renderer_factory: Option<RendererFactory>,
    shutdown_rx: oneshot::Receiver<()>,
    monitor_rx: Option<mpsc::UnboundedReceiver<u32>>,
) -> Result<()> {
    run_client_inner(config, renderer_factory, Some(shutdown_rx), monitor_rx).await
}

async fn run_client_inner(
    config: ClientConfig,
    renderer_factory: Option<RendererFactory>,
    mut shutdown_rx: Option<oneshot::Receiver<()>>,
    mut monitor_rx: Option<mpsc::UnboundedReceiver<u32>>,
) -> Result<()> {
    let runtime_stats = config.runtime_stats.clone();
    let _runtime_stats_guard = RuntimeStatsGuard::new(runtime_stats.clone());

    if config.no_encrypt {
        if !env_bool("WAVRY_ALLOW_INSECURE_NO_ENCRYPT", false) {
            return Err(anyhow!(
                "refusing to start without encryption; set WAVRY_ALLOW_INSECURE_NO_ENCRYPT=1 to override (NOT FOR PRODUCTION)"
            ));
        }
        warn!("ENCRYPTION DISABLED - not for production use");
    }

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

    if config.no_encrypt
        && !connect_addr.ip().is_loopback()
        && !env_bool("WAVRY_CLIENT_ALLOW_PUBLIC_CONNECT", false)
    {
        return Err(anyhow!(
            "refusing to connect to non-loopback address {} in --no-encrypt mode without WAVRY_CLIENT_ALLOW_PUBLIC_CONNECT=1",
            connect_addr
        ));
    }

    // Initialize crypto state
    let mut crypto = match config.no_encrypt {
        true => CryptoState::Disabled,
        false => {
            use rift_crypto::connection::SecureClient;
            if let Some(key) = config.identity_key {
                CryptoState::Handshaking(SecureClient::with_keypair(key)?)
            } else {
                CryptoState::Handshaking(SecureClient::new()?)
            }
        }
    };

    // Create input channel
    let (input_tx, mut input_rx) = mpsc::channel::<rift_core::InputMessage>(128);
    spawn_input_threads(input_tx, config.gamepad_enabled, config.gamepad_deadzone)?;

    // VR adapter wiring (optional)
    let (vr_tx, mut vr_rx) = mpsc::channel::<VrOutbound>(64);
    let vr_adapter: Option<Arc<Mutex<dyn VrAdapter>>> =
        if let Some(adapter) = config.vr_adapter.clone() {
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
        let phys1_wire = phys1.encode();

        // Retransmit msg1 on timeout and tolerate unrelated packets while waiting for msg2.
        let mut buf_arr = [0u8; 4096];
        let mut msg2_payload: Option<Bytes> = None;
        let mut last_msg2_decode_err: Option<String> = None;

        for attempt in 1..=CRYPTO_HANDSHAKE_ATTEMPTS {
            socket.send_to(&phys1_wire, connect_addr).await?;
            debug!(
                "sent crypto msg1 (attempt {}/{})",
                attempt, CRYPTO_HANDSHAKE_ATTEMPTS
            );

            let deadline = time::Instant::now() + CRYPTO_HANDSHAKE_STEP_TIMEOUT;
            loop {
                let now = time::Instant::now();
                if now >= deadline {
                    break;
                }

                let remaining = deadline - now;
                let recv = match time::timeout(remaining, socket.recv_from(&mut buf_arr)).await {
                    Ok(v) => v?,
                    Err(_) => break,
                };

                let (len, src) = recv;
                if src != connect_addr {
                    debug!("ignoring handshake packet from unexpected peer {}", src);
                    continue;
                }

                let phys2 = match PhysicalPacket::decode(Bytes::copy_from_slice(&buf_arr[..len])) {
                    Ok(p) => p,
                    Err(e) => {
                        last_msg2_decode_err =
                            Some(format!("RIFT decode error in handshake: {}", e));
                        continue;
                    }
                };

                // Crypto msg2 always uses session_id=0.
                if phys2.session_id != Some(0) {
                    continue;
                }

                msg2_payload = Some(phys2.payload);
                debug!("received crypto msg2 on attempt {}", attempt);
                break;
            }

            if msg2_payload.is_some() {
                break;
            }
        }

        let msg2_payload = msg2_payload.ok_or_else(|| {
            if let Some(detail) = last_msg2_decode_err {
                anyhow!(
                    "crypto handshake timeout after {} attempts with {}: {}",
                    CRYPTO_HANDSHAKE_ATTEMPTS,
                    connect_addr,
                    detail
                )
            } else {
                anyhow!(
                    "crypto handshake timeout after {} attempts waiting for host response from {}; verify host is running and port is correct",
                    CRYPTO_HANDSHAKE_ATTEMPTS,
                    connect_addr
                )
            }
        })?;

        // Process msg2 and send msg3
        let msg3_payload = client
            .process_server_response(&msg2_payload)
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
        platform: local_platform() as i32,
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
    let mut audio_disabled = false;
    #[cfg(target_os = "linux")]
    let mut video_disabled = false;
    #[cfg(not(target_os = "linux"))]
    let _video_disabled = false;
    let mut frames = FrameAssembler::new(FRAME_TIMEOUT_US);
    let mut fec_cache = FecCache::new();

    loop {
        tokio::select! {
            _ = async {
                if let Some(rx) = &mut shutdown_rx {
                    let _ = rx.await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                info!("client shutdown requested");
                break;
            }

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

            // Handle monitor selection from UI
            Some(monitor_id) = async {
                if let Some(rx) = monitor_rx.as_mut() {
                    rx.recv().await
                } else {
                    None
                }
            } => {
                if let Some(alias) = session_alias {
                    info!("Sending SelectMonitor request for display {}", monitor_id);
                    let msg = ProtoMessage {
                        content: Some(rift_core::message::Content::Control(ProtoControl {
                            content: Some(rift_core::control_message::Content::SelectMonitor(
                                rift_core::SelectMonitor { monitor_id },
                            )),
                        })),
                    };
                    if let Err(e) = send_rift_msg(&socket, &mut crypto, connect_addr, msg, Some(alias), next_packet_id(), relay_info).await {
                        warn!("SelectMonitor send error: {}", e);
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
                        VrOutbound::HandPose(hand_pose) => {
                            let msg = ProtoMessage {
                                content: Some(rift_core::message::Content::Control(ProtoControl {
                                    content: Some(rift_core::control_message::Content::HandPoseUpdate(hand_pose)),
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
                    let mut rendered = false;
                    if let Some(adapter) = vr_adapter.as_ref() {
                        if let Ok(mut adapter) = adapter.lock() {
                            let frame = VrVideoFrame {
                                timestamp_us: ready.timestamp_us,
                                frame_id: ready.frame_id,
                                keyframe: ready.keyframe,
                                data: Bytes::from(ready.data),
                            };
                            let _ = adapter.submit_video(frame);
                            rendered = true;
                        }
                    } else if let Some(r) = renderer.as_mut() {
                        r.render(&ready.data, ready.timestamp_us)?;
                        rendered = true;
                    }
                    if rendered {
                        if let Some(stats) = runtime_stats.as_ref() {
                            stats.frames_decoded.fetch_add(1, Ordering::Relaxed);
                        }
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
                                    if let Some(stats) = runtime_stats.as_ref() {
                                        stats.connected.store(true, Ordering::Relaxed);
                                    }

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
                                                enable_10bit: false,
                                                enable_hdr: false,
                                            };

                                            if let Some(factory) = &renderer_factory {
                                                match factory(config) {
                                                    Ok(r) => renderer = Some(r),
                                                    Err(e) => {
                                                        warn!("renderer factory failed: {}", e);
                                                    }
                                                }
                                            }

                                            if renderer.is_none() {
                                                // Fallback to default platform renderer
                                                #[cfg(target_os = "linux")]
                                                if !linux_has_display() {
                                                    if let Ok(fallback) = LinuxFallbackRenderer::new(config) {
                                                        renderer = Some(Box::new(fallback));
                                                        if !video_disabled {
                                                            warn!("video disabled: no display available");
                                                            video_disabled = true;
                                                        }
                                                    }
                                                }

                                                if renderer.is_none() {
                                                    match VideoRenderer::new(config) {
                                                        Ok(r) => renderer = Some(Box::new(r)),
                                                        Err(e) => {
                                                            warn!("video renderer init failed: {}", e);
                                                            #[cfg(target_os = "linux")]
                                                            {
                                                                if let Ok(fallback) = LinuxFallbackRenderer::new(config) {
                                                                    renderer = Some(Box::new(fallback));
                                                                    if !video_disabled {
                                                                        warn!("video disabled: falling back to headless renderer");
                                                                        video_disabled = true;
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }

                                                #[cfg(target_os = "linux")]
                                                {
                                                    match wavry_media::GstAudioRenderer::new() {
                                                        Ok(ar) => audio_renderer = Some(Box::new(ar)),
                                                        Err(e) => warn!("audio renderer init failed: {}", e),
                                                    }
                                                }
                                                #[cfg(target_os = "macos")]
                                                {
                                                    match wavry_media::MacAudioRenderer::new() {
                                                        Ok(ar) => audio_renderer = Some(Box::new(ar)),
                                                        Err(e) => warn!("audio renderer init failed: {}", e),
                                                    }
                                                }
                                                #[cfg(target_os = "windows")]
                                                {
                                                    match wavry_media::WindowsAudioRenderer::new() {
                                                        Ok(ar) => audio_renderer = Some(Box::new(ar)),
                                                        Err(e) => warn!("audio renderer init failed: {}", e),
                                                    }
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
                                rift_core::control_message::Content::MonitorList(list) => {
                                    info!("Received monitor list: {} displays", list.monitors.len());
                                    if let Some(stats) = runtime_stats.as_ref() {
                                        if let Ok(mut monitors) = stats.monitors.lock() {
                                            *monitors = list.monitors;
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
                                    if let Err(e) = ar.render(&packet.payload, packet.timestamp_us) {
                                        if !audio_disabled {
                                            warn!("audio render failed, disabling audio: {}", e);
                                        }
                                        audio_renderer = None;
                                        audio_disabled = true;
                                    }
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
                                                        if let Err(e) = ar.render(&packet.payload, packet.timestamp_us) {
                                                            if !audio_disabled {
                                                                warn!("audio render failed, disabling audio: {}", e);
                                                            }
                                                            audio_renderer = None;
                                                            audio_disabled = true;
                                                        }
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

    if let Some(adapter) = vr_adapter.as_ref() {
        if let Ok(mut adapter) = adapter.lock() {
            adapter.stop();
        }
    }

    // Send session feedback if using a relay
    if let (Some(master_url), Some(relay)) = (config.master_url, relay_info) {
        let frames_decoded = runtime_stats
            .as_ref()
            .map(|s| s.frames_decoded.load(Ordering::Relaxed))
            .unwrap_or(0);

        // Simple heuristic for quality score: 100 if > 100 frames, 0 if 0
        let quality_score = if frames_decoded > 100 {
            100
        } else if frames_decoded > 0 {
            50
        } else {
            0
        };

        let feedback = wavry_common::protocol::RelayFeedbackRequest {
            session_id: relay.session_id,
            relay_id: relay.relay_id.clone(),
            quality_score,
            issues: if quality_score < 100 {
                vec!["low_frame_count".to_string()]
            } else {
                vec![]
            },
            signature: String::new(), // TODO: Sign feedback with identity key
        };

        let client = reqwest::Client::new();
        let _ = client
            .post(format!("{}/v1/feedback", master_url))
            .json(&feedback)
            .send()
            .await;
        info!(
            "sent session feedback to master for relay {}: score={}",
            relay.relay_id, quality_score
        );
    }

    Ok(())
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

async fn discover_host(timeout: Duration) -> Result<SocketAddr> {
    use mdns_sd::ServiceEvent;
    let handle = tokio::task::spawn_blocking(move || {
        let daemon = mdns_sd::ServiceDaemon::new()?;
        let receiver = daemon.browse("_wavry._udp.local.")?;
        for event in receiver {
            if let ServiceEvent::ServiceResolved(info) = event {
                if let Some(addr) = info.get_addresses().iter().next() {
                    return Ok(SocketAddr::new(*addr, info.get_port()));
                }
            }
        }
        Err(anyhow!("no wavry hosts discovered"))
    });
    time::timeout(timeout, handle).await??
}
