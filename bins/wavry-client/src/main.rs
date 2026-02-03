#![forbid(unsafe_code)]

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("wavry-client is Linux-only in Step Two");
}

#[cfg(target_os = "linux")]
mod linux_client {
    use std::{
        collections::HashMap,
        net::SocketAddr,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, Mutex,
        },
        thread,
        time::Duration,
    };

    use anyhow::{anyhow, Result};
    use clap::Parser;
    use evdev::{AbsoluteAxisType, Device, EventType, RelativeAxisType};
    use mdns_sd::{ServiceDaemon, ServiceEvent};
    use rift_core::{
        decode_packet, encode_packet, Channel, Codec as RiftCodec, ControlMessage, FecPacket,
        Handshake, HandshakeError, InputEvent as RiftInputEvent, InputMessage, Message, Packet,
        Ping, StatsReport, UNASSIGNED_SESSION_ID, RIFT_VERSION,
    };
    use wavry_media::{Codec, DecodeConfig, GstVideoRenderer, Resolution as MediaResolution};
    use tokio::{net::UdpSocket, time};
    use tracing::{info, warn};

    const FRAME_TIMEOUT_US: u64 = 50_000;
    const MAX_FEC_CACHE: usize = 256;

    #[derive(Parser, Debug)]
    #[command(name = "wavry-client")]
    struct Args {
        #[arg(long)]
        connect: Option<SocketAddr>,
        #[arg(long, default_value = "wavry-client")]
        name: String,
    }

    pub async fn run() -> Result<()> {
        tracing_subscriber::fmt().with_env_filter("info").init();

        let args = Args::parse();
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        let connect_addr = match args.connect {
            Some(addr) => addr,
            None => discover_host(Duration::from_secs(3)).await?,
        };

        let mut handshake = Handshake::new(rift_core::Role::Client);
        let hello = rift_core::Hello {
            client_name: args.name,
            platform: rift_core::Platform::Linux,
            supported_codecs: vec![RiftCodec::Hevc, RiftCodec::H264],
            max_resolution: rift_core::Resolution {
                width: 1920,
                height: 1080,
            },
            max_fps: 60,
            input_caps: rift_core::InputCaps::KEYBOARD
                | rift_core::InputCaps::MOUSE_BUTTONS
                | rift_core::InputCaps::MOUSE_RELATIVE
                | rift_core::InputCaps::MOUSE_ABSOLUTE,
        };

        handshake.on_send_hello().map_err(|e| anyhow!(format_handshake_error(e)))?;

        let mut next_packet_id = 1u64;
        let packet = Packet {
            version: RIFT_VERSION,
            session_id: UNASSIGNED_SESSION_ID,
            packet_id: next_packet_id,
            channel: Channel::Control,
            message: Message::Control(ControlMessage::Hello(hello)),
        };
        next_packet_id = next_packet_id.wrapping_add(1);

        socket.send_to(&encode_packet(&packet), connect_addr).await?;
        info!("sent hello to {}", connect_addr);

        let mut buf = vec![0u8; 64 * 1024];
        let mut interval = time::interval(Duration::from_millis(500));
        let mut stats_interval = time::interval(Duration::from_millis(1000));
        let mut session_id = UNASSIGNED_SESSION_ID;
        let session_shared = Arc::new(Mutex::new(UNASSIGNED_SESSION_ID));
        let packet_counter = Arc::new(AtomicU64::new(1000));
        let input_socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
        input_socket.connect(connect_addr)?;
        spawn_input_threads(input_socket, session_shared.clone(), packet_counter.clone())?;

        let mut last_packet_id: Option<u64> = None;
        let mut received_packets: u32 = 0;
        let mut lost_packets: u32 = 0;
        let mut last_rtt_us: u64 = 0;

        let mut renderer: Option<GstVideoRenderer> = None;
        let mut frames = FrameAssembler::new(FRAME_TIMEOUT_US);
        let mut fec_cache = FecCache::new();

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if session_id == UNASSIGNED_SESSION_ID {
                        continue;
                    }
                    let ping = Packet {
                        version: RIFT_VERSION,
                        session_id,
                        packet_id: next_packet_id,
                        channel: Channel::Control,
                        message: Message::Control(ControlMessage::Ping(Ping { timestamp_us: now_us() })),
                    };
                    next_packet_id = next_packet_id.wrapping_add(1);
                    socket.send_to(&encode_packet(&ping), connect_addr).await?;
                }
                _ = stats_interval.tick() => {
                    if session_id == UNASSIGNED_SESSION_ID {
                        continue;
                    }
                    let stats = StatsReport {
                        period_ms: 1000,
                        received_packets,
                        lost_packets,
                        rtt_us: last_rtt_us,
                    };
                    let stats_packet = Packet {
                        version: RIFT_VERSION,
                        session_id,
                        packet_id: next_packet_id,
                        channel: Channel::Control,
                        message: Message::Control(ControlMessage::Stats(stats)),
                    };
                    next_packet_id = next_packet_id.wrapping_add(1);
                    received_packets = 0;
                    lost_packets = 0;
                    socket.send_to(&encode_packet(&stats_packet), connect_addr).await?;
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

                    if let Some(last_id) = last_packet_id {
                        if packet.packet_id > last_id + 1 {
                            lost_packets = lost_packets.saturating_add((packet.packet_id - last_id - 1) as u32);
                        }
                    }
                    last_packet_id = Some(packet.packet_id);
                    received_packets = received_packets.saturating_add(1);

                    if let Err(reason) = validate_channel_message(&packet) {
                        warn!("invalid packet from {}: {}", peer, reason);
                        continue;
                    }

                    if let Message::Control(ControlMessage::HelloAck(ack)) = &packet.message {
                        if !ack.accepted {
                            warn!("session rejected by {}", peer);
                            continue;
                        }
                        if packet.session_id != ack.session_id || ack.session_id == UNASSIGNED_SESSION_ID {
                            warn!(
                                "invalid hello_ack session_id from {}: packet {}, ack {}",
                                peer, packet.session_id, ack.session_id
                            );
                            continue;
                        }
                        if let Err(err) = handshake.on_receive_hello_ack(ack) {
                            warn!("handshake error from {}: {}", peer, format_handshake_error(err));
                            continue;
                        }
                        session_id = ack.session_id;
                        *session_shared.lock().unwrap() = session_id;
                        renderer = Some(GstVideoRenderer::new(DecodeConfig {
                            codec: match ack.selected_codec {
                                RiftCodec::Hevc => Codec::Hevc,
                                RiftCodec::H264 => Codec::H264,
                            },
                            resolution: MediaResolution {
                                width: ack.stream_resolution.width,
                                height: ack.stream_resolution.height,
                            },
                        })?);
                        info!("session established with {}", peer);
                        continue;
                    }

                    if let Message::Control(ControlMessage::Pong(pong)) = &packet.message {
                        last_rtt_us = now_us().saturating_sub(pong.timestamp_us);
                        continue;
                    }

                    if session_id != UNASSIGNED_SESSION_ID && packet.session_id != session_id {
                        warn!(
                            "session_id mismatch from {}: expected {}, got {}",
                            peer, session_id, packet.session_id
                        );
                        continue;
                    }

                    match packet.message {
                        Message::Video(chunk) => {
                            fec_cache.insert(packet.packet_id, raw.to_vec());
                            if let Some(frame) = frames.push(chunk) {
                                if let Some(renderer) = renderer.as_ref() {
                                    renderer.push(&frame.data, frame.timestamp_us)?;
                                }
                            }
                        }
                        Message::Fec(fec) => {
                            if let Some(recovered) = fec_cache.try_recover(&fec) {
                                if let Ok(recovered_packet) = decode_packet(&recovered) {
                                    if let Message::Video(chunk) = recovered_packet.message {
                                        if let Some(frame) = frames.push(chunk) {
                                            if let Some(renderer) = renderer.as_ref() {
                                                renderer.push(&frame.data, frame.timestamp_us)?;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {
                            info!("packet from {}: {:?}", peer, packet.message);
                        }
                    }
                }
            }
        }
    }

    struct FrameAssembler {
        timeout_us: u64,
        frames: HashMap<u64, FrameBuffer>,
    }

    struct FrameBuffer {
        first_seen_us: u64,
        timestamp_us: u64,
        keyframe: bool,
        chunk_count: u16,
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

        fn insert(&mut self, packet_id: u64, bytes: Vec<u8>) {
            self.packets.insert(packet_id, bytes);
            if self.packets.len() > MAX_FEC_CACHE {
                if let Some(oldest) = self.packets.keys().cloned().min() {
                    self.packets.remove(&oldest);
                }
            }
        }

        fn try_recover(&self, fec: &FecPacket) -> Option<Vec<u8>> {
            let mut shards: Vec<Option<Vec<u8>>> = Vec::with_capacity(fec.shard_count as usize);
            for i in 0..fec.shard_count {
                let packet_id = fec.first_packet_id + i as u64;
                shards.push(self.packets.get(&packet_id).cloned());
            }
            match rift_core::recover_missing_shard(fec, &mut shards) {
                Ok(bytes) => Some(bytes),
                Err(_) => None,
            }
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

    async fn discover_host(timeout: Duration) -> Result<SocketAddr> {
        let handle = tokio::task::spawn_blocking(move || discover_host_blocking());
        let addr = time::timeout(timeout, handle).await??;
        Ok(addr)
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

    fn spawn_input_threads(
        socket: std::net::UdpSocket,
        session_id: Arc<Mutex<u128>>,
        packet_counter: Arc<AtomicU64>,
    ) -> Result<()> {
        let keyboard = find_device(DeviceKind::Keyboard)?;
        let mouse = find_device(DeviceKind::Mouse)?;

        if let Some(mut keyboard) = keyboard {
            let socket = socket.try_clone()?;
            let session_id = session_id.clone();
            let packet_counter = packet_counter.clone();
            thread::spawn(move || loop {
                if let Ok(events) = keyboard.fetch_events() {
                    for event in events {
                        if event.event_type() == EventType::KEY {
                            let keycode = event.code();
                            let pressed = event.value() != 0;
                            let input = InputMessage {
                                event: RiftInputEvent::Key { keycode, pressed },
                                timestamp_us: now_us(),
                            };
                            if let Err(err) = send_input(&socket, &session_id, &packet_counter, input) {
                                eprintln!("input send error: {err}");
                            }
                        }
                    }
                }
            });
        }

        if let Some(mut mouse) = mouse {
            let socket = socket.try_clone()?;
            let session_id = session_id.clone();
            let packet_counter = packet_counter.clone();
            let abs_info_x = mouse.abs_info(AbsoluteAxisType::ABS_X);
            let abs_info_y = mouse.abs_info(AbsoluteAxisType::ABS_Y);
            thread::spawn(move || {
                let mut last_abs_x: Option<i32> = None;
                let mut last_abs_y: Option<i32> = None;
                loop {
                if let Ok(events) = mouse.fetch_events() {
                    let mut dx = 0;
                    let mut dy = 0;
                    let mut abs_x = None;
                    let mut abs_y = None;
                    for event in events {
                        match event.event_type() {
                            EventType::RELATIVE => {
                                if event.code() == RelativeAxisType::REL_X.0 {
                                    dx += event.value();
                                } else if event.code() == RelativeAxisType::REL_Y.0 {
                                    dy += event.value();
                                }
                            }
                            EventType::ABSOLUTE => {
                                if event.code() == AbsoluteAxisType::ABS_X.0 {
                                    abs_x = Some(event.value());
                                } else if event.code() == AbsoluteAxisType::ABS_Y.0 {
                                    abs_y = Some(event.value());
                                }
                            }
                            EventType::KEY => {
                                let button = match event.code() {
                                    0x110 => 1,
                                    0x111 => 3,
                                    0x112 => 2,
                                    _ => 0,
                                };
                                if button != 0 {
                                    let pressed = event.value() != 0;
                                    let input = InputMessage {
                                        event: RiftInputEvent::MouseButton { button, pressed },
                                        timestamp_us: now_us(),
                                    };
                                    if let Err(err) = send_input(&socket, &session_id, &packet_counter, input) {
                                        eprintln!("input send error: {err}");
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    if dx != 0 || dy != 0 {
                        let input = InputMessage {
                            event: RiftInputEvent::MouseMotion { dx, dy },
                            timestamp_us: now_us(),
                        };
                        if let Err(err) = send_input(&socket, &session_id, &packet_counter, input) {
                            eprintln!("input send error: {err}");
                        }
                    }
                    if let (Some(x), Some(y)) = (abs_x, abs_y) {
                        last_abs_x = Some(x);
                        last_abs_y = Some(y);
                    }
                    if let (Some(x), Some(y)) = (last_abs_x, last_abs_y) {
                        if let (Some(info_x), Some(info_y)) = (abs_info_x.as_ref(), abs_info_y.as_ref()) {
                            let scaled_x = scale_abs(x, info_x.minimum, info_x.maximum);
                            let scaled_y = scale_abs(y, info_y.minimum, info_y.maximum);
                            let input = InputMessage {
                                event: RiftInputEvent::MouseAbsolute { x: scaled_x, y: scaled_y },
                                timestamp_us: now_us(),
                            };
                            if let Err(err) = send_input(&socket, &session_id, &packet_counter, input) {
                                eprintln!("input send error: {err}");
                            }
                        }
                    }
                }
            }
            });
        }

        Ok(())
    }

    fn send_input(
        socket: &std::net::UdpSocket,
        session_id: &Arc<Mutex<u128>>,
        packet_counter: &Arc<AtomicU64>,
        input: InputMessage,
    ) -> Result<()> {
        let session = *session_id.lock().unwrap();
        if session == UNASSIGNED_SESSION_ID {
            return Ok(());
        }
        let packet = Packet {
            version: RIFT_VERSION,
            session_id: session,
            packet_id: packet_counter.fetch_add(1, Ordering::Relaxed),
            channel: Channel::Input,
            message: Message::Input(input),
        };
        socket.send(&encode_packet(&packet))?;
        Ok(())
    }

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

    enum DeviceKind {
        Keyboard,
        Mouse,
    }

    fn scale_abs(value: i32, min: i32, max: i32) -> i32 {
        if max <= min {
            return value;
        }
        let clamped = value.clamp(min, max);
        let normalized = (clamped - min) as f32 / (max - min) as f32;
        (normalized * 65535.0) as i32
    }

    fn now_us() -> u64 {
        let now = std::time::SystemTime::now();
        now.duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0)
    }
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    linux_client::run().await
}
