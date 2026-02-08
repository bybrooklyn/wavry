use crate::protocol::{ControlStreamFrame, InputDatagram};
use anyhow::{anyhow, Result};
use std::sync::Arc;

#[cfg(feature = "webtransport-runtime")]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(feature = "webtransport-runtime")]
use tokio::sync::mpsc;

/// Skeleton for a WebTransport server implementation.
pub struct WebTransportServer {
    #[cfg_attr(not(feature = "webtransport-runtime"), allow(dead_code))]
    bind_addr: String,
}

impl WebTransportServer {
    pub async fn bind(addr: &str) -> Result<Self> {
        Ok(Self {
            bind_addr: addr.to_string(),
        })
    }

    pub async fn run(self, handler: impl WebTransportSessionHandler) -> Result<()> {
        let handler: Arc<dyn WebTransportSessionHandler> = Arc::new(handler);

        #[cfg(feature = "webtransport-runtime")]
        {
            return run_real_runtime(&self.bind_addr, handler).await;
        }

        #[cfg(not(feature = "webtransport-runtime"))]
        {
            let _ = handler;
            Err(anyhow!(
                "WebTransportServer::run is a skeleton; enable feature `webtransport-runtime` for runtime binding"
            ))
        }
    }
}

/// A single WebTransport session with a browser client.
#[derive(Debug)]
pub struct WebTransportSession {
    pub session_id: String,
    #[cfg(feature = "webtransport-runtime")]
    pub tx: mpsc::Sender<ControlStreamFrame>,
}

/// Callback interface for a host implementation.
pub trait WebTransportSessionHandler: Send + Sync + 'static {
    fn on_session_started(&self, session: WebTransportSession);
    fn on_input_datagram(&self, session_id: &str, datagram: InputDatagram);
    fn on_control_frame(&self, session_id: &str, frame: ControlStreamFrame);
}

#[cfg(feature = "webtransport-runtime")]
async fn run_real_runtime(
    bind_addr: &str,
    handler: Arc<dyn WebTransportSessionHandler>,
) -> Result<()> {
    use std::net::SocketAddr;
    use wtransport::Endpoint;
    use wtransport::ServerConfig;

    let addr: SocketAddr = bind_addr.parse()?;

    let cert_path = std::env::var("WAVRY_WT_CERT").unwrap_or_else(|_| "cert.pem".to_string());
    let key_path = std::env::var("WAVRY_WT_KEY").unwrap_or_else(|_| "key.pem".to_string());

    let identity = wtransport::Identity::load_pemfiles(
        std::path::Path::new(&cert_path),
        std::path::Path::new(&key_path),
    )
    .await?;

    let config = ServerConfig::builder()
        .with_bind_address(addr)
        .with_identity(identity)
        .build();

    let endpoint = Endpoint::server(config)?;
    tracing::info!("WebTransport (QUIC) server listening on {}", bind_addr);

    loop {
        let incoming_session = endpoint.accept().await;
        let handler = handler.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_session(incoming_session, handler).await {
                tracing::error!("WebTransport session error: {}", e);
            }
        });
    }
}

#[cfg(feature = "webtransport-runtime")]
async fn handle_session(
    incoming_session: wtransport::endpoint::IncomingSession,
    handler: Arc<dyn WebTransportSessionHandler>,
) -> Result<()> {
    let session_request = incoming_session.await?;
    let connection = session_request.accept().await?;
    let session_id = connection.remote_address().to_string();
    tracing::info!("Accepted WebTransport session from {}", session_id);

    let connection = Arc::new(connection);
    let (tx, mut rx) = mpsc::channel::<ControlStreamFrame>(100);

    handler.on_session_started(WebTransportSession {
        session_id: session_id.clone(),
        tx,
    });

    let h1 = handler.clone();
    let sid1 = session_id.clone();
    let c1 = connection.clone();
    let datagram_task = tokio::spawn(async move {
        loop {
            match c1.receive_datagram().await {
                Ok(data) => {
                    let bytes = bytes::Bytes::copy_from_slice(&data);
                    if let Some(datagram) = InputDatagram::decode(bytes) {
                        h1.on_input_datagram(&sid1, datagram);
                    }
                }
                Err(_) => break,
            }
        }
    });

    let h2 = handler.clone();
    let sid2 = session_id.clone();
    let c2 = connection.clone();
    let stream_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                stream = c2.accept_bi() => {
                    match stream {
                        Ok((_send, mut recv)) => {
                            let h = h2.clone();
                            let sid = sid2.clone();
                            tokio::spawn(async move {
                                let mut buf = Vec::new();
                                if recv.read_to_end(&mut buf).await.is_ok() {
                                    if let Ok(frame) = serde_json::from_slice::<ControlStreamFrame>(&buf) {
                                        h.on_control_frame(&sid, frame);
                                    }
                                }
                            });
                        }
                        Err(_) => break,
                    }
                }
                stream = c2.accept_uni() => {
                    match stream {
                        Ok(mut recv) => {
                            let h = h2.clone();
                            let sid = sid2.clone();
                            tokio::spawn(async move {
                                let mut buf = Vec::new();
                                if recv.read_to_end(&mut buf).await.is_ok() {
                                    if let Ok(frame) = serde_json::from_slice::<ControlStreamFrame>(&buf) {
                                        h.on_control_frame(&sid, frame);
                                    }
                                }
                            });
                        }
                        Err(_) => break,
                    }
                }
                Some(frame) = rx.recv() => {
                    if let Ok(mut stream) = c2.open_uni().await {
                        // TODO: Fix write_all trait bound issue
                        // if let Ok(json) = serde_json::to_vec(&frame) {
                        //    let _ = stream.write_all(&json).await;
                        // }
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = datagram_task => {}
        _ = stream_task => {}
        _ = connection.closed() => {
            tracing::info!("WebTransport session {} closed", session_id);
        }
    }

    Ok(())
}
