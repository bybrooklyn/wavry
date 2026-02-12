use anyhow::{anyhow, Result};
use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

pub use wavry_common::protocol::SignalMessage;

pub struct SignalingClient {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
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

fn validate_signaling_url(url: &str) -> Result<()> {
    let insecure = url.trim().to_ascii_lowercase().starts_with("ws://");
    let production = env_bool("WAVRY_ENVIRONMENT_PRODUCTION", false)
        || std::env::var("WAVRY_ENVIRONMENT")
            .map(|v| v.eq_ignore_ascii_case("production"))
            .unwrap_or(false);
    let allow_insecure = env_bool("WAVRY_ALLOW_INSECURE_SIGNALING", false);

    if insecure && production && !allow_insecure {
        return Err(anyhow!(
            "refusing insecure ws:// signaling URL in production; use wss:// or set WAVRY_ALLOW_INSECURE_SIGNALING=1"
        ));
    }

    Ok(())
}

impl SignalingClient {
    pub async fn connect(url: &str, token: &str) -> Result<Self> {
        validate_signaling_url(url)?;
        let (mut ws_stream, _) = connect_async(url).await?;

        // Auth
        let bind_msg = SignalMessage::BIND {
            token: token.to_string(),
        };
        ws_stream
            .send(tokio_tungstenite::tungstenite::Message::Text(
                serde_json::to_string(&bind_msg)?.into(),
            ))
            .await?;

        // Expect OK? Gateway might send something back or just be silent until error.
        // Assuming silent success for now based on gateway impl.

        Ok(Self { ws: ws_stream })
    }

    pub async fn send(&mut self, msg: SignalMessage) -> Result<()> {
        let text = serde_json::to_string(&msg)?;
        self.ws
            .send(tokio_tungstenite::tungstenite::Message::Text(text.into()))
            .await?;
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
