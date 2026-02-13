#![forbid(unsafe_code)]

mod session;

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use bytes::Bytes;
use clap::Parser;
use rift_core::relay::{
    ForwardPayloadHeader, LeaseAckPayload, LeaseRejectPayload, LeaseRejectReason, RelayHeader,
    RelayPacketType, RELAY_HEADER_SIZE, RELAY_MAX_PACKET_SIZE,
};
use rift_core::PhysicalPacket;
use serde::{Deserialize, Serialize};
use session::{PeerRole, SessionError, SessionPool};
use tokio::net::{TcpListener, UdpSocket};
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
const DEFAULT_LOAD_SHED_THRESHOLD_PCT: u8 = 95;
const DEFAULT_HEALTH_LISTEN: &str = "127.0.0.1:9091";
const MAX_CLOCK_SKEW_SECS: i64 = 30;
const MAX_LEASE_HORIZON_SECS: i64 = 3600;
const MAX_LEASE_TOKEN_BYTES: usize = 8192;

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

    /// Bearer token for authenticated relay register/heartbeat requests.
    #[arg(long, env = "WAVRY_RELAY_MASTER_TOKEN")]
    master_auth_token: Option<String>,

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

    /// Threshold (percent of max sessions) where new sessions are shed early.
    #[arg(long, default_value_t = DEFAULT_LOAD_SHED_THRESHOLD_PCT)]
    load_shed_threshold_pct: u8,

    /// HTTP listen address for health/readiness/metrics endpoints.
    #[arg(long, env = "WAVRY_RELAY_HEALTH_LISTEN", default_value = DEFAULT_HEALTH_LISTEN)]
    health_listen: SocketAddr,

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

fn running_in_container() -> bool {
    std::path::Path::new("/.dockerenv").exists()
        || std::path::Path::new("/run/.containerenv").exists()
        || std::env::var_os("container").is_some()
}

fn with_master_auth(
    request: reqwest::RequestBuilder,
    master_auth_token: Option<&str>,
) -> reqwest::RequestBuilder {
    if let Some(token) = master_auth_token {
        request.bearer_auth(token)
    } else {
        request
    }
}

#[derive(Clone)]
struct MasterRegistrationConfig {
    register_url: String,
    relay_id: String,
    endpoints: Vec<String>,
    region: Option<String>,
    asn: Option<u32>,
    max_sessions: usize,
    max_bitrate_kbps: u32,
    master_auth_token: Option<String>,
}

async fn register_with_master(
    client: &reqwest::Client,
    config: &MasterRegistrationConfig,
) -> RelayRegisterResponse {
    let mut retry_delay = Duration::from_secs(1);
    let max_retry_delay = Duration::from_secs(60);
    loop {
        let request = RelayRegisterRequest {
            relay_id: config.relay_id.clone(),
            endpoints: config.endpoints.clone(),
            region: config.region.clone(),
            asn: config.asn,
            max_sessions: Some(config.max_sessions as u32),
            max_bitrate_kbps: Some(config.max_bitrate_kbps),
            features: vec!["ipv4".into()],
        };
        match with_master_auth(
            client.post(&config.register_url),
            config.master_auth_token.as_deref(),
        )
        .json(&request)
        .send()
        .await
        {
            Ok(resp) => {
                if resp.status().is_success() {
                    match resp.json::<RelayRegisterResponse>().await {
                        Ok(data) => return data,
                        Err(err) => warn!("failed to parse master registration response: {}", err),
                    }
                } else {
                    warn!("master registration failed with status: {}", resp.status());
                }
            }
            Err(err) => warn!("failed to connect to master: {}", err),
        }
        tokio::time::sleep(retry_delay).await;
        retry_delay = std::cmp::min(retry_delay * 2, max_retry_delay);
    }
}

/// Per-source-IP packet rate limiter to prevent abuse.
///
/// Uses a simple fixed-window algorithm with a 1-second window.
/// IP addresses that exceed the configured packets-per-second limit
/// are throttled until the window resets.
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
    #[serde(rename = "rid")]
    relay_id: Option<String>,
    #[serde(rename = "kid")]
    key_id: Option<String>,
    #[serde(rename = "iat_rfc3339")]
    issued_at: Option<String>,
    #[serde(rename = "nbf_rfc3339")]
    not_before: Option<String>,
    #[serde(rename = "exp_rfc3339")]
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
    session_not_found_packets: AtomicU64,
    session_not_active_packets: AtomicU64,
    unknown_peer_packets: AtomicU64,
    replay_dropped_packets: AtomicU64,
    session_full_rejects: AtomicU64,
    wrong_relay_rejects: AtomicU64,
    expired_lease_rejects: AtomicU64,
    cleanup_expired_sessions: AtomicU64,
    cleanup_idle_sessions: AtomicU64,
    overload_shed_packets: AtomicU64,
    nat_rebind_events: AtomicU64,
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
    session_not_found_packets: u64,
    session_not_active_packets: u64,
    unknown_peer_packets: u64,
    replay_dropped_packets: u64,
    session_full_rejects: u64,
    wrong_relay_rejects: u64,
    expired_lease_rejects: u64,
    cleanup_expired_sessions: u64,
    cleanup_idle_sessions: u64,
    overload_shed_packets: u64,
    nat_rebind_events: u64,
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
            session_not_found_packets: self.session_not_found_packets.load(Ordering::Relaxed),
            session_not_active_packets: self.session_not_active_packets.load(Ordering::Relaxed),
            unknown_peer_packets: self.unknown_peer_packets.load(Ordering::Relaxed),
            replay_dropped_packets: self.replay_dropped_packets.load(Ordering::Relaxed),
            session_full_rejects: self.session_full_rejects.load(Ordering::Relaxed),
            wrong_relay_rejects: self.wrong_relay_rejects.load(Ordering::Relaxed),
            expired_lease_rejects: self.expired_lease_rejects.load(Ordering::Relaxed),
            cleanup_expired_sessions: self.cleanup_expired_sessions.load(Ordering::Relaxed),
            cleanup_idle_sessions: self.cleanup_idle_sessions.load(Ordering::Relaxed),
            overload_shed_packets: self.overload_shed_packets.load(Ordering::Relaxed),
            nat_rebind_events: self.nat_rebind_events.load(Ordering::Relaxed),
        }
    }
}

/// The core relay server responsible for forwarding encrypted UDP packets between peers.
///
/// # Overview
/// The relay server acts as a transparent packet forwarder that:
/// - Validates PASETO v4 session leases from the Master server
/// - Maintains per-session state with replay protection
/// - Enforces bandwidth limits and rate limiting
/// - Provides load shedding when capacity is exceeded
/// - Exports metrics for monitoring
///
/// # Security
/// All forwarded data is end-to-end encrypted. The relay never decrypts packet contents.
/// Authentication is based on cryptographically signed leases (PASETO tokens) issued
/// by the Master server.
///
/// # Load Management
/// When active sessions exceed the configured threshold (default 95%), new session
/// requests are rejected to maintain service quality for existing sessions.
struct RelayServer {
    relay_id: String,
    socket: UdpSocket,
    sessions: RwLock<SessionPool>,
    ip_limiter: RwLock<IpRateLimiter>,
    max_sessions: usize,
    load_shed_threshold_pct: u8,
    lease_duration: Duration,
    cleanup_interval: Duration,
    stats_log_interval: Duration,
    metrics: RelayMetrics,
    master_public_key: Option<pasetors::keys::AsymmetricPublicKey<pasetors::version4::V4>>,
    expected_master_key_id: Option<String>,
    registered_with_master: AtomicBool,
    started_at: Instant,
}

impl RelayServer {
    #[allow(clippy::too_many_arguments)]
    async fn new(
        relay_id: String,
        socket: UdpSocket,
        max_sessions: usize,
        idle_timeout: Duration,
        lease_duration: Duration,
        cleanup_interval: Duration,
        stats_log_interval: Duration,
        load_shed_threshold_pct: u8,
        ip_rate_limit_pps: u64,
        master_key_hex: Option<&str>,
        registration_master_key: Option<&[u8]>,
        expected_master_key_id: Option<String>,
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
            relay_id,
            socket,
            sessions: RwLock::new(SessionPool::new(max_sessions, idle_timeout)),
            ip_limiter: RwLock::new(IpRateLimiter::new(ip_rate_limit_pps.max(1))),
            max_sessions: max_sessions.max(1),
            load_shed_threshold_pct: load_shed_threshold_pct.clamp(50, 100),
            lease_duration,
            cleanup_interval,
            stats_log_interval,
            metrics: RelayMetrics::default(),
            master_public_key,
            expected_master_key_id,
            registered_with_master: AtomicBool::new(true),
            started_at: Instant::now(),
        })
    }

    async fn active_session_count(&self) -> usize {
        self.sessions.read().await.active_count().await
    }

    async fn total_session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    fn has_master_key(&self) -> bool {
        self.master_public_key.is_some()
    }

    async fn is_ready(&self) -> bool {
        if !self.has_master_key() {
            return false;
        }
        if !self.registered_with_master.load(Ordering::Relaxed) {
            return false;
        }
        let sessions = self.sessions.read().await;
        let used = sessions.len();
        let threshold = ((self.max_sessions as u64 * self.load_shed_threshold_pct as u64) / 100)
            .max(1) as usize;
        used < threshold
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
        let header = RelayHeader::decode(packet).map_err(|_| PacketError::InvalidHeader)?;
        if header.session_id.is_nil() {
            return Err(PacketError::InvalidSessionId);
        }

        {
            let mut limiter = self.ip_limiter.write().await;
            if !limiter.check(src.ip()) {
                if matches!(
                    header.packet_type,
                    RelayPacketType::LeasePresent | RelayPacketType::LeaseRenew
                ) {
                    self.send_lease_reject(header.session_id, src, LeaseRejectReason::RateLimited)
                        .await;
                }
                return Err(PacketError::RateLimited);
            }
        }

        if matches!(header.packet_type, RelayPacketType::LeasePresent)
            && self.should_shed_new_session(header.session_id).await
        {
            self.send_lease_reject(header.session_id, src, LeaseRejectReason::SessionFull)
                .await;
            return Err(PacketError::Overloaded);
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

    async fn should_shed_new_session(&self, session_id: Uuid) -> bool {
        let sessions = self.sessions.read().await;
        if sessions.contains(&session_id) {
            return false;
        }
        let threshold = ((sessions.max_sessions() as u64 * self.load_shed_threshold_pct as u64)
            / 100)
            .max(1) as usize;
        sessions.len() >= threshold
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
        if payload.lease_token.is_empty() || payload.lease_token.len() > MAX_LEASE_TOKEN_BYTES {
            self.send_lease_reject(header.session_id, src, LeaseRejectReason::InvalidSignature)
                .await;
            return Err(PacketError::InvalidPayload);
        }

        let mut maybe_claims = None;
        let mut peer_role = payload.peer_role;
        let wavry_id = if let Some(ref master_key) = self.master_public_key {
            let token_str =
                String::from_utf8(payload.lease_token).map_err(|_| PacketError::InvalidPayload)?;
            let validation_rules = pasetors::claims::ClaimsValidationRules::new();
            let untrusted_token = match pasetors::token::UntrustedToken::<
                pasetors::token::Public,
                pasetors::version4::V4,
            >::try_from(&token_str)
            {
                Ok(token) => token,
                Err(_) => {
                    self.send_lease_reject(
                        header.session_id,
                        src,
                        LeaseRejectReason::InvalidSignature,
                    )
                    .await;
                    return Err(PacketError::InvalidSignature);
                }
            };
            let claims = match pasetors::public::verify(
                master_key,
                &untrusted_token,
                &validation_rules,
                None,
                None,
            ) {
                Ok(claims) => claims,
                Err(_) => {
                    self.send_lease_reject(
                        header.session_id,
                        src,
                        LeaseRejectReason::InvalidSignature,
                    )
                    .await;
                    return Err(PacketError::InvalidSignature);
                }
            };
            let claims_json = decode_lease_claims_value(claims.payload().into())
                .map_err(|_| PacketError::InvalidPayload)?;
            let validated = match validate_lease_claims(
                &claims_json,
                header.session_id,
                &self.relay_id,
                self.expected_master_key_id.as_deref(),
                payload.peer_role,
            ) {
                Ok(validated) => validated,
                Err(PacketError::ExpiredLease) => {
                    self.send_lease_reject(header.session_id, src, LeaseRejectReason::Expired)
                        .await;
                    return Err(PacketError::ExpiredLease);
                }
                Err(PacketError::WrongRelay) => {
                    self.send_lease_reject(header.session_id, src, LeaseRejectReason::WrongRelay)
                        .await;
                    return Err(PacketError::WrongRelay);
                }
                Err(PacketError::InvalidRole | PacketError::KeyIdMismatch) => {
                    self.send_lease_reject(
                        header.session_id,
                        src,
                        LeaseRejectReason::InvalidSignature,
                    )
                    .await;
                    return Err(PacketError::InvalidSignature);
                }
                Err(other) => return Err(other),
            };
            peer_role = validated.peer_role;
            maybe_claims = Some(claims_json);
            validated.wavry_id
        } else {
            format!("dev-peer-{}", src)
        };
        let session_lock = {
            let mut sessions = self.sessions.write().await;
            match sessions.get_or_create(header.session_id, self.lease_duration) {
                Ok(lock) => lock,
                Err(SessionError::SessionFull) => {
                    self.send_lease_reject(header.session_id, src, LeaseRejectReason::SessionFull)
                        .await;
                    return Err(PacketError::SessionFull);
                }
                Err(_) => return Err(PacketError::SessionError),
            }
        };
        let mut session = session_lock.write().await;
        if let Err(e) = session.register_peer(peer_role, wavry_id, src) {
            warn!("Failed to register peer from {}: {}", src, e);
            let reject_reason = match e {
                SessionError::InvalidLease | SessionError::PeerAlreadyRegistered => {
                    LeaseRejectReason::InvalidSignature
                }
                SessionError::SessionFull => LeaseRejectReason::SessionFull,
                _ => LeaseRejectReason::WrongRelay,
            };
            self.send_lease_reject(header.session_id, src, reject_reason)
                .await;
            return Err(PacketError::SessionError);
        }
        if let Some(claims) = maybe_claims {
            if let Some(soft) = claims.soft_limit_kbps {
                session.soft_limit_kbps = soft.max(1_000);
            }
            if let Some(hard) = claims.hard_limit_kbps {
                session.hard_limit_kbps = hard.max(session.soft_limit_kbps);
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
            match sessions.get(&header.session_id) {
                Some(session) => session,
                None => {
                    self.send_lease_reject(header.session_id, src, LeaseRejectReason::Expired)
                        .await;
                    return Err(PacketError::SessionNotFound);
                }
            }
        };
        let mut session = session_lock.write().await;
        if session.identify_peer(src).is_none() {
            self.send_lease_reject(header.session_id, src, LeaseRejectReason::InvalidSignature)
                .await;
            return Err(PacketError::UnknownPeer);
        }
        if let Err(err) = session.renew_lease(self.lease_duration) {
            match err {
                SessionError::LeaseExpired => {
                    self.send_lease_reject(header.session_id, src, LeaseRejectReason::Expired)
                        .await;
                    return Err(PacketError::ExpiredLease);
                }
                _ => return Err(PacketError::SessionError),
            }
        }
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
                self.metrics
                    .nat_rebind_events
                    .fetch_add(1, Ordering::Relaxed);
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
        let cleanup = sessions.cleanup().await;
        if cleanup.total_removed() > 0 {
            self.metrics
                .cleanup_expired_sessions
                .fetch_add(cleanup.expired_sessions as u64, Ordering::Relaxed);
            self.metrics
                .cleanup_idle_sessions
                .fetch_add(cleanup.idle_sessions as u64, Ordering::Relaxed);
            debug!(
                "relay cleanup removed expired={} idle={}",
                cleanup.expired_sessions, cleanup.idle_sessions
            );
        }
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
                self.metrics
                    .expired_lease_rejects
                    .fetch_add(1, Ordering::Relaxed);
            }
            PacketError::SessionNotFound => {
                self.metrics
                    .session_not_found_packets
                    .fetch_add(1, Ordering::Relaxed);
            }
            PacketError::SessionNotActive => {
                self.metrics
                    .session_not_active_packets
                    .fetch_add(1, Ordering::Relaxed);
            }
            PacketError::UnknownPeer => {
                self.metrics
                    .unknown_peer_packets
                    .fetch_add(1, Ordering::Relaxed);
            }
            PacketError::ReplayDetected(_) => {
                self.metrics
                    .replay_dropped_packets
                    .fetch_add(1, Ordering::Relaxed);
            }
            PacketError::SessionFull => {
                self.metrics
                    .session_full_rejects
                    .fetch_add(1, Ordering::Relaxed);
            }
            PacketError::WrongRelay => {
                self.metrics
                    .wrong_relay_rejects
                    .fetch_add(1, Ordering::Relaxed);
            }
            PacketError::Overloaded => {
                self.metrics
                    .overload_shed_packets
                    .fetch_add(1, Ordering::Relaxed);
            }
            PacketError::InvalidSize
            | PacketError::InvalidMagic
            | PacketError::InvalidHeader
            | PacketError::InvalidPayload
            | PacketError::InvalidSessionId
            | PacketError::InvalidRole
            | PacketError::UnexpectedType
            | PacketError::KeyIdMismatch => {
                self.metrics.invalid_packets.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    async fn log_metrics(&self) {
        let active_sessions = self.active_session_count().await;
        let total_sessions = self.total_session_count().await;
        let snapshot = self.metrics.snapshot();
        info!(
            "relay metrics relay_id={} active_sessions={} total_sessions={} packets_rx={} bytes_rx={} forwarded_packets={} forwarded_bytes={} lease_present={} lease_renew={} dropped={} rate_limited={} invalid={} auth_rejects={} session_not_found={} session_not_active={} unknown_peer={} replay_drops={} session_full={} wrong_relay={} expired_leases={} cleanup_expired={} cleanup_idle={} overload_shed={} nat_rebinds={}",
            self.relay_id,
            active_sessions,
            total_sessions,
            snapshot.packets_rx,
            snapshot.bytes_rx,
            snapshot.packets_forwarded,
            snapshot.bytes_forwarded,
            snapshot.lease_present_packets,
            snapshot.lease_renew_packets,
            snapshot.dropped_packets,
            snapshot.rate_limited_packets,
            snapshot.invalid_packets,
            snapshot.auth_reject_packets,
            snapshot.session_not_found_packets,
            snapshot.session_not_active_packets,
            snapshot.unknown_peer_packets,
            snapshot.replay_dropped_packets,
            snapshot.session_full_rejects,
            snapshot.wrong_relay_rejects,
            snapshot.expired_lease_rejects,
            snapshot.cleanup_expired_sessions,
            snapshot.cleanup_idle_sessions,
            snapshot.overload_shed_packets,
            snapshot.nat_rebind_events
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
    #[error("wrong relay for lease")]
    WrongRelay,
    #[error("lease key id mismatch")]
    KeyIdMismatch,
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
    #[error("relay overloaded, shedding new session")]
    Overloaded,
    #[error("session error")]
    SessionError,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
struct ValidatedLease {
    wavry_id: String,
    peer_role: PeerRole,
}

fn parse_claim_time(value: &str) -> Result<chrono::DateTime<chrono::Utc>, PacketError> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|_| PacketError::InvalidPayload)
}

fn decode_lease_claims_value(value: serde_json::Value) -> Result<LeaseClaims, serde_json::Error> {
    match value {
        serde_json::Value::String(raw) => serde_json::from_str(&raw),
        other => serde_json::from_value(other),
    }
}

fn validate_lease_claims(
    claims: &LeaseClaims,
    expected_session_id: Uuid,
    expected_relay_id: &str,
    expected_key_id: Option<&str>,
    requested_role: PeerRole,
) -> Result<ValidatedLease, PacketError> {
    if claims.session_id.is_nil() {
        return Err(PacketError::InvalidSessionId);
    }
    if claims.session_id != expected_session_id {
        return Err(PacketError::InvalidPayload);
    }
    if claims.wavry_id.trim().is_empty() {
        return Err(PacketError::InvalidPayload);
    }

    let lease_role = match claims.role.as_str() {
        "client" => PeerRole::Client,
        "server" => PeerRole::Server,
        _ => return Err(PacketError::InvalidRole),
    };
    if lease_role != requested_role {
        return Err(PacketError::InvalidRole);
    }

    if let Some(relay_id) = claims.relay_id.as_deref() {
        if relay_id != expected_relay_id {
            return Err(PacketError::WrongRelay);
        }
    } else {
        return Err(PacketError::WrongRelay);
    }

    if let Some(expected_kid) = expected_key_id {
        if claims.key_id.as_deref() != Some(expected_kid) {
            return Err(PacketError::KeyIdMismatch);
        }
    }

    let now = chrono::Utc::now();
    let skew = chrono::Duration::seconds(MAX_CLOCK_SKEW_SECS);
    let max_horizon = chrono::Duration::seconds(MAX_LEASE_HORIZON_SECS);

    let exp = parse_claim_time(&claims.expiration)?;
    if exp <= now - skew {
        return Err(PacketError::ExpiredLease);
    }
    if exp > now + max_horizon {
        return Err(PacketError::InvalidPayload);
    }

    if let Some(nbf_raw) = claims.not_before.as_deref() {
        let nbf = parse_claim_time(nbf_raw)?;
        if nbf > now + skew {
            return Err(PacketError::InvalidPayload);
        }
    }

    if let Some(iat_raw) = claims.issued_at.as_deref() {
        let iat = parse_claim_time(iat_raw)?;
        if iat > now + skew {
            return Err(PacketError::InvalidPayload);
        }
        if exp <= iat {
            return Err(PacketError::InvalidPayload);
        }
    }

    Ok(ValidatedLease {
        wavry_id: claims.wavry_id.clone(),
        peer_role: lease_role,
    })
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

#[derive(Clone)]
struct RelayHttpState {
    server: Arc<RelayServer>,
}

#[derive(Debug, Serialize)]
struct RelayStatusResponse {
    relay_id: String,
    status: &'static str,
    ready: bool,
    has_master_key: bool,
    registered_with_master: bool,
    active_sessions: usize,
    total_sessions: usize,
    max_sessions: usize,
    uptime_secs: u64,
    metrics: RelayMetricsSnapshot,
}

async fn relay_health(State(state): State<RelayHttpState>) -> impl IntoResponse {
    let active_sessions = state.server.active_session_count().await;
    let total_sessions = state.server.total_session_count().await;
    let metrics = state.server.metrics.snapshot();
    let response = RelayStatusResponse {
        relay_id: state.server.relay_id.clone(),
        status: "ok",
        ready: state.server.is_ready().await,
        has_master_key: state.server.has_master_key(),
        registered_with_master: state.server.registered_with_master.load(Ordering::Relaxed),
        active_sessions,
        total_sessions,
        max_sessions: state.server.max_sessions,
        uptime_secs: state.server.started_at.elapsed().as_secs(),
        metrics,
    };
    (StatusCode::OK, Json(response))
}

async fn relay_ready(State(state): State<RelayHttpState>) -> impl IntoResponse {
    let ready = state.server.is_ready().await;
    let code = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        code,
        Json(serde_json::json!({
            "relay_id": state.server.relay_id.clone(),
            "ready": ready
        })),
    )
}

async fn relay_metrics(State(state): State<RelayHttpState>) -> impl IntoResponse {
    (StatusCode::OK, Json(state.server.metrics.snapshot()))
}

async fn relay_metrics_prometheus(State(state): State<RelayHttpState>) -> impl IntoResponse {
    let snapshot = state.server.metrics.snapshot();
    let relay_id = &state.server.relay_id;
    let active_sessions = state.server.active_session_count().await;

    let prometheus_text = format!(
        r#"# HELP wavry_relay_packets_rx Total packets received
# TYPE wavry_relay_packets_rx counter
wavry_relay_packets_rx{{relay_id="{relay_id}"}} {packets_rx}
# HELP wavry_relay_bytes_rx Total bytes received
# TYPE wavry_relay_bytes_rx counter
wavry_relay_bytes_rx{{relay_id="{relay_id}"}} {bytes_rx}
# HELP wavry_relay_packets_forwarded Total packets forwarded
# TYPE wavry_relay_packets_forwarded counter
wavry_relay_packets_forwarded{{relay_id="{relay_id}"}} {packets_forwarded}
# HELP wavry_relay_bytes_forwarded Total bytes forwarded
# TYPE wavry_relay_bytes_forwarded counter
wavry_relay_bytes_forwarded{{relay_id="{relay_id}"}} {bytes_forwarded}
# HELP wavry_relay_lease_present_packets Lease present packets received
# TYPE wavry_relay_lease_present_packets counter
wavry_relay_lease_present_packets{{relay_id="{relay_id}"}} {lease_present_packets}
# HELP wavry_relay_lease_renew_packets Lease renew packets received
# TYPE wavry_relay_lease_renew_packets counter
wavry_relay_lease_renew_packets{{relay_id="{relay_id}"}} {lease_renew_packets}
# HELP wavry_relay_dropped_packets Total packets dropped
# TYPE wavry_relay_dropped_packets counter
wavry_relay_dropped_packets{{relay_id="{relay_id}"}} {dropped_packets}
# HELP wavry_relay_rate_limited_packets Packets dropped due to rate limiting
# TYPE wavry_relay_rate_limited_packets counter
wavry_relay_rate_limited_packets{{relay_id="{relay_id}"}} {rate_limited_packets}
# HELP wavry_relay_invalid_packets Invalid packets received
# TYPE wavry_relay_invalid_packets counter
wavry_relay_invalid_packets{{relay_id="{relay_id}"}} {invalid_packets}
# HELP wavry_relay_auth_reject_packets Packets rejected due to auth failure
# TYPE wavry_relay_auth_reject_packets counter
wavry_relay_auth_reject_packets{{relay_id="{relay_id}"}} {auth_reject_packets}
# HELP wavry_relay_session_not_found_packets Packets for unknown sessions
# TYPE wavry_relay_session_not_found_packets counter
wavry_relay_session_not_found_packets{{relay_id="{relay_id}"}} {session_not_found_packets}
# HELP wavry_relay_session_not_active_packets Packets for inactive sessions
# TYPE wavry_relay_session_not_active_packets counter
wavry_relay_session_not_active_packets{{relay_id="{relay_id}"}} {session_not_active_packets}
# HELP wavry_relay_unknown_peer_packets Packets from unknown peers
# TYPE wavry_relay_unknown_peer_packets counter
wavry_relay_unknown_peer_packets{{relay_id="{relay_id}"}} {unknown_peer_packets}
# HELP wavry_relay_replay_dropped_packets Packets dropped due to replay detection
# TYPE wavry_relay_replay_dropped_packets counter
wavry_relay_replay_dropped_packets{{relay_id="{relay_id}"}} {replay_dropped_packets}
# HELP wavry_relay_session_full_rejects Session creations rejected (capacity)
# TYPE wavry_relay_session_full_rejects counter
wavry_relay_session_full_rejects{{relay_id="{relay_id}"}} {session_full_rejects}
# HELP wavry_relay_wrong_relay_rejects Packets for wrong relay
# TYPE wavry_relay_wrong_relay_rejects counter
wavry_relay_wrong_relay_rejects{{relay_id="{relay_id}"}} {wrong_relay_rejects}
# HELP wavry_relay_expired_lease_rejects Packets with expired leases
# TYPE wavry_relay_expired_lease_rejects counter
wavry_relay_expired_lease_rejects{{relay_id="{relay_id}"}} {expired_lease_rejects}
# HELP wavry_relay_cleanup_expired_sessions Sessions cleaned up (expired)
# TYPE wavry_relay_cleanup_expired_sessions counter
wavry_relay_cleanup_expired_sessions{{relay_id="{relay_id}"}} {cleanup_expired_sessions}
# HELP wavry_relay_cleanup_idle_sessions Sessions cleaned up (idle)
# TYPE wavry_relay_cleanup_idle_sessions counter
wavry_relay_cleanup_idle_sessions{{relay_id="{relay_id}"}} {cleanup_idle_sessions}
# HELP wavry_relay_overload_shed_packets Packets shed due to overload
# TYPE wavry_relay_overload_shed_packets counter
wavry_relay_overload_shed_packets{{relay_id="{relay_id}"}} {overload_shed_packets}
# HELP wavry_relay_nat_rebind_events NAT rebinding events
# TYPE wavry_relay_nat_rebind_events counter
wavry_relay_nat_rebind_events{{relay_id="{relay_id}"}} {nat_rebind_events}
# HELP wavry_relay_active_sessions Current number of active sessions
# TYPE wavry_relay_active_sessions gauge
wavry_relay_active_sessions{{relay_id="{relay_id}"}} {active_sessions}
# HELP wavry_relay_uptime_seconds Relay uptime in seconds
# TYPE wavry_relay_uptime_seconds gauge
wavry_relay_uptime_seconds{{relay_id="{relay_id}"}} {uptime_seconds}
"#,
        relay_id = relay_id,
        packets_rx = snapshot.packets_rx,
        bytes_rx = snapshot.bytes_rx,
        packets_forwarded = snapshot.packets_forwarded,
        bytes_forwarded = snapshot.bytes_forwarded,
        lease_present_packets = snapshot.lease_present_packets,
        lease_renew_packets = snapshot.lease_renew_packets,
        dropped_packets = snapshot.dropped_packets,
        rate_limited_packets = snapshot.rate_limited_packets,
        invalid_packets = snapshot.invalid_packets,
        auth_reject_packets = snapshot.auth_reject_packets,
        session_not_found_packets = snapshot.session_not_found_packets,
        session_not_active_packets = snapshot.session_not_active_packets,
        unknown_peer_packets = snapshot.unknown_peer_packets,
        replay_dropped_packets = snapshot.replay_dropped_packets,
        session_full_rejects = snapshot.session_full_rejects,
        wrong_relay_rejects = snapshot.wrong_relay_rejects,
        expired_lease_rejects = snapshot.expired_lease_rejects,
        cleanup_expired_sessions = snapshot.cleanup_expired_sessions,
        cleanup_idle_sessions = snapshot.cleanup_idle_sessions,
        overload_shed_packets = snapshot.overload_shed_packets,
        nat_rebind_events = snapshot.nat_rebind_events,
        active_sessions = active_sessions,
        uptime_seconds = state.server.started_at.elapsed().as_secs(),
    );

    (
        StatusCode::OK,
        [("Content-Type", "text/plain; version=0.0.4")],
        prometheus_text,
    )
}

async fn serve_health_http(server: Arc<RelayServer>, listen: SocketAddr) -> Result<()> {
    let app_state = RelayHttpState { server };
    let app = Router::new()
        .route("/health", get(relay_health))
        .route("/ready", get(relay_ready))
        .route("/metrics", get(relay_metrics))
        .route("/metrics/prometheus", get(relay_metrics_prometheus))
        .with_state(app_state);
    let listener = match TcpListener::bind(listen).await {
        Ok(listener) => listener,
        Err(err) if err.kind() == ErrorKind::AddrInUse => {
            let fallback_addr = SocketAddr::new(listen.ip(), 0);
            warn!(
                "relay health bind {} is already in use, falling back to {}",
                listen, fallback_addr
            );
            TcpListener::bind(fallback_addr).await?
        }
        Err(err) => return Err(err.into()),
    };
    let bound_addr = listener.local_addr()?;
    info!("relay health endpoint listening on http://{}", bound_addr);
    axum::serve(listener, app).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if !args.listen.ip().is_loopback() || !args.health_listen.ip().is_loopback() {
        if !env_bool("WAVRY_RELAY_ALLOW_PUBLIC_BIND", false) {
            return Err(anyhow::anyhow!(
                "refusing non-loopback relay bind without WAVRY_RELAY_ALLOW_PUBLIC_BIND=1"
            ));
        }
        if !running_in_container() && !env_bool("WAVRY_RELAY_ALLOW_HOST_PROD_BIND", false) {
            return Err(anyhow::anyhow!(
                "non-loopback relay bind outside containers is unsupported for production; run via container or set WAVRY_RELAY_ALLOW_HOST_PROD_BIND=1 for local override"
            ));
        }
    }
    let filter = format!("{},hyper=warn,tokio=warn", args.log_level);
    tracing_subscriber::fmt().with_env_filter(filter).init();
    info!("Starting wavry-relay v{}", env!("CARGO_PKG_VERSION"));

    let socket = match UdpSocket::bind(args.listen).await {
        Ok(socket) => socket,
        Err(err) if err.kind() == ErrorKind::AddrInUse => {
            let fallback_addr = SocketAddr::new(args.listen.ip(), 0);
            warn!(
                "relay bind {} is already in use, falling back to {}",
                args.listen, fallback_addr
            );
            UdpSocket::bind(fallback_addr).await?
        }
        Err(err) => return Err(err.into()),
    };
    let bound_addr = socket.local_addr()?;
    info!("Relay listening on {}", bound_addr);

    let relay_id = Uuid::new_v4().to_string();
    info!("Relay ID: {}", relay_id);

    let client = reqwest::Client::new();
    let endpoints = vec![bound_addr.to_string()];
    let registration = MasterRegistrationConfig {
        register_url: format!("{}/v1/relays/register", args.master_url),
        relay_id: relay_id.clone(),
        endpoints: endpoints.clone(),
        region: args.region.clone(),
        asn: args.asn,
        max_sessions: args.max_sessions,
        max_bitrate_kbps: args.max_bitrate_kbps,
        master_auth_token: args.master_auth_token.clone(),
    };

    info!("Registering with Master at {}...", args.master_url);
    let reg_data = register_with_master(&client, &registration).await;
    info!(
        "Registered successfully. Heartbeat interval: {}ms",
        reg_data.heartbeat_interval_ms
    );
    let server = Arc::new(
        RelayServer::new(
            relay_id.clone(),
            socket,
            args.max_sessions,
            Duration::from_secs(args.idle_timeout),
            Duration::from_secs(args.lease_duration_secs.max(1)),
            Duration::from_secs(args.cleanup_interval_secs.max(1)),
            Duration::from_secs(args.stats_log_interval_secs.max(5)),
            args.load_shed_threshold_pct,
            args.ip_rate_limit_pps.max(1),
            args.master_public_key.as_deref(),
            Some(&reg_data.master_public_key),
            reg_data.master_key_id.clone(),
            args.allow_insecure_dev,
        )
        .await?,
    );

    let health_server = server.clone();
    let health_listen = args.health_listen;
    tokio::spawn(async move {
        if let Err(err) = serve_health_http(health_server, health_listen).await {
            warn!("relay health endpoint stopped: {}", err);
        }
    });

    let server_clone = server.clone();
    let master_url = args.master_url.clone();
    let max_sessions = args.max_sessions;
    let registration_for_hb = registration.clone();
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let heartbeat_url = format!("{}/v1/relays/heartbeat", master_url);
        let mut interval = tokio::time::interval(Duration::from_millis(
            reg_data.heartbeat_interval_ms.max(500),
        ));
        let mut consecutive_failures = 0u32;
        loop {
            interval.tick().await;
            let active = server_clone.active_session_count().await;
            let load = if max_sessions > 0 {
                (active as f32 / max_sessions as f32) * 100.0
            } else {
                100.0
            } as u8;
            let req = RelayHeartbeatRequest {
                relay_id: registration_for_hb.relay_id.clone(),
                load_pct: load as f32,
            };
            match with_master_auth(
                client.post(&heartbeat_url),
                registration_for_hb.master_auth_token.as_deref(),
            )
            .json(&req)
            .send()
            .await
            {
                Ok(resp) if resp.status().is_success() => {
                    consecutive_failures = 0;
                    server_clone
                        .registered_with_master
                        .store(true, Ordering::Relaxed);
                }
                Ok(resp) => {
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    warn!("relay heartbeat failed with status {}", resp.status());
                    server_clone
                        .registered_with_master
                        .store(false, Ordering::Relaxed);
                    if resp.status().as_u16() == 404 || consecutive_failures >= 6 {
                        info!(
                            "attempting relay re-registration after heartbeat failure (status={}, failures={})",
                            resp.status(),
                            consecutive_failures
                        );
                        let reg_data = register_with_master(&client, &registration_for_hb).await;
                        let next_interval =
                            Duration::from_millis(reg_data.heartbeat_interval_ms.max(500));
                        interval = tokio::time::interval(next_interval);
                        consecutive_failures = 0;
                        server_clone
                            .registered_with_master
                            .store(true, Ordering::Relaxed);
                        info!(
                            "relay re-registered with master; heartbeat interval now {}ms",
                            next_interval.as_millis()
                        );
                    }
                }
                Err(err) => {
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    warn!("relay heartbeat request failed: {}", err);
                    server_clone
                        .registered_with_master
                        .store(false, Ordering::Relaxed);
                    if consecutive_failures >= 6 {
                        info!(
                            "attempting relay re-registration after heartbeat transport errors (failures={})",
                            consecutive_failures
                        );
                        let reg_data = register_with_master(&client, &registration_for_hb).await;
                        let next_interval =
                            Duration::from_millis(reg_data.heartbeat_interval_ms.max(500));
                        interval = tokio::time::interval(next_interval);
                        consecutive_failures = 0;
                        server_clone
                            .registered_with_master
                            .store(true, Ordering::Relaxed);
                        info!(
                            "relay re-registered with master; heartbeat interval now {}ms",
                            next_interval.as_millis()
                        );
                    }
                }
            }
        }
    });

    // Setup graceful shutdown handler
    let shutdown_server = server.clone();
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("Received SIGINT, initiating graceful shutdown...");
                // Log final metrics before shutdown
                let snapshot = shutdown_server.metrics.snapshot();
                let active_sessions = shutdown_server.active_session_count().await;
                info!(
                    "Final metrics: packets_rx={}, packets_forwarded={}, active_sessions={}",
                    snapshot.packets_rx, snapshot.packets_forwarded, active_sessions
                );
            }
            Err(err) => {
                warn!("Failed to listen for shutdown signal: {}", err);
            }
        }
    });

    server.run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_claims(session_id: Uuid) -> LeaseClaims {
        let now = chrono::Utc::now();
        LeaseClaims {
            wavry_id: "user-123".to_string(),
            session_id,
            role: "client".to_string(),
            relay_id: Some("relay-a".to_string()),
            key_id: Some("kid-a".to_string()),
            issued_at: Some(now.to_rfc3339()),
            not_before: Some((now - chrono::Duration::seconds(1)).to_rfc3339()),
            expiration: (now + chrono::Duration::minutes(5)).to_rfc3339(),
            soft_limit_kbps: Some(30_000),
            hard_limit_kbps: Some(60_000),
        }
    }

    #[test]
    fn validate_claims_accepts_valid_lease() {
        let session_id = Uuid::new_v4();
        let claims = build_claims(session_id);
        let validated = validate_lease_claims(
            &claims,
            session_id,
            "relay-a",
            Some("kid-a"),
            PeerRole::Client,
        )
        .expect("valid lease should pass");
        assert_eq!(validated.wavry_id, "user-123");
        assert!(matches!(validated.peer_role, PeerRole::Client));
    }

    #[test]
    fn validate_claims_rejects_wrong_relay() {
        let session_id = Uuid::new_v4();
        let claims = build_claims(session_id);
        let err = validate_lease_claims(
            &claims,
            session_id,
            "relay-b",
            Some("kid-a"),
            PeerRole::Client,
        )
        .expect_err("wrong relay should fail");
        assert!(matches!(err, PacketError::WrongRelay));
    }

    #[test]
    fn validate_claims_rejects_key_id_mismatch() {
        let session_id = Uuid::new_v4();
        let claims = build_claims(session_id);
        let err = validate_lease_claims(
            &claims,
            session_id,
            "relay-a",
            Some("kid-b"),
            PeerRole::Client,
        )
        .expect_err("key id mismatch should fail");
        assert!(matches!(err, PacketError::KeyIdMismatch));
    }

    #[test]
    fn validate_claims_rejects_expired_lease() {
        let session_id = Uuid::new_v4();
        let mut claims = build_claims(session_id);
        claims.expiration = (chrono::Utc::now() - chrono::Duration::minutes(2)).to_rfc3339();
        let err = validate_lease_claims(
            &claims,
            session_id,
            "relay-a",
            Some("kid-a"),
            PeerRole::Client,
        )
        .expect_err("expired lease should fail");
        assert!(matches!(err, PacketError::ExpiredLease));
    }
}
