use anyhow::{anyhow, Result};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time;

use base64::{engine::general_purpose, Engine as _};
use rift_core::{
    decode_msg, encode_msg, Codec as RiftCodec, ControlMessage as ProtoControl,
    Hello as ProtoHello, Message as ProtoMessage, Resolution as ProtoResolution, RIFT_VERSION,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_bool_true_values() {
        std::env::set_var("TEST_ENV_TRUE", "true");
        assert!(env_bool("TEST_ENV_TRUE", false));

        std::env::set_var("TEST_ENV_1", "1");
        assert!(env_bool("TEST_ENV_1", false));

        std::env::set_var("TEST_ENV_YES", "yes");
        assert!(env_bool("TEST_ENV_YES", false));

        std::env::set_var("TEST_ENV_ON", "on");
        assert!(env_bool("TEST_ENV_ON", false));

        std::env::set_var("TEST_ENV_UPPER", "TRUE");
        assert!(env_bool("TEST_ENV_UPPER", false));
    }

    #[test]
    fn test_env_bool_false_values() {
        std::env::set_var("TEST_ENV_FALSE", "false");
        assert!(!env_bool("TEST_ENV_FALSE", true));

        std::env::set_var("TEST_ENV_0", "0");
        assert!(!env_bool("TEST_ENV_0", true));

        std::env::set_var("TEST_ENV_NO", "no");
        assert!(!env_bool("TEST_ENV_NO", true));

        std::env::set_var("TEST_ENV_OFF", "off");
        assert!(!env_bool("TEST_ENV_OFF", true));
    }

    #[test]
    fn test_env_bool_missing_uses_default() {
        // Use a variable that definitely won't be set
        assert!(env_bool("DEFINITELY_NOT_SET_ENV_VAR_12345", true));
        assert!(!env_bool("DEFINITELY_NOT_SET_ENV_VAR_12345", false));
    }

    #[test]
    fn test_env_bool_whitespace_handling() {
        std::env::set_var("TEST_ENV_SPACES", "  true  ");
        assert!(env_bool("TEST_ENV_SPACES", false));

        std::env::set_var("TEST_ENV_TAB", "\ttrue\t");
        assert!(env_bool("TEST_ENV_TAB", false));
    }

    #[test]
    fn test_now_us_returns_positive() {
        let timestamp = now_us();
        assert!(timestamp > 0, "Timestamp should be positive");
    }

    #[test]
    fn test_now_us_monotonic() {
        let t1 = now_us();
        let t2 = now_us();
        assert!(t2 >= t1, "Timestamps should be monotonically increasing");
    }

    #[test]
    fn test_now_us_reasonable_range() {
        // Timestamp should be in range of 2020-2030 (in microseconds since epoch)
        let timestamp = now_us();
        let year_2020_us = 1577836800u64 * 1_000_000;
        let year_2030_us = 1893456000u64 * 1_000_000;

        assert!(
            timestamp > year_2020_us && timestamp < year_2030_us,
            "Timestamp should be in reasonable range"
        );
    }

    #[test]
    fn test_local_platform_detection() {
        let platform = local_platform();

        #[cfg(target_os = "windows")]
        assert_eq!(platform, rift_core::Platform::Windows);

        #[cfg(target_os = "macos")]
        assert_eq!(platform, rift_core::Platform::Macos);

        #[cfg(target_os = "linux")]
        assert_eq!(platform, rift_core::Platform::Linux);
    }

    #[test]
    fn test_create_hello_base64_valid_encoding() {
        let result = create_hello_base64("TestClient".to_string(), None);
        assert!(result.is_ok(), "Should create valid Hello message");

        let b64 = result.unwrap();
        assert!(!b64.is_empty(), "Base64 string should not be empty");

        // Verify it's valid base64
        assert!(
            general_purpose::STANDARD.decode(&b64).is_ok(),
            "Should be valid base64"
        );
    }

    #[test]
    fn test_create_hello_base64_with_public_addr() {
        let result = create_hello_base64(
            "TestClient".to_string(),
            Some("192.168.1.1:5000".to_string()),
        );
        assert!(result.is_ok());

        let b64 = result.unwrap();
        let decoded = general_purpose::STANDARD.decode(&b64).unwrap();
        assert!(!decoded.is_empty());
    }

    #[test]
    fn test_create_hello_ack_base64_accepted() {
        let session_id = [42u8; 16];
        let result =
            create_hello_ack_base64(true, session_id, 999, None, 1920, 1080, RiftCodec::H264);
        assert!(result.is_ok());

        let b64 = result.unwrap();
        let decoded = general_purpose::STANDARD.decode(&b64).unwrap();
        assert!(!decoded.is_empty());
    }

    #[test]
    fn test_create_hello_ack_base64_rejected() {
        let session_id = [0u8; 16];
        let result = create_hello_ack_base64(
            false,
            session_id,
            0,
            Some("10.0.0.1:5000".to_string()),
            0,
            0,
            RiftCodec::H264,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_decode_hello_base64_roundtrip() {
        let original_name = "MyClient".to_string();
        let public_addr = Some("203.0.113.1:5000".to_string());

        let b64 = create_hello_base64(original_name.clone(), public_addr.clone()).unwrap();
        let decoded = decode_hello_base64(&b64).unwrap();

        assert_eq!(decoded.client_name, original_name);
        assert_eq!(decoded.public_addr, "203.0.113.1:5000");
        assert!(decoded.supported_codecs.contains(&(RiftCodec::H264 as i32)));
    }

    #[test]
    fn test_decode_hello_base64_invalid_input() {
        let invalid_b64 = "not-valid-base64!!!";
        assert!(decode_hello_base64(invalid_b64).is_err());
    }

    #[test]
    fn test_decode_hello_ack_base64_roundtrip() {
        let session_id = [123u8; 16];
        let session_alias = 42;
        let public_addr = Some("198.51.100.1:5000".to_string());

        let b64 = create_hello_ack_base64(
            true,
            session_id,
            session_alias,
            public_addr.clone(),
            1920,
            1080,
            RiftCodec::H264,
        )
        .unwrap();

        let decoded = decode_hello_ack_base64(&b64).unwrap();

        assert!(decoded.accepted);
        assert_eq!(decoded.session_id, session_id.to_vec());
        assert_eq!(decoded.session_alias, session_alias);
        assert_eq!(decoded.public_addr, "198.51.100.1:5000");
        assert_eq!(decoded.stream_resolution.unwrap().width, 1920);
        assert_eq!(decoded.stream_resolution.unwrap().height, 1080);
    }

    #[test]
    fn test_decode_hello_ack_base64_invalid_input() {
        let invalid_b64 = "invalid!!!base64";
        assert!(decode_hello_ack_base64(invalid_b64).is_err());
    }

    #[test]
    fn test_hello_message_contains_expected_fields() {
        let b64 = create_hello_base64("TestClient".to_string(), None).unwrap();
        let hello = decode_hello_base64(&b64).unwrap();

        assert_eq!(hello.client_name, "TestClient");
        assert_eq!(hello.max_fps, 60);
        assert_eq!(hello.protocol_version, RIFT_VERSION as u32);
        assert!(hello.max_resolution.is_some());
        assert_eq!(hello.max_resolution.unwrap().width, 1920);
        assert_eq!(hello.max_resolution.unwrap().height, 1080);
    }

    #[test]
    fn test_hello_ack_message_contains_expected_fields() {
        let b64 =
            create_hello_ack_base64(true, [1u8; 16], 1, None, 3840, 2160, RiftCodec::Hevc).unwrap();
        let ack = decode_hello_ack_base64(&b64).unwrap();

        assert_eq!(ack.fps, 60);
        assert!(ack.initial_bitrate_kbps > 0);
        assert_eq!(ack.keyframe_interval_ms, 2000);
        assert_eq!(ack.stream_resolution.unwrap().width, 3840);
        assert_eq!(ack.stream_resolution.unwrap().height, 2160);
    }
}
