use crate::state::{ClientSessionState, CLIENT_SESSION_STATE};
use tokio::sync::{mpsc, oneshot};
use wavry_client::{run_client_with_shutdown, ClientConfig};

pub fn register_client_session(
    stop_tx: oneshot::Sender<()>,
    monitor_tx: mpsc::UnboundedSender<u32>,
) -> Result<(), String> {
    let mut state = CLIENT_SESSION_STATE.lock().unwrap();
    if state.is_some() {
        return Err("Client session already active".into());
    }
    *state = Some(ClientSessionState {
        stop_tx: Some(stop_tx),
        monitor_tx: Some(monitor_tx),
    });
    Ok(())
}

pub fn clear_client_session() {
    if let Ok(mut state) = CLIENT_SESSION_STATE.lock() {
        *state = None;
    }
}

pub fn spawn_client_session(config: ClientConfig) -> Result<(), String> {
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let (monitor_tx, monitor_rx) = mpsc::unbounded_channel::<u32>();
    register_client_session(stop_tx, monitor_tx)?;

    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_client_with_shutdown(config, None, stop_rx, Some(monitor_rx)).await {
            log::error!("Client error: {}", e);
        }
        clear_client_session();
    });

    Ok(())
}
