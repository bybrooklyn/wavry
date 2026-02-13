//! Relay session management and state machine.
//!
//! Implements the session lifecycle from WAVRY_RELAY.md spec:
//! - INIT: First LEASE_PRESENT received
//! - WAITING_PEER: One peer validated, waiting for other
//! - ACTIVE: Both peers ready, forwarding enabled
//! - EXPIRED: Session ended

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rift_crypto::seq_window::SequenceWindow;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Session state machine states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SessionState {
    /// First LEASE_PRESENT received, validating
    Init,
    /// One peer ready, waiting for other
    WaitingPeer,
    /// Both peers ready, forwarding enabled
    Active,
    /// Lease renewed, transitioning back to ACTIVE
    Renewed,
    /// Session ended, cleaning up
    Expired,
    /// Validation failed
    Rejected,
}

pub use rift_core::relay::PeerRole;

/// Per-peer state within a session
#[derive(Debug)]
#[allow(dead_code)]
pub struct PeerState {
    /// Peer's Wavry ID (from lease)
    pub wavry_id: String,
    /// Last seen socket address (may change due to NAT rebinding)
    pub socket_addr: SocketAddr,
    /// Last activity time
    pub last_seen: Instant,
    /// Sequence window for replay protection
    pub seq_window: SequenceWindow,
}

impl PeerState {
    pub fn new(wavry_id: String, socket_addr: SocketAddr) -> Self {
        Self {
            wavry_id,
            socket_addr,
            last_seen: Instant::now(),
            seq_window: SequenceWindow::new(),
        }
    }
}

/// A relay session between two peers
#[derive(Debug)]
#[allow(dead_code)]
pub struct RelaySession {
    /// Unique session ID (UUID)
    pub session_id: Uuid,
    /// Current state
    pub state: SessionState,
    /// Client peer (if connected)
    pub client: Option<PeerState>,
    /// Server peer (if connected)
    pub server: Option<PeerState>,
    /// Client Wavry ID (from lease)
    pub client_id: Option<String>,
    /// Server Wavry ID (from lease)
    pub server_id: Option<String>,
    /// Lease expiration time
    pub lease_expires: Instant,
    /// Session creation time
    pub created_at: Instant,
    /// Last activity time
    pub last_activity: Instant,
    /// Packets forwarded
    pub packets_forwarded: u64,
    /// Bytes forwarded
    pub bytes_forwarded: u64,
    /// Soft rate limit (kbps)
    pub soft_limit_kbps: u32,
    /// Hard rate limit (kbps)
    pub hard_limit_kbps: u32,
    /// Last stats reset time
    pub last_stats_reset: Instant,
    /// Bytes sent in the current window
    pub bytes_sent_window: u64,
    /// Current bandwidth usage (bits per second)
    pub current_bps: f32,
}

impl RelaySession {
    /// Create a new session
    pub fn new(session_id: Uuid, lease_duration: Duration) -> Self {
        let now = Instant::now();
        Self {
            session_id,
            state: SessionState::Init,
            client: None,
            server: None,
            client_id: None,
            server_id: None,
            lease_expires: now + lease_duration,
            created_at: now,
            last_activity: now,
            packets_forwarded: 0,
            bytes_forwarded: 0,
            soft_limit_kbps: 50_000,
            hard_limit_kbps: 100_000,
            last_stats_reset: now,
            bytes_sent_window: 0,
            current_bps: 0.0,
        }
    }

    /// Register a peer with this session
    pub fn register_peer(
        &mut self,
        role: PeerRole,
        wavry_id: String,
        socket_addr: SocketAddr,
    ) -> Result<(), SessionError> {
        let now = Instant::now();

        // Validate against session-locked IDs if they exist
        match role {
            PeerRole::Client => {
                if let Some(ref locked_id) = self.client_id {
                    if locked_id != &wavry_id {
                        return Err(SessionError::InvalidLease);
                    }
                } else {
                    self.client_id = Some(wavry_id.clone());
                }
            }
            PeerRole::Server => {
                if let Some(ref locked_id) = self.server_id {
                    if locked_id != &wavry_id {
                        return Err(SessionError::InvalidLease);
                    }
                } else {
                    self.server_id = Some(wavry_id.clone());
                }
            }
        }

        // Allow same-ID peer re-registration so NAT rebinding and reconnects can recover
        // without forcing session recreation.
        let slot = match role {
            PeerRole::Client => &mut self.client,
            PeerRole::Server => &mut self.server,
        };

        if let Some(existing) = slot.as_mut() {
            if existing.wavry_id != wavry_id {
                return Err(SessionError::PeerAlreadyRegistered);
            }
            existing.socket_addr = socket_addr;
            existing.last_seen = now;
        } else {
            *slot = Some(PeerState::new(wavry_id, socket_addr));
        }

        // Update state based on how many peers we have
        match (&self.client, &self.server) {
            (Some(_), Some(_)) => self.state = SessionState::Active,
            (Some(_), None) | (None, Some(_)) => self.state = SessionState::WaitingPeer,
            (None, None) => self.state = SessionState::Init,
        }

        self.last_activity = now;
        Ok(())
    }

    /// Check if session is active and can forward packets
    pub fn is_active(&self) -> bool {
        matches!(self.state, SessionState::Active | SessionState::Renewed)
            && Instant::now() < self.lease_expires
    }

    /// Check if session has expired
    pub fn is_expired(&self) -> bool {
        matches!(self.state, SessionState::Expired | SessionState::Rejected)
            || Instant::now() >= self.lease_expires
    }

    /// Identify which peer sent a packet and get the destination
    pub fn identify_peer(&self, src: SocketAddr) -> Option<(PeerRole, &PeerState, &PeerState)> {
        match (&self.client, &self.server) {
            (Some(client), Some(server)) => {
                if client.socket_addr == src {
                    Some((PeerRole::Client, client, server))
                } else if server.socket_addr == src {
                    Some((PeerRole::Server, server, client))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Get mutable peer state for updating
    pub fn get_peer_mut(&mut self, role: PeerRole) -> Option<&mut PeerState> {
        match role {
            PeerRole::Client => self.client.as_mut(),
            PeerRole::Server => self.server.as_mut(),
        }
    }

    /// Update NAT rebinding (peer address changed)
    #[allow(dead_code)]
    pub fn update_peer_address(&mut self, role: PeerRole, new_addr: SocketAddr) {
        if let Some(peer) = self.get_peer_mut(role) {
            peer.socket_addr = new_addr;
            peer.last_seen = Instant::now();
        }
        self.last_activity = Instant::now();
    }

    /// Record forwarded packet stats
    pub fn record_forward(&mut self, bytes: usize) {
        self.packets_forwarded += 1;
        self.bytes_forwarded += bytes as u64;
        self.last_activity = Instant::now();
    }

    /// Renew the lease
    pub fn renew_lease(&mut self, new_duration: Duration) -> Result<(), SessionError> {
        if self.is_expired() {
            self.state = SessionState::Expired;
            return Err(SessionError::LeaseExpired);
        }
        self.lease_expires = Instant::now() + new_duration;
        self.state = if self.client.is_some() && self.server.is_some() {
            SessionState::Renewed
        } else {
            SessionState::WaitingPeer
        };
        self.last_activity = Instant::now();
        Ok(())
    }

    /// Expire the session
    #[allow(dead_code)]
    pub fn expire(&mut self) {
        self.state = SessionState::Expired;
    }
}

/// Session management errors
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum SessionError {
    #[error("peer already registered")]
    PeerAlreadyRegistered,
    #[error("session not active")]
    SessionNotActive,
    #[error("lease expired")]
    LeaseExpired,
    #[error("unknown peer")]
    UnknownPeer,
    #[error("replay detected")]
    ReplayDetected,
    #[error("rate limited")]
    RateLimited,
    #[error("session not found")]
    SessionNotFound,
    #[error("session full")]
    SessionFull,
    #[error("invalid lease")]
    InvalidLease,
}

/// Session pool managing all active sessions
#[derive(Debug)]
pub struct SessionPool {
    sessions: HashMap<Uuid, Arc<RwLock<RelaySession>>>,
    max_sessions: usize,
    session_idle_timeout: Duration,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CleanupStats {
    pub expired_sessions: usize,
    pub idle_sessions: usize,
}

impl CleanupStats {
    pub fn total_removed(self) -> usize {
        self.expired_sessions + self.idle_sessions
    }
}

impl SessionPool {
    /// Create a new session pool
    pub fn new(max_sessions: usize, idle_timeout: Duration) -> Self {
        Self {
            sessions: HashMap::new(),
            max_sessions,
            session_idle_timeout: idle_timeout,
        }
    }

    /// Get or create a session
    pub fn get_or_create(
        &mut self,
        session_id: Uuid,
        lease_duration: Duration,
    ) -> Result<Arc<RwLock<RelaySession>>, SessionError> {
        if !self.sessions.contains_key(&session_id) {
            if self.sessions.len() >= self.max_sessions {
                return Err(SessionError::SessionFull);
            }
            let session = RelaySession::new(session_id, lease_duration);
            self.sessions
                .insert(session_id, Arc::new(RwLock::new(session)));
        }
        Ok(self.sessions.get(&session_id).unwrap().clone())
    }

    /// Get an existing session
    #[allow(dead_code)]
    pub fn get(&self, session_id: &Uuid) -> Option<Arc<RwLock<RelaySession>>> {
        self.sessions.get(session_id).cloned()
    }

    /// Remove a session
    #[allow(dead_code)]
    pub fn remove(&mut self, session_id: &Uuid) -> Option<Arc<RwLock<RelaySession>>> {
        self.sessions.remove(session_id)
    }

    /// Clean up expired and idle sessions
    pub async fn cleanup(&mut self) -> CleanupStats {
        let now = Instant::now();
        let idle_timeout = self.session_idle_timeout;
        let mut expired_ids = Vec::new();
        let mut idle_ids = Vec::new();

        // Identify expired sessions
        for (id, session_lock) in &self.sessions {
            // Use read lock to check status
            let session = session_lock.read().await;
            let expired = session.is_expired();
            let idle = now.duration_since(session.last_activity) > idle_timeout;
            if expired || idle {
                if expired {
                    expired_ids.push(*id);
                } else {
                    idle_ids.push(*id);
                }
            }
        }

        let expired_count = expired_ids.len();
        let idle_count = idle_ids.len();

        for id in expired_ids {
            self.sessions.remove(&id);
        }
        for id in idle_ids {
            self.sessions.remove(&id);
        }

        CleanupStats {
            expired_sessions: expired_count,
            idle_sessions: idle_count,
        }
    }

    /// Get session count
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn max_sessions(&self) -> usize {
        self.max_sessions
    }

    pub fn contains(&self, session_id: &Uuid) -> bool {
        self.sessions.contains_key(session_id)
    }

    /// Get active session count
    pub async fn active_count(&self) -> usize {
        let mut count = 0;
        for session_lock in self.sessions.values() {
            let session = session_lock.read().await;
            if matches!(session.state, SessionState::Active | SessionState::Renewed) {
                count += 1;
            }
        }
        count
    }

    /// Check if empty
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    #[test]
    fn register_peer_allows_nat_rebind_for_same_identity() {
        let session_id = Uuid::new_v4();
        let mut session = RelaySession::new(session_id, Duration::from_secs(60));

        session
            .register_peer(PeerRole::Client, "client-a".to_string(), addr(41000))
            .expect("client register");
        assert_eq!(session.state, SessionState::WaitingPeer);

        session
            .register_peer(PeerRole::Server, "server-a".to_string(), addr(42000))
            .expect("server register");
        assert_eq!(session.state, SessionState::Active);

        session
            .register_peer(PeerRole::Client, "client-a".to_string(), addr(43000))
            .expect("client rebinding register");
        assert_eq!(
            session
                .client
                .as_ref()
                .expect("client present after rebinding")
                .socket_addr,
            addr(43000)
        );
        assert!(session.is_active());
    }

    #[test]
    fn register_peer_rejects_role_identity_swap() {
        let session_id = Uuid::new_v4();
        let mut session = RelaySession::new(session_id, Duration::from_secs(60));

        session
            .register_peer(PeerRole::Client, "client-a".to_string(), addr(41000))
            .expect("client register");

        let err = session
            .register_peer(PeerRole::Client, "client-b".to_string(), addr(42000))
            .expect_err("expected identity swap rejection");
        assert!(matches!(err, SessionError::InvalidLease));
    }

    #[test]
    fn renew_expired_session_fails() {
        let session_id = Uuid::new_v4();
        let mut session = RelaySession::new(session_id, Duration::from_secs(0));

        let err = session
            .renew_lease(Duration::from_secs(30))
            .expect_err("expired lease renew should fail");
        assert!(matches!(err, SessionError::LeaseExpired));
        assert_eq!(session.state, SessionState::Expired);
    }

    #[tokio::test]
    async fn cleanup_reports_expired_and_idle_sessions() {
        let mut pool = SessionPool::new(8, Duration::from_secs(5));

        let expired_id = Uuid::new_v4();
        let expired = pool
            .get_or_create(expired_id, Duration::from_secs(0))
            .expect("create expired");
        {
            let mut guard = expired.write().await;
            guard.expire();
        }

        let idle_id = Uuid::new_v4();
        let idle = pool
            .get_or_create(idle_id, Duration::from_secs(120))
            .expect("create idle");
        {
            let mut guard = idle.write().await;
            guard.last_activity = Instant::now() - Duration::from_secs(10);
        }

        let cleanup = pool.cleanup().await;
        assert_eq!(cleanup.expired_sessions, 1);
        assert_eq!(cleanup.idle_sessions, 1);
        assert_eq!(cleanup.total_removed(), 2);
        assert!(pool.is_empty());
    }
}
