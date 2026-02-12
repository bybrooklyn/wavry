use crate::state::{ClientSessionState, CLIENT_SESSION_STATE};
use tokio::sync::{broadcast, mpsc, oneshot};
use wavry_client::{run_client_with_shutdown, ClientConfig, FileTransferCommand};

pub fn register_client_session(
    stop_tx: oneshot::Sender<()>,
    monitor_tx: mpsc::UnboundedSender<u32>,
    file_command_tx: broadcast::Sender<FileTransferCommand>,
) -> Result<(), String> {
    let mut state = CLIENT_SESSION_STATE.lock().unwrap();
    if state.is_some() {
        return Err("Client session already active".into());
    }
    *state = Some(ClientSessionState {
        stop_tx: Some(stop_tx),
        monitor_tx: Some(monitor_tx),
        file_command_tx: Some(file_command_tx),
    });
    Ok(())
}

pub fn clear_client_session() {
    if let Ok(mut state) = CLIENT_SESSION_STATE.lock() {
        *state = None;
    }
}

pub fn spawn_client_session(mut config: ClientConfig) -> Result<(), String> {
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let (monitor_tx, monitor_rx) = mpsc::unbounded_channel::<u32>();
    let (file_command_tx, _file_command_rx) = broadcast::channel::<FileTransferCommand>(64);
    config.file_command_bus = Some(file_command_tx.clone());
    register_client_session(stop_tx, monitor_tx, file_command_tx)?;

    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_client_with_shutdown(config, None, stop_rx, Some(monitor_rx)).await {
            log::error!("Client error: {}", e);
        }
        clear_client_session();
    });

    Ok(())
}
