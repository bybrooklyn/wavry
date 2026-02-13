pub mod auth;
pub mod client_manager;
pub mod commands;
pub mod media_utils;
pub mod secure_storage;
pub mod state;

#[cfg(target_os = "linux")]
fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|value| value.trim().eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn enforce_wayland_env(key: &str, desired_value: &str, reason: &str) {
    match std::env::var_os(key) {
        Some(current) if current == std::ffi::OsStr::new(desired_value) => {}
        Some(current) => {
            log::warn!(
                "Wayland detected: overriding {}={} -> {} ({})",
                key,
                current.to_string_lossy(),
                desired_value,
                reason
            );
            std::env::set_var(key, desired_value);
        }
        None => {
            log::info!(
                "Wayland detected: set {}={} ({})",
                key,
                desired_value,
                reason
            );
            std::env::set_var(key, desired_value);
        }
    }
}

#[cfg(target_os = "linux")]
fn configure_linux_runtime_workarounds() {
    if !is_wayland_session() {
        return;
    }

    // Keep all Linux UI stacks on native Wayland to avoid split-backend behavior.
    enforce_wayland_env("GDK_BACKEND", "wayland", "force GTK backend to Wayland");
    enforce_wayland_env(
        "WINIT_UNIX_BACKEND",
        "wayland",
        "force Tao/Winit windows to Wayland",
    );

    // Work around known WebKitGTK protocol instability on Plasma/Wayland.
    enforce_wayland_env(
        "WEBKIT_DISABLE_DMABUF_RENDERER",
        "1",
        "disable WebKit dmabuf renderer on Wayland",
    );
    enforce_wayland_env(
        "WEBKIT_DISABLE_COMPOSITING_MODE",
        "1",
        "use conservative WebKit compositing on Wayland",
    );
}

#[cfg(target_os = "linux")]
fn log_linux_runtime_diagnostics() {
    match wavry_media::linux_runtime_diagnostics() {
        Ok(diag) => {
            log::info!(
                "Linux runtime diagnostics: session={} wayland={} x11={} desktop={:?} expected_portal_backends={:?} expected_descriptors={:?} available_descriptors={:?} missing_descriptors={:?} required_video_source={} available={} audio_sources={:?} h264_encoders={:?} missing_elements={:?}",
                diag.session_type,
                diag.wayland_display,
                diag.x11_display,
                diag.xdg_current_desktop,
                diag.expected_portal_backends,
                diag.expected_portal_descriptors,
                diag.available_portal_descriptors,
                diag.missing_expected_portal_descriptors,
                diag.required_video_source,
                diag.required_video_source_available,
                diag.available_audio_sources,
                diag.available_h264_encoders,
                diag.missing_gstreamer_elements
            );
            for recommendation in diag.recommendations {
                log::warn!("Linux runtime recommendation: {}", recommendation);
            }
        }
        Err(err) => {
            log::warn!("Linux runtime diagnostics unavailable: {}", err);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    configure_linux_runtime_workarounds();
    #[cfg(target_os = "linux")]
    log_linux_runtime_diagnostics();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::get_pcvr_status,
            commands::set_cc_config,
            commands::get_cc_stats,
            commands::register,
            commands::login_full,
            commands::set_signaling_token,
            commands::start_session,
            commands::stop_session,
            commands::send_file_transfer_command,
            commands::list_monitors,
            commands::linux_runtime_health,
            commands::linux_host_preflight,
            commands::connect_via_id,
            commands::start_host,
            commands::stop_host,
            commands::save_secure_token,
            commands::load_secure_token,
            commands::delete_secure_token,
            commands::save_secure_data,
            commands::load_secure_data,
            commands::delete_secure_data,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
