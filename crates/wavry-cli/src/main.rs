//! Wavry CLI tools: key generation, diagnostics, debugging.

#![forbid(unsafe_code)]

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "wavry")]
#[command(about = "Wavry CLI tools")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Generate a new Ed25519 identity keypair
    Keygen {
        /// Output file path (without extension)
        #[arg(short, long, default_value = "wavry")]
        output: String,
    },

    /// Show Wavry ID from a public key file
    ShowId {
        /// Public key file path
        #[arg(short, long)]
        key: String,
    },

    /// Ping a Wavry server to check connectivity
    Ping {
        /// Server address (host:port)
        #[arg(short, long)]
        server: String,
    },

    /// Show version information
    Version,
}

fn main() -> Result<()> {
    wavry_common::init_tracing();

    let args = Args::parse();

    match args.command {
        Command::Keygen { output } => {
            println!("Generating Ed25519 keypair...");

            let keypair = rift_crypto::identity::IdentityKeypair::generate();
            let wavry_id = keypair.wavry_id();

            let private_path = format!("{}.key", output);
            let public_path = format!("{}.pub", output);

            keypair.save(&private_path, &public_path)?;

            println!("Private key: {}", private_path);
            println!("Public key:  {}", public_path);
            println!("Wavry ID:    {}", wavry_id);
        }
        Command::ShowId { key } => {
            let keypair = rift_crypto::identity::IdentityKeypair::load_public(&key)?;
            println!("{}", keypair.wavry_id());
        }
        Command::Ping { server } => {
            println!("Ping not yet implemented for: {}", server);
            // TODO: Send RIFT ping packet
        }
        Command::Version => {
            println!("wavry {}", env!("CARGO_PKG_VERSION"));
        }
    }

    Ok(())
}
