mod host {
    use std::{collections::HashMap, net::SocketAddr, fmt};

    use anyhow::{anyhow, Result};
    use clap::Parser;
    use mdns_sd::{ServiceDaemon, ServiceInfo};
    use rift_core::{
        chunk_video_payload, decode_msg, encode_msg, PhysicalPacket, Codec as RiftCodec,
        ControlMessage as ProtoControl, FecBuilder, Handshake, HandshakeError, HandshakeState, HelloAck as ProtoHelloAck, 
        Message as ProtoMessage, Role, 
        UNASSIGNED_SESSION_ID as CORE_UNASSIGNED_SESSION_ID, RIFT_VERSION, Hello as ProtoHello,
        Resolution as ProtoResolution,
    };
    use rift_crypto::connection::{SecureServer};
    use wavry_media::{Codec, EncodeConfig, Resolution as MediaResolution, EncodedFrame};
    #[cfg(target_os = "linux")]
    use wavry_media::PipewireEncoder as VideoEncoder;
    #[cfg(not(target_os = "linux"))]
    use wavry_media::DummyEncoder as VideoEncoder;

    use wavry_platform::InputInjector;
    #[cfg(target_os = "linux")]
    use wavry_platform::UinputInjector as InjectorImpl;
    #[cfg(not(target_os = "linux"))]
    use wavry_platform::DummyInjector as InjectorImpl;
    use tokio::{net::UdpSocket, sync::mpsc};
    use tracing::{debug, info, warn};

    const MAX_DATAGRAM_SIZE: usize = 1200;
    const FEC_SHARD_COUNT: u32 = 8;
    const UNASSIGNED_SESSION_ID: [u8; 16] = [0u8; 16];

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

    #[derive(Debug)]
    struct PeerState {
        crypto: CryptoState,
        handshake: Handshake,
        session_id: Option<Vec<u8>>,
        session_alias: u32,
        next_packet_id: u64,
        frame_id: u64,
        fec_builder: FecBuilder,
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
                fec_builder: FecBuilder::new(FEC_SHARD_COUNT).unwrap(),
            }
        }
    }

    pub async fn run() -> Result<()> {
        let args = Args::parse();
        tracing_subscriber::fmt().with_env_filter("info").init();

        let socket = UdpSocket::bind(args.listen).await?;
        info!("listening on {}", args.listen);
        if args.no_encrypt {
            warn!("ENCRYPTION DISABLED - not for production use");
        }
        let _mdns = advertise_mdns(args.listen)?;

        let encoder_config = EncodeConfig {
            codec: Codec::Hevc,
            resolution: MediaResolution {
                width: 1280,
                height: 720,
            },
            fps: 60,
            bitrate_kbps: 20_000,
            keyframe_interval_ms: 1000,
        };

        let mut injector = InjectorImpl::new()?;
        let encoder = VideoEncoder::new(encoder_config).await?;
        let (frame_tx, mut frame_rx) = mpsc::channel(2);
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

        let mut buf = vec![0u8; 64 * 1024];
        let mut peers: HashMap<SocketAddr, PeerState> = HashMap::new();
        let mut active_peer: Option<SocketAddr> = None;
        let no_encrypt = args.no_encrypt;

        loop {
            tokio::select! {
                Some(frame) = frame_rx.recv() => {
                    if let Some(peer) = active_peer {
                        if let Some(peer_state) = peers.get_mut(&peer) {
                            if let Err(err) = send_video_frame(&socket, peer, peer_state, frame).await {
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
                    if let Err(e) = handle_raw_packet(&socket, peer_state, &mut active_peer, peer, raw, &mut injector).await {
                        debug!("packet from {} dropped: {}", peer, e);
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
    ) -> Result<()> {
        let phys = PhysicalPacket::decode(raw).map_err(|e| anyhow!("RIFT decode error: {}", e))?;

        // Dispatch based on crypto state
        match &mut peer_state.crypto {
            CryptoState::Disabled => {
                let msg = decode_msg(&phys.payload).map_err(|e| anyhow!("Proto decode error: {}", e))?;
                handle_rift_msg(socket, peer_state, active_peer, peer, msg, injector).await
            }
            CryptoState::Handshaking(server) => {
                if let Some(sid) = phys.session_id {
                    if sid == 0 {
                        info!("crypto handshake msg1 from {}", peer);
                        let msg2_payload = server.process_client_hello(&phys.payload)
                            .map_err(|e| anyhow!("Noise error: {}", e))?;
                        
                        let resp = PhysicalPacket {
                            version: RIFT_VERSION,
                            session_id: Some(0),
                            session_alias: None,
                            packet_id: 0,
                            payload: msg2_payload,
                        };
                        socket.send_to(&resp.encode(), peer).await?;
                        Ok(())
                    } else {
                        Err(anyhow!("unexpected session_id in crypto handshake"))
                    }
                } else if phys.session_alias.is_some() {
                    info!("crypto handshake msg3 from {}", peer);
                    server.process_client_finish(&phys.payload)
                        .map_err(|e| anyhow!("Noise error: {}", e))?;

                    // Transition to established
                    let old_crypto = std::mem::replace(&mut peer_state.crypto, CryptoState::Disabled);
                    if let CryptoState::Handshaking(server) = old_crypto {
                        peer_state.crypto = CryptoState::Established(server);
                        info!("crypto established with {}", peer);
                    }
                    Ok(())
                } else {
                    Err(anyhow!("unexpected packet format during crypto handshake"))
                }
            }
            CryptoState::Established(server) => {
                let plaintext = server.decrypt(phys.packet_id, &phys.payload)
                    .map_err(|e| anyhow!("Decrypt failed: {}", e))?;

                let msg = decode_msg(&plaintext).map_err(|e| anyhow!("Proto decode error: {}", e))?;
                handle_rift_msg(socket, peer_state, active_peer, peer, msg, injector).await
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
    ) -> Result<()> {
        use rift_core::message::Content;

        let content = msg.content.ok_or_else(|| anyhow!("empty message content"))?;
        match content {
            Content::Control(ctrl) => {
                let ctrl_content = ctrl.content.ok_or_else(|| anyhow!("empty control content"))?;
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
                            };
                            send_rift_msg(socket, peer_state, peer, ProtoMessage {
                                content: Some(Content::Control(ProtoControl {
                                    content: Some(rift_core::control_message::Content::HelloAck(ack)),
                                })),
                            }).await?;
                            return Ok(());
                        }

                        info!("RIFT hello from {}", hello.client_name);
                        peer_state.handshake.on_receive_hello(&hello).map_err(|e| anyhow!("Handshake error: {}", e))?;

                        let session_id = rand::random::<[u8; 16]>().to_vec();
                        peer_state.session_id = Some(session_id.clone());
                        peer_state.frame_id = 0;

                        let ack = ProtoHelloAck {
                            accepted: true,
                            selected_codec: RiftCodec::Hevc as i32,
                            stream_resolution: Some(ProtoResolution { width: 1280, height: 720 }),
                            fps: 60,
                            initial_bitrate_kbps: 20_000,
                            keyframe_interval_ms: 1000,
                            session_id: session_id.clone(),
                            session_alias: peer_state.session_alias,
                        };

                        peer_state.handshake.on_send_hello_ack(&ack).map_err(|e| anyhow!("Handshake error: {}", e))?;
                        *active_peer = Some(peer);

                        send_rift_msg(socket, peer_state, peer, ProtoMessage {
                            content: Some(Content::Control(ProtoControl {
                                content: Some(rift_core::control_message::Content::HelloAck(ack)),
                            })),
                        }).await?;
                        info!("session established with {}: {}", peer, hex::encode(&session_id));
                    }
                    rift_core::control_message::Content::Ping(ping) => {
                        let pong = rift_core::Pong { timestamp_us: ping.timestamp_us };
                        send_rift_msg(socket, peer_state, peer, ProtoMessage {
                            content: Some(Content::Control(ProtoControl {
                                content: Some(rift_core::control_message::Content::Pong(pong)),
                            })),
                        }).await?;
                    }
                    rift_core::control_message::Content::Stats(report) => {
                        info!("stats from {}: rtt={}ms", peer, report.rtt_us / 1000);
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
        Ok(())
    }

    fn handle_input_event(injector: &mut InjectorImpl, event: rift_core::input_message::Event) -> Result<()> {
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
            CryptoState::Established(server) => server.encrypt(packet_id, &plaintext)
                .map_err(|e| anyhow!("Encrypt failed: {}", e))?,
            _ => return Err(anyhow!("cannot send RIFT msg during handshake")),
        };

        let phys = PhysicalPacket {
            version: RIFT_VERSION,
            session_id: None,
            session_alias: Some(peer_state.session_alias),
            packet_id,
            payload,
        };

        socket.send_to(&phys.encode(), peer).await?;
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
        ).map_err(|e| anyhow!("Chunking error: {}", e))?;
        peer_state.frame_id = peer_state.frame_id.wrapping_add(1);

        for chunk in chunks {
            let msg = ProtoMessage {
                content: Some(rift_core::message::Content::Media(rift_core::MediaMessage {
                    content: Some(rift_core::media_message::Content::Video(chunk)),
                })),
            };
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
        )?.enable_addr_auto();
        mdns.register(service_info)?;
        Ok(mdns)
    }

    fn format_handshake_error(err: HandshakeError) -> String {
        err.to_string()
    }
}

fn main() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(host::run())
}
