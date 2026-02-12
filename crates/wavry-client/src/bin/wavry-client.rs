use clap::Parser;
use std::io::{self, BufRead};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use wavry_client::{run_client, ClientConfig, FileTransferAction, FileTransferCommand};
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
    /// Enable local recording to MP4
    #[arg(long, default_value_t = false)]
    record: bool,
    /// Directory to store recordings
    #[arg(long, default_value = "recordings")]
    record_dir: String,
    /// Send file to host after session establishment (repeatable)
    #[arg(long = "send-file", value_name = "PATH")]
    send_files: Vec<PathBuf>,
    /// Directory for received files
    #[arg(long, default_value = "received-files")]
    file_out_dir: PathBuf,
    /// Maximum inbound file size in bytes
    #[arg(long, default_value_t = wavry_common::file_transfer::DEFAULT_MAX_FILE_BYTES)]
    file_max_bytes: u64,
    /// Read file-transfer commands from stdin as: `<file_id> <pause|resume|cancel|retry>`
    #[arg(long, default_value_t = false)]
    file_control_stdin: bool,
}

fn parse_file_control_line(line: &str) -> Result<FileTransferCommand, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err("empty command".into());
    }

    let mut parts = trimmed.split_whitespace();
    let file_id = parts
        .next()
        .ok_or_else(|| "missing file_id".to_string())?
        .parse::<u64>()
        .map_err(|_| "file_id must be an unsigned integer".to_string())?;
    let action = parts
        .next()
        .ok_or_else(|| "missing action".to_string())?
        .parse::<FileTransferAction>()
        .map_err(|e| e.to_string())?;

    if parts.next().is_some() {
        return Err("expected exactly two tokens: <file_id> <action>".into());
    }

    Ok(FileTransferCommand { file_id, action })
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

    let recorder_config = if args.record {
        Some(wavry_media::RecorderConfig {
            enabled: true,
            output_dir: std::path::PathBuf::from(args.record_dir),
            ..Default::default()
        })
    } else {
        None
    };

    let file_command_bus = if args.file_control_stdin {
        let (tx, _rx) = broadcast::channel::<FileTransferCommand>(64);
        let tx_reader = tx.clone();
        std::thread::spawn(move || {
            eprintln!("File control stdin enabled: use `<file_id> <pause|resume|cancel|retry>`");
            let stdin = io::stdin();
            for line in stdin.lock().lines() {
                match line {
                    Ok(line) => match parse_file_control_line(&line) {
                        Ok(cmd) => {
                            if let Err(err) = tx_reader.send(cmd) {
                                eprintln!("file control send failed: {}", err);
                                break;
                            }
                        }
                        Err(err) => {
                            eprintln!("invalid file control command `{}`: {}", line.trim(), err);
                        }
                    },
                    Err(err) => {
                        eprintln!("stdin read error: {}", err);
                        break;
                    }
                }
            }
        });
        Some(tx)
    } else {
        None
    };

    let config = ClientConfig {
        connect_addr: args.connect,
        client_name: args.name,
        no_encrypt: args.no_encrypt,
        identity_key: None,
        relay_info: None,
        master_url: None,
        max_resolution: None,
        gamepad_enabled: true,
        gamepad_deadzone: 0.1,
        vr_adapter,
        runtime_stats: None,
        recorder_config,
        send_files: args.send_files,
        file_out_dir: args.file_out_dir,
        file_max_bytes: args.file_max_bytes,
        file_command_bus,
    };

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run_client(config, None, None))
}
