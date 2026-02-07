use anyhow::{anyhow, Result};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time;

use base64::{engine::general_purpose, Engine as _};
use rift_core::{
    decode_msg, encode_msg,
    Codec as RiftCodec, Message as ProtoMessage,
    ControlMessage as ProtoControl, Hello as ProtoHello,
    Resolution as ProtoResolution, RIFT_VERSION,
};

pub fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

pub fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

pub fn local_platform() -> rift_core::Platform {
    if cfg!(target_os = "windows") {
        rift_core::Platform::Windows
    } else if cfg!(target_os = "macos") {
        rift_core::Platform::Macos
    } else {
        rift_core::Platform::Linux
    }
}

pub async fn discover_public_addr(socket: &UdpSocket) -> Result<SocketAddr> {
    use rift_core::stun::StunMessage;
    let stun_server = "stun.l.google.com:19302";
    let stun_msg = StunMessage::new_binding_request();
    let encoded = stun_msg.encode();

    socket.send_to(&encoded, stun_server).await?;

    let mut buf = [0u8; 1024];
    let (len, _) = time::timeout(Duration::from_secs(2), socket.recv_from(&mut buf)).await??;

    StunMessage::decode_address(&buf[..len])
}

pub fn create_hello_base64(client_name: String, public_addr: Option<String>) -> Result<String> {
    // Note: this should ideally use a codec probe, but for CLI/minimal use we can default
    let hello = ProtoHello {
        client_name,
        platform: local_platform() as i32,
        supported_codecs: vec![RiftCodec::H264 as i32],
        max_resolution: Some(ProtoResolution {
            width: 1920,
            height: 1080,
        }),
        max_fps: 60,
        input_caps: 0xF,
        protocol_version: RIFT_VERSION as u32,
        public_addr: public_addr.unwrap_or_default(),
    };
    let msg = ProtoMessage {
        content: Some(rift_core::message::Content::Control(ProtoControl {
            content: Some(rift_core::control_message::Content::Hello(hello)),
        })),
    };
    let bytes = encode_msg(&msg);
    Ok(general_purpose::STANDARD.encode(bytes))
}

pub fn create_hello_ack_base64(
    accepted: bool,
    session_id: [u8; 16],
    session_alias: u32,
    public_addr: Option<String>,
    width: u32,
    height: u32,
    selected_codec: RiftCodec,
) -> Result<String> {
    let ack = rift_core::HelloAck {
        accepted,
        selected_codec: selected_codec as i32,
        stream_resolution: Some(ProtoResolution { width, height }),
        fps: 60,
        initial_bitrate_kbps: 8000,
        keyframe_interval_ms: 2000,
        session_id: session_id.to_vec(),
        session_alias,
        public_addr: public_addr.unwrap_or_default(),
    };
    let msg = ProtoMessage {
        content: Some(rift_core::message::Content::Control(ProtoControl {
            content: Some(rift_core::control_message::Content::HelloAck(ack)),
        })),
    };
    let bytes = encode_msg(&msg);
    Ok(general_purpose::STANDARD.encode(bytes))
}

pub fn decode_hello_base64(b64: &str) -> Result<ProtoHello> {
    let bytes = general_purpose::STANDARD.decode(b64)?;
    let msg = decode_msg(&bytes)?;
    match msg.content {
        Some(rift_core::message::Content::Control(ctrl)) => match ctrl.content {
            Some(rift_core::control_message::Content::Hello(h)) => Ok(h),
            _ => Err(anyhow!("Not a Hello message")),
        },
        _ => Err(anyhow!("Not a Control message")),
    }
}

pub fn decode_hello_ack_base64(b64: &str) -> Result<rift_core::HelloAck> {
    let bytes = general_purpose::STANDARD.decode(b64)?;
    let msg = decode_msg(&bytes)?;
    match msg.content {
        Some(rift_core::message::Content::Control(ctrl)) => match ctrl.content {
            Some(rift_core::control_message::Content::HelloAck(a)) => Ok(a),
            _ => Err(anyhow!("Not a HelloAck message")),
        },
        _ => Err(anyhow!("Not a Control message")),
    }
}

