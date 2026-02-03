#![forbid(unsafe_code)]

use anyhow::Result;
use clap::Parser;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "wavry-relay")]
struct Args {
    #[arg(long, default_value = "0.0.0.0:7000")]
    listen: String,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let args = Args::parse();
    info!(
        "wavry-relay is a stub in v0.0.1; planned UDP relay at {}",
        args.listen
    );
    Ok(())
}
