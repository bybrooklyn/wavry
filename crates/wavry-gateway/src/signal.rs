use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, State,
    },
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
};
use futures::{stream::StreamExt, SinkExt};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};
use uuid::Uuid;

use crate::db;
use crate::relay::{RelayMap, RelaySession};
use crate::security;
use rift_crypto::seq_window::SequenceWindow;

#[cfg(feature = "webtransport-runtime")]
use wavry_web as web_transport;

const WS_OUTBOX_CAPACITY: usize = 128;
const WS_MAX_TEXT_BYTES: usize = 64 * 1024;
const WS_MAX_MESSAGES_PER_MINUTE: u32 = 600;
const MAX_SIGNAL_SDP_BYTES: usize = 32 * 1024;
const MAX_SIGNAL_CANDIDATE_BYTES: usize = 4096;
const WS_BIND_TIMEOUT: Duration = Duration::from_secs(10);

static ACTIVE_WS_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
static IP_CONNECTIONS: Lazy<Mutex<HashMap<std::net::IpAddr, usize>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

use once_cell::sync::Lazy;
use std::sync::Mutex;

#[derive(Clone)]
pub enum Signaler {
    WebSocket(mpsc::Sender<Message>),
    #[cfg(feature = "webtransport-runtime")]
    WebTransport(mpsc::Sender<web_transport::ControlStreamFrame>),
}

impl Signaler {
    pub fn try_send(&self, signal: SignalMessage) -> bool {
        match self {
            Signaler::WebSocket(tx) => {
                if let Ok(json) = serde_json::to_string(&signal) {
                    return tx.try_send(Message::Text(json)).is_ok();
                }
                false
            }
            #[cfg(feature = "webtransport-runtime")]
            Signaler::WebTransport(tx) => {
                let msg = match signal {
                    SignalMessage::Offer {
                        target_username,
                        sdp,
                        ..
                    } => Some(web_transport::ControlMessage::WebRtcOffer {
                        target_username,
                        sdp,
                    }),
                    SignalMessage::Answer {
                        target_username,
                        sdp,
                        ..
                    } => Some(web_transport::ControlMessage::WebRtcAnswer {
                        target_username,
                        sdp,
                    }),
                    SignalMessage::Candidate {
                        target_username,
                        candidate,
                    } => Some(web_transport::ControlMessage::WebRtcCandidate {
                        target_username,
                        candidate,
                    }),
                    _ => None,
                };
                if let Some(msg) = msg {
                    return tx
                        .try_send(web_transport::ControlStreamFrame::Control(msg))
                        .is_ok();
                }
                false
            }
        }
    }

    pub fn try_send_binary(&self, data: Vec<u8>) -> bool {
        match self {
            Signaler::WebSocket(tx) => tx.try_send(Message::Binary(data)).is_ok(),
            #[cfg(feature = "webtransport-runtime")]
            Signaler::WebTransport(_) => false, // WebTransport uses datagrams, handled separately?
                                                // Actually, if we are forwarding TO a WebTransport client, we would use datagrams.
                                                // But Signaler only has ControlStreamFrame sender.
                                                // To support datagrams here, we'd need the datagram sender in Signaler.
        }
    }
}

pub type ConnectionMap = Arc<RwLock<HashMap<String, Signaler>>>;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum SignalMessage {
    Bind {
        token: String,
    },

    #[serde(rename = "OFFER_RIFT")]
    OfferRift {
        target_username: String,
        hello_base64: String,
    },
    #[serde(rename = "ANSWER_RIFT")]
    AnswerRift {
        target_username: String,
        ack_base64: String,
    },

    Offer {
        target_username: String,
        sdp: String,
    },
    Answer {
        target_username: String,
        sdp: String,
    },
    Candidate {
        target_username: String,
        candidate: String,
    },

    #[serde(rename = "REQUEST_RELAY")]
    RequestRelay {
        target_username: String,
    },
    #[serde(rename = "RELAY_CREDENTIALS")]
    RelayCredentials {
        token: String,
        addr: String,
        session_id: Uuid,
    },

    Error {
        message: String,
    },
    Bound,
}

fn to_ws_message(signal: &SignalMessage) -> Option<Message> {
    serde_json::to_string(signal).ok().map(Message::Text)
}

async fn send_signal(tx: &mpsc::Sender<Message>, signal: &SignalMessage) -> bool {
    let Some(message) = to_ws_message(signal) else {
        return false;
    };
    tx.send(message).await.is_ok()
}

fn relay_session_limit() -> usize {
    std::env::var("WAVRY_RELAY_SESSION_LIMIT")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(4096)
}

fn relay_session_ttl() -> Duration {
    Duration::from_secs(
        std::env::var("WAVRY_RELAY_SESSION_TTL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300)
            .max(30),
    )
}

fn relay_public_addr() -> String {
    std::env::var("WAVRY_RELAY_PUBLIC_ADDR").unwrap_or_else(|_| "127.0.0.1:3478".to_string())
}

fn random_relay_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn ws_connection_limit() -> usize {
    std::env::var("WAVRY_WS_MAX_CONNECTIONS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(4096)
}

fn ws_max_per_ip() -> usize {
    std::env::var("WAVRY_WS_MAX_PER_IP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16)
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(connections): State<ConnectionMap>,
    State(relay_sessions): State<RelayMap>,
    State(pool): State<SqlitePool>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    if !security::ws_origin_allowed(origin) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let ip = addr.ip();
    {
        let mut counts = IP_CONNECTIONS.lock().unwrap();
        let count = counts.get(&ip).cloned().unwrap_or(0);
        if count >= ws_max_per_ip() {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
        counts.insert(ip, count + 1);
    }

    if ACTIVE_WS_CONNECTIONS.load(Ordering::Relaxed) >= ws_connection_limit() {
        let mut counts = IP_CONNECTIONS.lock().unwrap();
        if let Some(count) = counts.get_mut(&ip) {
            *count = count.saturating_sub(1);
        }
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }

    ws.max_message_size(WS_MAX_TEXT_BYTES)
        .max_frame_size(WS_MAX_TEXT_BYTES)
        .on_upgrade(move |socket| handle_socket(socket, connections, relay_sessions, pool, addr))
        .into_response()
}

async fn handle_socket(
    stream: WebSocket,
    connections: ConnectionMap,
    relay_sessions: RelayMap,
    pool: SqlitePool,
    addr: SocketAddr,
) {
    ACTIVE_WS_CONNECTIONS.fetch_add(1, Ordering::Relaxed);
    info!("client connecting from {}", addr);
    let (mut sender, mut receiver) = stream.split();
    let (tx, mut rx) = mpsc::channel::<Message>(WS_OUTBOX_CAPACITY);

    let (disconnect_tx, mut disconnect_rx) = mpsc::channel::<()>(1);

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
        let _ = disconnect_tx.try_send(());
    });

    let mut authenticated_username: Option<String> = None;
    let mut message_window_start = Instant::now();
    let mut message_count: u32 = 0;
    let connection_start = Instant::now();

    loop {
        tokio::select! {
            _ = disconnect_rx.recv() => {
                break;
            }
            msg = receiver.next() => {
                let Some(msg) = msg else { break; };
                let Ok(msg) = msg else { break; };

                let now = Instant::now();

                // Bind timeout
                if authenticated_username.is_none() && now.duration_since(connection_start) > WS_BIND_TIMEOUT {
                    let _ = send_signal(&tx, &SignalMessage::Error { message: "Bind timeout".into() }).await;
                    break;
                }

                if now.duration_since(message_window_start) >= Duration::from_secs(60) {
                    message_window_start = now;
                    message_count = 0;
                }
                message_count = message_count.saturating_add(1);
                if message_count > WS_MAX_MESSAGES_PER_MINUTE {
                    let _ = send_signal(
                        &tx,
                        &SignalMessage::Error {
                            message: "Rate limit exceeded".into(),
                        },
                    )
                    .await;
                    break;
                }

                let text = match msg {
                    Message::Text(text) => text,
                    Message::Binary(_) => {
                        let _ = send_signal(
                            &tx,
                            &SignalMessage::Error {
                                message: "Binary messages are not supported".into(),
                            },
                        )
                        .await;
                        break;
                    }
                    Message::Close(_) => break,
                    Message::Ping(_) | Message::Pong(_) => continue,
                };

                if text.len() > WS_MAX_TEXT_BYTES {
                    let _ = send_signal(
                        &tx,
                        &SignalMessage::Error {
                            message: "Message too large".into(),
                        },
                    )
                    .await;
                    break;
                }

                let signal: SignalMessage = match serde_json::from_str(&text) {
                    Ok(signal) => signal,
                    Err(err) => {
                        warn!("invalid JSON from {}: {}", addr, err);
                        let _ = send_signal(
                            &tx,
                            &SignalMessage::Error {
                                message: "Invalid JSON".into(),
                            },
                        )
                        .await;
                        break;
                    }
                };

                match signal {
                    SignalMessage::Bind { token } => {
                        if authenticated_username.is_some() {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Already bound".into(),
                                },
                            )
                            .await;
                            break;
                        }

                        if !security::allow_ws_bind_request(&format!("bind:{}", addr.ip())) {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Bind rate limit exceeded".into(),
                                },
                            )
                            .await;
                            break;
                        }

                        if !security::is_valid_session_token(&token) {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Invalid token format".into(),
                                },
                            )
                            .await;
                            break;
                        }

                        let username = match db::get_username_by_session_token(&pool, &token).await {
                            Ok(Some(username)) => username,
                            Ok(None) => {
                                let _ = send_signal(
                                    &tx,
                                    &SignalMessage::Error {
                                        message: "Invalid Token".into(),
                                    },
                                )
                                .await;
                                break;
                            }
                            Err(err) => {
                                warn!("token lookup failed for {}: {}", addr, err);
                                let _ = send_signal(
                                    &tx,
                                    &SignalMessage::Error {
                                        message: "Token lookup failed".into(),
                                    },
                                )
                                .await;
                                break;
                            }
                        };

                        let replaced = connections
                            .write()
                            .await
                            .insert(username.clone(), Signaler::WebSocket(tx.clone()));
                        if let Some(previous) = replaced {
                            let _ = previous.try_send(SignalMessage::Error {
                                message: "Session replaced by a newer connection".into(),
                            });
                        }

                        authenticated_username = Some(username.clone());
                        let _ = send_signal(&tx, &SignalMessage::Bound).await;
                        info!("bound signaling session for user {}", username);
                    }
                    SignalMessage::OfferRift {
                        target_username,
                        hello_base64,
                    } => {
                        let Some(src) = &authenticated_username else {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Bind required before signaling".into(),
                                },
                            )
                            .await;
                            break;
                        };
                        if !security::is_valid_username(&target_username) || hello_base64.len() > 8192 {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Invalid OFFER_RIFT payload".into(),
                                },
                            )
                            .await;
                            continue;
                        }
                        relay_message(
                            &connections,
                            &target_username,
                            SignalMessage::OfferRift {
                                target_username: src.clone(),
                                hello_base64,
                            },
                        )
                        .await;
                    }
                    SignalMessage::AnswerRift {
                        target_username,
                        ack_base64,
                    } => {
                        let Some(src) = &authenticated_username else {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Bind required before signaling".into(),
                                },
                            )
                            .await;
                            break;
                        };
                        if !security::is_valid_username(&target_username) || ack_base64.len() > 8192 {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Invalid ANSWER_RIFT payload".into(),
                                },
                            )
                            .await;
                            continue;
                        }
                        relay_message(
                            &connections,
                            &target_username,
                            SignalMessage::AnswerRift {
                                target_username: src.clone(),
                                ack_base64,
                            },
                        )
                        .await;
                    }
                    SignalMessage::Offer {
                        target_username,
                        sdp,
                    } => {
                        let Some(src) = &authenticated_username else {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Bind required before signaling".into(),
                                },
                            )
                            .await;
                            break;
                        };
                        if !security::is_valid_username(&target_username)
                            || sdp.len() > MAX_SIGNAL_SDP_BYTES
                        {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Invalid OFFER payload".into(),
                                },
                            )
                            .await;
                            continue;
                        }
                        relay_message(
                            &connections,
                            &target_username,
                            SignalMessage::Offer {
                                target_username: src.clone(),
                                sdp,
                            },
                        )
                        .await;
                    }
                    SignalMessage::Answer {
                        target_username,
                        sdp,
                    } => {
                        let Some(src) = &authenticated_username else {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Bind required before signaling".into(),
                                },
                            )
                            .await;
                            break;
                        };
                        if !security::is_valid_username(&target_username)
                            || sdp.len() > MAX_SIGNAL_SDP_BYTES
                        {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Invalid ANSWER payload".into(),
                                },
                            )
                            .await;
                            continue;
                        }
                        relay_message(
                            &connections,
                            &target_username,
                            SignalMessage::Answer {
                                target_username: src.clone(),
                                sdp,
                            },
                        )
                        .await;
                    }
                    SignalMessage::Candidate {
                        target_username,
                        candidate,
                    } => {
                        let Some(src) = &authenticated_username else {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Bind required before signaling".into(),
                                },
                            )
                            .await;
                            break;
                        };
                        if !security::is_valid_username(&target_username)
                            || candidate.len() > MAX_SIGNAL_CANDIDATE_BYTES
                        {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Invalid CANDIDATE payload".into(),
                                },
                            )
                            .await;
                            continue;
                        }
                        relay_message(
                            &connections,
                            &target_username,
                            SignalMessage::Candidate {
                                target_username: src.clone(),
                                candidate,
                            },
                        )
                        .await;
                    }
                    SignalMessage::RequestRelay { target_username } => {
                        let Some(src) = &authenticated_username else {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Bind required before signaling".into(),
                                },
                            )
                            .await;
                            break;
                        };

                        if !security::is_valid_username(&target_username) {
                            let _ = send_signal(
                                &tx,
                                &SignalMessage::Error {
                                    message: "Invalid target username".into(),
                                },
                            )
                            .await;
                            continue;
                        }

                        let session_id = Uuid::new_v4();
                        let token = random_relay_token();
                        let ttl = relay_session_ttl();
                        let now = Instant::now();

                        {
                            let mut guard = relay_sessions.write().await;
                            guard.retain(|_, session| now.duration_since(session.created_at) < ttl);
                            if guard.len() >= relay_session_limit() {
                                let _ = send_signal(
                                    &tx,
                                    &SignalMessage::Error {
                                        message: "Relay session capacity reached".into(),
                                    },
                                )
                                .await;
                                continue;
                            }

                            guard.insert(
                                token.clone(),
                                RelaySession {
                                    host_email: src.clone(),
                                    client_email: target_username.clone(),
                                    session_id,
                                    host_addr: None,
                                    client_addr: None,
                                    created_at: Instant::now(),
                                    bytes_sent: 0,
                                    last_tick: Instant::now(),
                                    host_seq: SequenceWindow::new(),
                                    client_seq: SequenceWindow::new(),
                                },
                            );
                        }

                        let resp = SignalMessage::RelayCredentials {
                            token: token.clone(),
                            addr: relay_public_addr(),
                            session_id,
                        };

                        let _ = send_signal(&tx, &resp).await;
                        relay_message(&connections, &target_username, resp).await;
                    }
                    SignalMessage::RelayCredentials { .. }
                    | SignalMessage::Error { .. }
                    | SignalMessage::Bound => {
                        let _ = send_signal(
                            &tx,
                            &SignalMessage::Error {
                                message: "Unsupported client message type".into(),
                            },
                        )
                        .await;
                    }
                }
            }
        }
    }

    if let Some(user) = authenticated_username {
        info!("client disconnected: {}", user);
        connections.write().await.remove(&user);
    }
    ACTIVE_WS_CONNECTIONS.fetch_sub(1, Ordering::Relaxed);
    {
        let mut counts = IP_CONNECTIONS.lock().unwrap();
        if let Some(count) = counts.get_mut(&addr.ip()) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                counts.remove(&addr.ip());
            }
        }
    }
}

async fn relay_message(connections: &ConnectionMap, target_username: &str, msg: SignalMessage) {
    let tx = {
        let guard = connections.read().await;
        guard.get(target_username).cloned()
    };

    if let Some(tx) = tx {
        if !tx.try_send(msg) {
            warn!("failed to queue signaling message for {}", target_username);
        }
    } else {
        warn!("target user not connected: {}", target_username);
    }
}
