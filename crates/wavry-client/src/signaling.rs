use anyhow::{anyhow, Result};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SignalMessage {
    BIND { token: String },
    OFFER { target_username: String, sdp: String, public_addr: Option<String> },
    ANSWER { target_username: String, sdp: String, public_addr: Option<String> },
    CANDIDATE { target_username: String, candidate: String },
    
    // Relay
    #[serde(rename = "REQUEST_RELAY")]
    RequestRelay { target_username: String },
    #[serde(rename = "RELAY_CREDENTIALS")]
    RelayCredentials { token: String, addr: String },

    ERROR { code: u16, message: String },
}

pub struct SignalingClient {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl SignalingClient {
    pub async fn connect(url: &str, token: &str) -> Result<Self> {
        let (mut ws_stream, _) = connect_async(url).await?;
        
        // Auth
        let bind_msg = SignalMessage::BIND { token: token.to_string() };
        ws_stream.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&bind_msg)?.into())).await?;
        
        // Expect OK? Gateway might send something back or just be silent until error.
        // Assuming silent success for now based on gateway impl.
        
        Ok(Self { ws: ws_stream })
    }

    pub async fn send(&mut self, msg: SignalMessage) -> Result<()> {
        let text = serde_json::to_string(&msg)?;
        self.ws.send(tokio_tungstenite::tungstenite::Message::Text(text.into())).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<SignalMessage> {
        while let Some(msg) = self.ws.next().await {
            let msg = msg?;
            if let tokio_tungstenite::tungstenite::Message::Text(text) = msg {
                let signal: SignalMessage = serde_json::from_str(&text)?;
                return Ok(signal);
            }
        }
        Err(anyhow!("Signaling connection closed"))
    }
}
