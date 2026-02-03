#![forbid(unsafe_code)]

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("wavry-server is Linux-only in Step Two");
}

#[cfg(target_os = "linux")]
mod linux_host {
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
    use wavry_media::{Codec, EncodeConfig, PipewireEncoder, Resolution as MediaResolution};
    use wavry_platform::UinputInjector;
    use tokio::{net::UdpSocket, sync::mpsc};
    use tracing::{info, warn};

    const MAX_DATAGRAM_SIZE: usize = 1200;
    const FEC_SHARD_COUNT: u8 = 8;

    #[derive(Parser, Debug)]
    #[command(name = "wavry-server")]
    struct Args {
        #[arg(long, default_value = "0.0.0.0:5000")]
        listen: SocketAddr,
    }

    pub async fn run() -> Result<()> {
        tracing_subscriber::fmt().with_env_filter("info").init();

        let args = Args::parse();
        let socket = UdpSocket::bind(args.listen).await?;
        info!("listening on {}", args.listen);
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

        let mut injector = UinputInjector::new()?;
        let encoder = PipewireEncoder::new(encoder_config).await?;
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
                    let packet = match decode_packet(raw) {
                        Ok(packet) => packet,
                        Err(err) => {
                            warn!("invalid packet from {}: {}", peer, err);
                            continue;
                        }
                    };

                    if packet.version != RIFT_VERSION {
                        warn!("version mismatch from {}", peer);
                        continue;
                    }

                    if let Err(reason) = validate_channel_message(&packet) {
                        warn!("invalid packet from {}: {}", peer, reason);
                        continue;
                    }

                    match packet.channel {
                        Channel::Control => {
                            handle_control(&socket, &mut peers, &mut active_peer, peer, packet).await?;
                        }
                        Channel::Input => {
                            if !validate_established(&peers, peer, packet.session_id) {
                                warn!("input before established or session mismatch from {}", peer);
                                continue;
                            }
                            if let Message::Input(input) = packet.message {
                                apply_input(&mut injector, input)?;
                            }
                        }
                        Channel::Media => {
                            if !validate_established(&peers, peer, packet.session_id) {
                                warn!("media before established or session mismatch from {}", peer);
                                continue;
                            }
                            // Host does not consume media in Step Two.
                        }
                    }
                }
            }
        }
    }

    #[derive(Debug)]
    struct PeerState {
        handshake: Handshake,
        session_id: Option<u128>,
        next_packet_id: u64,
        frame_id: u64,
        fec_builder: FecBuilder,
    }

    impl PeerState {
        fn new() -> Self {
            Self {
                handshake: Handshake::new(Role::Host),
                session_id: None,
                next_packet_id: 1,
                frame_id: 0,
                fec_builder: FecBuilder::new(FEC_SHARD_COUNT).unwrap(),
            }
        }
    }

    async fn send_video_frame(
        socket: &UdpSocket,
        peer: SocketAddr,
        peer_state: &mut PeerState,
        frame: wavry_media::EncodedFrame,
    ) -> Result<()> {
        let max_payload = video_payload_capacity(peer_state)?;
        let chunks = chunk_video_payload(
            peer_state.frame_id,
            frame.timestamp_us,
            frame.keyframe,
            &frame.data,
            max_payload,
        )?;

        for chunk in chunks {
            let packet = Packet {
                version: RIFT_VERSION,
                session_id: peer_state.session_id.ok_or_else(|| anyhow!("missing session"))?,
                packet_id: next_packet_id(peer_state),
                channel: Channel::Media,
                message: Message::Video(chunk),
            };
            let encoded = encode_packet(&packet);
            socket.send_to(&encoded, peer).await?;
            if let Some(fec) = peer_state.fec_builder.push(packet.packet_id, &encoded) {
                let fec_packet = Packet {
                    version: RIFT_VERSION,
                    session_id: peer_state.session_id.unwrap(),
                    packet_id: next_packet_id(peer_state),
                    channel: Channel::Media,
                    message: Message::Fec(fec),
                };
                socket.send_to(&encode_packet(&fec_packet), peer).await?;
            }
        }

        peer_state.frame_id = peer_state.frame_id.wrapping_add(1);
        Ok(())
    }

    fn video_payload_capacity(peer_state: &PeerState) -> Result<usize> {
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
        let max_payload = MAX_DATAGRAM_SIZE.saturating_sub(overhead);
        if max_payload == 0 {
            Err(anyhow!("max payload too small for packet header"))
        } else {
            Ok(max_payload)
        }
    }

    async fn handle_control(
        socket: &UdpSocket,
        peers: &mut HashMap<SocketAddr, PeerState>,
        active_peer: &mut Option<SocketAddr>,
        peer: SocketAddr,
        packet: Packet,
    ) -> Result<()> {
        let peer_state = peers.entry(peer).or_insert_with(PeerState::new);
        match packet.message {
            Message::Control(ControlMessage::Hello(_hello)) => {
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
                    socket.send_to(&encode_packet(&response), peer).await?;
                    return Ok(());
                }

                info!("hello from {}", peer);
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
                if let Err(err) = peer_state.handshake.on_send_hello_ack(&ack) {
                    warn!("handshake error for {}: {}", peer, format_handshake_error(err));
                    return Ok(());
                }
                let response = Packet {
                    version: RIFT_VERSION,
                    session_id,
                    packet_id: next_packet_id(peer_state),
                    channel: Channel::Control,
                    message: Message::Control(ControlMessage::HelloAck(ack)),
                };
                socket.send_to(&encode_packet(&response), peer).await?;
                *active_peer = Some(peer);
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
                socket.send_to(&encode_packet(&pong), peer).await?;
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

    fn apply_input(injector: &mut UinputInjector, input: InputMessage) -> Result<()> {
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

    fn validate_established(
        peers: &HashMap<SocketAddr, PeerState>,
        peer: SocketAddr,
        session_id: u128,
    ) -> bool {
        if session_id == UNASSIGNED_SESSION_ID {
            return false;
        }
        match peers.get(&peer) {
            Some(state) => {
                matches!(state.handshake.state(), HandshakeState::Established { .. })
                    && state.session_id == Some(session_id)
            }
            None => false,
        }
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

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    linux_host::run().await
}
