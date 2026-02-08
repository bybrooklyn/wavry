use bytes::{Buf, BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};

pub const INPUT_PROTOCOL_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebClientCapabilities {
    pub max_width: u16,
    pub max_height: u16,
    pub max_fps: u16,
    pub supports_gamepad: bool,
    pub supports_touch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMessage {
    Connect {
        session_token: String,
        client_name: String,
        capabilities: WebClientCapabilities,
    },
    Disconnect {
        reason: String,
    },
    Resize {
        width: u16,
        height: u16,
    },
    Settings {
        bitrate_kbps: u32,
        fps: u16,
    },
    Key {
        keycode: u32,
        pressed: bool,
        timestamp_us: u64,
    },
    MouseButton {
        button: u8,
        pressed: bool,
        timestamp_us: u64,
    },
    GamepadButton {
        gamepad_id: u8,
        button: u16,
        pressed: bool,
        timestamp_us: u64,
    },
    GamepadAxis {
        gamepad_id: u8,
        axis: u8,
        value: f32,
        timestamp_us: u64,
    },
    WebRtcOffer {
        target_username: String,
        sdp: String,
    },
    WebRtcAnswer {
        target_username: String,
        sdp: String,
    },
    WebRtcCandidate {
        target_username: String,
        candidate: String,
    },
    StatsRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WebControlResponse {
    Connected {
        server_name: String,
    },
    Error {
        message: String,
    },
    WebRtcOffer {
        from_username: String,
        sdp: String,
    },
    WebRtcAnswer {
        from_username: String,
        sdp: String,
    },
    WebRtcCandidate {
        from_username: String,
        candidate: String,
    },
    Stats(StatsReport),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsReport {
    pub rtt_ms: u32,
    pub jitter_ms: f32,
    pub packet_loss: f32,
    pub bitrate_kbps: u32,
    pub encoder_delay_ms: f32,
    pub decoder_delay_ms: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InputKind {
    MouseMove = 1,
    Scroll = 2,
    Analog = 3,
    Gamepad = 4,
}

#[derive(Debug, Clone)]
pub enum InputDatagram {
    MouseMove {
        dx: i16,
        dy: i16,
        timestamp_us: u64,
    },
    Scroll {
        dx: i16,
        dy: i16,
        timestamp_us: u64,
    },
    Analog {
        axis: u8,
        value: f32,
        timestamp_us: u64,
    },
    Gamepad {
        gamepad_id: u8,
        buttons: u16,
        axes: [i16; 4],
        timestamp_us: u64,
    },
}

impl InputDatagram {
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(32);
        buf.put_u8(INPUT_PROTOCOL_VERSION);
        match self {
            InputDatagram::MouseMove {
                dx,
                dy,
                timestamp_us,
            } => {
                buf.put_u8(InputKind::MouseMove as u8);
                buf.put_u64_le(*timestamp_us);
                buf.put_i16_le(*dx);
                buf.put_i16_le(*dy);
            }
            InputDatagram::Scroll {
                dx,
                dy,
                timestamp_us,
            } => {
                buf.put_u8(InputKind::Scroll as u8);
                buf.put_u64_le(*timestamp_us);
                buf.put_i16_le(*dx);
                buf.put_i16_le(*dy);
            }
            InputDatagram::Analog {
                axis,
                value,
                timestamp_us,
            } => {
                buf.put_u8(InputKind::Analog as u8);
                buf.put_u64_le(*timestamp_us);
                buf.put_u8(*axis);
                buf.put_f32_le(*value);
            }
            InputDatagram::Gamepad {
                gamepad_id,
                buttons,
                axes,
                timestamp_us,
            } => {
                buf.put_u8(InputKind::Gamepad as u8);
                buf.put_u64_le(*timestamp_us);
                buf.put_u8(*gamepad_id);
                buf.put_u16_le(*buttons);
                for axis in axes {
                    buf.put_i16_le(*axis);
                }
            }
        }
        buf.freeze()
    }

    pub fn decode(mut bytes: Bytes) -> Option<Self> {
        if bytes.remaining() < 2 {
            return None;
        }
        let version = bytes.get_u8();
        if version != INPUT_PROTOCOL_VERSION {
            return None;
        }
        let kind = bytes.get_u8();
        match kind {
            x if x == InputKind::MouseMove as u8 => {
                if bytes.remaining() < 12 {
                    return None;
                }
                let timestamp_us = bytes.get_u64_le();
                let dx = bytes.get_i16_le();
                let dy = bytes.get_i16_le();
                Some(InputDatagram::MouseMove {
                    dx,
                    dy,
                    timestamp_us,
                })
            }
            x if x == InputKind::Scroll as u8 => {
                if bytes.remaining() < 12 {
                    return None;
                }
                let timestamp_us = bytes.get_u64_le();
                let dx = bytes.get_i16_le();
                let dy = bytes.get_i16_le();
                Some(InputDatagram::Scroll {
                    dx,
                    dy,
                    timestamp_us,
                })
            }
            x if x == InputKind::Analog as u8 => {
                if bytes.remaining() < 13 {
                    return None;
                }
                let timestamp_us = bytes.get_u64_le();
                let axis = bytes.get_u8();
                let value = bytes.get_f32_le();
                Some(InputDatagram::Analog {
                    axis,
                    value,
                    timestamp_us,
                })
            }
            x if x == InputKind::Gamepad as u8 => {
                if bytes.remaining() < 19 {
                    return None;
                }
                let timestamp_us = bytes.get_u64_le();
                let gamepad_id = bytes.get_u8();
                let buttons = bytes.get_u16_le();
                let mut axes = [0i16; 4];
                for axis in axes.iter_mut() {
                    *axis = bytes.get_i16_le();
                }
                Some(InputDatagram::Gamepad {
                    gamepad_id,
                    buttons,
                    axes,
                    timestamp_us,
                })
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlStreamFrame {
    Control(ControlMessage),
    Stats(StatsReport),
    Response(WebControlResponse),
}
