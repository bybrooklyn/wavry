use std::sync::{Mutex, OnceLock};

fn default_status() -> String {
    #[cfg(target_os = "linux")]
    {
        "PCVR: idle (Linux runtime not started)".to_string()
    }
    #[cfg(target_os = "windows")]
    {
        "PCVR: idle (Windows runtime not started)".to_string()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        "PCVR: Not available on this platform".to_string()
    }
}

static PCVR_STATUS: OnceLock<Mutex<String>> = OnceLock::new();

fn status_cell() -> &'static Mutex<String> {
    PCVR_STATUS.get_or_init(|| Mutex::new(default_status()))
}

pub fn pcvr_status() -> String {
    match status_cell().lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

pub fn set_pcvr_status(status: impl Into<String>) {
    let mut guard = match status_cell().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = status.into();
}
