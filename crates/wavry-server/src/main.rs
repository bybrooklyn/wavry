mod host {
    use std::{collections::HashMap, net::SocketAddr, time::SystemTime};

    use anyhow::{anyhow, Result};
    use clap::Parser;
    use mdns_sd::{ServiceDaemon, ServiceInfo};
    use rift_core::{
        chunk_video_payload, decode_packet, encode_packet, Channel, Codec as RiftCodec,
        ControlMessage, FecBuilder, Handshake, HandshakeError, HandshakeState, HelloAck, InputEvent,
        InputMessage, Message, Packet, Ping, Pong, Role, StatsReport, UNASSIGNED_SESSION_ID,
        RIFT_VERSION,
    };
    use rift_crypto::connection::{handshake_type, SecureServer, ConnectionError};
    use wavry_media::{Codec, EncodeConfig, Resolution as MediaResolution};
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
    const FEC_SHARD_COUNT: u8 = 8;

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

    pub async fn run() -> Result<()> {
        tracing_subscriber::fmt().with_env_filter("info").init();

        let args = Args::parse();
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

                    // Handle based on crypto state
                    match handle_raw_packet(&socket, peer_state, &mut active_peer, peer, raw, &mut injector).await {
                        Ok(()) => {},
                        Err(e) => {
                            debug!("packet from {} dropped: {}", peer, e);
                        }
                    }
                }
            }
        }
    }

    #[derive(Debug)]
    struct PeerState {
        crypto: CryptoState,
        handshake: Handshake,
        session_id: Option<u128>,
        next_packet_id: u64,
        frame_id: u64,
        fec_builder: FecBuilder,
    }

    // Manual Debug impl since CryptoState doesn't derive Debug
    impl std::fmt::Debug for CryptoState {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                CryptoState::Disabled => write!(f, "Disabled"),
                CryptoState::Handshaking(_) => write!(f, "Handshaking"),
                CryptoState::Established(_) => write!(f, "Established"),
            }
        }
    }

    impl PeerState {
        fn new(no_encrypt: bool) -> Self {
            Self {
                crypto: CryptoState::new(no_encrypt),
                handshake: Handshake::new(Role::Host),
                session_id: None,
                next_packet_id: 1,
                frame_id: 0,
                fec_builder: FecBuilder::new(FEC_SHARD_COUNT).unwrap(),
            }
        }
    }

    /// Handle a raw packet, dispatching to crypto or RIFT handling
    async fn handle_raw_packet(
        socket: &UdpSocket,
        peer_state: &mut PeerState,
        active_peer: &mut Option<SocketAddr>,
        peer: SocketAddr,
        raw: &[u8],
        injector: &mut InjectorImpl,
    ) -> Result<()> {
        if raw.is_empty() {
            return Err(anyhow!("empty packet"));
        }

        // Dispatch based on crypto state and packet type
        match &mut peer_state.crypto {
            CryptoState::Disabled => {
                // No encryption, decode RIFT directly
                let packet = decode_packet(raw)?;
                handle_rift_packet(socket, peer_state, active_peer, peer, packet, injector).await
            }
            CryptoState::Handshaking(server) => {
                // Handle crypto handshake
                match raw[0] {
                    handshake_type::MSG1 => {
                        info!("crypto handshake msg1 from {}", peer);
                        let response = server.process_client_hello(raw)?;
                        socket.send_to(&response, peer).await?;
                        Ok(())
                    }
                    handshake_type::MSG3 => {
                        info!("crypto handshake msg3 from {}", peer);
                        server.process_client_finish(raw)?;
                        
                        // Transition to established
                        // We need to take ownership, so swap with a placeholder
                        let old_crypto = std::mem::replace(
                            &mut peer_state.crypto,
                            CryptoState::Disabled
                        );
                        if let CryptoState::Handshaking(server) = old_crypto {
                            peer_state.crypto = CryptoState::Established(server);
                            info!("crypto established with {}", peer);
                        }
                        Ok(())
                    }
                    _ => {
                        warn!("unexpected packet type 0x{:02x} during handshake from {}", raw[0], peer);
                        Err(anyhow!("unexpected packet type during crypto handshake"))
                    }
                }
            }
            CryptoState::Established(server) => {
                // Decrypt and handle RIFT
                if raw[0] != handshake_type::DATA && raw.len() > 8 {
                    // Data packet: first 8 bytes are packet_id, rest is ciphertext
                    let packet_id = u64::from_le_bytes(raw[0..8].try_into()?);
                    let ciphertext = &raw[8..];
                    
                    let plaintext = server.decrypt(packet_id, ciphertext)
                        .map_err(|e| anyhow!("decrypt failed: {}", e))?;
                    
                    let packet = decode_packet(&plaintext)?;
                    handle_rift_packet(socket, peer_state, active_peer, peer, packet, injector).await
                } else if raw[0] == handshake_type::DATA {
                    // Explicit DATA prefix
                    if raw.len() < 9 {
                        return Err(anyhow!("data packet too short"));
                    }
                    let packet_id = u64::from_le_bytes(raw[1..9].try_into()?);
                    let ciphertext = &raw[9..];
                    
                    let plaintext = server.decrypt(packet_id, ciphertext)
                        .map_err(|e| anyhow!("decrypt failed: {}", e))?;
                    
                    let packet = decode_packet(&plaintext)?;
                    handle_rift_packet(socket, peer_state, active_peer, peer, packet, injector).await
                } else {
                    // Could be raw RIFT packet for backwards compat?
                    warn!("unexpected format from established peer {}", peer);
                    Err(anyhow!("unexpected packet format"))
                }
            }
        }
    }

    /// Handle a decoded RIFT packet
    async fn handle_rift_packet(
        socket: &UdpSocket,
        peer_state: &mut PeerState,
        active_peer: &mut Option<SocketAddr>,
        peer: SocketAddr,
        packet: Packet,
        injector: &mut InjectorImpl,
    ) -> Result<()> {
        if packet.version != RIFT_VERSION {
            return Err(anyhow!("version mismatch"));
        }

        if let Err(reason) = validate_channel_message(&packet) {
            return Err(anyhow!("invalid packet: {}", reason));
        }

        match packet.channel {
            Channel::Control => {
                handle_control(socket, peer_state, active_peer, peer, packet).await
            }
            Channel::Input => {
                if !validate_established_rift(peer_state, packet.session_id) {
                    return Err(anyhow!("input before established"));
                }
                if let Message::Input(input) = packet.message {
                    apply_input(injector, input)?;
                }
                Ok(())
            }
            Channel::Media => {
                // Host does not consume media
                Ok(())
            }
        }
    }

    /// Send a RIFT packet, encrypting if crypto is established
    async fn send_packet(
        socket: &UdpSocket,
        peer_state: &mut PeerState,
        peer: SocketAddr,
        packet: &Packet,
    ) -> Result<()> {
        let encoded = encode_packet(packet);
        
        match &mut peer_state.crypto {
            CryptoState::Disabled => {
                socket.send_to(&encoded, peer).await?;
            }
            CryptoState::Established(server) => {
                let (packet_id, ciphertext) = server.encrypt(&encoded)
                    .map_err(|e| anyhow!("encrypt failed: {}", e))?;
                
                // Format: [8 bytes packet_id] [ciphertext]
                let mut buf = Vec::with_capacity(8 + ciphertext.len());
                buf.extend_from_slice(&packet_id.to_le_bytes());
                buf.extend_from_slice(&ciphertext);
                
                socket.send_to(&buf, peer).await?;
            }
            CryptoState::Handshaking(_) => {
                // Can't send RIFT packets during handshake
                return Err(anyhow!("cannot send during crypto handshake"));
            }
        }
        
        Ok(())
    }

    async fn send_video_frame(
        socket: &UdpSocket,
        peer: SocketAddr,
        peer_state: &mut PeerState,
        frame: wavry_media::EncodedFrame,
    ) -> Result<()> {
        let max_payload = calculate_max_payload(peer_state)?;
        let chunks = chunk_video_payload(
            peer_state.frame_id,
            frame.timestamp_us,
            frame.keyframe,
            &frame.data,
            max_payload
        )?;
        
        peer_state.frame_id = peer_state.frame_id.wrapping_add(1);

        let chunk_count = chunks.len();
        for (i, chunk) in chunks.into_iter().enumerate() {
            let packet = Packet {
                version: RIFT_VERSION,
                session_id: peer_state.session_id.unwrap(),
                packet_id: next_packet_id(peer_state),
                channel: Channel::Media,
                message: Message::Video(chunk),
            };
            
            if let Err(e) = send_packet(socket, peer_state, peer, &packet).await {
                 debug!("failed to send video chunk {}/{}: {}", i + 1, chunk_count, e);
            }
        }
        Ok(())
    }

    fn calculate_max_payload(peer_state: &PeerState) -> Result<usize> {
        let dummy = Packet {
            version: RIFT_VERSION,
            session_id: peer_state
                .session_id
                .ok_or_else(|| anyhow!("missing session"))?,
            packet_id: 0,
            channel: Channel::Media,
            message: Message::Video(rift_core::VideoChunk {
                frame_id: 0,
                chunk_index: 0,
                chunk_count: 1,
                timestamp_us: 0,
                keyframe: false,
                payload: Vec::new(),
            }),
        };
        let overhead = encode_packet(&dummy).len();
        
        // Account for crypto overhead (8 byte packet_id + 16 byte tag)
        let crypto_overhead = match &peer_state.crypto {
            CryptoState::Established(_) => 24,
            _ => 0,
        };
        
        let max_payload = MAX_DATAGRAM_SIZE.saturating_sub(overhead + crypto_overhead);
        if max_payload == 0 {
            Err(anyhow!("max payload too small for packet header"))
        } else {
            Ok(max_payload)
        }
    }

    async fn handle_control(
        socket: &UdpSocket,
        peer_state: &mut PeerState,
        active_peer: &mut Option<SocketAddr>,
        peer: SocketAddr,
        packet: Packet,
    ) -> Result<()> {
        match packet.message {
            Message::Control(ControlMessage::Hello(_hello)) => {
                // Reject if crypto required but not established
                if !peer_state.crypto.is_established() {
                    warn!("RIFT hello before crypto established from {}", peer);
                    return Err(anyhow!("crypto handshake required first"));
                }

                if active_peer.is_some() && *active_peer != Some(peer) {
                    let ack = HelloAck {
                        accepted: false,
                        selected_codec: RiftCodec::Hevc,
                        stream_resolution: rift_core::Resolution { width: 0, height: 0 },
                        fps: 0,
                        initial_bitrate_kbps: 0,
                        keyframe_interval_ms: 0,
                        session_id: UNASSIGNED_SESSION_ID,
                    };
                    let response = Packet {
                        version: RIFT_VERSION,
                        session_id: UNASSIGNED_SESSION_ID,
                        packet_id: next_packet_id(peer_state),
                        channel: Channel::Control,
                        message: Message::Control(ControlMessage::HelloAck(ack)),
                    };
                    send_packet(socket, peer_state, peer, &response).await?;
                    return Ok(());
                }

                info!("RIFT hello from {}", peer);
                if packet.session_id != UNASSIGNED_SESSION_ID {
                    warn!(
                        "hello from {} had non-zero session_id: {}",
                        peer, packet.session_id
                    );
                }
                if let Err(err) = peer_state.handshake.on_receive_hello(&_hello) {
                    warn!("invalid hello from {}: {}", peer, format_handshake_error(err));
                    return Ok(());
                }
                let session_id = generate_session_id(peer);
                peer_state.session_id = Some(session_id);
                peer_state.frame_id = 0;
                peer_state.fec_builder = FecBuilder::new(FEC_SHARD_COUNT).unwrap();

                let ack = HelloAck {
                    accepted: true,
                    selected_codec: RiftCodec::Hevc,
                    stream_resolution: rift_core::Resolution {
                        width: 1280,
                        height: 720,
                    },
                    fps: 60,
                    initial_bitrate_kbps: 20_000,
                    keyframe_interval_ms: 1000,
                    session_id,
                };
                let response = Packet {
                    version: RIFT_VERSION,
                    session_id,
                    packet_id: next_packet_id(peer_state),
                    channel: Channel::Control,
                    message: Message::Control(ControlMessage::HelloAck(ack)),
                };
                send_packet(socket, peer_state, peer, &response).await?;
                *active_peer = Some(peer);
                info!("session established with {}: {}", peer, session_id);
            }
            Message::Control(ControlMessage::Ping(Ping { timestamp_us })) => {
                let session_id = packet.session_id;
                if session_id == UNASSIGNED_SESSION_ID {
                    warn!("ping without session_id from {}", peer);
                } else if let Some(expected) = peer_state.session_id {
                    if expected != session_id {
                        warn!(
                            "ping session_id mismatch from {}: expected {}, got {}",
                            peer, expected, session_id
                        );
                        return Ok(());
                    }
                } else {
                    warn!("ping before session established from {}", peer);
                    return Ok(());
                }
                let pong = Packet {
                    version: RIFT_VERSION,
                    session_id,
                    packet_id: next_packet_id(peer_state),
                    channel: Channel::Control,
                    message: Message::Control(ControlMessage::Pong(Pong { timestamp_us })),
                };
                send_packet(socket, peer_state, peer, &pong).await?;
            }
            Message::Control(ControlMessage::Stats(report)) => {
                log_stats(peer, report);
            }
            other => {
                info!("control message from {}: {:?}", peer, other);
            }
        }
        Ok(())
    }

    fn log_stats(peer: SocketAddr, report: StatsReport) {
        info!(
            "stats from {}: rtt_us={} received={} lost={} period_ms={}",
            peer, report.rtt_us, report.received_packets, report.lost_packets, report.period_ms
        );
    }

    fn apply_input(injector: &mut InjectorImpl, input: InputMessage) -> Result<()> {
        match input.event {
            InputEvent::Key { keycode, pressed } => injector.key(keycode, pressed),
            InputEvent::MouseButton { button, pressed } => injector.mouse_button(button, pressed),
            InputEvent::MouseMotion { dx, dy } => injector.mouse_motion(dx, dy),
            InputEvent::MouseAbsolute { x, y } => injector.mouse_absolute(x, y),
        }
    }

    fn validate_channel_message(packet: &Packet) -> Result<(), &'static str> {
        match (&packet.channel, &packet.message) {
            (Channel::Control, Message::Control(_)) => Ok(()),
            (Channel::Input, Message::Input(_)) => Ok(()),
            (Channel::Media, Message::Video(_)) => Ok(()),
            (Channel::Media, Message::Fec(_)) => Ok(()),
            _ => Err("channel/message mismatch"),
        }
    }

    fn validate_established_rift(
        peer_state: &PeerState,
        session_id: u128,
    ) -> bool {
        if session_id == UNASSIGNED_SESSION_ID {
            return false;
        }
        matches!(peer_state.handshake.state(), HandshakeState::Established { .. })
            && peer_state.session_id == Some(session_id)
    }

    fn format_handshake_error(err: HandshakeError) -> String {
        match err {
            HandshakeError::InvalidRole => "invalid role".to_string(),
            HandshakeError::InvalidTransition(state, event) => {
                format!("invalid transition from {state:?} via {event:?}")
            }
            HandshakeError::DuplicateHello => "duplicate hello".to_string(),
            HandshakeError::InvalidSessionId => "invalid session id".to_string(),
        }
    }

    fn next_packet_id(peer_state: &mut PeerState) -> u64 {
        let id = peer_state.next_packet_id;
        peer_state.next_packet_id = peer_state.next_packet_id.wrapping_add(1);
        id
    }

    fn advertise_mdns(listen: SocketAddr) -> Result<ServiceDaemon> {
        let daemon = ServiceDaemon::new()?;
        let service_type = "_wavry._udp.local.";
        let instance_name = "wavry-server";
        let host_name = "wavry-server.local.";
        let properties = [("rift_version", "0.0.1"), ("codec", "hevc")];
        let service_info = ServiceInfo::new(
            service_type,
            instance_name,
            host_name,
            "",
            listen.port(),
            &properties[..],
        )?
        .enable_addr_auto();
        daemon.register(service_info)?;
        Ok(daemon)
    }

    fn generate_session_id(peer: SocketAddr) -> u128 {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u128)
            .unwrap_or(1);
        let addr_hash = peer.ip().to_string().bytes().fold(0u128, |acc, b| {
            acc.wrapping_mul(257).wrapping_add(b as u128)
        });
        (now << 32) ^ (addr_hash << 1) ^ (peer.port() as u128)
    }
}



#[tokio::main]
async fn main() -> anyhow::Result<()> {
    host::run().await
}
