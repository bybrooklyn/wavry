mod client {
    use std::{
        collections::HashMap,
        net::SocketAddr,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
        thread,
        time::Duration,
    };

    use anyhow::{anyhow, Result};
    use clap::Parser;
    #[cfg(target_os = "linux")]
    use evdev::{AbsoluteAxisType, Device, EventType, RelativeAxisType};
    use mdns_sd::{ServiceDaemon, ServiceEvent};
    use rift_core::{
        decode_packet, encode_packet, Channel, Codec as RiftCodec, ControlMessage, FecPacket,
        Handshake, HandshakeError, InputEvent as RiftInputEvent, InputMessage, Message, Packet,
        Ping, StatsReport, UNASSIGNED_SESSION_ID, RIFT_VERSION,
    };
    use rift_crypto::connection::{handshake_type, SecureClient};
    use wavry_media::{Codec, DecodeConfig, Resolution as MediaResolution};
    #[cfg(target_os = "linux")]
    use wavry_media::GstVideoRenderer as VideoRenderer;
    #[cfg(not(target_os = "linux"))]
    use wavry_media::DummyRenderer as VideoRenderer;
    use tokio::{net::UdpSocket, sync::mpsc, time};
    use tracing::{debug, info, warn};

    const FRAME_TIMEOUT_US: u64 = 50_000;
    const MAX_FEC_CACHE: usize = 256;

    #[derive(Parser, Debug)]
    #[command(name = "wavry-client")]
    struct Args {
        #[arg(long)]
        connect: Option<SocketAddr>,
        #[arg(long, default_value = "wavry-client")]
        name: String,
        /// Disable encryption (for testing/debugging)
        #[arg(long, default_value = "false")]
        no_encrypt: bool,
    }

    /// Crypto state for the client
    enum CryptoState {
        /// No encryption (--no-encrypt mode)
        Disabled,
        /// Crypto handshake in progress
        Handshaking(SecureClient),
        /// Crypto established
        Established(SecureClient),
    }

    impl CryptoState {
        fn new(disabled: bool) -> Result<Self> {
            if disabled {
                Ok(CryptoState::Disabled)
            } else {
                Ok(CryptoState::Handshaking(SecureClient::new()?))
            }
        }

        #[allow(dead_code)]
        fn is_established(&self) -> bool {
            matches!(self, CryptoState::Established(_) | CryptoState::Disabled)
        }
    }

    pub async fn run() -> Result<()> {
        tracing_subscriber::fmt().with_env_filter("info").init();

        let args = Args::parse();
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        let connect_addr = match args.connect {
            Some(addr) => addr,
            None => discover_host(Duration::from_secs(3)).await?,
        };

        if args.no_encrypt {
            warn!("ENCRYPTION DISABLED - not for production use");
        }

        // Initialize crypto state
        let mut crypto = CryptoState::new(args.no_encrypt)?;

        // Create input channel - threads send InputMessage, main loop handles sending
        let (input_tx, mut input_rx) = mpsc::channel::<InputMessage>(128);

        // Spawn input capture threads that send to channel
        spawn_input_threads(input_tx)?;

        // Perform crypto handshake if enabled
        if let CryptoState::Handshaking(ref mut client) = crypto {
            info!("starting crypto handshake with {}", connect_addr);
            
            // Send message 1
            let msg1 = client.start_handshake()
                .map_err(|e| anyhow!("crypto handshake: {}", e))?;
            socket.send_to(&msg1, connect_addr).await?;
            debug!("sent crypto msg1");

            // Wait for message 2
            let mut buf = vec![0u8; 4096];
            let (len, _) = time::timeout(Duration::from_secs(5), socket.recv_from(&mut buf))
                .await
                .map_err(|_| anyhow!("crypto handshake timeout"))??;
            
            if buf[0] != handshake_type::MSG2 {
                return Err(anyhow!("expected crypto msg2, got 0x{:02x}", buf[0]));
            }
            debug!("received crypto msg2");

            // Process msg2 and send msg3
            let msg3 = client.process_server_response(&buf[..len])
                .map_err(|e| anyhow!("crypto handshake: {}", e))?;
            socket.send_to(&msg3, connect_addr).await?;
            debug!("sent crypto msg3");

            info!("crypto handshake complete");
        }

        // Transition to established
        if let CryptoState::Handshaking(client) = crypto {
            crypto = CryptoState::Established(client);
        }

        // Now perform RIFT handshake
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

        let packet_counter = Arc::new(AtomicU64::new(1));
        let next_packet_id = || {
            packet_counter.fetch_add(1, Ordering::Relaxed)
        };

        let hello_packet = Packet {
            version: RIFT_VERSION,
            session_id: UNASSIGNED_SESSION_ID,
            packet_id: next_packet_id(),
            channel: Channel::Control,
            message: Message::Control(ControlMessage::Hello(hello)),
        };
        send_packet(&socket, &mut crypto, connect_addr, &hello_packet).await?;
        info!("sent RIFT hello to {}", connect_addr);

        // Main recv loop
        let mut buf = vec![0u8; 64 * 1024];
        let mut ping_interval = time::interval(Duration::from_millis(500));
        let mut stats_interval = time::interval(Duration::from_millis(1000));
        let mut session_id = UNASSIGNED_SESSION_ID;

        let mut last_packet_id: Option<u64> = None;
        let mut received_packets: u32 = 0;
        let mut lost_packets: u32 = 0;
        let mut last_rtt_us: u64 = 0;

        let mut renderer: Option<VideoRenderer> = None;
        let mut frames = FrameAssembler::new(FRAME_TIMEOUT_US);
        let mut fec_cache = FecCache::new();

        loop {
            tokio::select! {
                // Handle input from capture threads
                Some(input) = input_rx.recv() => {
                    if session_id != UNASSIGNED_SESSION_ID {
                        let packet = Packet {
                            version: RIFT_VERSION,
                            session_id,
                            packet_id: next_packet_id(),
                            channel: Channel::Input,
                            message: Message::Input(input),
                        };
                        if let Err(e) = send_packet(&socket, &mut crypto, connect_addr, &packet).await {
                            debug!("input send error: {}", e);
                        }
                    }
                }

                // Ping interval
                _ = ping_interval.tick() => {
                    if session_id == UNASSIGNED_SESSION_ID {
                        continue;
                    }
                    let ping = Packet {
                        version: RIFT_VERSION,
                        session_id,
                        packet_id: next_packet_id(),
                        channel: Channel::Control,
                        message: Message::Control(ControlMessage::Ping(Ping { timestamp_us: now_us() })),
                    };
                    send_packet(&socket, &mut crypto, connect_addr, &ping).await?;
                }

                // Stats interval
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
                        packet_id: next_packet_id(),
                        channel: Channel::Control,
                        message: Message::Control(ControlMessage::Stats(stats)),
                    };
                    received_packets = 0;
                    lost_packets = 0;
                    send_packet(&socket, &mut crypto, connect_addr, &stats_packet).await?;
                }

                // Receive packets
                recv = socket.recv_from(&mut buf) => {
                    let (len, peer) = recv?;
                    let raw = &buf[..len];

                    // Decrypt if needed
                    let plaintext = match decrypt_packet(&mut crypto, raw) {
                        Ok(p) => p,
                        Err(e) => {
                            debug!("decrypt error from {}: {}", peer, e);
                            continue;
                        }
                    };

                    let packet = match decode_packet(&plaintext) {
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
                        renderer = Some(VideoRenderer::new(DecodeConfig {
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
                            fec_cache.insert(packet.packet_id, plaintext.clone());
                            if let Some(frame) = frames.push(chunk) {
                                if let Some(renderer) = renderer.as_mut() {
                                    renderer.push(&frame.data, frame.timestamp_us)?;
                                }
                            }
                        }
                        Message::Fec(fec) => {
                            if let Some(recovered) = fec_cache.try_recover(&fec) {
                                if let Ok(recovered_packet) = decode_packet(&recovered) {
                                    if let Message::Video(chunk) = recovered_packet.message {
                                        if let Some(frame) = frames.push(chunk) {
                                            if let Some(renderer) = renderer.as_mut() {
                                                renderer.push(&frame.data, frame.timestamp_us)?;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {
                            debug!("packet from {}: {:?}", peer, packet.message);
                        }
                    }
                }
            }
        }
    }

    /// Send a RIFT packet, encrypting if crypto is established
    async fn send_packet(
        socket: &UdpSocket,
        crypto: &mut CryptoState,
        dest: SocketAddr,
        packet: &Packet,
    ) -> Result<()> {
        let encoded = encode_packet(packet);

        match crypto {
            CryptoState::Disabled => {
                socket.send_to(&encoded, dest).await?;
            }
            CryptoState::Established(client) => {
                let (packet_id, ciphertext) = client.encrypt(&encoded)
                    .map_err(|e| anyhow!("encrypt failed: {}", e))?;

                // Format: [8 bytes packet_id] [ciphertext]
                let mut buf = Vec::with_capacity(8 + ciphertext.len());
                buf.extend_from_slice(&packet_id.to_le_bytes());
                buf.extend_from_slice(&ciphertext);

                socket.send_to(&buf, dest).await?;
            }
            CryptoState::Handshaking(_) => {
                return Err(anyhow!("cannot send during crypto handshake"));
            }
        }

        Ok(())
    }

    /// Decrypt a received packet
    fn decrypt_packet(crypto: &mut CryptoState, raw: &[u8]) -> Result<Vec<u8>> {
        match crypto {
            CryptoState::Disabled => Ok(raw.to_vec()),
            CryptoState::Established(client) => {
                if raw.len() < 8 {
                    return Err(anyhow!("packet too short"));
                }
                let packet_id = u64::from_le_bytes(raw[0..8].try_into()?);
                let ciphertext = &raw[8..];
                client.decrypt(packet_id, ciphertext)
                    .map_err(|e| anyhow!("decrypt failed: {}", e))
            }
            CryptoState::Handshaking(_) => {
                Err(anyhow!("received data during handshake"))
            }
        }
    }

    /// Spawn input capture threads that send to channel
    #[cfg(target_os = "linux")]
    fn spawn_input_threads(input_tx: mpsc::Sender<InputMessage>) -> Result<()> {
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
                            let input = InputMessage {
                                event: RiftInputEvent::Key { keycode, pressed },
                                timestamp_us: now_us(),
                            };
                            if tx.blocking_send(input).is_err() {
                                return; // Channel closed
                            }
                        }
                    }
                }
            });
        }

        if let Some(mut mouse) = mouse {
            let tx = input_tx;
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
                                        if tx.blocking_send(input).is_err() {
                                            return;
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
                            if tx.blocking_send(input).is_err() {
                                return;
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
                                if tx.blocking_send(input).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
            });
        }

        Ok(())
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

        fn insert(&mut self, packet_id: u64, data: Vec<u8>) {
            if self.packets.len() >= MAX_FEC_CACHE {
                // Remove oldest
                if let Some(min_id) = self.packets.keys().min().copied() {
                    self.packets.remove(&min_id);
                }
            }
            self.packets.insert(packet_id, data);
        }

        fn try_recover(&self, _fec: &FecPacket) -> Option<Vec<u8>> {
            // FEC recovery not implemented yet
            None
        }
    }

    // ============= Helpers =============

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

    #[cfg(not(target_os = "linux"))]
    fn spawn_input_threads(input_tx: mpsc::Sender<InputMessage>) -> Result<()> {
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(2));
                // Send dummy 'A' key press/release
                let press = InputMessage {
                    event: RiftInputEvent::Key { keycode: 30, pressed: true }, // 30 is A
                    timestamp_us: now_us(),
                };
                let _ = input_tx.blocking_send(press);
                
                thread::sleep(Duration::from_millis(100));
                
                let release = InputMessage {
                    event: RiftInputEvent::Key { keycode: 30, pressed: false },
                    timestamp_us: now_us(),
                };
                let _ = input_tx.blocking_send(release);
            }
        });
        Ok(())
    }

    #[cfg(target_os = "linux")]
    enum DeviceKind {
        Keyboard,
        Mouse,
    }

    #[cfg(not(target_os = "linux"))]
    enum _DeviceKind {}

    #[allow(dead_code)]
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



#[tokio::main]
async fn main() -> anyhow::Result<()> {
    client::run().await
}
