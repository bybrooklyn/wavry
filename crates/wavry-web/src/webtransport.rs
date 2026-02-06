use crate::protocol::{ControlStreamFrame, InputDatagram};
use std::sync::Arc;

/// Skeleton for a WebTransport server implementation.
///
/// This intentionally does not bind to a specific QUIC/WebTransport crate.
/// Implementations should translate WebTransport streams/datagrams into
/// `ControlStreamFrame` and `InputDatagram`.
pub struct WebTransportServer {
    #[cfg_attr(not(feature = "webtransport-runtime"), allow(dead_code))]
    bind_addr: String,
}

impl WebTransportServer {
    pub async fn bind(addr: &str) -> anyhow::Result<Self> {
        Ok(Self {
            bind_addr: addr.to_string(),
        })
    }

    pub async fn run(self, handler: impl WebTransportSessionHandler) -> anyhow::Result<()> {
        let handler: Arc<dyn WebTransportSessionHandler> = Arc::new(handler);

        #[cfg(feature = "webtransport-runtime")]
        {
            return run_dev_runtime(&self.bind_addr, handler).await;
        }

        #[cfg(not(feature = "webtransport-runtime"))]
        {
            let _ = handler;
            Err(anyhow::anyhow!(
                "WebTransportServer::run is a skeleton; enable feature `webtransport-runtime` for dev runtime binding"
            ))
        }
    }
}

/// A single WebTransport session with a browser client.
#[derive(Debug)]
pub struct WebTransportSession {
    pub session_id: String,
}

/// Callback interface for a host implementation.
pub trait WebTransportSessionHandler: Send + Sync + 'static {
    fn on_input_datagram(&self, session_id: &str, datagram: InputDatagram);
    fn on_control_frame(&self, session_id: &str, frame: ControlStreamFrame);
}

#[cfg(feature = "webtransport-runtime")]
async fn run_dev_runtime(
    bind_addr: &str,
    handler: Arc<dyn WebTransportSessionHandler>,
) -> anyhow::Result<()> {
    use bytes::Bytes;
    use tokio::net::UdpSocket;

    let enabled = std::env::var("WAVRY_ENABLE_INSECURE_WEBTRANSPORT_RUNTIME")
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false);
    if !enabled {
        return Err(anyhow::anyhow!(
            "dev webtransport runtime is disabled by default; set WAVRY_ENABLE_INSECURE_WEBTRANSPORT_RUNTIME=1 to enable"
        ));
    }

    let allow_remote = std::env::var("WAVRY_WEBTRANSPORT_ALLOW_REMOTE")
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false);

    // Development-only runtime binding:
    // one UDP socket that accepts either binary input datagrams or JSON control frames.
    // This is not standards-compliant WebTransport, but it exercises host wiring end-to-end.
    let socket = UdpSocket::bind(bind_addr).await?;
    tracing::info!("WebTransport dev runtime listening on {}", bind_addr);

    let mut buf = [0u8; 2048];
    loop {
        let (len, peer) = socket.recv_from(&mut buf).await?;
        if !allow_remote && !peer.ip().is_loopback() {
            tracing::warn!(
                "webtransport dev runtime rejected non-loopback peer {} (set WAVRY_WEBTRANSPORT_ALLOW_REMOTE=1 to allow)",
                peer
            );
            continue;
        }
        let session_id = peer.to_string();
        let payload = Bytes::copy_from_slice(&buf[..len]);

        if let Some(datagram) = InputDatagram::decode(payload.clone()) {
            handler.on_input_datagram(&session_id, datagram);
            continue;
        }

        match serde_json::from_slice::<ControlStreamFrame>(&payload) {
            Ok(frame) => handler.on_control_frame(&session_id, frame),
            Err(err) => tracing::debug!(
                "webtransport runtime ignored payload from {}: {}",
                session_id,
                err
            ),
        }
    }
}
