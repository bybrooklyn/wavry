#[allow(unused_imports)]
use std::net::SocketAddr;
#[allow(unused_imports)]
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
#[allow(unused_imports)]
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
#[allow(unused_imports)]
use bytes::Bytes;
use tokio::net::UdpSocket;
use tokio::sync::oneshot;

// Imports
use wavry_media::{
    EncodeConfig, Codec, Resolution, Renderer
};

#[cfg(target_os = "macos")]
use wavry_media::{MacScreenEncoder, MacVideoRenderer};

#[cfg(not(target_os = "macos"))]
use wavry_media::DummyRenderer as MacVideoRenderer;
use wavry_client::{run_client as run_rift_client, ClientConfig, RendererFactory};
#[allow(unused_imports)]
use rift_core::{
   decode_msg, encode_msg, PhysicalPacket,
   ControlMessage as ProtoControl, Pong as ProtoPong, CongestionControl as ProtoCongestion,
   Message as ProtoMessage, RIFT_MAGIC, RIFT_VERSION,
};

#[allow(dead_code)]
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

// Stats shared with FFI
#[derive(Debug, Default)]
pub struct SessionStats {
    pub connected: AtomicBool,
    pub fps: AtomicU32,
    pub rtt_ms: AtomicU32,
    pub bitrate_kbps: AtomicU32,
    pub frames_encoded: AtomicU64,
    pub frames_decoded: AtomicU64,
}

pub struct SessionHandle {
    pub stop_tx: Option<oneshot::Sender<()>>,
    pub stats: Arc<SessionStats>,
}

impl SessionHandle {
    pub fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
    }
}

pub async fn run_host(
    port: u16,
    stats: Arc<SessionStats>,
    mut _stop_rx: oneshot::Receiver<()>,
    init_tx: oneshot::Sender<Result<()>>,
) -> Result<()> {
    #![allow(unused_variables)]
    // 1. Setup UDP
    let addr = format!("0.0.0.0:{}", port);
    
    let socket = match std::net::UdpSocket::bind(&addr) {
        Ok(s) => {
            let _ = s.set_nonblocking(true);
            match UdpSocket::from_std(s) {
                Ok(ts) => Arc::new(ts),
                Err(e) => {
                    let _ = init_tx.send(Err(anyhow!("Failed to convert socket: {}", e)));
                    return Err(e.into());
                }
            }
        },
        Err(e) => {
            let _ = init_tx.send(Err(anyhow!("Failed to bind UDP: {}", e)));
            return Err(e.into());
        }
    };
    log::info!("Host listening on {}", addr);

    // 2. Setup Encoder
    let config = EncodeConfig {
        codec: Codec::H264,
        resolution: Resolution { width: 1920, height: 1080 }, // TODO: Dynamic
        fps: 60,
        bitrate_kbps: 8000,
        keyframe_interval_ms: 2000,
    };
    
    #[cfg(target_os = "macos")]
    {
        // 2. Setup Encoder (Mac Only)
        let mut encoder = match MacScreenEncoder::new(config).await {
            Ok(enc) => enc,
            Err(e) => {
                let _ = init_tx.send(Err(anyhow!("Failed to create encoder: {}", e)));
                return Err(e);
            }
        };

        // Signal Init Success
        let _ = init_tx.send(Ok(()));
        
        // Notify Signaling Layer
        crate::signaling_ffi::set_hosting(port);

        // 3. Client state
        let mut client_addr: Option<SocketAddr> = None;
        let mut sequence: u64 = 0;
        
        // 4. DELTA Congestion Control
        let mut cc = DeltaCC::new(DeltaConfig::default(), config.bitrate_kbps, config.fps as u32);
        let mut last_target_bitrate = config.bitrate_kbps;
        
        // Loop
        let mut fps_counter = 0;
        let mut last_fps_time = std::time::Instant::now();
        let mut last_packet_time = std::time::Instant::now(); // To track client activity
        let mut bytes_sent = 0;

        loop {
            // Enforce timeout
            if client_addr.is_some() && last_packet_time.elapsed() > CONNECTION_TIMEOUT {
                log::warn!("Client timed out");
                client_addr = None;
                stats.connected.store(false, Ordering::Relaxed);
            }

            tokio::select! {
                _ = &mut stop_rx => {
                    log::info!("Host session stopped");
                    stats.connected.store(false, Ordering::Relaxed);
                    crate::signaling_ffi::clear_hosting();
                    break;
                }
                
                // Check for incoming packets (Control/Keepalive)
                res = async {
                    let mut buf = [0u8; 2048];
                    socket.recv_from(&mut buf).await.map(|(len, src)| (buf, len, src))
                } => {
                    match res {
                        Ok((buf, len, src)) => {
                            // Accept new client or update existing
                            if client_addr.is_none() || client_addr == Some(src) {
                                if client_addr.is_none() {
                                    log::info!("Client connected from {}", src);
                                    client_addr = Some(src);
                                    stats.connected.store(true, Ordering::Relaxed);
                                }
                                last_packet_time = std::time::Instant::now();
                            }
                            
                            // Parse RIFT message if this is a valid RIFT packet
                            if len >= 2 && buf[0..2] == RIFT_MAGIC {
                                if let Ok(phys) = PhysicalPacket::decode(Bytes::copy_from_slice(&buf[..len])) {
                                    // For now, no encryption on host (MVP)
                                    if let Ok(msg) = decode_msg(&phys.payload) {
                                        if let Some(rift_core::message::Content::Control(ctrl)) = msg.content {
                                            match ctrl.content {
                                                Some(rift_core::control_message::Content::Ping(ping)) => {
                                                    // Respond with Pong
                                                    let pong = ProtoMessage {
                                                        content: Some(rift_core::message::Content::Control(ProtoControl {
                                                            content: Some(rift_core::control_message::Content::Pong(ProtoPong {
                                                                timestamp_us: ping.timestamp_us,
                                                            })),
                                                        })),
                                                    };
                                                    let payload = encode_msg(&pong);
                                                    let phys_out = PhysicalPacket {
                                                        version: RIFT_VERSION,
                                                        session_id: None,
                                                        session_alias: phys.session_alias,
                                                        packet_id: sequence,
                                                        payload: Bytes::from(payload),
                                                    };
                                                    let _ = socket.send_to(&phys_out.encode(), src).await;
                                                    sequence = sequence.wrapping_add(1);
                                                }
                                                Some(rift_core::control_message::Content::Stats(report)) => {
                                                    // Feed to DELTA CC (include jitter for preemptive FEC)
                                                    let loss_ratio = if report.received_packets > 0 {
                                                        report.lost_packets as f32 / (report.received_packets + report.lost_packets) as f32
                                                    } else {
                                                        0.0
                                                    };
                                                    cc.on_rtt_sample(report.rtt_us, loss_ratio, report.jitter_us);
                                                    stats.rtt_ms.store((report.rtt_us / 1000) as u32, Ordering::Relaxed);
                                                    
                                                    // Check if bitrate target changed
                                                    let new_bitrate = cc.target_bitrate_kbps();
                                                    if new_bitrate != last_target_bitrate {
                                                        log::info!("DELTA: Bitrate {} -> {} kbps", last_target_bitrate, new_bitrate);
                                                        if let Err(e) = encoder.set_bitrate(new_bitrate) {
                                                            log::warn!("Failed to set encoder bitrate: {}", e);
                                                        }
                                                        last_target_bitrate = new_bitrate;
                                                        
                                                        // Send CongestionControl to client
                                                        let cc_msg = ProtoMessage {
                                                            content: Some(rift_core::message::Content::Control(ProtoControl {
                                                                content: Some(rift_core::control_message::Content::Congestion(ProtoCongestion {
                                                                    target_bitrate_kbps: new_bitrate,
                                                                    target_fps: cc.target_fps(),
                                                                })),
                                                            })),
                                                        };
                                                        let payload = encode_msg(&cc_msg);
                                                        let phys_out = PhysicalPacket {
                                                            version: RIFT_VERSION,
                                                            session_id: None,
                                                            session_alias: phys.session_alias,
                                                            packet_id: sequence,
                                                            payload: Bytes::from(payload),
                                                        };
                                                        let _ = socket.send_to(&phys_out.encode(), src).await;
                                                        sequence = sequence.wrapping_add(1);
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => log::warn!("Socket recv error: {}", e),
                    }
                }
                
                // Encode next frame
                res = encoder.next_frame_async() => {
                    match res {
                        Ok(frame) => {
                            if let Some(addr) = client_addr {
                                // Packetize and send (Simplified for MVP: 1 chunk)
                                let max_payload = 1400;
                                let data = frame.data;
                                let total_chunks = data.len().div_ceil(max_payload);
                                
                                for i in 0..total_chunks {
                                    let start = i * max_payload;
                                    let end = std::cmp::min(start + max_payload, data.len());
                                    let chunk = &data[start..end];
                                    
                                    let mut packet = Vec::with_capacity(10 + chunk.len());
                                    packet.extend_from_slice(&sequence.to_be_bytes());
                                    packet.push(i as u8);
                                    packet.push(total_chunks as u8);
                                    packet.extend_from_slice(chunk);
                                    
                                    if let Err(e) = socket.send_to(&packet, addr).await {
                                        log::warn!("Send error: {}", e);
                                    }
                                    
                                    bytes_sent += packet.len();
                                }
                                
                                sequence = sequence.wrapping_add(1);
                                stats.frames_encoded.fetch_add(1, Ordering::Relaxed);
                                
                                // FPS calc
                                fps_counter += 1;
                                if last_fps_time.elapsed() >= std::time::Duration::from_secs(1) {
                                    stats.fps.store(fps_counter, Ordering::Relaxed);
                                    stats.bitrate_kbps.store((bytes_sent as u32 * 8) / 1000, Ordering::Relaxed);
                                    fps_counter = 0;
                                    bytes_sent = 0;
                                    last_fps_time = std::time::Instant::now();
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Encoder error: {}", e);
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        // On non-macOS, we just error out immediately.
        let _ = init_tx.send(Err(anyhow!("Hosting only supported on macOS")));
        anyhow::bail!("Hosting only supported on macOS");
    }
}

struct SharedRenderer(Arc<Mutex<Option<Box<MacVideoRenderer>>>>);
impl Renderer for SharedRenderer {
    fn render(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        if let Ok(mut g) = self.0.lock() {
            if let Some(r) = g.as_mut() {
                return r.render(payload, timestamp_us);
            }
        }
        Ok(())
    }
}

pub async fn run_client(
    host_ip: String,
    port: u16,
    renderer_handle: Arc<std::sync::Mutex<Option<Box<MacVideoRenderer>>>>,
    _stats: Arc<SessionStats>,
    stop_rx: oneshot::Receiver<()>,
    init_tx: oneshot::Sender<Result<()>>,
) -> Result<()> {
    
    // Config for lib
    let config = ClientConfig {
        connect_addr: match format!("{}:{}", host_ip, port).parse() {
            Ok(a) => Some(a),
            Err(e) => {
                 let _ = init_tx.send(Err(anyhow!("Invalid address: {}", e)));
                 return Err(e.into());
            }
        },
        client_name: "WavryMacOS".to_string(),
        no_encrypt: false,
        identity_key: crate::identity::get_private_key(),
    };

    // Factory
    let factory: RendererFactory = Box::new(move |_config| {
        // Return a new SharedRenderer wrapper
        Ok(Box::new(SharedRenderer(renderer_handle.clone())))
    });

    log::info!("Starting Wavry Client (Refactored) connecting to {}:{}", host_ip, port);
    
    // We need to signal init_tx when it *starts*? 
    // run_client is blocking (async). 
    // We need to signal success early? No, run_client takes over.
    // The previous implementation signaled init success after socket connect.
    // wavry_client::run_client performs discovery/connect.
    // We should signal success immediately for FFI to unblock, OR spawn run_client separately.
    // But `run_client` here IS the spawned task.
    // So we signal success now.
    let _ = init_tx.send(Ok(()));

    // Run the library client
    // We invoke it, but we need to support cancellation via stop_rx.
    // run_client doesn't have a stop channel in arguments... it runs until error or return.
    // But we are inside tokio::spawn in lib.rs.
    // So `select!` on stop_rx vs run_client.

    tokio::select! {
        res = run_rift_client(config, Some(factory)) => {
            match res {
                Ok(_) => log::info!("Client finished normally"),
                Err(e) => log::error!("Client returned error: {}", e),
            }
        }
        _ = stop_rx => {
            log::info!("Client stopped via FFI");
            // run_client will be dropped/cancelled.
        }
    }

    Ok(())
}
