//! Web client transport layer: WebTransport (control/input) + WebRTC (media).
//!
//! This crate intentionally avoids binding to a specific WebTransport/WebRTC runtime.
//! It provides protocol types and server integration points.

mod config;
mod protocol;
mod webrtc;
mod webtransport;

pub use config::WebGatewayConfig;
pub use protocol::{
    ControlMessage, ControlStreamFrame, InputDatagram, StatsReport, WebClientCapabilities,
    WebControlResponse,
};
pub use webrtc::{WebRtcPeer, WebRtcSignaling, WebRtcStartParams};
pub use webtransport::{WebTransportServer, WebTransportSession, WebTransportSessionHandler};

/// High-level skeleton for a unified host gateway.
///
/// This does not replace the native RIFT/DELTA server. It layers WebTransport + WebRTC
/// alongside it, using shared control-plane state.
pub struct WebGateway {
    config: WebGatewayConfig,
}

impl WebGateway {
    pub fn new(config: WebGatewayConfig) -> Self {
        Self { config }
    }

    /// Start WebTransport control plane and WebRTC signaling.
    ///
    /// The native RIFT server remains in its own binary/crate and is not modified here.
    pub async fn start(self) -> anyhow::Result<()> {
        #[cfg(feature = "webtransport-runtime")]
        {
            struct NoopHandler;
            impl WebTransportSessionHandler for NoopHandler {
                fn on_input_datagram(&self, _session_id: &str, _datagram: InputDatagram) {}

                fn on_control_frame(&self, _session_id: &str, _frame: ControlStreamFrame) {}
            }

            let wt = WebTransportServer::bind(&self.config.webtransport_bind_addr).await?;
            wt.run(NoopHandler).await
        }

        #[cfg(not(feature = "webtransport-runtime"))]
        {
            let _ = self.config;
            Err(anyhow::anyhow!(
                "WebGateway::start is a skeleton; enable `webtransport-runtime` or integrate your own runtime"
            ))
        }
    }
}
