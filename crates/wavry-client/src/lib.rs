pub mod client;
pub mod signaling;
pub mod types;
pub mod helpers;
pub mod input;
pub mod media;

pub use client::{run_client, run_client_with_shutdown};
pub use types::{ClientConfig, ClientRuntimeStats, RelayInfo, RendererFactory, CryptoState};
pub use helpers::{
    discover_public_addr, now_us, local_platform, env_bool,
    create_hello_ack_base64, create_hello_base64, decode_hello_ack_base64, decode_hello_base64,
};

pub fn pcvr_status() -> String {
    wavry_vr::pcvr_status()
}
