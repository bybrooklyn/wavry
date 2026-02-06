mod host {
    use std::{
        collections::{HashMap, VecDeque},
        fmt,
        net::SocketAddr,
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
    use wavry_media::PipewireEncoder as VideoEncoder;
    use wavry_media::{Codec, EncodeConfig, EncodedFrame, Resolution as MediaResolution};

    use bytes::Bytes;
    use socket2::SockRef;
    use tokio::{net::UdpSocket, sync::mpsc, time};
    use tracing::{debug, info, warn};
    #[cfg(not(target_os = "linux"))]
    use wavry_platform::DummyInjector as InjectorImpl;
    use wavry_platform::InputInjector;
    #[cfg(target_os = "linux")]
    use wavry_platform::UinputInjector as InjectorImpl;

    const MAX_DATAGRAM_SIZE: usize = 1200;
    const FEC_SHARD_COUNT: u32 = 8;
    const UNASSIGNED_SESSION_ID: [u8; 16] = [0u8; 16];
    const DSCP_EF: u32 = 0x2E;
    const PACER_MIN_US: u64 = 20;
    const PACER_MAX_US: u64 = 500;
    const PACER_BASE_US: f64 = 30.0;
    const NACK_HISTORY: usize = 512;

    #[derive(Parser, Debug)]
    #[command(name = "wavry-server")]
    struct Args {
        #[arg(long, default_value = "0.0.0.0:5000")]
        listen: SocketAddr,

        /// Disable encryption (for testing/debugging)
        #[arg(long, default_value = "false")]
        no_encrypt: bool,

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
    }

    async fn ensure_encoder(
        frame_rx: &mut Option<mpsc::Receiver<FrameIn>>,
        selected_codec: &mut Option<Codec>,
        base: EncodeConfig,
        codec: Codec,
    ) -> Result<()> {
        if selected_codec == &Some(codec) && frame_rx.is_some() {
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
        info!("Selected encoder codec: {:?}", codec);
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

    impl PeerState {
        fn new(no_encrypt: bool) -> Self {
            Self {
                crypto: CryptoState::new(no_encrypt),
                handshake: Handshake::new(Role::Host),
                session_id: None,
                session_alias: rand::random::<u32>().max(1),
                next_packet_id: 1,
                frame_id: 0,
                pacer: Pacer::new(),
                send_history: SendHistory::new(NACK_HISTORY),
                target_bitrate_kbps: 20_000,
                skip_frames: 0,
                fec_builder: FecBuilder::new(FEC_SHARD_COUNT).unwrap(),
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

        let socket = UdpSocket::bind(args.listen).await?;
        if let Err(e) = SockRef::from(&socket).set_tos_v4(DSCP_EF) {
            debug!("failed to set DSCP/TOS: {}", e);
        }
        info!("listening on {}", args.listen);
        if args.no_encrypt {
            warn!("ENCRYPTION DISABLED - not for production use");
        }
        let _mdns = advertise_mdns(args.listen)?;

        let mut injector = InjectorImpl::new()?;

        let base_config = EncodeConfig {
            codec: Codec::H264,
            resolution: MediaResolution {
                width: 1280,
                height: 720,
            },
            fps: 60,
            bitrate_kbps: 20_000,
            keyframe_interval_ms: 1000,
            display_id: None,
        };

        let mut buf = vec![0u8; 64 * 1024];
        let mut peers: HashMap<SocketAddr, PeerState> = HashMap::new();
        let mut active_peer: Option<SocketAddr> = None;
        let mut frame_rx: Option<mpsc::Receiver<FrameIn>> = None;
        let mut selected_codec: Option<Codec> = None;
        let local_supported = {
            #[cfg(target_os = "linux")]
            {
                wavry_media::LinuxProbe
                    .supported_encoders()
                    .unwrap_or_else(|_| vec![Codec::H264])
            }
            #[cfg(not(target_os = "linux"))]
            {
                vec![Codec::H264]
            }
        };
        let no_encrypt = args.no_encrypt;

        loop {
            tokio::select! {
                Some(frame) = async {
                    if let Some(rx) = frame_rx.as_mut() {
                        rx.recv().await
                    } else {
                        None
                    }
                } => {
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

                    // Get or create peer state
                    let peer_state = peers.entry(peer).or_insert_with(|| PeerState::new(no_encrypt));

                    // Handle raw packet
                    match handle_raw_packet(
                        &socket,
                        peer_state,
                        &mut active_peer,
                        peer,
                        raw,
                        &mut injector,
                        &local_supported,
                    )
                    .await
                    {
                        Ok(Some(codec)) => {
                            if let Err(err) =
                                ensure_encoder(&mut frame_rx, &mut selected_codec, base_config, codec).await
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

    async fn handle_raw_packet(
        socket: &UdpSocket,
        peer_state: &mut PeerState,
        active_peer: &mut Option<SocketAddr>,
        peer: SocketAddr,
        raw: &[u8],
        injector: &mut InjectorImpl,
        local_supported: &[Codec],
    ) -> Result<Option<Codec>> {
        let phys = PhysicalPacket::decode(Bytes::copy_from_slice(raw))
            .map_err(|e| anyhow!("RIFT decode error: {}", e))?;

        // Dispatch based on crypto state
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
                    local_supported,
                )
                .await
            }
            CryptoState::Handshaking(server) => {
                if let Some(sid) = phys.session_id {
                    if sid == 0 {
                        info!("crypto handshake msg1 from {}", peer);
                        let msg2_payload = server
                            .process_client_hello(&phys.payload)
                            .map_err(|e| anyhow!("Noise error: {}", e))?;

                        let resp = PhysicalPacket {
                            version: RIFT_VERSION,
                            session_id: Some(0),
                            session_alias: None,
                            packet_id: 0,
                            payload: Bytes::copy_from_slice(&msg2_payload),
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

                    // Transition to established
                    let old_crypto =
                        std::mem::replace(&mut peer_state.crypto, CryptoState::Disabled);
                    if let CryptoState::Handshaking(server) = old_crypto {
                        peer_state.crypto = CryptoState::Established(server);
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
                handle_rift_msg(socket, peer_state, active_peer, peer, msg, injector, local_supported)
                .await
            }
        }
    }

    async fn handle_rift_msg(
        socket: &UdpSocket,
        peer_state: &mut PeerState,
        active_peer: &mut Option<SocketAddr>,
        peer: SocketAddr,
        msg: ProtoMessage,
        injector: &mut InjectorImpl,
        local_supported: &[Codec],
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
                            // Already busy
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

                        info!("RIFT hello from {}", hello.client_name);
                        peer_state
                            .handshake
                            .on_receive_hello(&hello)
                            .map_err(|e| anyhow!("Handshake error: {}", e))?;

                        let session_id = rand::random::<[u8; 16]>().to_vec();
                        peer_state.session_id = Some(session_id.clone());
                        peer_state.frame_id = 0;

                        let desired_codec = choose_codec_for_hello(&hello, local_supported);
                        let ack = ProtoHelloAck {
                            accepted: true,
                            selected_codec: match desired_codec {
                                Codec::Av1 => RiftCodec::Av1 as i32,
                                Codec::Hevc => RiftCodec::Hevc as i32,
                                Codec::H264 => RiftCodec::H264 as i32,
                            },
                            stream_resolution: Some(hello.max_resolution.unwrap_or(
                                ProtoResolution {
                                    width: 1280,
                                    height: 720,
                                },
                            )),
                            fps: 60,
                            initial_bitrate_kbps: 20_000,
                            keyframe_interval_ms: 1000,
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
                        info!(
                            "session established with {}: {}",
                            peer,
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
                        info!("stats from {}: rtt={}ms", peer, report.rtt_us / 1000);
                        peer_state.pacer.on_stats(
                            report.rtt_us,
                            report.jitter_us,
                            peer_state.target_bitrate_kbps,
                        );
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
                    rift_core::control_message::Content::VrTiming(_timing) => {
                        // Reserved for VR timing hints.
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
            Event::MouseMove(m) => injector.mouse_motion(m.x as i32, m.y as i32)?,
            _ => {} // Handle scroll/absolute if needed
        }
        Ok(())
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
}

fn main() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(host::run())
}
