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
            let addr: std::net::SocketAddr = server
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid server address: {}", e))?;

            println!("Pinging {}...", addr);

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;

            rt.block_on(async {
                use bytes::Bytes;
                use rift_core::{
                    control_message::Content, decode_msg, encode_msg, ControlMessage, Message,
                    PhysicalPacket, RIFT_VERSION,
                };
                use tokio::net::UdpSocket;
                use tokio::time::{timeout, Duration, Instant};

                let socket = UdpSocket::bind("0.0.0.0:0").await?;
                socket.connect(addr).await?;

                let start_time = Instant::now();
                let timestamp_us = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as u64;

                let ping_msg = Message {
                    content: Some(rift_core::message::Content::Control(ControlMessage {
                        content: Some(Content::Ping(rift_core::Ping { timestamp_us })),
                    })),
                };

                let phys = PhysicalPacket {
                    version: RIFT_VERSION,
                    session_id: Some(0),
                    session_alias: None,
                    packet_id: 0,
                    payload: Bytes::from(encode_msg(&ping_msg)),
                };

                socket.send(&phys.encode()).await?;

                let mut buf = [0u8; 1500];
                match timeout(Duration::from_secs(2), socket.recv(&mut buf)).await {
                    Ok(Ok(len)) => {
                        let rtt = start_time.elapsed();
                        let resp_phys =
                            PhysicalPacket::decode(Bytes::copy_from_slice(&buf[..len]))?;
                        let resp_msg = decode_msg(&resp_phys.payload)?;

                        match resp_msg.content {
                            Some(rift_core::message::Content::Control(ctrl)) => {
                                match ctrl.content {
                                    Some(Content::Pong(_)) => {
                                        println!("Response from {}: RTT={:?}", addr, rtt);
                                    }
                                    _ => println!(
                                        "Received unexpected RIFT message type from {}",
                                        addr
                                    ),
                                }
                            }
                            _ => println!("Received non-control RIFT message from {}", addr),
                        }
                    }
                    Ok(Err(e)) => println!("Error receiving from {}: {}", addr, e),
                    Err(_) => println!("Ping timeout for {}", addr),
                }

                Ok::<(), anyhow::Error>(())
            })?;
        }
        Command::Version => {
            println!("wavry {}", env!("CARGO_PKG_VERSION"));
        }
    }

    Ok(())
}
