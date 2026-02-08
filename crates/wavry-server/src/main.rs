mod webrtc_bridge;

mod host {
    use std::{
        collections::{HashMap, VecDeque},
        fmt,
        net::SocketAddr,
        sync::Arc,
        time::Duration,
    };

    use anyhow::{anyhow, Result};
    use clap::Parser;
    use mdns_sd::{ServiceDaemon, ServiceInfo};
    use rift_core::{
        chunk_video_payload, decode_msg, encode_msg, Codec as RiftCodec,
        ControlMessage as ProtoControl, FecBuilder, Handshake, HelloAck as ProtoHelloAck,
        Message as ProtoMessage, PhysicalPacket, Resolution as ProtoResolution, Role, RIFT_VERSION,
    };
    use rift_crypto::connection::SecureServer;
    #[cfg(not(target_os = "linux"))]
    use wavry_media::DummyEncoder as VideoEncoder;
    #[cfg(target_os = "linux")]
    use wavry_media::LinuxProbe;
    #[cfg(target_os = "macos")]
    use wavry_media::MacProbe;
    #[cfg(target_os = "linux")]
    use wavry_media::PipewireEncoder as VideoEncoder;
    #[cfg(target_os = "windows")]
    use wavry_media::WindowsProbe;
    use wavry_media::{
        CapabilityProbe, Codec, EncodeConfig, EncodedFrame, Resolution as MediaResolution,
    };

    use bytes::Bytes;
    use socket2::SockRef;
    use tokio::{net::UdpSocket, sync::mpsc, time};
    use tracing::{debug, error, info, warn};
    #[cfg(not(target_os = "linux"))]
    use wavry_platform::DummyInjector as InjectorImpl;
    use wavry_platform::InputInjector;
    #[cfg(target_os = "linux")]
    use wavry_platform::UinputInjector as InjectorImpl;

    use crate::webrtc_bridge::WebRtcBridge;

    const MAX_DATAGRAM_SIZE: usize = 1200;
    const FEC_SHARD_COUNT: u32 = 8;
    const UNASSIGNED_SESSION_ID: [u8; 16] = [0u8; 16];
    const DSCP_EF: u32 = 0x2E;
    const PACER_MIN_US: u64 = 20;
    const PACER_MAX_US: u64 = 500;
    const PACER_BASE_US: f64 = 30.0;
    const NACK_HISTORY: usize = 512;
    const PEER_CLEANUP_INTERVAL_SECS: u64 = 2;
    const DEFAULT_RESOLUTION_WIDTH: u16 = 1280;
    const DEFAULT_RESOLUTION_HEIGHT: u16 = 720;
    const MIN_STREAM_DIMENSION: u32 = 320;
    const MAX_STREAM_DIMENSION: u32 = 8192;

    #[derive(Parser, Debug)]
    #[command(name = "wavry-server")]
    struct Args {
        /// UDP listen address (use :0 for random)
        #[arg(long, env = "WAVRY_LISTEN_ADDR", default_value = "0.0.0.0:0")]
        listen: SocketAddr,

        /// Disable encryption (for testing/debugging)
        #[arg(long, env = "WAVRY_NO_ENCRYPT", default_value = "false")]
        no_encrypt: bool,

        /// Default stream width
        #[arg(long, default_value_t = DEFAULT_RESOLUTION_WIDTH as u32)]
        width: u32,

        /// Default stream height
        #[arg(long, default_value_t = DEFAULT_RESOLUTION_HEIGHT as u32)]
        height: u32,

        /// Target stream FPS
        #[arg(long, default_value_t = 60)]
        fps: u32,

        /// Initial target bitrate in kbps
        #[arg(long, default_value_t = 20_000)]
        bitrate_kbps: u32,

        /// Keyframe interval in milliseconds
        #[arg(long, default_value_t = 1_000)]
        keyframe_interval_ms: u32,

        /// Display ID for capture backends
        #[arg(long, env = "WAVRY_DISPLAY_ID")]
        display_id: Option<u32>,

        /// Disable mDNS host advertisement
        #[arg(long, default_value_t = false)]
        disable_mdns: bool,

        /// Maximum number of tracked peer endpoints
        #[arg(long, default_value_t = 64)]
        max_peers: usize,

        /// Drop peers that stay silent for this many seconds
        #[arg(long, default_value_t = 30)]
        peer_idle_timeout_secs: u64,

        /// Minimum interval between detailed stats logs
        #[arg(long, default_value_t = 10)]
        stats_log_interval_secs: u64,

        /// Signaling gateway URL (WebSocket)
        #[arg(
            long,
            env = "WAVRY_GATEWAY_URL",
            default_value = "ws://127.0.0.1:3000/ws"
        )]
        gateway_url: String,

        /// Session token for signaling
        #[arg(long, env = "WAVRY_SESSION_TOKEN")]
        session_token: Option<String>,

        /// Enable WebRTC bridge for web clients
        #[arg(long, env = "WAVRY_ENABLE_WEBRTC", default_value_t = false)]
        enable_webrtc: bool,
    }

    #[derive(Clone, Copy, Debug)]
    struct HostRuntimeConfig {
        default_resolution: MediaResolution,
        fps: u32,
        initial_bitrate_kbps: u32,
        keyframe_interval_ms: u32,
        max_peers: usize,
        peer_idle_timeout: Duration,
        stats_log_interval: Duration,
    }

    fn env_bool(name: &str, default: bool) -> bool {
        match std::env::var(name) {
            Ok(value) => matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            ),
            Err(_) => default,
        }
    }

    /// Crypto state for a peer
    enum CryptoState {
        /// No encryption (--no-encrypt mode)
        Disabled,
        /// Crypto handshake in progress
        Handshaking(SecureServer),
        /// Crypto established
        Established(SecureServer),
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
        fn new(disabled: bool) -> Self {
            if disabled {
                CryptoState::Disabled
            } else {
                CryptoState::Handshaking(SecureServer::new().expect("failed to create crypto"))
            }
        }

        fn is_established(&self) -> bool {
            matches!(self, CryptoState::Established(_) | CryptoState::Disabled)
        }
    }

    struct PeerState {
        crypto: CryptoState,
        handshake: Handshake,
        pending_crypto_msg2: Option<Bytes>,
        session_id: Option<Vec<u8>>,
        session_alias: u32,
        next_packet_id: u64,
        frame_id: u64,
        pacer: Pacer,
        send_history: SendHistory,
        target_bitrate_kbps: u32,
        skip_frames: u32,
        #[allow(dead_code)]
        fec_builder: FecBuilder,
        last_seen: time::Instant,
        last_stats_log: time::Instant,
        client_name: Option<String>,
    }

    async fn ensure_encoder(
        frame_rx: &mut Option<mpsc::Receiver<FrameIn>>,
        selected_codec: &mut Option<Codec>,
        current_display_id: &mut Option<u32>,
        base: EncodeConfig,
        codec: Codec,
    ) -> Result<()> {
        if selected_codec == &Some(codec) && current_display_id == &base.display_id && frame_rx.is_some() {
            return Ok(());
        }

        let mut config = base;
        config.codec = codec;
        let encoder = VideoEncoder::new(config).await?;
        let (frame_tx, rx) = mpsc::channel::<FrameIn>(2);

        std::thread::spawn(move || {
            let mut encoder = encoder;
            loop {
                match encoder.next_frame() {
                    Ok(frame) => {
                        if frame_tx.blocking_send(frame).is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        eprintln!("encoder error: {err}");
                        break;
                    }
                }
            }
        });

        *frame_rx = Some(rx);
        *selected_codec = Some(codec);
        *current_display_id = base.display_id;
        info!("Selected encoder codec: {:?}, display: {:?}", codec, base.display_id);
        Ok(())
    }

    fn choose_codec_for_hello(hello: &rift_core::Hello, local_supported: &[Codec]) -> Codec {
        let remote_supported: Vec<RiftCodec> = hello
            .supported_codecs
            .iter()
            .filter_map(|c| RiftCodec::try_from(*c).ok())
            .collect();

        let supports = |codec: Codec| {
            let remote_ok = match codec {
                Codec::Av1 => remote_supported.contains(&RiftCodec::Av1),
                Codec::Hevc => remote_supported.contains(&RiftCodec::Hevc),
                Codec::H264 => remote_supported.contains(&RiftCodec::H264),
            };
            local_supported.contains(&codec) && remote_ok
        };

        if supports(Codec::Av1) {
            Codec::Av1
        } else if supports(Codec::Hevc) {
            Codec::Hevc
        } else {
            Codec::H264
        }
    }

    fn filter_realtime_codecs(
        caps: Vec<wavry_media::VideoCodecCapability>,
        fallback: Vec<Codec>,
    ) -> Vec<Codec> {
        let mut preferred: Vec<Codec> = caps
            .into_iter()
            .filter(|cap| cap.codec != Codec::Av1 || cap.hardware_accelerated)
            .map(|cap| cap.codec)
            .collect();

        if preferred.is_empty() {
            preferred = fallback
                .into_iter()
                .filter(|codec| *codec != Codec::Av1)
                .collect();
        }

        if !preferred.contains(&Codec::H264) {
            preferred.push(Codec::H264);
        }
        preferred
    }

    #[cfg(target_os = "linux")]
    fn local_supported_encoders() -> Vec<Codec> {
        let probe = LinuxProbe;
        filter_realtime_codecs(
            probe.encoder_capabilities().unwrap_or_default(),
            probe
                .supported_encoders()
                .unwrap_or_else(|_| vec![Codec::H264]),
        )
    }

    #[cfg(target_os = "macos")]
    fn local_supported_encoders() -> Vec<Codec> {
        let probe = MacProbe;
        filter_realtime_codecs(
            probe.encoder_capabilities().unwrap_or_default(),
            probe
                .supported_encoders()
                .unwrap_or_else(|_| vec![Codec::H264]),
        )
    }

    #[cfg(target_os = "windows")]
    fn local_supported_encoders() -> Vec<Codec> {
        let probe = WindowsProbe;
        filter_realtime_codecs(
            probe.encoder_capabilities().unwrap_or_default(),
            probe
                .supported_encoders()
                .unwrap_or_else(|_| vec![Codec::H264]),
        )
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    fn local_supported_encoders() -> Vec<Codec> {
        vec![Codec::H264]
    }

    fn get_monitor_list() -> Vec<rift_core::MonitorInfo> {
        #[cfg(target_os = "linux")]
        let probe = LinuxProbe;
        #[cfg(target_os = "macos")]
        let probe = MacProbe;
        #[cfg(target_os = "windows")]
        let probe = WindowsProbe;

        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        {
            if let Ok(displays) = probe.enumerate_displays() {
                return displays
                    .into_iter()
                    .map(|d| rift_core::MonitorInfo {
                        id: d.id,
                        name: d.name,
                        width: d.resolution.width as u32,
                        height: d.resolution.height as u32,
                    })
                    .collect();
            }
        }

        vec![]
    }

    impl PeerState {
        fn new(no_encrypt: bool, initial_bitrate_kbps: u32) -> Self {
            let now = time::Instant::now();
            Self {
                crypto: CryptoState::new(no_encrypt),
                handshake: Handshake::new(Role::Host),
                pending_crypto_msg2: None,
                session_id: None,
                session_alias: rand::random::<u32>().max(1),
                next_packet_id: 1,
                frame_id: 0,
                pacer: Pacer::new(),
                send_history: SendHistory::new(NACK_HISTORY),
                target_bitrate_kbps: initial_bitrate_kbps,
                skip_frames: 0,
                fec_builder: FecBuilder::new(FEC_SHARD_COUNT).unwrap(),
                last_seen: now,
                last_stats_log: now,
                client_name: None,
            }
        }
    }

    type FrameIn = EncodedFrame;

    #[derive(Debug)]
    struct SendHistory {
        capacity: usize,
        order: VecDeque<u64>,
        packets: HashMap<u64, Bytes>,
    }

    impl SendHistory {
        fn new(capacity: usize) -> Self {
            Self {
                capacity,
                order: VecDeque::new(),
                packets: HashMap::new(),
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
            let rtt_increase =
                ((self.rtt_smooth_us - rtt_base).max(0.0) / rtt_base).clamp(0.0, 2.0);
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

    pub async fn run() -> Result<()> {
        let args = Args::parse();
        tracing_subscriber::fmt().with_env_filter("info").init();

        let runtime = validate_runtime_config(&args)?;
        if !args.listen.ip().is_loopback() && !env_bool("WAVRY_SERVER_ALLOW_PUBLIC_BIND", false) {
            return Err(anyhow!(
                "refusing non-loopback server bind without WAVRY_SERVER_ALLOW_PUBLIC_BIND=1"
            ));
        }

        let socket = UdpSocket::bind(args.listen).await?;
        let local_addr = socket.local_addr()?;
        info!("listening on {}", local_addr);

        if let Err(e) = SockRef::from(&socket).set_tos_v4(DSCP_EF) {
            debug!("failed to set DSCP/TOS: {}", e);
        }

        if args.no_encrypt {
            warn!("ENCRYPTION DISABLED - not for production use");
        }
        let _mdns = if args.disable_mdns {
            info!("mDNS advertisement disabled");
            None
        } else {
            Some(advertise_mdns(local_addr)?)
        };

        let mut injector = InjectorImpl::new()?;

        let (webrtc_input_tx, mut webrtc_input_rx) =
            mpsc::unbounded_channel::<rift_core::input_message::Event>();

        let webrtc_bridge = if args.enable_webrtc {
            if let Some(token) = &args.session_token {
                let bridge = Arc::new(
                    WebRtcBridge::new(args.gateway_url.clone(), token.clone(), webrtc_input_tx)
                        .await?,
                );
                let bridge_clone = Arc::clone(&bridge);
                tokio::spawn(async move {
                    if let Err(e) = bridge_clone.run().await {
                        error!("WebRTC bridge error: {}", e);
                    }
                });
                Some(bridge)
            } else {
                warn!("WebRTC enabled but no session token provided; bridge will not start");
                None
            }
        } else {
            None
        };

        let mut base_config = EncodeConfig {
            codec: Codec::H264,
            resolution: runtime.default_resolution,
            fps: runtime.fps as u16,
            bitrate_kbps: runtime.initial_bitrate_kbps,
            keyframe_interval_ms: runtime.keyframe_interval_ms,
            display_id: args.display_id,
            enable_10bit: false,
            enable_hdr: false,
        };

        let mut buf = vec![0u8; 64 * 1024];
        let mut peers: HashMap<SocketAddr, PeerState> = HashMap::new();
        let mut active_peer: Option<SocketAddr> = None;
        let mut frame_rx: Option<mpsc::Receiver<FrameIn>> = None;
        let mut selected_codec: Option<Codec> = None;
        let mut current_display_id: Option<u32> = None;
        let local_supported = local_supported_encoders();
        info!("Local encoder candidates: {:?}", local_supported);
        let no_encrypt = args.no_encrypt;
        let mut peer_cleanup_interval =
            time::interval(Duration::from_secs(PEER_CLEANUP_INTERVAL_SECS));

        if args.enable_webrtc && selected_codec.is_none() {
            ensure_encoder(&mut frame_rx, &mut selected_codec, &mut current_display_id, base_config, Codec::H264).await?;
        }

        loop {
            tokio::select! {
                Some(event) = webrtc_input_rx.recv() => {
                    if let Err(e) = handle_input_event(&mut injector, event) {
                        warn!("WebRTC input injection failed: {}", e);
                    }
                }
                _ = peer_cleanup_interval.tick() => {
                    cleanup_inactive_peers(
                        &mut peers,
                        &mut active_peer,
                        runtime.peer_idle_timeout,
                    );
                }
                Some(frame) = async {
                    if let Some(rx) = frame_rx.as_mut() {
                        rx.recv().await
                    } else {
                        None
                    }
                } => {
                    if let Some(ref bridge) = webrtc_bridge {
                        let _ = bridge.push_frame(frame.clone()).await;
                    }

                    if let Some(peer) = active_peer {
                        if let Some(peer_state) = peers.get_mut(&peer) {
                            if peer_state.skip_frames > 0 {
                                peer_state.skip_frames = peer_state.skip_frames.saturating_sub(1);
                                continue;
                            }
                            let result = send_video_frame(&socket, peer, peer_state, frame).await;
                            if let Err(err) = result {
                                warn!("failed to send video frame to {}: {}", peer, err);
                            }
                        }
                    }
                }
                recv = socket.recv_from(&mut buf) => {
                    let (len, peer) = recv?;
                    let raw = &buf[..len];

                    if !peers.contains_key(&peer) && peers.len() >= runtime.max_peers {
                        warn!(
                            "dropping packet from {}: peer table full (max_peers={})",
                            peer, runtime.max_peers
                        );
                        continue;
                    }

                    let peer_state = peers
                        .entry(peer)
                        .or_insert_with(|| PeerState::new(no_encrypt, runtime.initial_bitrate_kbps));

                    match handle_raw_packet(
                        &socket,
                        peer_state,
                        &mut active_peer,
                        peer,
                        raw,
                        &mut injector,
                        runtime,
                        &local_supported,
                        &mut base_config,
                    )
                    .await
                    {
                        Ok(Some(codec)) => {
                            if let Err(err) =
                                ensure_encoder(&mut frame_rx, &mut selected_codec, &mut current_display_id, base_config, codec).await
                            {
                                warn!("encoder start failed: {}", err);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            debug!("packet from {} dropped: {}", peer, e);
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_raw_packet(
        socket: &UdpSocket,
        peer_state: &mut PeerState,
        active_peer: &mut Option<SocketAddr>,
        peer: SocketAddr,
        raw: &[u8],
        injector: &mut InjectorImpl,
        runtime: HostRuntimeConfig,
        local_supported: &[Codec],
        base_config: &mut EncodeConfig,
    ) -> Result<Option<Codec>> {
        peer_state.last_seen = time::Instant::now();
        let phys = PhysicalPacket::decode(Bytes::copy_from_slice(raw))
            .map_err(|e| anyhow!("RIFT decode error: {}", e))?;

        match &mut peer_state.crypto {
            CryptoState::Disabled => {
                let msg =
                    decode_msg(&phys.payload).map_err(|e| anyhow!("Proto decode error: {}", e))?;
                handle_rift_msg(
                    socket,
                    peer_state,
                    active_peer,
                    peer,
                    msg,
                    injector,
                    runtime,
                    local_supported,
                    base_config,
                )
                .await
            }
            CryptoState::Handshaking(server) => {
                if let Some(sid) = phys.session_id {
                    if sid == 0 {
                        let msg2_payload =
                            if let Some(cached) = peer_state.pending_crypto_msg2.clone() {
                                debug!("resending cached crypto msg2 to {}", peer);
                                cached
                            } else {
                                info!("crypto handshake msg1 from {}", peer);
                                let msg2_payload = server
                                    .process_client_hello(&phys.payload)
                                    .map_err(|e| anyhow!("Noise error: {}", e))?;
                                let cached = Bytes::copy_from_slice(&msg2_payload);
                                peer_state.pending_crypto_msg2 = Some(cached.clone());
                                cached
                            };

                        let resp = PhysicalPacket {
                            version: RIFT_VERSION,
                            session_id: Some(0),
                            session_alias: None,
                            packet_id: 0,
                            payload: msg2_payload,
                        };
                        socket.send_to(&resp.encode(), peer).await?;
                        Ok(None)
                    } else {
                        Err(anyhow!("unexpected session_id in crypto handshake"))
                    }
                } else if phys.session_alias.is_some() {
                    info!("crypto handshake msg3 from {}", peer);
                    server
                        .process_client_finish(&phys.payload)
                        .map_err(|e| anyhow!("Noise error: {}", e))?;

                    let old_crypto =
                        std::mem::replace(&mut peer_state.crypto, CryptoState::Disabled);
                    if let CryptoState::Handshaking(server) = old_crypto {
                        peer_state.crypto = CryptoState::Established(server);
                        peer_state.pending_crypto_msg2 = None;
                        info!("crypto established with {}", peer);
                    }
                    Ok(None)
                } else {
                    Err(anyhow!("unexpected packet format during crypto handshake"))
                }
            }
            CryptoState::Established(server) => {
                let plaintext = server
                    .decrypt(phys.packet_id, &phys.payload)
                    .map_err(|e| anyhow!("Decrypt failed: {}", e))?;

                let msg =
                    decode_msg(&plaintext).map_err(|e| anyhow!("Proto decode error: {}", e))?;
                handle_rift_msg(
                    socket,
                    peer_state,
                    active_peer,
                    peer,
                    msg,
                    injector,
                    runtime,
                    local_supported,
                    base_config,
                )
                .await
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_rift_msg(
        socket: &UdpSocket,
        peer_state: &mut PeerState,
        active_peer: &mut Option<SocketAddr>,
        peer: SocketAddr,
        msg: ProtoMessage,
        injector: &mut InjectorImpl,
        runtime: HostRuntimeConfig,
        local_supported: &[Codec],
        base_config: &mut EncodeConfig,
    ) -> Result<Option<Codec>> {
        use rift_core::message::Content;

        let content = msg
            .content
            .ok_or_else(|| anyhow!("empty message content"))?;
        match content {
            Content::Control(ctrl) => {
                let ctrl_content = ctrl
                    .content
                    .ok_or_else(|| anyhow!("empty control content"))?;
                match ctrl_content {
                    rift_core::control_message::Content::Hello(hello) => {
                        if !peer_state.crypto.is_established() {
                            return Err(anyhow!("crypto required before RIFT hello"));
                        }

                        if active_peer.is_some() && *active_peer != Some(peer) {
                            let ack = ProtoHelloAck {
                                accepted: false,
                                selected_codec: 0,
                                stream_resolution: None,
                                fps: 0,
                                initial_bitrate_kbps: 0,
                                keyframe_interval_ms: 0,
                                session_id: UNASSIGNED_SESSION_ID.to_vec(),
                                session_alias: 0,
                                public_addr: String::new(),
                            };
                            send_rift_msg(
                                socket,
                                peer_state,
                                peer,
                                ProtoMessage {
                                    content: Some(Content::Control(ProtoControl {
                                        content: Some(
                                            rift_core::control_message::Content::HelloAck(ack),
                                        ),
                                    })),
                                },
                            )
                            .await?;
                            return Ok(None);
                        }

                        info!(
                            "RIFT hello from {} (platform={:?}, codecs={:?}, max_fps={})",
                            hello.client_name,
                            hello.platform(),
                            hello.supported_codecs,
                            hello.max_fps
                        );
                        peer_state
                            .handshake
                            .on_receive_hello(&hello)
                            .map_err(|e| anyhow!("Handshake error: {}", e))?;

                        let session_id = rand::random::<[u8; 16]>().to_vec();
                        peer_state.session_id = Some(session_id.clone());
                        peer_state.frame_id = 0;
                        peer_state.client_name = Some(hello.client_name.clone());
                        peer_state.target_bitrate_kbps = runtime.initial_bitrate_kbps;

                        let desired_codec = choose_codec_for_hello(&hello, local_supported);
                        let stream_resolution = normalize_stream_resolution(
                            hello.max_resolution,
                            runtime.default_resolution,
                        );
                        let ack = ProtoHelloAck {
                            accepted: true,
                            selected_codec: match desired_codec {
                                Codec::Av1 => RiftCodec::Av1 as i32,
                                Codec::Hevc => RiftCodec::Hevc as i32,
                                Codec::H264 => RiftCodec::H264 as i32,
                            },
                            stream_resolution: Some(stream_resolution),
                            fps: runtime.fps,
                            initial_bitrate_kbps: runtime.initial_bitrate_kbps,
                            keyframe_interval_ms: runtime.keyframe_interval_ms,
                            session_id: session_id.clone(),
                            session_alias: peer_state.session_alias,
                            public_addr: String::new(),
                        };

                        peer_state
                            .handshake
                            .on_send_hello_ack(&ack)
                            .map_err(|e| anyhow!("Handshake error: {}", e))?;
                        *active_peer = Some(peer);

                        send_rift_msg(
                            socket,
                            peer_state,
                            peer,
                            ProtoMessage {
                                content: Some(Content::Control(ProtoControl {
                                    content: Some(rift_core::control_message::Content::HelloAck(
                                        ack,
                                    )),
                                })),
                            },
                        )
                        .await?;

                        // Send monitor list for discovery
                        let monitors = get_monitor_list();
                        if !monitors.is_empty() {
                            let list_msg = ProtoMessage {
                                content: Some(Content::Control(ProtoControl {
                                    content: Some(rift_core::control_message::Content::MonitorList(
                                        rift_core::MonitorList { monitors },
                                    )),
                                })),
                            };
                            let _ = send_rift_msg(socket, peer_state, peer, list_msg).await;
                        }

                        info!(
                            "session established with {} (client={}, codec={:?}, resolution={}x{}, session_id={})",
                            peer,
                            hello.client_name,
                            desired_codec,
                            stream_resolution.width,
                            stream_resolution.height,
                            hex::encode(&session_id)
                        );
                        return Ok(Some(desired_codec));
                    }
                    rift_core::control_message::Content::Ping(ping) => {
                        let pong = rift_core::Pong {
                            timestamp_us: ping.timestamp_us,
                        };
                        send_rift_msg(
                            socket,
                            peer_state,
                            peer,
                            ProtoMessage {
                                content: Some(Content::Control(ProtoControl {
                                    content: Some(rift_core::control_message::Content::Pong(pong)),
                                })),
                            },
                        )
                        .await?;
                    }
                    rift_core::control_message::Content::Stats(report) => {
                        if peer_state.last_stats_log.elapsed() >= runtime.stats_log_interval {
                            let total = report.received_packets.saturating_add(report.lost_packets);
                            let loss_percent = if total == 0 {
                                0.0
                            } else {
                                (report.lost_packets as f64 * 100.0) / total as f64
                            };
                            info!(
                                "stats from {}: rtt={}ms jitter={}us loss={:.2}% rx={} lost={}",
                                peer,
                                report.rtt_us / 1000,
                                report.jitter_us,
                                loss_percent,
                                report.received_packets,
                                report.lost_packets
                            );
                            peer_state.last_stats_log = time::Instant::now();
                        }
                        peer_state.pacer.on_stats(
                            report.rtt_us,
                            report.jitter_us,
                            peer_state.target_bitrate_kbps,
                        );
                    }
                    rift_core::control_message::Content::Congestion(cc) => {
                        let requested = cc.target_bitrate_kbps.clamp(1_000, 100_000);
                        if requested != peer_state.target_bitrate_kbps {
                            debug!(
                                "peer {} congestion target update: {} -> {} kbps",
                                peer, peer_state.target_bitrate_kbps, requested
                            );
                            peer_state.target_bitrate_kbps = requested;
                        }
                    }
                    rift_core::control_message::Content::Nack(nack) => {
                        for packet_id in nack.packet_ids {
                            if let Some(payload) = peer_state.send_history.get(packet_id) {
                                let _ = socket.send_to(&payload, peer).await;
                            }
                        }
                    }
                    rift_core::control_message::Content::EncoderControl(ctrl) => {
                        if ctrl.skip_frames > 0 {
                            peer_state.skip_frames =
                                (peer_state.skip_frames + ctrl.skip_frames).min(4);
                        }
                    }
                    rift_core::control_message::Content::PoseUpdate(pose) => {
                        let _ = pose;
                    }
                    rift_core::control_message::Content::HandPoseUpdate(hand_pose) => {
                        let _ = hand_pose;
                    }
                    rift_core::control_message::Content::VrTiming(_timing) => {}
                    rift_core::control_message::Content::SelectMonitor(select) => {
                        info!("Client selected monitor: {}", select.monitor_id);
                        base_config.display_id = Some(select.monitor_id);
                        return Ok(Some(selected_codec.unwrap_or(Codec::H264)));
                    }
                    _ => {}
                }
            }
            Content::Input(input_msg) => {
                if let Some(event) = input_msg.event {
                    handle_input_event(injector, event)?;
                }
            }
            _ => {}
        }
        Ok(None)
    }

    fn handle_input_event(
        injector: &mut InjectorImpl,
        event: rift_core::input_message::Event,
    ) -> Result<()> {
        use rift_core::input_message::Event;
        match event {
            Event::Key(k) => injector.key(k.keycode, k.pressed)?,
            Event::MouseButton(m) => injector.mouse_button(m.button as u8, m.pressed)?,
            Event::MouseMove(m) => injector.mouse_absolute(m.x, m.y)?,
            Event::Scroll(s) => {
                // TODO: Add scroll to InputInjector trait
                debug!("Scroll event received: dx={}, dy={}", s.dx, s.dy);
            }
            Event::Gamepad(g) => {
                // TODO: Add gamepad to InputInjector trait
                debug!("Gamepad event received from ID {}", g.gamepad_id);
            }
        }
        Ok(())
    }

    fn validate_runtime_config(args: &Args) -> Result<HostRuntimeConfig> {
        if args.width < MIN_STREAM_DIMENSION || args.width > MAX_STREAM_DIMENSION {
            return Err(anyhow!(
                "--width must be between {} and {}",
                MIN_STREAM_DIMENSION,
                MAX_STREAM_DIMENSION
            ));
        }
        if args.height < MIN_STREAM_DIMENSION || args.height > MAX_STREAM_DIMENSION {
            return Err(anyhow!(
                "--height must be between {} and {}",
                MIN_STREAM_DIMENSION,
                MAX_STREAM_DIMENSION
            ));
        }
        if args.width > u16::MAX as u32 || args.height > u16::MAX as u32 {
            return Err(anyhow!(
                "--width/--height must fit within 16-bit capture backends"
            ));
        }
        if args.fps == 0 || args.fps > 240 {
            return Err(anyhow!("--fps must be between 1 and 240"));
        }
        if args.bitrate_kbps < 500 || args.bitrate_kbps > 200_000 {
            return Err(anyhow!("--bitrate-kbps must be between 500 and 200000"));
        }
        if args.keyframe_interval_ms < 100 || args.keyframe_interval_ms > 10_000 {
            return Err(anyhow!(
                "--keyframe-interval-ms must be between 100 and 10000"
            ));
        }
        if args.max_peers == 0 {
            return Err(anyhow!("--max-peers must be at least 1"));
        }
        if args.peer_idle_timeout_secs == 0 {
            return Err(anyhow!("--peer-idle-timeout-secs must be at least 1"));
        }
        if args.stats_log_interval_secs == 0 {
            return Err(anyhow!("--stats-log-interval-secs must be at least 1"));
        }

        Ok(HostRuntimeConfig {
            default_resolution: MediaResolution {
                width: args.width as u16,
                height: args.height as u16,
            },
            fps: args.fps,
            initial_bitrate_kbps: args.bitrate_kbps,
            keyframe_interval_ms: args.keyframe_interval_ms,
            max_peers: args.max_peers,
            peer_idle_timeout: Duration::from_secs(args.peer_idle_timeout_secs),
            stats_log_interval: Duration::from_secs(args.stats_log_interval_secs),
        })
    }

    fn normalize_stream_resolution(
        requested: Option<ProtoResolution>,
        fallback: MediaResolution,
    ) -> ProtoResolution {
        let (requested_w, requested_h) = requested
            .map(|r| (r.width, r.height))
            .unwrap_or((fallback.width as u32, fallback.height as u32));
        let width = requested_w
            .clamp(MIN_STREAM_DIMENSION, MAX_STREAM_DIMENSION)
            .min(u16::MAX as u32);
        let height = requested_h
            .clamp(MIN_STREAM_DIMENSION, MAX_STREAM_DIMENSION)
            .min(u16::MAX as u32);
        ProtoResolution { width, height }
    }

    fn cleanup_inactive_peers(
        peers: &mut HashMap<SocketAddr, PeerState>,
        active_peer: &mut Option<SocketAddr>,
        idle_timeout: Duration,
    ) {
        let now = time::Instant::now();
        let mut removed = 0usize;
        let mut removed_active_peer = false;
        peers.retain(|addr, state| {
            let stale = now.duration_since(state.last_seen) > idle_timeout;
            if stale {
                removed += 1;
                if Some(*addr) == *active_peer {
                    removed_active_peer = true;
                }
                warn!(
                    "dropping stale peer {} after {:?} of inactivity",
                    addr,
                    now.duration_since(state.last_seen)
                );
            }
            !stale
        });
        if removed_active_peer {
            *active_peer = None;
            info!("active peer expired; host is ready for new clients");
        }
        if removed > 0 {
            debug!("peer cleanup removed {} stale peer(s)", removed);
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

        let payload = match &mut peer_state.crypto {
            CryptoState::Disabled => plaintext,
            CryptoState::Established(server) => server
                .encrypt(packet_id, &plaintext)
                .map_err(|e| anyhow!("Encrypt failed: {}", e))?,
            _ => return Err(anyhow!("cannot send RIFT msg during handshake")),
        };

        let phys = PhysicalPacket {
            version: RIFT_VERSION,
            session_id: None,
            session_alias: Some(peer_state.session_alias),
            packet_id,
            payload: Bytes::copy_from_slice(&payload),
        };

        let bytes = phys.encode();
        peer_state.send_history.insert(packet_id, bytes.clone());
        socket.send_to(&bytes, peer).await?;
        Ok(())
    }

    async fn send_video_frame(
        socket: &UdpSocket,
        peer: SocketAddr,
        peer_state: &mut PeerState,
        frame: EncodedFrame,
    ) -> Result<()> {
        let chunks = chunk_video_payload(
            peer_state.frame_id,
            frame.timestamp_us,
            frame.keyframe,
            &frame.data,
            MAX_DATAGRAM_SIZE,
        )
        .map_err(|e| anyhow!("Chunking error: {}", e))?;
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
                .note_packet_bytes(packet_bytes, peer_state.target_bitrate_kbps);
            peer_state.pacer.wait().await;
            send_rift_msg(socket, peer_state, peer, msg).await?;
        }
        Ok(())
    }

    fn advertise_mdns(listen_addr: SocketAddr) -> Result<ServiceDaemon> {
        let mdns = ServiceDaemon::new()?;
        let service_info = ServiceInfo::new(
            "_wavry._udp.local.",
            "wavry-host",
            "wavry.local.",
            listen_addr.ip().to_string(),
            listen_addr.port(),
            &[("v", "1")][..],
        )?
        .enable_addr_auto();
        mdns.register(service_info)?;
        Ok(mdns)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn normalize_stream_resolution_clamps_bounds() {
            let fallback = MediaResolution {
                width: DEFAULT_RESOLUTION_WIDTH,
                height: DEFAULT_RESOLUTION_HEIGHT,
            };
            let out = normalize_stream_resolution(
                Some(ProtoResolution {
                    width: 10,
                    height: 90_000,
                }),
                fallback,
            );
            assert_eq!(out.width, MIN_STREAM_DIMENSION);
            assert_eq!(out.height, MAX_STREAM_DIMENSION);
        }
    }
}

fn main() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(host::run())
}
