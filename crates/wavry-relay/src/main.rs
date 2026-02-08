#![forbid(unsafe_code)]

mod session;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
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

const DEFAULT_MAX_SESSIONS: usize = 100;
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 60;
const DEFAULT_LEASE_DURATION_SECS: u64 = 300;
const DEFAULT_CLEANUP_INTERVAL_SECS: u64 = 10;
const DEFAULT_IP_RATE_LIMIT_PPS: u64 = 1000;
const DEFAULT_STATS_LOG_INTERVAL_SECS: u64 = 30;

#[derive(Parser, Debug)]
#[command(name = "wavry-relay")]
#[command(about = "Wavry relay node - forwards encrypted UDP traffic between peers")]
struct Args {
    /// UDP listen address (use :0 for random)
    #[arg(long, env = "WAVRY_RELAY_LISTEN", default_value = "0.0.0.0:4000")]
    listen: SocketAddr,

    /// Master server URL
    #[arg(
        long,
        env = "WAVRY_MASTER_URL",
        default_value = "http://localhost:8080"
    )]
    master_url: String,

    /// Maximum concurrent sessions
    #[arg(long, default_value_t = DEFAULT_MAX_SESSIONS)]
    max_sessions: usize,

    /// Session idle timeout in seconds
    #[arg(long, default_value_t = DEFAULT_IDLE_TIMEOUT_SECS)]
    idle_timeout: u64,

    /// Master public key (hex encoded Ed25519)
    #[arg(long, env = "WAVRY_RELAY_MASTER_PUBLIC_KEY")]
    master_public_key: Option<String>,

    /// Allow running without master signature validation (development only)
    #[arg(long, env = "WAVRY_RELAY_ALLOW_INSECURE_DEV", default_value_t = false)]
    allow_insecure_dev: bool,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Per-source-IP packet rate limit (packets/sec)
    #[arg(long, default_value_t = DEFAULT_IP_RATE_LIMIT_PPS)]
    ip_rate_limit_pps: u64,

    /// Session cleanup interval in seconds
    #[arg(long, default_value_t = DEFAULT_CLEANUP_INTERVAL_SECS)]
    cleanup_interval_secs: u64,

    /// Lease duration in seconds
    #[arg(long, default_value_t = DEFAULT_LEASE_DURATION_SECS)]
    lease_duration_secs: u64,

    /// Relay stats log interval in seconds
    #[arg(long, default_value_t = DEFAULT_STATS_LOG_INTERVAL_SECS)]
    stats_log_interval_secs: u64,

    /// Geographic region (e.g. us-east-1)
    #[arg(long, env = "WAVRY_RELAY_REGION")]
    region: Option<String>,

    /// Autonomous System Number
    #[arg(long, env = "WAVRY_RELAY_ASN")]
    asn: Option<u32>,

    /// Maximum supported bitrate in kbps (minimum 10000)
    #[arg(long, env = "WAVRY_RELAY_MAX_BITRATE", default_value_t = 20_000)]
    max_bitrate_kbps: u32,
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

struct IpRateLimiter {
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

#[derive(Debug, Serialize, Deserialize)]
struct LeaseClaims {
    #[serde(rename = "sub")]
    wavry_id: String,
    #[serde(rename = "sid")]
    session_id: Uuid,
    role: String,
    #[serde(rename = "exp")]
    expiration: String,
    #[serde(rename = "slimit")]
    soft_limit_kbps: Option<u32>,
    #[serde(rename = "hlimit")]
    hard_limit_kbps: Option<u32>,
}

#[derive(Default)]
struct RelayMetrics {
    packets_rx: AtomicU64,
    bytes_rx: AtomicU64,
    packets_forwarded: AtomicU64,
    bytes_forwarded: AtomicU64,
    lease_present_packets: AtomicU64,
    lease_renew_packets: AtomicU64,
    dropped_packets: AtomicU64,
    rate_limited_packets: AtomicU64,
    invalid_packets: AtomicU64,
    auth_reject_packets: AtomicU64,
}

#[derive(Debug, Serialize)]
struct RelayMetricsSnapshot {
    packets_rx: u64,
    bytes_rx: u64,
    packets_forwarded: u64,
    bytes_forwarded: u64,
    lease_present_packets: u64,
    lease_renew_packets: u64,
    dropped_packets: u64,
    rate_limited_packets: u64,
    invalid_packets: u64,
    auth_reject_packets: u64,
}

impl RelayMetrics {
    fn snapshot(&self) -> RelayMetricsSnapshot {
        RelayMetricsSnapshot {
            packets_rx: self.packets_rx.load(Ordering::Relaxed),
            bytes_rx: self.bytes_rx.load(Ordering::Relaxed),
            packets_forwarded: self.packets_forwarded.load(Ordering::Relaxed),
            bytes_forwarded: self.bytes_forwarded.load(Ordering::Relaxed),
            lease_present_packets: self.lease_present_packets.load(Ordering::Relaxed),
            lease_renew_packets: self.lease_renew_packets.load(Ordering::Relaxed),
            dropped_packets: self.dropped_packets.load(Ordering::Relaxed),
            rate_limited_packets: self.rate_limited_packets.load(Ordering::Relaxed),
            invalid_packets: self.invalid_packets.load(Ordering::Relaxed),
            auth_reject_packets: self.auth_reject_packets.load(Ordering::Relaxed),
        }
    }
}

struct RelayServer {
    socket: UdpSocket,
    sessions: RwLock<SessionPool>,
    ip_limiter: RwLock<IpRateLimiter>,
    lease_duration: Duration,
    cleanup_interval: Duration,
    stats_log_interval: Duration,
    metrics: RelayMetrics,
    master_public_key: Option<pasetors::keys::AsymmetricPublicKey<pasetors::version4::V4>>,
}

impl RelayServer {
    #[allow(clippy::too_many_arguments)]
    async fn new(
        socket: UdpSocket,
        max_sessions: usize,
        idle_timeout: Duration,
        lease_duration: Duration,
        cleanup_interval: Duration,
        stats_log_interval: Duration,
        ip_rate_limit_pps: u64,
        master_key_hex: Option<&str>,
        registration_master_key: Option<&[u8]>,
        allow_insecure_dev: bool,
    ) -> Result<Self> {
        let master_public_key = if let Some(hex_key) = master_key_hex {
            let key_bytes = hex::decode(hex_key)?;
            let key =
                pasetors::keys::AsymmetricPublicKey::<pasetors::version4::V4>::from(&key_bytes)?;
            Some(key)
        } else if let Some(key_bytes) = registration_master_key {
            let key =
                pasetors::keys::AsymmetricPublicKey::<pasetors::version4::V4>::from(key_bytes)?;
            Some(key)
        } else if allow_insecure_dev {
            if !env_bool("WAVRY_ALLOW_INSECURE_RELAY", false) {
                return Err(anyhow::anyhow!(
                    "refusing to start in insecure dev mode; set WAVRY_ALLOW_INSECURE_RELAY=1 to override (NOT FOR PRODUCTION)"
                ));
            }
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
            ip_limiter: RwLock::new(IpRateLimiter::new(ip_rate_limit_pps.max(1))),
            lease_duration,
            cleanup_interval,
            stats_log_interval,
            metrics: RelayMetrics::default(),
            master_public_key,
        })
    }

    async fn active_session_count(&self) -> usize {
        self.sessions.read().await.active_count().await
    }

    async fn run(&self) -> Result<()> {
        let mut buf = vec![0u8; RELAY_MAX_PACKET_SIZE];
        let mut cleanup_interval = tokio::time::interval(self.cleanup_interval);
        let mut last_stats_log = std::time::Instant::now();

        loop {
            tokio::select! {
                result = self.socket.recv_from(&mut buf) => {
                    let (len, src) = result?;
                    let packet = &buf[..len];
                    self.metrics.packets_rx.fetch_add(1, Ordering::Relaxed);
                    self.metrics.bytes_rx.fetch_add(packet.len() as u64, Ordering::Relaxed);

                    if let Err(e) = self.handle_packet(packet, src).await {
                        self.record_packet_error(&e, src);
                    }
                }
                _ = cleanup_interval.tick() => {
                    self.cleanup().await;
                    if last_stats_log.elapsed() >= self.stats_log_interval {
                        self.log_metrics().await;
                        last_stats_log = std::time::Instant::now();
                    }
                }
            }
        }
    }

    async fn handle_packet(&self, packet: &[u8], src: SocketAddr) -> Result<(), PacketError> {
        if packet.len() < RELAY_HEADER_SIZE || packet.len() > RELAY_MAX_PACKET_SIZE {
            return Err(PacketError::InvalidSize);
        }
        if !RelayHeader::quick_check(packet) {
            return Err(PacketError::InvalidMagic);
        }
        {
            let mut limiter = self.ip_limiter.write().await;
            if !limiter.check(src.ip()) {
                return Err(PacketError::RateLimited);
            }
        }
        let header = RelayHeader::decode(packet).map_err(|_| PacketError::InvalidHeader)?;
        if header.session_id.is_nil() {
            return Err(PacketError::InvalidSessionId);
        }
        let payload = &packet[RELAY_HEADER_SIZE..];
        match header.packet_type {
            RelayPacketType::LeasePresent => {
                self.metrics
                    .lease_present_packets
                    .fetch_add(1, Ordering::Relaxed);
                self.handle_lease_present(&header, payload, src).await
            }
            RelayPacketType::LeaseRenew => {
                self.metrics
                    .lease_renew_packets
                    .fetch_add(1, Ordering::Relaxed);
                self.handle_lease_renew(&header, src).await
            }
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
        let payload =
            LeasePresentPayload::decode(payload).map_err(|_| PacketError::InvalidPayload)?;
        let mut maybe_claims = None;
        let wavry_id = if let Some(ref master_key) = self.master_public_key {
            let token_str =
                String::from_utf8(payload.lease_token).map_err(|_| PacketError::InvalidPayload)?;
            let validation_rules = pasetors::claims::ClaimsValidationRules::new();
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
            if claims_json.session_id != header.session_id {
                return Err(PacketError::InvalidPayload);
            }
            if claims_json.session_id.is_nil() {
                return Err(PacketError::InvalidSessionId);
            }
            let lease_expiration = chrono::DateTime::parse_from_rfc3339(&claims_json.expiration)
                .map_err(|_| PacketError::InvalidPayload)?
                .with_timezone(&chrono::Utc);
            if lease_expiration <= chrono::Utc::now() {
                self.send_lease_reject(header.session_id, src, LeaseRejectReason::Expired)
                    .await;
                return Err(PacketError::ExpiredLease);
            }
            if !matches!(claims_json.role.as_str(), "client" | "server") {
                return Err(PacketError::InvalidRole);
            }
            let id = claims_json.wavry_id.clone();
            maybe_claims = Some(claims_json);
            id
        } else {
            format!("dev-peer-{}", src)
        };
        let session_lock = {
            let mut sessions = self.sessions.write().await;
            sessions
                .get_or_create(header.session_id, self.lease_duration)
                .map_err(|e| match e {
                    SessionError::SessionFull => PacketError::SessionFull,
                    _ => PacketError::SessionError,
                })?
        };
        let mut session = session_lock.write().await;
        let peer_role = match maybe_claims.as_ref().map(|c| c.role.as_str()) {
            Some("server") => PeerRole::Server,
            _ => PeerRole::Client,
        };
        if let Err(e) = session.register_peer(peer_role, wavry_id, src) {
            warn!("Failed to register peer from {}: {}", src, e);
            self.send_lease_reject(header.session_id, src, LeaseRejectReason::SessionFull)
                .await;
            return Err(PacketError::SessionError);
        }
        if let Some(claims) = maybe_claims {
            if let Some(soft) = claims.soft_limit_kbps {
                session.soft_limit_kbps = soft;
            }
            if let Some(hard) = claims.hard_limit_kbps {
                session.hard_limit_kbps = hard;
            }
        }
        let expires = session.lease_expires;
        let soft_limit = session.soft_limit_kbps;
        let hard_limit = session.hard_limit_kbps;
        drop(session);
        self.send_lease_ack(header.session_id, src, expires, soft_limit, hard_limit)
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
        let session_lock = {
            let sessions = self.sessions.read().await;
            sessions
                .get(&header.session_id)
                .ok_or(PacketError::SessionNotFound)?
        };
        let mut session = session_lock.write().await;
        if session.identify_peer(src).is_none() {
            return Err(PacketError::UnknownPeer);
        }
        session.renew_lease(self.lease_duration);
        let expires = session.lease_expires;
        let soft = session.soft_limit_kbps;
        let hard = session.hard_limit_kbps;
        drop(session);
        self.send_lease_ack(header.session_id, src, expires, soft, hard)
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
        let session_lock = {
            let sessions = self.sessions.read().await;
            sessions
                .get(&header.session_id)
                .ok_or(PacketError::SessionNotFound)?
        };
        let mut session = session_lock.write().await;
        if !session.is_active() {
            return Err(PacketError::SessionNotActive);
        }
        let (sender_role, _sender_id, dest) =
            session.identify_peer(src).ok_or(PacketError::UnknownPeer)?;
        let dest_addr = dest.socket_addr;
        let sequence = extract_forward_sequence(payload)?;
        if let Some(sender) = session.get_peer_mut(sender_role) {
            if !sender.seq_window.check_and_update(sequence) {
                return Err(PacketError::ReplayDetected(sequence));
            }
        }
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(session.last_stats_reset).as_secs_f32();
        if elapsed >= 1.0 {
            session.current_bps = (session.bytes_sent_window as f32 / elapsed) * 8.0;
            session.bytes_sent_window = 0;
            session.last_stats_reset = now;
        }
        if session.current_bps > (session.hard_limit_kbps as f32 * 1000.0) {
            return Err(PacketError::RateLimited);
        }
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
        let forward_size = RELAY_HEADER_SIZE + payload.len();
        session.record_forward(forward_size);
        session.bytes_sent_window += forward_size as u64;
        let mut forward_buf = vec![0u8; RELAY_HEADER_SIZE + payload.len()];
        header
            .encode(&mut forward_buf)
            .map_err(|_| PacketError::InvalidHeader)?;
        forward_buf[RELAY_HEADER_SIZE..].copy_from_slice(payload);
        drop(session);
        self.socket.send_to(&forward_buf, dest_addr).await?;
        self.metrics
            .packets_forwarded
            .fetch_add(1, Ordering::Relaxed);
        self.metrics
            .bytes_forwarded
            .fetch_add(forward_buf.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    async fn send_lease_ack(
        &self,
        session_id: uuid::Uuid,
        dest: SocketAddr,
        expires: std::time::Instant,
        soft_limit_kbps: u32,
        hard_limit_kbps: u32,
    ) {
        let header = RelayHeader::new(RelayPacketType::LeaseAck, session_id);
        let expires_ms = expires
            .saturating_duration_since(std::time::Instant::now())
            .as_millis() as u64;
        let unix_expires = chrono::Utc::now().timestamp_millis() as u64 + expires_ms;
        let payload = LeaseAckPayload {
            expires_ms: unix_expires,
            soft_limit_kbps,
            hard_limit_kbps,
        };
        let mut packet = vec![0u8; RELAY_HEADER_SIZE + LeaseAckPayload::SIZE];
        if header.encode(&mut packet).is_err() {
            return;
        }
        if payload.encode(&mut packet[RELAY_HEADER_SIZE..]).is_err() {
            return;
        }
        let _ = self.socket.send_to(&packet, dest).await;
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
        let _ = self.socket.send_to(&packet, dest).await;
    }

    async fn cleanup(&self) {
        let mut sessions = self.sessions.write().await;
        sessions.cleanup().await;
        let mut limiter = self.ip_limiter.write().await;
        limiter.cleanup();
    }

    fn record_packet_error(&self, err: &PacketError, src: SocketAddr) {
        self.metrics.dropped_packets.fetch_add(1, Ordering::Relaxed);
        match err {
            PacketError::RateLimited => {
                self.metrics
                    .rate_limited_packets
                    .fetch_add(1, Ordering::Relaxed);
            }
            PacketError::InvalidSignature => {
                self.metrics
                    .auth_reject_packets
                    .fetch_add(1, Ordering::Relaxed);
                warn!(
                    "Invalid lease signature from {}: Possible unauthorized access attempt",
                    src
                );
            }
            PacketError::ExpiredLease => {
                self.metrics
                    .auth_reject_packets
                    .fetch_add(1, Ordering::Relaxed);
            }
            PacketError::InvalidSize
            | PacketError::InvalidMagic
            | PacketError::InvalidHeader
            | PacketError::InvalidPayload
            | PacketError::InvalidSessionId
            | PacketError::InvalidRole
            | PacketError::UnexpectedType => {
                self.metrics.invalid_packets.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    async fn log_metrics(&self) {
        let active_sessions = self.active_session_count().await;
        let snapshot = self.metrics.snapshot();
        info!(
            "relay metrics active_sessions={} packets_rx={} bytes_rx={} forwarded_packets={} forwarded_bytes={} lease_present={} lease_renew={} dropped={} rate_limited={} invalid={} auth_rejects={}",
            active_sessions,
            snapshot.packets_rx,
            snapshot.bytes_rx,
            snapshot.packets_forwarded,
            snapshot.bytes_forwarded,
            snapshot.lease_present_packets,
            snapshot.lease_renew_packets,
            snapshot.dropped_packets,
            snapshot.rate_limited_packets,
            snapshot.invalid_packets,
            snapshot.auth_reject_packets
        );
    }
}

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
    #[error("invalid session id")]
    InvalidSessionId,
    #[error("expired lease")]
    ExpiredLease,
    #[error("invalid role in lease")]
    InvalidRole,
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
    let filter = format!("{},hyper=warn,tokio=warn", args.log_level);
    tracing_subscriber::fmt().with_env_filter(filter).init();
    info!("Starting wavry-relay v{}", env!("CARGO_PKG_VERSION"));

    let socket = UdpSocket::bind(args.listen).await?;
    let bound_addr = socket.local_addr()?;
    info!("Relay listening on {}", bound_addr);

    let relay_id = Uuid::new_v4();
    info!("Relay ID: {}", relay_id);

    let client = reqwest::Client::new();
    let register_url = format!("{}/v1/relays/register", args.master_url);
    let endpoints = vec![bound_addr.to_string()];

    let mut retry_delay = Duration::from_secs(1);
    let max_retry_delay = Duration::from_secs(60);
    info!("Registering with Master at {}...", args.master_url);
    let reg_data = loop {
        match client
            .post(&register_url)
            .json(&RelayRegisterRequest {
                relay_id: relay_id.to_string(),
                endpoints: endpoints.clone(),
                region: args.region.clone(),
                asn: args.asn,
                max_sessions: Some(args.max_sessions as u32),
                max_bitrate_kbps: Some(args.max_bitrate_kbps),
                features: vec!["ipv4".into()],
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
        tokio::time::sleep(retry_delay).await;
        retry_delay = std::cmp::min(retry_delay * 2, max_retry_delay);
    };
    info!(
        "Registered successfully. Heartbeat interval: {}ms",
        reg_data.heartbeat_interval_ms
    );
    let server = Arc::new(
        RelayServer::new(
            socket,
            args.max_sessions,
            Duration::from_secs(args.idle_timeout),
            Duration::from_secs(args.lease_duration_secs.max(1)),
            Duration::from_secs(args.cleanup_interval_secs.max(1)),
            Duration::from_secs(args.stats_log_interval_secs.max(5)),
            args.ip_rate_limit_pps.max(1),
            args.master_public_key.as_deref(),
            Some(&reg_data.master_public_key),
            args.allow_insecure_dev,
        )
        .await?,
    );
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
            let load = if max_sessions > 0 {
                (active as f32 / max_sessions as f32) * 100.0
            } else {
                100.0
            } as u8;
            let req = RelayHeartbeatRequest {
                relay_id: relay_id.to_string(),
                load_pct: load as f32,
            };
            let _ = client.post(&heartbeat_url).json(&req).send().await;
        }
    });
    server.run().await
}
