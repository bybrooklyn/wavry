#![forbid(unsafe_code)]

//! Wavry Relay - blind UDP forwarder for encrypted peer traffic.
//!
//! The relay:
//! 1. Validates leases presented by peers
//! 2. Forwards encrypted packets between peers
//! 3. Never decrypts traffic (E2E encryption between peers)

mod session;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bytes::Bytes;
use clap::Parser;
use rift_core::relay::{
    ForwardPayloadHeader, LeaseAckPayload, LeaseRejectPayload, LeaseRejectReason, RelayHeader,
    RelayPacketType, RELAY_HEADER_SIZE, RELAY_MAX_PACKET_SIZE,
};
use rift_core::PhysicalPacket;
use serde::{Deserialize, Serialize};
use session::{PeerRole, SessionError, SessionPool};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;
use wavry_common::protocol::{RelayHeartbeatRequest, RelayRegisterRequest, RelayRegisterResponse};

/// Default configuration values
const DEFAULT_MAX_SESSIONS: usize = 100;
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 60;
const DEFAULT_LEASE_DURATION_SECS: u64 = 300;
const CLEANUP_INTERVAL_SECS: u64 = 10;

#[derive(Parser, Debug)]
#[command(name = "wavry-relay")]
#[command(about = "Wavry relay node - forwards encrypted UDP traffic between peers")]
struct Args {
    /// UDP listen address
    #[arg(long, default_value = "127.0.0.1:4000")]
    listen: SocketAddr,

    /// Master server URL
    #[arg(long, default_value = "http://localhost:8080")]
    master_url: String,

    /// Maximum concurrent sessions
    #[arg(long, default_value_t = DEFAULT_MAX_SESSIONS)]
    max_sessions: usize,

    /// Session idle timeout in seconds
    #[arg(long, default_value_t = DEFAULT_IDLE_TIMEOUT_SECS)]
    idle_timeout: u64,

    /// Master public key (hex encoded Ed25519)
    #[arg(long)]
    master_public_key: Option<String>,

    /// Allow running without master signature validation (development only)
    #[arg(long, default_value_t = false)]
    allow_insecure_dev: bool,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,
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

/// Rate limiter for per-IP flood protection
struct IpRateLimiter {
    /// IP -> (packet_count, window_start)
    counts: HashMap<std::net::IpAddr, (u64, std::time::Instant)>,
    max_pps: u64,
    window: Duration,
}

impl IpRateLimiter {
    fn new(max_pps: u64) -> Self {
        Self {
            counts: HashMap::new(),
            max_pps,
            window: Duration::from_secs(1),
        }
    }

    fn check(&mut self, ip: std::net::IpAddr) -> bool {
        let now = std::time::Instant::now();
        let entry = self.counts.entry(ip).or_insert((0, now));

        // Reset window if expired
        if now.duration_since(entry.1) > self.window {
            *entry = (0, now);
        }

        entry.0 += 1;
        entry.0 <= self.max_pps
    }

    fn cleanup(&mut self) {
        let now = std::time::Instant::now();
        self.counts
            .retain(|_, (_, start)| now.duration_since(*start) < self.window * 2);
    }
}

/// Lease claims in PASETO token
#[derive(Debug, Serialize, Deserialize)]
struct LeaseClaims {
    #[serde(rename = "sub")]
    wavry_id: String,
    #[serde(rename = "sid")]
    session_id: Uuid,
    role: String, // "client" or "server"
    #[serde(rename = "exp")]
    expiration: String,
    #[serde(rename = "slimit")]
    soft_limit_kbps: Option<u32>,
    #[serde(rename = "hlimit")]
    hard_limit_kbps: Option<u32>,
}

/// Relay server state
struct RelayServer {
    socket: UdpSocket,
    sessions: RwLock<SessionPool>,
    ip_limiter: RwLock<IpRateLimiter>,
    lease_duration: Duration,
    master_public_key: Option<pasetors::keys::AsymmetricPublicKey<pasetors::version4::V4>>,
}

impl RelayServer {
    async fn new(
        listen: SocketAddr,
        max_sessions: usize,
        idle_timeout: Duration,
        master_key_hex: Option<&str>,
        allow_insecure_dev: bool,
    ) -> Result<Self> {
        let socket = UdpSocket::bind(listen).await?;
        info!("Relay listening on {}", listen);

        let master_public_key = if let Some(hex_key) = master_key_hex {
            let key_bytes = hex::decode(hex_key)?;
            let key =
                pasetors::keys::AsymmetricPublicKey::<pasetors::version4::V4>::from(&key_bytes)?;
            Some(key)
        } else if allow_insecure_dev {
            warn!("relay running in insecure dev mode (lease signature checks disabled)");
            None
        } else {
            return Err(anyhow::anyhow!(
                "master public key is required; pass --master-public-key or --allow-insecure-dev"
            ));
        };

        Ok(Self {
            socket,
            sessions: RwLock::new(SessionPool::new(max_sessions, idle_timeout)),
            ip_limiter: RwLock::new(IpRateLimiter::new(1000)),
            lease_duration: Duration::from_secs(DEFAULT_LEASE_DURATION_SECS),
            master_public_key,
        })
    }

    async fn active_session_count(&self) -> usize {
        self.sessions.read().await.active_count()
    }

    async fn run(&self) -> Result<()> {
        let mut buf = vec![0u8; RELAY_MAX_PACKET_SIZE];
        let mut cleanup_interval =
            tokio::time::interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));

        loop {
            tokio::select! {
                result = self.socket.recv_from(&mut buf) => {
                    let (len, src) = result?;
                    let packet = &buf[..len];

                    if let Err(e) = self.handle_packet(packet, src).await {
                        debug!("Packet from {} dropped: {}", src, e);
                    }
                }
                _ = cleanup_interval.tick() => {
                    self.cleanup().await;
                }
            }
        }
    }

    async fn handle_packet(&self, packet: &[u8], src: SocketAddr) -> Result<(), PacketError> {
        // 1. Size check
        if packet.len() < RELAY_HEADER_SIZE || packet.len() > RELAY_MAX_PACKET_SIZE {
            return Err(PacketError::InvalidSize);
        }

        // 2. Magic and version check (fast path)
        if !RelayHeader::quick_check(packet) {
            return Err(PacketError::InvalidMagic);
        }

        // 3. Per-IP rate limit
        {
            let mut limiter = self.ip_limiter.write().await;
            if !limiter.check(src.ip()) {
                return Err(PacketError::RateLimited);
            }
        }

        // 4. Parse header
        let header = RelayHeader::decode(packet).map_err(|_| PacketError::InvalidHeader)?;
        let payload = &packet[RELAY_HEADER_SIZE..];

        // 5. Dispatch by type
        match header.packet_type {
            RelayPacketType::LeasePresent => self.handle_lease_present(&header, payload, src).await,
            RelayPacketType::LeaseRenew => self.handle_lease_renew(&header, src).await,
            RelayPacketType::Forward => self.handle_forward(&header, payload, src).await,
            _ => Err(PacketError::UnexpectedType),
        }
    }

    async fn handle_lease_present(
        &self,
        header: &RelayHeader,
        payload: &[u8],
        src: SocketAddr,
    ) -> Result<(), PacketError> {
        use rift_core::relay::LeasePresentPayload;

        // 1. Parse payload
        let payload =
            LeasePresentPayload::decode(payload).map_err(|_| PacketError::InvalidPayload)?;

        // 2. Validate PASETO lease token
        let mut maybe_claims = None;
        let wavry_id = if let Some(ref master_key) = self.master_public_key {
            let token_str =
                String::from_utf8(payload.lease_token).map_err(|_| PacketError::InvalidPayload)?;

            let validation_rules = pasetors::claims::ClaimsValidationRules::new();
            // validation_rules.validate_expiration(); // Removed or fix according to newer pasetors

            let untrusted_token = pasetors::token::UntrustedToken::<
                pasetors::token::Public,
                pasetors::version4::V4,
            >::try_from(&token_str)
            .map_err(|_| PacketError::InvalidSignature)?;

            let claims = pasetors::public::verify(
                master_key,
                &untrusted_token,
                &validation_rules,
                None,
                None,
            )
            .map_err(|_| PacketError::InvalidSignature)?;

            let claims_json: LeaseClaims = serde_json::from_value(claims.payload().into())
                .map_err(|_| PacketError::InvalidPayload)?;

            // Verify session ID matches
            if claims_json.session_id != header.session_id {
                return Err(PacketError::InvalidPayload);
            }

            let id = claims_json.wavry_id.clone();
            maybe_claims = Some(claims_json);
            id
        } else {
            // If no master key, accept all (for development)
            format!("dev-peer-{}", src)
        };

        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_or_create(header.session_id, self.lease_duration)
            .map_err(|e| match e {
                SessionError::SessionFull => PacketError::SessionFull,
                _ => PacketError::SessionError,
            })?;

        let peer_role = match maybe_claims.as_ref().map(|c| c.role.as_str()) {
            Some("server") => PeerRole::Server,
            _ => PeerRole::Client,
        };

        // 3. Register peer
        if let Err(e) = session.register_peer(peer_role, wavry_id, src) {
            warn!("Failed to register peer from {}: {}", src, e);
            self.send_lease_reject(header.session_id, src, LeaseRejectReason::SessionFull)
                .await;
            return Err(PacketError::SessionError);
        }

        // 5. Update limits from lease (if present)
        if let Some(claims) = maybe_claims {
            if let Some(soft) = claims.soft_limit_kbps {
                session.soft_limit_kbps = soft;
            }
            if let Some(hard) = claims.hard_limit_kbps {
                session.hard_limit_kbps = hard;
            }
        }

        // Send ACK
        self.send_lease_ack(header.session_id, src, session.lease_expires)
            .await;

        info!(
            "Peer {:?} registered for session {} from {}",
            peer_role, header.session_id, src
        );

        Ok(())
    }

    async fn handle_lease_renew(
        &self,
        header: &RelayHeader,
        src: SocketAddr,
    ) -> Result<(), PacketError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&header.session_id)
            .ok_or(PacketError::SessionNotFound)?;

        // Verify source is a known peer
        if session.identify_peer(src).is_none() {
            return Err(PacketError::UnknownPeer);
        }

        session.renew_lease(self.lease_duration);

        self.send_lease_ack(header.session_id, src, session.lease_expires)
            .await;

        debug!("Lease renewed for session {} by {}", header.session_id, src);

        Ok(())
    }

    async fn handle_forward(
        &self,
        header: &RelayHeader,
        payload: &[u8],
        src: SocketAddr,
    ) -> Result<(), PacketError> {
        // Get session
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&header.session_id)
            .ok_or(PacketError::SessionNotFound)?;

        // Check session is active
        if !session.is_active() {
            return Err(PacketError::SessionNotActive);
        }

        // Identify sender and get destination
        let (sender_role, _sender_id, dest) =
            session.identify_peer(src).ok_or(PacketError::UnknownPeer)?;

        let dest_addr = dest.socket_addr;
        let sequence = extract_forward_sequence(payload)?;

        if let Some(sender) = session.get_peer_mut(sender_role) {
            if !sender.seq_window.check_and_update(sequence) {
                return Err(PacketError::ReplayDetected(sequence));
            }
        }

        // 6. Bandwidth Rate Limiting
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(session.last_stats_reset).as_secs_f32();
        if elapsed >= 1.0 {
            session.current_bps = (session.bytes_sent_window as f32 / elapsed) * 8.0;
            session.bytes_sent_window = 0;
            session.last_stats_reset = now;
        }

        if session.current_bps > (session.hard_limit_kbps * 1000) as f32 {
            return Err(PacketError::RateLimited);
        }

        // 7. Update sender's last seen (for NAT rebinding)
        if let Some(sender) = session.get_peer_mut(sender_role) {
            if sender.socket_addr != src {
                debug!(
                    "NAT rebinding detected for {:?}: {} -> {}",
                    sender_role, sender.socket_addr, src
                );
                sender.socket_addr = src;
            }
            sender.last_seen = now;
        }

        // Record stats
        let forward_size = RELAY_HEADER_SIZE + payload.len();
        session.record_forward(forward_size);
        session.bytes_sent_window += forward_size as u64;

        // Forward the entire original packet to destination
        // We need to re-encode header + payload
        let mut forward_buf = vec![0u8; RELAY_HEADER_SIZE + payload.len()];
        header
            .encode(&mut forward_buf)
            .map_err(|_| PacketError::InvalidHeader)?;
        forward_buf[RELAY_HEADER_SIZE..].copy_from_slice(payload);

        drop(sessions); // Release lock before sending

        self.socket.send_to(&forward_buf, dest_addr).await?;

        Ok(())
    }

    async fn send_lease_ack(
        &self,
        session_id: uuid::Uuid,
        dest: SocketAddr,
        expires: std::time::Instant,
    ) {
        let header = RelayHeader::new(RelayPacketType::LeaseAck, session_id);

        // Calculate expiry as duration from now
        let expires_ms = expires
            .saturating_duration_since(std::time::Instant::now())
            .as_millis() as u64;
        let unix_expires = chrono::Utc::now().timestamp_millis() as u64 + expires_ms;

        let payload = LeaseAckPayload {
            expires_ms: unix_expires,
            soft_limit_kbps: 50_000,  // 50 Mbps
            hard_limit_kbps: 100_000, // 100 Mbps
        };

        let mut packet = vec![0u8; RELAY_HEADER_SIZE + LeaseAckPayload::SIZE];
        if header.encode(&mut packet).is_err() {
            warn!("Failed to encode header");
            return;
        }
        if payload.encode(&mut packet[RELAY_HEADER_SIZE..]).is_err() {
            warn!("Failed to encode payload");
            return;
        }

        if let Err(e) = self.socket.send_to(&packet, dest).await {
            warn!("Failed to send LEASE_ACK to {}: {}", dest, e);
        }
    }

    async fn send_lease_reject(
        &self,
        session_id: uuid::Uuid,
        dest: SocketAddr,
        reason: LeaseRejectReason,
    ) {
        let header = RelayHeader::new(RelayPacketType::LeaseReject, session_id);

        let payload = LeaseRejectPayload { reason };

        let mut packet = vec![0u8; RELAY_HEADER_SIZE + LeaseRejectPayload::SIZE];
        if header.encode(&mut packet).is_err() {
            return;
        }
        if payload.encode(&mut packet[RELAY_HEADER_SIZE..]).is_err() {
            return;
        }

        if let Err(e) = self.socket.send_to(&packet, dest).await {
            warn!("Failed to send LEASE_REJECT to {}: {}", dest, e);
        }
    }

    async fn cleanup(&self) {
        let mut sessions = self.sessions.write().await;
        let cleaned = sessions.cleanup();
        if cleaned > 0 {
            info!("Cleaned up {} expired/idle sessions", cleaned);
        }

        let mut limiter = self.ip_limiter.write().await;
        limiter.cleanup();
    }
}

/// Packet handling errors
#[derive(Debug, thiserror::Error)]
enum PacketError {
    #[error("invalid packet size")]
    InvalidSize,
    #[error("invalid magic/version")]
    InvalidMagic,
    #[error("rate limited")]
    RateLimited,
    #[error("invalid header")]
    InvalidHeader,
    #[error("invalid payload")]
    InvalidPayload,
    #[error("unexpected packet type")]
    UnexpectedType,
    #[error("invalid signature")]
    InvalidSignature,
    #[error("session not found")]
    SessionNotFound,
    #[error("session not active")]
    SessionNotActive,
    #[error("session full")]
    SessionFull,
    #[error("unknown peer")]
    UnknownPeer,
    #[error("replay detected for sequence {0}")]
    ReplayDetected(u64),
    #[error("session error")]
    SessionError,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

fn extract_forward_sequence(payload: &[u8]) -> Result<u64, PacketError> {
    if payload.starts_with(&rift_core::RIFT_MAGIC) {
        let packet = PhysicalPacket::decode(Bytes::copy_from_slice(payload))
            .map_err(|_| PacketError::InvalidPayload)?;
        return Ok(packet.packet_id);
    }

    let header = ForwardPayloadHeader::decode(payload).map_err(|_| PacketError::InvalidPayload)?;
    Ok(header.sequence)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if !args.listen.ip().is_loopback() && !env_bool("WAVRY_RELAY_ALLOW_PUBLIC_BIND", false) {
        return Err(anyhow::anyhow!(
            "refusing non-loopback relay bind without WAVRY_RELAY_ALLOW_PUBLIC_BIND=1"
        ));
    }

    // Initialize tracing
    let filter = format!("{},hyper=warn,tokio=warn", args.log_level);
    tracing_subscriber::fmt().with_env_filter(filter).init();

    info!("Starting wavry-relay v{}", env!("CARGO_PKG_VERSION"));

    // Generate Relay ID
    let relay_id = Uuid::new_v4();
    info!("Relay ID: {}", relay_id);

    // Register with Master (with retries)
    let client = reqwest::Client::new();
    let register_url = format!("{}/v1/relays/register", args.master_url);
    let endpoints = vec![args.listen.to_string()];

    let mut retry_delay = Duration::from_secs(1);
    let max_retry_delay = Duration::from_secs(60);

    info!("Registering with Master at {}...", args.master_url);

    let reg_data = loop {
        match client
            .post(&register_url)
            .json(&RelayRegisterRequest {
                relay_id: relay_id.to_string(),
                endpoints: endpoints.clone(),
            })
            .send()
            .await
        {
            Ok(resp) => {
                if resp.status().is_success() {
                    match resp.json::<RelayRegisterResponse>().await {
                        Ok(data) => break data,
                        Err(e) => warn!("Failed to parse registration response: {}", e),
                    }
                } else {
                    warn!("Master registration failed with status: {}", resp.status());
                }
            }
            Err(e) => warn!("Failed to connect to Master: {}", e),
        }

        info!("Retrying registration in {:?}...", retry_delay);
        tokio::time::sleep(retry_delay).await;
        retry_delay = std::cmp::min(retry_delay * 2, max_retry_delay);
    };

    info!(
        "Registered successfully. Heartbeat interval: {}ms",
        reg_data.heartbeat_interval_ms
    );

    let server = Arc::new(
        RelayServer::new(
            args.listen,
            args.max_sessions,
            Duration::from_secs(args.idle_timeout),
            args.master_public_key.as_deref(),
            args.allow_insecure_dev,
        )
        .await?,
    );

    // Spawn Heartbeat Task
    let server_clone = server.clone();
    let master_url = args.master_url.clone();
    let hb_interval = Duration::from_millis(reg_data.heartbeat_interval_ms);
    let max_sessions = args.max_sessions;

    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let heartbeat_url = format!("{}/v1/relays/heartbeat", master_url);
        let mut interval = tokio::time::interval(hb_interval);

        loop {
            interval.tick().await;

            let active = server_clone.active_session_count().await;

            // Calculate load (0-100)
            let load = if max_sessions > 0 {
                (active as f32 / max_sessions as f32) * 100.0
            } else {
                100.0
            } as u8;

            let req = RelayHeartbeatRequest {
                relay_id: relay_id.to_string(),
                load_pct: load as f32,
            };

            if let Err(e) = client.post(&heartbeat_url).json(&req).send().await {
                warn!("Failed to send heartbeat: {}", e);
            }
        }
    });

    server.run().await
}
