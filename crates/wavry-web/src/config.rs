#[derive(Debug, Clone)]
pub struct WebGatewayConfig {
    pub public_base_url: String,
    pub auth_base_url: String,
    pub relay_base_url: String,
    pub webtransport_url: String,
    pub webtransport_bind_addr: String,
    pub webrtc_signaling_url: String,
}

impl WebGatewayConfig {
    /// Build a domain-agnostic config compatible with wavry.dev-style subdomains.
    pub fn from_domain(domain: &str) -> Self {
        let base = domain.trim().trim_end_matches('/');
        let public_base_url = format!("https://{base}");
        let auth_base_url = format!("https://auth.{base}");
        let relay_base_url = format!("https://relay.{base}");
        let webtransport_url = format!("https://app.{base}/wt");
        let webtransport_bind_addr = "0.0.0.0:4444".to_string();
        let webrtc_signaling_url = format!("https://app.{base}/webrtc");
        Self {
            public_base_url,
            auth_base_url,
            relay_base_url,
            webtransport_url,
            webtransport_bind_addr,
            webrtc_signaling_url,
        }
    }
}

impl Default for WebGatewayConfig {
    fn default() -> Self {
        Self::from_domain("wavry.dev")
    }
}
