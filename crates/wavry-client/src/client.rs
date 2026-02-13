use anyhow::{anyhow, Result};
use bytes::Bytes;
use rand::Rng as _;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
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
    ClientConfig, ClientRuntimeStats, CryptoState, FileTransferCommand, RelayInfo, RendererFactory,
    VrOutbound,
};

use wavry_common::file_transfer::{FileOffer, IncomingFile, OutgoingFile, DEFAULT_CHUNK_SIZE};
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
use wavry_media::CapabilityProbe;
#[cfg(not(target_os = "linux"))]
use wavry_media::DummyRenderer as VideoRenderer;
#[cfg(target_os = "linux")]
use wavry_media::DummyRenderer as LinuxFallbackRenderer;
#[cfg(target_os = "linux")]
use wavry_media::GstVideoRenderer as VideoRenderer;
use wavry_media::{Codec, DecodeConfig, Renderer, Resolution as MediaResolution};
use wavry_platform::{ArboardClipboard, Clipboard};
use wavry_vr::types::{
    EncoderControl as VrEncoderControl, HandPose as VrHandPose, NetworkStats as VrNetworkStats,
    Pose as VrPose, StreamConfig as VrStreamConfig, VideoCodec as VrVideoCodec,
    VideoFrame as VrVideoFrame, VrTiming,
};
use wavry_vr::{VrAdapter, VrAdapterCallbacks};

const CRYPTO_HANDSHAKE_ATTEMPTS: u32 = 6;
const CRYPTO_HANDSHAKE_STEP_TIMEOUT: Duration = Duration::from_secs(2);
const DSCP_EF: u32 = 0x2E;
const FILE_TRANSFER_TICK_MS: u64 = 2;
const FILE_TRANSFER_PROGRESS_CHUNK_INTERVAL: u32 = 64;
const FILE_TRANSFER_SHARE_PERCENT: f32 = 15.0;
const FILE_TRANSFER_MIN_KBPS: u32 = 256;
const FILE_TRANSFER_MAX_KBPS: u32 = 4096;
const MAX_FILE_STATUS_MESSAGE_CHARS: usize = 512;

fn probe_supported_codecs() -> Vec<Codec> {
    #[cfg(target_os = "windows")]
    {
        wavry_media::WindowsProbe
            .supported_decoders()
            .unwrap_or_else(|_| vec![Codec::H264])
    }
    #[cfg(target_os = "macos")]
    {
        wavry_media::MacProbe
            .supported_decoders()
            .unwrap_or_else(|_| vec![Codec::H264])
    }
    #[cfg(target_os = "linux")]
    {
        wavry_media::LinuxProbe
            .supported_decoders()
            .unwrap_or_else(|_| vec![Codec::H264])
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

struct FileTransferState {
    outgoing: VecDeque<OutgoingFile>,
    incoming: HashMap<u64, IncomingFile>,
    output_dir: PathBuf,
    max_file_bytes: u64,
}

impl FileTransferState {
    fn new(send_files: &[PathBuf], output_dir: PathBuf, max_file_bytes: u64) -> Self {
        let mut outgoing = VecDeque::new();
        for path in send_files {
            let file_id = random_file_id();
            match OutgoingFile::from_path(path, file_id, DEFAULT_CHUNK_SIZE, max_file_bytes) {
                Ok(file) => {
                    info!("queued file for transfer to host: {}", path.display());
                    outgoing.push_back(file);
                }
                Err(err) => warn!("skipping file {}: {}", path.display(), err),
            }
        }
        Self {
            outgoing,
            incoming: HashMap::new(),
            output_dir,
            max_file_bytes,
        }
    }
}

fn random_file_id() -> u64 {
    loop {
        let id = rand::thread_rng().gen::<u64>();
        if id != 0 {
            return id;
        }
    }
}

fn offer_to_proto(offer: &FileOffer) -> rift_core::FileHeader {
    rift_core::FileHeader {
        file_id: offer.file_id,
        filename: offer.filename.clone(),
        file_size: offer.file_size,
        checksum_sha256: offer.checksum_sha256.clone(),
        chunk_size: offer.chunk_size,
        total_chunks: offer.total_chunks,
    }
}

fn offer_from_proto(header: rift_core::FileHeader, max_file_bytes: u64) -> Result<FileOffer> {
    let offer = FileOffer {
        file_id: header.file_id,
        filename: header.filename,
        file_size: header.file_size,
        checksum_sha256: header.checksum_sha256.to_ascii_lowercase(),
        chunk_size: header.chunk_size,
        total_chunks: header.total_chunks,
    };
    wavry_common::file_transfer::validate_offer(&offer, max_file_bytes)?;
    Ok(offer)
}

fn file_status_message(
    file_id: u64,
    status: rift_core::file_status::Status,
    message: impl Into<String>,
) -> rift_core::FileStatus {
    rift_core::FileStatus {
        file_id,
        status: status as i32,
        message: message.into(),
    }
}

#[derive(Debug)]
struct FileTransferLimiter {
    rate_kbps: u32,
    tokens: f64,
    capacity: f64,
    last_refill: time::Instant,
}

impl FileTransferLimiter {
    fn new(rate_kbps: u32) -> Self {
        let now = time::Instant::now();
        let capacity = Self::capacity_for(rate_kbps.max(1));
        Self {
            rate_kbps: rate_kbps.max(1),
            tokens: capacity,
            capacity,
            last_refill: now,
        }
    }

    fn capacity_for(rate_kbps: u32) -> f64 {
        let bytes_per_second = rate_kbps as f64 * 1000.0 / 8.0;
        (bytes_per_second * 0.5).max((DEFAULT_CHUNK_SIZE as f64) * 4.0)
    }

    fn set_rate_kbps(&mut self, rate_kbps: u32) {
        let rate_kbps = rate_kbps.max(1);
        if self.rate_kbps == rate_kbps {
            return;
        }
        self.refill();
        self.rate_kbps = rate_kbps;
        self.capacity = Self::capacity_for(rate_kbps);
        self.tokens = self.tokens.min(self.capacity);
    }

    fn refill(&mut self) {
        let now = time::Instant::now();
        let elapsed = (now - self.last_refill).as_secs_f64();
        self.last_refill = now;
        let bytes_per_second = self.rate_kbps as f64 * 1000.0 / 8.0;
        self.tokens = (self.tokens + elapsed * bytes_per_second).min(self.capacity);
    }

    fn try_take(&mut self, bytes: usize) -> bool {
        self.refill();
        let needed = bytes as f64;
        if self.tokens >= needed {
            self.tokens -= needed;
            true
        } else {
            false
        }
    }
}

fn file_transfer_budget_kbps(video_bitrate_kbps: u32) -> u32 {
    let shared = (video_bitrate_kbps as f32 * (FILE_TRANSFER_SHARE_PERCENT / 100.0)).round() as u32;
    shared.clamp(FILE_TRANSFER_MIN_KBPS, FILE_TRANSFER_MAX_KBPS)
}

fn parse_resume_chunk(message: &str) -> Option<u32> {
    message
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .find_map(|part| {
            let token = part.trim();
            token
                .strip_prefix("resume_chunk=")
                .and_then(|raw| raw.parse::<u32>().ok())
        })
}

fn parse_transfer_command(message: &str) -> Option<&'static str> {
    let token = message.trim().to_ascii_lowercase();
    match token.as_str() {
        "pause" => Some("pause"),
        "resume" => Some("resume"),
        "cancel" | "canceled" => Some("cancel"),
        "retry" => Some("retry"),
        _ => None,
    }
}

fn sanitize_file_status_message(raw: &str) -> String {
    let trimmed = raw.trim();
    let mut out = String::with_capacity(trimmed.len().min(MAX_FILE_STATUS_MESSAGE_CHARS));
    for ch in trimmed.chars().take(MAX_FILE_STATUS_MESSAGE_CHARS) {
        if ch.is_control() {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out.trim().to_string()
}

fn file_ready_for_transfer(file: &OutgoingFile) -> bool {
    if !file.header_sent() {
        return true;
    }
    !file.paused() && !file.finished()
}

fn rotate_to_next_ready_transfer(outgoing: &mut VecDeque<OutgoingFile>) -> bool {
    let Some(idx) = outgoing.iter().position(file_ready_for_transfer) else {
        return false;
    };
    if idx > 0 {
        outgoing.rotate_left(idx);
    }
    true
}

fn apply_file_status_to_outgoing(
    outgoing: &mut VecDeque<OutgoingFile>,
    status: &rift_core::FileStatus,
) {
    let Some(idx) = outgoing
        .iter()
        .position(|f| f.offer().file_id == status.file_id)
    else {
        return;
    };

    let status_kind = rift_core::file_status::Status::try_from(status.status).ok();
    let message = sanitize_file_status_message(&status.message);
    let message = message.as_str();

    let mut remove_file = false;
    {
        let file = outgoing
            .get_mut(idx)
            .expect("position came from current VecDeque");

        if let Some(chunk) = parse_resume_chunk(message) {
            if chunk < file.next_chunk_index() {
                if let Err(err) = file.set_next_chunk(chunk) {
                    warn!(
                        "invalid resume request for file_id={} chunk={}: {}",
                        status.file_id, chunk, err
                    );
                } else {
                    info!(
                        "rewinding outgoing file_id={} to chunk {}",
                        status.file_id, chunk
                    );
                }
            }
            file.resume();
        }

        if let Some(cmd) = parse_transfer_command(message) {
            match cmd {
                "pause" => {
                    file.pause();
                    info!("paused outgoing file transfer file_id={}", status.file_id);
                }
                "resume" => {
                    file.resume();
                    info!("resumed outgoing file transfer file_id={}", status.file_id);
                }
                "retry" => {
                    file.restart_from_beginning();
                    file.resume();
                    info!("retry requested for outgoing file_id={}", status.file_id);
                }
                "cancel" => {
                    remove_file = true;
                }
                _ => {}
            }
        }

        match status_kind {
            Some(rift_core::file_status::Status::Pending)
            | Some(rift_core::file_status::Status::InProgress) => {
                file.resume();
            }
            Some(rift_core::file_status::Status::Complete) => {
                remove_file = true;
            }
            Some(rift_core::file_status::Status::Error) => {
                if message.contains("no matching file offer") {
                    file.restart_from_beginning();
                    file.resume();
                    info!(
                        "host missing offer for file_id={}, restarting transfer",
                        status.file_id
                    );
                } else if !message.is_empty() {
                    warn!(
                        "stopping outgoing file_id={} after host error: {}",
                        status.file_id, message
                    );
                    remove_file = true;
                }
            }
            None => {}
        }
    }

    if remove_file {
        let _ = outgoing.remove(idx);
    }
}

fn apply_file_status_to_incoming(
    incoming: &mut HashMap<u64, IncomingFile>,
    status: &rift_core::FileStatus,
) {
    let message = sanitize_file_status_message(&status.message);
    let message = message.as_str();

    if let Some(cmd) = parse_transfer_command(message) {
        if matches!(cmd, "cancel" | "retry") {
            if let Some(partial) = incoming.remove(&status.file_id) {
                if let Err(err) = partial.abort() {
                    warn!(
                        "failed to discard incoming file_id={} after {} command: {}",
                        status.file_id, cmd, err
                    );
                }
            }
        }
    }

    if matches!(
        rift_core::file_status::Status::try_from(status.status).ok(),
        Some(rift_core::file_status::Status::Complete | rift_core::file_status::Status::Error)
    ) {
        if let Some(partial) = incoming.remove(&status.file_id) {
            if let Err(err) = partial.abort() {
                warn!(
                    "failed to clean up incoming file_id={} after terminal status: {}",
                    status.file_id, err
                );
            }
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

    // Alias 0 is reserved for physical handshake framing in rift-core decode.
    // Use a non-zero bootstrap alias until HelloAck provides the negotiated alias.
    send_rift_msg(
        &socket,
        &mut crypto,
        connect_addr,
        msg,
        Some(1),
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

    let mut clipboard = ArboardClipboard::new().ok();
    let mut last_clipboard_text = clipboard.as_mut().and_then(|c| c.get_text().ok()).flatten();
    let mut clipboard_poll_interval = time::interval(Duration::from_millis(500));

    let mut recorder = if let Some(config) = config.recorder_config {
        Some(wavry_media::VideoRecorder::new(config)?)
    } else {
        None
    };

    let mut stream_codec: Option<Codec> = None;
    let mut stream_resolution: Option<MediaResolution> = None;
    let mut file_transfer = FileTransferState::new(
        &config.send_files,
        config.file_out_dir.clone(),
        config.file_max_bytes.max(1),
    );
    let mut file_command_rx = config.file_command_bus.as_ref().map(|bus| bus.subscribe());
    let mut transfer_budget_kbps = FILE_TRANSFER_MAX_KBPS;
    let mut file_transfer_limiter = FileTransferLimiter::new(FILE_TRANSFER_MIN_KBPS);
    let mut file_transfer_tick = time::interval(Duration::from_millis(FILE_TRANSFER_TICK_MS));

    let use_experimental_transport = env_bool("WAVRY_TRANSPORT_EXPERIMENTAL", false);
    if use_experimental_transport {
        info!("EXPERIMENTAL transport variants enabled");
    }

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

            // User-initiated file-transfer command channel.
            maybe_cmd = async {
                if let Some(rx) = file_command_rx.as_mut() {
                    match rx.recv().await {
                        Ok(cmd) => Some(cmd),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            warn!("dropped {} queued file-transfer command(s)", skipped);
                            None
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            std::future::pending::<Option<FileTransferCommand>>().await
                        }
                    }
                } else {
                    std::future::pending::<Option<FileTransferCommand>>().await
                }
            } => {
                if let Some(cmd) = maybe_cmd {
                    let status = file_status_message(
                        cmd.file_id,
                        rift_core::file_status::Status::InProgress,
                        cmd.action.as_protocol_message(),
                    );

                    // Apply immediately for local-outgoing/local-incoming state.
                    apply_file_status_to_outgoing(&mut file_transfer.outgoing, &status);
                    apply_file_status_to_incoming(&mut file_transfer.incoming, &status);

                    if let Some(alias) = session_alias {
                        let msg = ProtoMessage {
                            content: Some(rift_core::message::Content::Control(ProtoControl {
                                content: Some(rift_core::control_message::Content::FileStatus(status)),
                            })),
                        };
                        if let Err(e) = send_rift_msg(
                            &socket,
                            &mut crypto,
                            connect_addr,
                            msg,
                            Some(alias),
                            next_packet_id(),
                            relay_info,
                        ).await {
                            warn!("failed to send file transfer command: {}", e);
                        }
                    } else {
                        warn!("file transfer command ignored: session not established yet");
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

            // Clipboard polling
            _ = clipboard_poll_interval.tick() => {
                if let Some(ref mut c) = clipboard {
                    if let Ok(Some(current_text)) = c.get_text() {
                        if Some(current_text.clone()) != last_clipboard_text {
                            last_clipboard_text = Some(current_text.clone());
                            if let Some(alias) = session_alias {
                                let msg = ProtoMessage {
                                    content: Some(rift_core::message::Content::Control(ProtoControl {
                                        content: Some(rift_core::control_message::Content::Clipboard(
                                            rift_core::ClipboardMessage { text: current_text }
                                        )),
                                    })),
                                };
                                if let Err(e) = send_rift_msg(&socket, &mut crypto, connect_addr, msg, Some(alias), next_packet_id(), relay_info).await {
                                    debug!("clipboard send error: {}", e);
                                }
                            }
                        }
                    }
                }
            }

            _ = file_transfer_tick.tick() => {
                if let Some(alias) = session_alias {
                    if let Err(e) = send_next_file_chunk(
                        &socket,
                        &mut crypto,
                        connect_addr,
                        alias,
                        next_packet_id(),
                        relay_info,
                        transfer_budget_kbps,
                        &mut file_transfer_limiter,
                        &mut file_transfer.outgoing,
                    ).await {
                        warn!("file transfer send error: {}", e);
                    }
                }
            }

            // Jitter buffer drain
            _ = jitter_interval.tick() => {
                while let Some(ready) = jitter_buffer.pop_ready(now_us()) {
                    let mut rendered = false;
                    let render_start = Instant::now();

                    if let Some(ref mut rec) = recorder {
                        if let (Some(codec), Some(res)) = (stream_codec, stream_resolution) {
                            let _ = rec.write_frame(&ready.data, ready.keyframe, codec, res, 60);
                        }
                    }

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
                        let render_duration_us = render_start.elapsed().as_micros() as u32;
                        if let Some(stats) = runtime_stats.as_ref() {
                            stats.frames_decoded.fetch_add(1, Ordering::Relaxed);
                        }

                        if let Some(alias) = session_alias {
                            let latency = rift_core::LatencyStats {
                                frame_id: ready.frame_id,
                                capture_us: ready.capture_duration_us,
                                encode_us: ready.encode_duration_us,
                                network_us: (last_rtt_us / 2) as u32,
                                decode_us: render_duration_us, // Simplified: decode+render
                                render_us: 0,
                                total_us: 0,
                            };
                            let msg = ProtoMessage {
                                content: Some(rift_core::message::Content::Control(ProtoControl {
                                    content: Some(rift_core::control_message::Content::Latency(latency)),
                                })),
                            };
                            let _ = send_rift_msg(&socket, &mut crypto, connect_addr, msg, Some(alias), next_packet_id(), relay_info).await;
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
                                    transfer_budget_kbps =
                                        file_transfer_budget_kbps(ack.initial_bitrate_kbps.max(1));
                                    file_transfer_limiter.set_rate_kbps(transfer_budget_kbps);
                                    if let Some(stats) = runtime_stats.as_ref() {
                                        stats.connected.store(true, Ordering::Relaxed);
                                    }

                                    let negotiated_codec = match ack.selected_codec {
                                        c if c == RiftCodec::Av1 as i32 => Codec::Av1,
                                        c if c == RiftCodec::Hevc as i32 => Codec::Hevc,
                                        _ => Codec::H264,
                                    };
                                    stream_codec = Some(negotiated_codec);

                                    if let Some(res) = ack.stream_resolution {
                                        let negotiated_res = MediaResolution {
                                            width: res.width as u16,
                                            height: res.height as u16,
                                        };
                                        stream_resolution = Some(negotiated_res);

                                        if vr_adapter.is_none() {
                                            let config = DecodeConfig {
                                                codec: negotiated_codec,
                                                resolution: negotiated_res,
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
                                rift_core::control_message::Content::Clipboard(clip) => {
                                    if clip.text.len() > rift_core::MAX_CLIPBOARD_TEXT_BYTES {
                                        warn!("Received clipboard message exceeds size limit ({} bytes), ignoring", clip.text.len());
                                    } else {
                                        debug!("Received clipboard update from host");
                                        if let Some(ref mut c) = clipboard {
                                            let _ = c.set_text(clip.text.clone());
                                            last_clipboard_text = Some(clip.text);
                                        }
                                    }
                                }
                                rift_core::control_message::Content::FileHeader(header) => {
                                    let file_id = header.file_id;
                                    match offer_from_proto(header, file_transfer.max_file_bytes) {
                                        Ok(offer) => {
                                            if let Some(existing) = file_transfer.incoming.get(&file_id) {
                                                if existing.offer() == &offer {
                                                    let resume_chunk = existing.next_missing_chunk();
                                                    let status_msg = ProtoMessage {
                                                        content: Some(rift_core::message::Content::Control(ProtoControl {
                                                            content: Some(rift_core::control_message::Content::FileStatus(
                                                                file_status_message(
                                                                    file_id,
                                                                    rift_core::file_status::Status::InProgress,
                                                                    format!("resume_chunk={resume_chunk}"),
                                                                ),
                                                            )),
                                                        })),
                                                    };
                                                    if let Some(alias) = session_alias {
                                                        let _ = send_rift_msg(
                                                            &socket,
                                                            &mut crypto,
                                                            connect_addr,
                                                            status_msg,
                                                            Some(alias),
                                                            next_packet_id(),
                                                            relay_info,
                                                        )
                                                        .await;
                                                    }
                                                    continue;
                                                }

                                                let status_msg = ProtoMessage {
                                                    content: Some(rift_core::message::Content::Control(ProtoControl {
                                                        content: Some(rift_core::control_message::Content::FileStatus(
                                                            file_status_message(
                                                                file_id,
                                                                rift_core::file_status::Status::Error,
                                                                "file_id conflict with different offer",
                                                            ),
                                                        )),
                                                    })),
                                                };
                                                if let Some(alias) = session_alias {
                                                    let _ = send_rift_msg(
                                                        &socket,
                                                        &mut crypto,
                                                        connect_addr,
                                                        status_msg,
                                                        Some(alias),
                                                        next_packet_id(),
                                                        relay_info,
                                                    )
                                                    .await;
                                                }
                                                continue;
                                            }

                                            match IncomingFile::new(
                                                &file_transfer.output_dir,
                                                offer,
                                                file_transfer.max_file_bytes,
                                            ) {
                                                Ok(incoming) => {
                                                    info!("receiving file {} from host", incoming.offer().filename);
                                                    file_transfer.incoming.insert(file_id, incoming);
                                                    let status_msg = ProtoMessage {
                                                        content: Some(rift_core::message::Content::Control(ProtoControl {
                                                            content: Some(rift_core::control_message::Content::FileStatus(
                                                                file_status_message(
                                                                    file_id,
                                                                    rift_core::file_status::Status::Pending,
                                                                    "ready",
                                                                ),
                                                            )),
                                                        })),
                                                    };
                                                    if let Some(alias) = session_alias {
                                                        let _ = send_rift_msg(
                                                            &socket,
                                                            &mut crypto,
                                                            connect_addr,
                                                            status_msg,
                                                            Some(alias),
                                                            next_packet_id(),
                                                            relay_info,
                                                        )
                                                        .await;
                                                    }
                                                }
                                                Err(err) => {
                                                    warn!("rejecting file {}: {}", file_id, err);
                                                }
                                            }
                                        }
                                        Err(err) => warn!("invalid file offer {}: {}", file_id, err),
                                    }
                                }
                                rift_core::control_message::Content::FileStatus(status) => {
                                    let status_name = rift_core::file_status::Status::try_from(status.status)
                                        .map(|s| format!("{:?}", s))
                                        .unwrap_or_else(|_| format!("UNKNOWN({})", status.status));
                                    let message = sanitize_file_status_message(&status.message);
                                    info!(
                                        "host file transfer status file_id={} status={} message={}",
                                        status.file_id, status_name, message
                                    );
                                    apply_file_status_to_outgoing(&mut file_transfer.outgoing, &status);
                                    apply_file_status_to_incoming(&mut file_transfer.incoming, &status);
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

                                if let Some(ref mut rec) = recorder {
                                    let _ = rec.write_audio(&packet.payload, packet.timestamp_us);
                                }

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
                                                            if let Some(ref mut rec) = recorder {
                                                                if let (Some(codec), Some(res)) = (stream_codec, stream_resolution) {
                                                                    let _ = rec.write_frame(&ready.data, ready.keyframe, codec, res, 60);
                                                                }
                                                            }

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
                                                    if let Some(ref mut rec) = recorder {
                                                        let _ = rec.write_audio(&packet.payload, packet.timestamp_us);
                                                    }

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
                                                Some(rift_core::media_message::Content::FileChunk(chunk)) => {
                                                    if let Some(alias) = session_alias {
                                                        if let Err(err) = handle_incoming_file_chunk(
                                                            &socket,
                                                            &mut crypto,
                                                            connect_addr,
                                                            alias,
                                                            next_packet_id(),
                                                            relay_info,
                                                            &mut file_transfer.incoming,
                                                            chunk,
                                                        ).await {
                                                            warn!("file chunk handling error: {}", err);
                                                        }
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                            Some(rift_core::media_message::Content::FileChunk(chunk)) => {
                                if let Some(alias) = session_alias {
                                    if let Err(err) = handle_incoming_file_chunk(
                                        &socket,
                                        &mut crypto,
                                        connect_addr,
                                        alias,
                                        next_packet_id(),
                                        relay_info,
                                        &mut file_transfer.incoming,
                                        chunk,
                                    ).await {
                                        warn!("file chunk handling error: {}", err);
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

        // Sign feedback with identity key if available
        let signature = if let Some(identity_key_bytes) = config.identity_key {
            use rift_crypto::identity::IdentityKeypair;
            let keypair = IdentityKeypair::from_bytes(&identity_key_bytes);
            // Create a stable message to sign: session_id + relay_id + quality_score
            let message = format!("{}:{}:{}", relay.session_id, relay.relay_id, quality_score);
            let sig_bytes = keypair.sign(message.as_bytes());
            hex::encode(sig_bytes)
        } else {
            String::new()
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
            signature,
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

    if let Some(mut rec) = recorder {
        let _ = rec.finalize();
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn send_next_file_chunk(
    socket: &UdpSocket,
    crypto: &mut CryptoState,
    connect_addr: SocketAddr,
    alias: u32,
    packet_id: u64,
    relay_info: Option<&RelayInfo>,
    budget_kbps: u32,
    limiter: &mut FileTransferLimiter,
    outgoing: &mut VecDeque<OutgoingFile>,
) -> Result<()> {
    if !rotate_to_next_ready_transfer(outgoing) {
        return Ok(());
    }

    let mut progressed = false;
    limiter.set_rate_kbps(budget_kbps);

    {
        let front = outgoing
            .front_mut()
            .expect("rotate helper guaranteed front");
        if !front.header_sent() {
            let header = offer_to_proto(front.offer());
            let msg = ProtoMessage {
                content: Some(rift_core::message::Content::Control(ProtoControl {
                    content: Some(rift_core::control_message::Content::FileHeader(header)),
                })),
            };
            send_rift_msg(
                socket,
                crypto,
                connect_addr,
                msg,
                Some(alias),
                packet_id,
                relay_info,
            )
            .await?;
            front.mark_header_sent();
            info!(
                "started sending file {} ({} bytes)",
                front.offer().filename,
                front.offer().file_size
            );
            progressed = true;
        } else {
            let current_chunk = front.next_chunk_index();
            match front.next_chunk()? {
                Some(chunk) => {
                    if !limiter.try_take(chunk.payload.len()) {
                        front.set_next_chunk(current_chunk)?;
                    } else {
                        let msg = ProtoMessage {
                            content: Some(rift_core::message::Content::Media(
                                rift_core::MediaMessage {
                                    content: Some(rift_core::media_message::Content::FileChunk(
                                        rift_core::FileChunk {
                                            file_id: chunk.file_id,
                                            chunk_index: chunk.chunk_index,
                                            payload: chunk.payload,
                                        },
                                    )),
                                },
                            )),
                        };
                        send_rift_msg(
                            socket,
                            crypto,
                            connect_addr,
                            msg,
                            Some(alias),
                            packet_id,
                            relay_info,
                        )
                        .await?;
                        progressed = true;
                    }
                }
                None => {
                    // completion is finalized after remote FileStatus::Complete
                }
            }
        }
    }

    if progressed && outgoing.len() > 1 {
        outgoing.rotate_left(1);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_incoming_file_chunk(
    socket: &UdpSocket,
    crypto: &mut CryptoState,
    connect_addr: SocketAddr,
    alias: u32,
    packet_id: u64,
    relay_info: Option<&RelayInfo>,
    incoming: &mut HashMap<u64, IncomingFile>,
    chunk: rift_core::FileChunk,
) -> Result<()> {
    let file_id = chunk.file_id;
    let mut progress_update: Option<(u32, u32, u32)> = None;
    let complete = if let Some(entry) = incoming.get_mut(&file_id) {
        let complete = entry.write_chunk(chunk.chunk_index, &chunk.payload)?;
        if !complete {
            let resume_chunk = entry.next_missing_chunk();
            let received = entry.received_count();
            let total = entry.offer().total_chunks;
            let remaining = total.saturating_sub(received);
            let gap_detected = resume_chunk < chunk.chunk_index;
            let progress_due = received % FILE_TRANSFER_PROGRESS_CHUNK_INTERVAL == 0;
            if gap_detected || progress_due || remaining <= 2 {
                progress_update = Some((resume_chunk, received, total));
            }
        }
        complete
    } else {
        let msg = ProtoMessage {
            content: Some(rift_core::message::Content::Control(ProtoControl {
                content: Some(rift_core::control_message::Content::FileStatus(
                    file_status_message(
                        file_id,
                        rift_core::file_status::Status::Error,
                        "no matching file offer",
                    ),
                )),
            })),
        };
        let _ = send_rift_msg(
            socket,
            crypto,
            connect_addr,
            msg,
            Some(alias),
            packet_id,
            relay_info,
        )
        .await;
        return Ok(());
    };

    if !complete {
        if let Some((resume_chunk, received, total)) = progress_update {
            let msg = ProtoMessage {
                content: Some(rift_core::message::Content::Control(ProtoControl {
                    content: Some(rift_core::control_message::Content::FileStatus(
                        file_status_message(
                            file_id,
                            rift_core::file_status::Status::InProgress,
                            format!("resume_chunk={resume_chunk} received={received}/{total}"),
                        ),
                    )),
                })),
            };
            let _ = send_rift_msg(
                socket,
                crypto,
                connect_addr,
                msg,
                Some(alias),
                packet_id,
                relay_info,
            )
            .await;
        }
        return Ok(());
    }

    if let Some(entry) = incoming.remove(&file_id) {
        match entry.finalize() {
            Ok(path) => {
                info!(
                    "received file from host file_id={} path={}",
                    file_id,
                    path.display()
                );
                let msg = ProtoMessage {
                    content: Some(rift_core::message::Content::Control(ProtoControl {
                        content: Some(rift_core::control_message::Content::FileStatus(
                            file_status_message(
                                file_id,
                                rift_core::file_status::Status::Complete,
                                path.display().to_string(),
                            ),
                        )),
                    })),
                };
                let _ = send_rift_msg(
                    socket,
                    crypto,
                    connect_addr,
                    msg,
                    Some(alias),
                    packet_id,
                    relay_info,
                )
                .await;
            }
            Err(err) => {
                warn!("failed to finalize incoming file {}: {}", file_id, err);
                let msg = ProtoMessage {
                    content: Some(rift_core::message::Content::Control(ProtoControl {
                        content: Some(rift_core::control_message::Content::FileStatus(
                            file_status_message(
                                file_id,
                                rift_core::file_status::Status::Error,
                                err.to_string(),
                            ),
                        )),
                    })),
                };
                let _ = send_rift_msg(
                    socket,
                    crypto,
                    connect_addr,
                    msg,
                    Some(alias),
                    packet_id,
                    relay_info,
                )
                .await;
            }
        }
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
