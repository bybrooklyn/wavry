use clap::Parser;
use std::net::SocketAddr;
use wavry_client::{run_client, ClientConfig};

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
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let args = Args::parse();

    let config = ClientConfig {
        connect_addr: args.connect,
        client_name: "wavry-cli".to_string(),
        no_encrypt: args.no_encrypt,
        identity_key: None,
    };

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run_client(config, None))
}
