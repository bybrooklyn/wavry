use std::sync::atomic::Ordering;
use tokio::sync::oneshot;
use wavry_client::{run_client_with_shutdown, ClientConfig};
use crate::state::{CLIENT_SESSION_STATE, ClientSessionState};

pub fn register_client_session(stop_tx: oneshot::Sender<()>) -> Result<(), String> {
    let mut state = CLIENT_SESSION_STATE.lock().unwrap();
    if state.is_some() {
        return Err("Client session already active".into());
    }
    *state = Some(ClientSessionState {
        stop_tx: Some(stop_tx),
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
    register_client_session(stop_tx)?;

    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_client_with_shutdown(config, None, stop_rx).await {
            log::error!("Client error: {}", e);
        }
        clear_client_session();
    });

    Ok(())
}
