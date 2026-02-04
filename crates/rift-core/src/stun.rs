use std::net::{SocketAddr, IpAddr, Ipv4Addr};
use anyhow::{Result, anyhow};

pub const STUN_MAGIC_COOKIE: u32 = 0x2112A442;
pub const BINDING_REQUEST: u16 = 0x0001;
pub const BINDING_RESPONSE: u16 = 0x0101;

pub struct StunMessage {
    pub msg_type: u16,
    pub transaction_id: [u8; 12],
}

impl StunMessage {
    pub fn new_binding_request() -> Self {
        use rand::RngCore;
        let mut transaction_id = [0u8; 12];
        rand::thread_rng().fill_bytes(&transaction_id);
        Self {
            msg_type: BINDING_REQUEST,
            transaction_id,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(20);
        buf.extend_from_slice(&self.msg_type.to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes()); // Length
        buf.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        buf.extend_from_slice(&self.transaction_id);
        buf
    }

    pub fn decode_address(buf: &[u8]) -> Result<SocketAddr> {
        if buf.len() < 20 {
            return Err(anyhow!("STUN message too short"));
        }

        let msg_type = u16::from_be_bytes([buf[0], buf[1]]);
        if msg_type != BINDING_RESPONSE {
            return Err(anyhow!("Not a binding response: 0x{:04x}", msg_type));
        }

        let cookie = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
        if cookie != STUN_MAGIC_COOKIE {
            return Err(anyhow!("Invalid magic cookie"));
        }

        let mut pos = 20;
        let end = buf.len();

        while pos + 4 <= end {
            let attr_type = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
            let attr_len = u16::from_be_bytes([buf[pos + 2], buf[pos + 3]]) as usize;
            pos += 4;

            if pos + attr_len > end {
                break;
            }

            // XOR-MAPPED-ADDRESS is 0x0020
            if attr_type == 0x0020 {
                if attr_len < 8 { return Err(anyhow!("Invalid XOR-MAPPED-ADDRESS length")); }
                let port = u16::from_be_bytes([buf[pos + 2], buf[pos + 3]]) ^ (STUN_MAGIC_COOKIE >> 16) as u16;
                let family = buf[pos + 1];
                if family == 0x01 { // IPv4
                    let a = buf[pos + 4] ^ (STUN_MAGIC_COOKIE >> 24) as u8;
                    let b = buf[pos + 5] ^ (STUN_MAGIC_COOKIE >> 16) as u8;
                    let c = buf[pos + 6] ^ (STUN_MAGIC_COOKIE >> 8) as u8;
                    let d = buf[pos + 7] ^ (STUN_MAGIC_COOKIE) as u8;
                    return Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(a, b, c, d)), port));
                }
            }
            
            // MAPPED-ADDRESS is 0x0001
            if attr_type == 0x0001 {
                if attr_len < 8 { return Err(anyhow!("Invalid MAPPED-ADDRESS length")); }
                let port = u16::from_be_bytes([buf[pos + 2], buf[pos + 3]]);
                let family = buf[pos + 1];
                if family == 0x01 { // IPv4
                    let ip = Ipv4Addr::new(buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7]);
                    return Ok(SocketAddr::new(IpAddr::V4(ip), port));
                }
            }

            pos += attr_len;
            if pos % 4 != 0 {
                pos += 4 - (pos % 4);
            }
        }

        Err(anyhow!("No mapped address found in STUN response"))
    }
}
