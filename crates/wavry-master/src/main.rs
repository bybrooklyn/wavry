//! Wavry Master server stub.
//!
//! This will be the coordination service for identity, relay registry,
//! lease issuance, and matchmaking.

#![forbid(unsafe_code)]

use anyhow::Result;
use clap::Parser;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "wavry-master")]
#[command(about = "Wavry Master coordination server")]
struct Args {
    /// HTTP listen address
    #[arg(long, default_value = "0.0.0.0:8080")]
    listen: String,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    wavry_common::init_tracing_with_default(&args.log_level);

    info!("wavry-master starting on {}", args.listen);
    info!("Master server implementation pending - see WAVRY_MASTER.md for spec");

    // TODO: Implement REST API endpoints
    // TODO: Implement database layer
    // TODO: Implement relay registry
    // TODO: Implement lease issuance

    Ok(())
}
