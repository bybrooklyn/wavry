pub mod client;
pub mod helpers;
pub mod input;
pub mod media;
pub mod signaling;
pub mod types;

pub use client::{run_client, run_client_with_shutdown};
pub use helpers::{
    create_hello_ack_base64, create_hello_base64, decode_hello_ack_base64, decode_hello_base64,
    discover_public_addr, env_bool, local_platform, now_us,
};
pub use types::{ClientConfig, ClientRuntimeStats, CryptoState, RelayInfo, RendererFactory};

pub fn pcvr_status() -> String {
    wavry_vr::pcvr_status()
}
