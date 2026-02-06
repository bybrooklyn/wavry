use clap::Parser;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use wavry_client::{run_client, ClientConfig};
use wavry_vr::VrAdapter;

#[cfg(any(target_os = "linux", target_os = "windows"))]
use wavry_vr_alvr::AlvrAdapter;

#[derive(Parser, Debug)]
#[command(name = "wavry-client")]
struct Args {
    #[arg(long)]
    connect: Option<SocketAddr>,
    #[arg(long, default_value = "wavry-client")]
    name: String,
    /// Disable encryption (for testing/debugging)
    #[arg(long, default_value = "false")]
    no_encrypt: bool,
    /// Enable PCVR adapter (Linux/Windows only)
    #[arg(long, default_value_t = false)]
    vr: bool,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let args = Args::parse();

    let vr_adapter: Option<Arc<Mutex<dyn VrAdapter>>> = if args.vr {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            Some(Arc::new(Mutex::new(AlvrAdapter::new())))
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            return Err(anyhow::anyhow!(
                "PCVR adapter is only supported on Linux and Windows"
            ));
        }
    } else {
        None
    };

    let config = ClientConfig {
        connect_addr: args.connect,
        client_name: args.name,
        no_encrypt: args.no_encrypt,
        identity_key: None,
        relay_info: None,
        max_resolution: None,
        vr_adapter,
    };

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run_client(config, None))
}
