use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State, ConnectInfo},
    response::IntoResponse,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::{mpsc, RwLock};
use futures::{stream::StreamExt, SinkExt};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use sqlx::SqlitePool;

// Shared State for connected clients
pub type ConnectionMap = Arc<RwLock<HashMap<String, mpsc::UnboundedSender<Message>>>>;

use crate::relay::{RelayMap, RelaySession};
use uuid::Uuid;

// Protocol Definitions
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum SignalMessage {
    // Auth
    Bind { token: String },
    
    // RIFT-v1 SDP Exchange
    #[serde(rename = "OFFER_RIFT")]
    OfferRift { target_username: String, hello_base64: String },
    #[serde(rename = "ANSWER_RIFT")]
    AnswerRift { target_username: String, ack_base64: String },

    // Legacy / WebRTC Signaling
    Offer { target_username: String, sdp: String },
    Answer { target_username: String, sdp: String },
    Candidate { target_username: String, candidate: String },
    
    // Relay
    #[serde(rename = "REQUEST_RELAY")]
    RequestRelay { target_username: String },
    #[serde(rename = "RELAY_CREDENTIALS")]
    RelayCredentials { token: String, addr: String, session_id: Uuid },
    
    // Errors / Status
    Error { message: String },
    Bound,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(connections): State<ConnectionMap>,
    State(relay_sessions): State<RelayMap>,
    State(pool): State<SqlitePool>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, connections, relay_sessions, pool, addr))
}

async fn handle_socket(
    stream: WebSocket,
    connections: ConnectionMap,
    relay_sessions: RelayMap,
    pool: SqlitePool,
    addr: SocketAddr,
) {
    info!("Client connecting from {}", addr);
    let (mut sender, mut receiver) = stream.split();
    let (tx, mut rx) = mpsc::unbounded_channel();
    
    // Task to forward messages from channel to websocket
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    let mut authenticated_username: Option<String> = None;

    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(text) = msg {
            let signal: SignalMessage = match serde_json::from_str(&text) {
                Ok(s) => s,
                Err(e) => {
                    warn!("Invalid JSON from {}: {}", addr, e);
                    continue;
                }
            };

            match signal {
                SignalMessage::Bind { token } => {
                    // 1. Verify Token from DB
                    // For MVP simplicity, we might query DB.
                    // Ideally we have a cache or fast lookup.
                    // SELECT user_id FROM sessions WHERE token = ? AND expires_at > NOW
                    // Runtime query to avoid build-time DB requirement
                    #[derive(sqlx::FromRow)]
                    struct UserEmail { username: String }

                    let user = sqlx::query_as::<_, UserEmail>(
                        r#"
                        SELECT u.username 
                        FROM sessions s
                        JOIN users u ON s.user_id = u.id
                        WHERE s.token = ? AND s.expires_at > datetime('now')
                        "#
                    )
                    .bind(token)
                    .fetch_optional(&pool)
                    .await;
                    
                    match user {
                        Ok(Some(rec)) => {
                            authenticated_username = Some(rec.username.clone());
                            connections.write().await.insert(rec.username.clone(), tx.clone());
                            let _ = tx.send(Message::Text(serde_json::to_string(&SignalMessage::Bound).unwrap()));
                            info!("Bound session for user: {}", rec.username);
                        }
                        _ => {
                            let _ = tx.send(Message::Text(serde_json::to_string(&SignalMessage::Error{ message: "Invalid Token".into() }).unwrap()));
                             // Disconnect?
                        }
                    }
                },
                SignalMessage::OfferRift { target_username, hello_base64 } => {
                    if let Some(src) = &authenticated_username {
                        relay_message(&connections, &target_username, SignalMessage::OfferRift { target_username: src.clone(), hello_base64 }).await;
                    }
                },
                SignalMessage::AnswerRift { target_username, ack_base64 } => {
                    if let Some(src) = &authenticated_username {
                        relay_message(&connections, &target_username, SignalMessage::AnswerRift { target_username: src.clone(), ack_base64 }).await;
                    }
                },
                SignalMessage::Offer { target_username, sdp } => {
                    if let Some(src) = &authenticated_username {
                        relay_message(&connections, &target_username, SignalMessage::Offer { target_username: src.clone(), sdp }).await;
                    }
                },
                SignalMessage::Answer { target_username, sdp } => {
                     if let Some(src) = &authenticated_username {
                        relay_message(&connections, &target_username, SignalMessage::Answer { target_username: src.clone(), sdp }).await;
                    }
                },
                SignalMessage::Candidate { target_username, candidate } => {
                    if let Some(src) = &authenticated_username {
                        relay_message(&connections, &target_username, SignalMessage::Candidate { target_username: src.clone(), candidate }).await;
                    }
                },
                SignalMessage::RequestRelay { target_username } => {
                    if let Some(src) = &authenticated_username {
                        let session_id = Uuid::new_v4();
                        let token = session_id.to_string(); // Use session_id as token for MVP simplicity
                        
                        // Store in RelayMap
                        {
                            let mut guard = relay_sessions.write().await;
                            guard.insert(token.clone(), RelaySession {
                                host_email: src.clone(),
                                client_email: target_username.clone(),
                                host_addr: None,
                                client_addr: None,
                                created_at: std::time::Instant::now(),
                                bytes_sent: 0,
                                last_tick: std::time::Instant::now(),
                            });
                        }
                        
                        let resp = SignalMessage::RelayCredentials { 
                            token: token.clone(),
                            addr: "wavry.example.com:3478".into(), // Dynamic IP needed in prod
                            session_id,
                        };
                        
                        // Send to requester
                         let _ = tx.send(Message::Text(serde_json::to_string(&resp).unwrap()));
                         
                        // Forward to peer
                        relay_message(&connections, &target_username, resp).await;
                    }
                },
                _ => {}
            }
        }
    }

    // Cleanup
    if let Some(user) = authenticated_username {
        info!("Client disconnected: {}", user);
        connections.write().await.remove(&user);
    }
}

async fn relay_message(connections: &ConnectionMap, target_username: &str, msg: SignalMessage) {
    let guard = connections.read().await;
    if let Some(tx) = guard.get(target_username) {
        let json = serde_json::to_string(&msg).unwrap();
        let _ = tx.send(Message::Text(json));
    } else {
        warn!("Target user not connected: {}", target_username);
        // Ideally send error back to sender
    }
}
