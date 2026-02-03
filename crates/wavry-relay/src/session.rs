//! Relay session management and state machine.
//!
//! Implements the session lifecycle from WAVRY_RELAY.md spec:
//! - INIT: First LEASE_PRESENT received
//! - WAITING_PEER: One peer validated, waiting for other
//! - ACTIVE: Both peers ready, forwarding enabled
//! - EXPIRED: Session ended

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use rift_crypto::seq_window::SequenceWindow;
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

/// Peer role in a relay session
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerRole {
    Client = 0,
    Server = 1,
}

impl TryFrom<u8> for PeerRole {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(PeerRole::Client),
            1 => Ok(PeerRole::Server),
            _ => Err(()),
        }
    }
}

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
            lease_expires: now + lease_duration,
            created_at: now,
            last_activity: now,
            packets_forwarded: 0,
            bytes_forwarded: 0,
        }
    }

    /// Register a peer with this session
    pub fn register_peer(
        &mut self,
        role: PeerRole,
        wavry_id: String,
        socket_addr: SocketAddr,
    ) -> Result<(), SessionError> {
        let peer = PeerState::new(wavry_id, socket_addr);

        match role {
            PeerRole::Client => {
                if self.client.is_some() {
                    return Err(SessionError::PeerAlreadyRegistered);
                }
                self.client = Some(peer);
            }
            PeerRole::Server => {
                if self.server.is_some() {
                    return Err(SessionError::PeerAlreadyRegistered);
                }
                self.server = Some(peer);
            }
        }

        // Update state based on how many peers we have
        match (&self.client, &self.server) {
            (Some(_), Some(_)) => self.state = SessionState::Active,
            (Some(_), None) | (None, Some(_)) => self.state = SessionState::WaitingPeer,
            (None, None) => self.state = SessionState::Init,
        }

        self.last_activity = Instant::now();
        Ok(())
    }

    /// Check if session is active and can forward packets
    pub fn is_active(&self) -> bool {
        self.state == SessionState::Active && Instant::now() < self.lease_expires
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
    pub fn renew_lease(&mut self, new_duration: Duration) {
        self.lease_expires = Instant::now() + new_duration;
        self.state = SessionState::Active;
        self.last_activity = Instant::now();
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
    sessions: HashMap<Uuid, RelaySession>,
    max_sessions: usize,
    session_idle_timeout: Duration,
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
    ) -> Result<&mut RelaySession, SessionError> {
        if !self.sessions.contains_key(&session_id) {
            if self.sessions.len() >= self.max_sessions {
                return Err(SessionError::SessionFull);
            }
            let session = RelaySession::new(session_id, lease_duration);
            self.sessions.insert(session_id, session);
        }
        Ok(self.sessions.get_mut(&session_id).unwrap())
    }

    /// Get an existing session
    #[allow(dead_code)]
    pub fn get(&self, session_id: &Uuid) -> Option<&RelaySession> {
        self.sessions.get(session_id)
    }

    /// Get a mutable session
    pub fn get_mut(&mut self, session_id: &Uuid) -> Option<&mut RelaySession> {
        self.sessions.get_mut(session_id)
    }

    /// Remove a session
    #[allow(dead_code)]
    pub fn remove(&mut self, session_id: &Uuid) -> Option<RelaySession> {
        self.sessions.remove(session_id)
    }

    /// Clean up expired and idle sessions
    pub fn cleanup(&mut self) -> usize {
        let now = Instant::now();
        let idle_timeout = self.session_idle_timeout;
        let before = self.sessions.len();

        self.sessions.retain(|_, session: &mut RelaySession| {
            let expired = session.is_expired();
            let idle = now.duration_since(session.last_activity) > idle_timeout;
            !expired && !idle
        });

        before - self.sessions.len()
    }

    /// Get session count
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Check if empty
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Get counts by state
    #[allow(dead_code)]
    pub fn state_counts(&self) -> SessionStateCounts {
        let mut counts = SessionStateCounts::default();
        for session in self.sessions.values() {
            match session.state {
                SessionState::Init => counts.init += 1,
                SessionState::WaitingPeer => counts.waiting_peer += 1,
                SessionState::Active | SessionState::Renewed => counts.active += 1,
                SessionState::Expired => counts.expired += 1,
                SessionState::Rejected => counts.rejected += 1,
            }
        }
        counts
    }
}

/// Session state counts for metrics
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct SessionStateCounts {
    pub init: usize,
    pub waiting_peer: usize,
    pub active: usize,
    pub expired: usize,
    pub rejected: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_lifecycle() {
        let session_id = Uuid::new_v4();
        let mut session = RelaySession::new(session_id, Duration::from_secs(300));

        assert_eq!(session.state, SessionState::Init);
        assert!(!session.is_active());

        // Register client
        session
            .register_peer(
                PeerRole::Client,
                "client-id".to_string(),
                "127.0.0.1:5000".parse().unwrap(),
            )
            .unwrap();
        assert_eq!(session.state, SessionState::WaitingPeer);
        assert!(!session.is_active());

        // Register server
        session
            .register_peer(
                PeerRole::Server,
                "server-id".to_string(),
                "127.0.0.1:6000".parse().unwrap(),
            )
            .unwrap();
        assert_eq!(session.state, SessionState::Active);
        assert!(session.is_active());
    }

    #[test]
    fn test_peer_identification() {
        let session_id = Uuid::new_v4();
        let mut session = RelaySession::new(session_id, Duration::from_secs(300));

        let client_addr: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let server_addr: SocketAddr = "127.0.0.1:6000".parse().unwrap();

        session
            .register_peer(PeerRole::Client, "client".to_string(), client_addr)
            .unwrap();
        session
            .register_peer(PeerRole::Server, "server".to_string(), server_addr)
            .unwrap();

        // Identify from client
        let (role, _sender, dest) = session.identify_peer(client_addr).unwrap();
        assert_eq!(role, PeerRole::Client);
        assert_eq!(dest.socket_addr, server_addr);

        // Identify from server
        let (role, _sender, dest) = session.identify_peer(server_addr).unwrap();
        assert_eq!(role, PeerRole::Server);
        assert_eq!(dest.socket_addr, client_addr);

        // Unknown peer
        let unknown: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        assert!(session.identify_peer(unknown).is_none());
    }

    #[test]
    fn test_session_pool() {
        let mut pool = SessionPool::new(10, Duration::from_secs(60));

        let session_id = Uuid::new_v4();
        pool.get_or_create(session_id, Duration::from_secs(300))
            .unwrap();

        assert_eq!(pool.len(), 1);
        assert!(pool.get(&session_id).is_some());
    }

    #[test]
    fn test_session_pool_max_limit() {
        let mut pool = SessionPool::new(2, Duration::from_secs(60));

        pool.get_or_create(Uuid::new_v4(), Duration::from_secs(300))
            .unwrap();
        pool.get_or_create(Uuid::new_v4(), Duration::from_secs(300))
            .unwrap();

        // Third should fail
        let result = pool.get_or_create(Uuid::new_v4(), Duration::from_secs(300));
        assert!(matches!(result, Err(SessionError::SessionFull)));
    }
}
