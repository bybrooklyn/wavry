use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use anyhow::{anyhow, Context, Result};
use ashpd::desktop::{
    screencast::{CursorMode, Screencast, SourceType, Stream},
    PersistMode,
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::future::Future;
use tokio::time::{sleep, Duration};
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as RandrExt;

use crate::{Codec, DecodeConfig, EncodeConfig, EncodedFrame, MediaError, MediaResult, Renderer};

fn element_available(name: &str) -> bool {
    gst::ElementFactory::find(name).is_some()
}

fn has_x11_display() -> bool {
    env::var_os("DISPLAY").is_some()
}

fn has_wayland_display() -> bool {
    env::var_os("WAYLAND_DISPLAY").is_some()
        || matches!(
            env::var("XDG_SESSION_TYPE"),
            Ok(ref value) if value.eq_ignore_ascii_case("wayland")
        )
}

fn xdg_current_desktop() -> Option<String> {
    env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn expected_portal_backends_from_desktop(desktop: Option<&str>) -> Vec<&'static str> {
    let Some(desktop) = desktop else {
        return vec![
            "xdg-desktop-portal-kde",
            "xdg-desktop-portal-gnome",
            "xdg-desktop-portal-wlr",
            "xdg-desktop-portal-gtk",
        ];
    };

    let normalized = desktop.to_ascii_lowercase();
    if normalized.contains("kde") || normalized.contains("plasma") {
        return vec!["xdg-desktop-portal-kde", "xdg-desktop-portal-gtk"];
    }
    if normalized.contains("gnome")
        || normalized.contains("unity")
        || normalized.contains("cinnamon")
        || normalized.contains("pantheon")
    {
        return vec!["xdg-desktop-portal-gnome", "xdg-desktop-portal-gtk"];
    }
    if normalized.contains("hyprland") {
        return vec![
            "xdg-desktop-portal-hyprland",
            "xdg-desktop-portal-wlr",
            "xdg-desktop-portal-gtk",
        ];
    }
    if normalized.contains("sway")
        || normalized.contains("wlroots")
        || normalized.contains("river")
        || normalized.contains("wayfire")
    {
        return vec!["xdg-desktop-portal-wlr", "xdg-desktop-portal-gtk"];
    }
    vec![
        "xdg-desktop-portal-kde",
        "xdg-desktop-portal-gnome",
        "xdg-desktop-portal-wlr",
        "xdg-desktop-portal-gtk",
    ]
}

fn backend_to_portal_descriptor(backend: &str) -> Option<&'static str> {
    match backend {
        "xdg-desktop-portal-kde" => Some("kde.portal"),
        "xdg-desktop-portal-gnome" => Some("gnome.portal"),
        "xdg-desktop-portal-wlr" => Some("wlr.portal"),
        "xdg-desktop-portal-hyprland" => Some("hyprland.portal"),
        "xdg-desktop-portal-gtk" => Some("gtk.portal"),
        _ => None,
    }
}

fn portal_descriptor_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(data_home));
    } else if let Some(home) = env::var_os("HOME") {
        roots.push(Path::new(&home).join(".local").join("share"));
    }

    if let Ok(data_dirs) = env::var("XDG_DATA_DIRS") {
        for part in data_dirs.split(':') {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                roots.push(PathBuf::from(trimmed));
            }
        }
    } else {
        roots.push(PathBuf::from("/usr/local/share"));
        roots.push(PathBuf::from("/usr/share"));
    }

    roots
}

fn portal_descriptor_exists(descriptor_name: &str) -> bool {
    portal_descriptor_search_roots().into_iter().any(|root| {
        root.join("xdg-desktop-portal")
            .join("portals")
            .join(descriptor_name)
            .is_file()
    })
}

fn known_portal_descriptors() -> &'static [&'static str] {
    &[
        "kde.portal",
        "gnome.portal",
        "wlr.portal",
        "hyprland.portal",
        "gtk.portal",
    ]
}

fn available_portal_descriptors() -> Vec<String> {
    known_portal_descriptors()
        .iter()
        .copied()
        .filter(|descriptor| portal_descriptor_exists(descriptor))
        .map(str::to_string)
        .collect()
}

fn portal_backend_hint_message() -> String {
    let desktop = xdg_current_desktop();
    let desktop_label = desktop.as_deref().unwrap_or("unknown desktop");
    let expected = expected_portal_backends_from_desktop(desktop.as_deref()).join(", ");
    format!(
        "Desktop '{}': expected portal backend(s): {}",
        desktop_label, expected
    )
}

fn available_h264_encoder_candidates() -> Vec<String> {
    hardware_encoder_candidates(Codec::H264)
        .iter()
        .chain(software_encoder_candidates(Codec::H264).iter())
        .copied()
        .filter(|name| element_available(name))
        .map(str::to_string)
        .collect()
}

fn available_hevc_encoder_candidates() -> Vec<String> {
    hardware_encoder_candidates(Codec::Hevc)
        .iter()
        .chain(software_encoder_candidates(Codec::Hevc).iter())
        .copied()
        .filter(|name| element_available(name))
        .map(str::to_string)
        .collect()
}

fn available_av1_encoder_candidates() -> Vec<String> {
    hardware_encoder_candidates(Codec::Av1)
        .iter()
        .chain(software_encoder_candidates(Codec::Av1).iter())
        .copied()
        .filter(|name| element_available(name))
        .map(str::to_string)
        .collect()
}

fn detect_compositor_name() -> Option<String> {
    // Try to detect compositor from various environment variables and methods
    if let Ok(compositor) = env::var("WAYLAND_DISPLAY") {
        // Check common Wayland compositors
        if Command::new("pgrep")
            .arg("-x")
            .arg("kwin_wayland")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some("KWin (KDE Plasma)".to_string());
        }
        if Command::new("pgrep")
            .arg("-x")
            .arg("gnome-shell")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some("GNOME Shell (Mutter)".to_string());
        }
        if Command::new("pgrep")
            .arg("-x")
            .arg("sway")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some("Sway".to_string());
        }
        if Command::new("pgrep")
            .arg("-x")
            .arg("Hyprland")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some("Hyprland".to_string());
        }
        return Some(format!("Wayland ({})", compositor));
    }

    // X11 fallback
    if env::var("DISPLAY").is_ok() {
        return Some("X11".to_string());
    }

    None
}

fn check_pipewire_running() -> bool {
    // Check if PipeWire daemon is running
    Command::new("pgrep")
        .arg("-x")
        .arg("pipewire")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn check_portal_service_running() -> bool {
    // Check if xdg-desktop-portal service is running
    Command::new("pgrep")
        .arg("-x")
        .arg("xdg-desktop-portal")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct LinuxRuntimeDiagnostics {
    pub session_type: String,
    pub wayland_display: bool,
    pub x11_display: bool,
    pub xdg_current_desktop: Option<String>,
    pub expected_portal_backends: Vec<String>,
    pub expected_portal_descriptors: Vec<String>,
    pub available_portal_descriptors: Vec<String>,
    pub missing_expected_portal_descriptors: Vec<String>,
    pub required_video_source: String,
    pub required_video_source_available: bool,
    pub available_audio_sources: Vec<String>,
    pub available_h264_encoders: Vec<String>,
    pub available_hevc_encoders: Vec<String>,
    pub available_av1_encoders: Vec<String>,
    pub missing_gstreamer_elements: Vec<String>,
    pub recommendations: Vec<String>,
    pub compositor_name: Option<String>,
    pub pipewire_running: bool,
    pub portal_service_running: bool,
}

pub fn linux_runtime_diagnostics() -> Result<LinuxRuntimeDiagnostics> {
    gst::init()?;

    let wayland_display = has_wayland_display();
    let x11_display = has_x11_display();
    let session_type = if wayland_display {
        "wayland"
    } else if x11_display {
        "x11"
    } else {
        "headless"
    };

    let xdg_desktop = xdg_current_desktop();
    let expected_portal_backends_raw =
        expected_portal_backends_from_desktop(xdg_desktop.as_deref());
    let expected_portal_backends = expected_portal_backends_raw
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let expected_portal_descriptors = expected_portal_backends_raw
        .iter()
        .filter_map(|backend| backend_to_portal_descriptor(backend))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let available_portal_descriptors = available_portal_descriptors();

    let available_portal_descriptor_set = available_portal_descriptors
        .iter()
        .map(|value| value.as_str())
        .collect::<BTreeSet<_>>();
    let missing_expected_portal_descriptors = expected_portal_descriptors
        .iter()
        .filter(|descriptor| !available_portal_descriptor_set.contains(descriptor.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    let required_video_source = if wayland_display {
        "pipewiresrc"
    } else if x11_display {
        "ximagesrc"
    } else {
        "none"
    };
    let required_video_source_available =
        required_video_source == "none" || element_available(required_video_source);

    let base_required = [
        "videoconvert",
        "videoscale",
        "queue",
        "appsink",
        "audioconvert",
        "audioresample",
        "opusenc",
    ];
    let mut missing_gstreamer_elements = base_required
        .iter()
        .copied()
        .filter(|name| !element_available(name))
        .map(str::to_string)
        .collect::<Vec<_>>();

    if !required_video_source_available {
        missing_gstreamer_elements.push(required_video_source.to_string());
    }
    if x11_display && !element_available("videocrop") {
        missing_gstreamer_elements.push("videocrop".to_string());
    }

    missing_gstreamer_elements.sort();
    missing_gstreamer_elements.dedup();

    let audio_source_candidates = ["pipewiresrc", "pulsesrc", "autoaudiosrc"];
    let available_audio_sources = audio_source_candidates
        .iter()
        .copied()
        .filter(|name| element_available(name))
        .map(str::to_string)
        .collect::<Vec<_>>();

    let available_h264_encoders = available_h264_encoder_candidates();
    let available_hevc_encoders = available_hevc_encoder_candidates();
    let available_av1_encoders = available_av1_encoder_candidates();

    let mut recommendations = Vec::new();
    if wayland_display {
        recommendations.push(format!(
            "Validate portal backend availability. {}",
            portal_backend_hint_message()
        ));
        if !missing_expected_portal_descriptors.is_empty() {
            recommendations.push(format!(
                "Install missing portal backend descriptor(s): {}",
                missing_expected_portal_descriptors.join(", ")
            ));
        }
    }
    if !required_video_source_available {
        recommendations.push(format!(
            "Install missing GStreamer video source plugin: {}",
            required_video_source
        ));
    }
    if available_audio_sources.is_empty() {
        recommendations.push(
            "Install an audio source plugin (pipewiresrc, pulsesrc, or autoaudiosrc).".to_string(),
        );
    }
    if available_h264_encoders.is_empty() {
        recommendations.push(
            "Install at least one H264 encoder plugin (x264enc, openh264enc, VAAPI, NVENC, or V4L2)."
                .to_string(),
        );
    }
    if available_hevc_encoders.is_empty() {
        recommendations.push(
            "HEVC/H.265 encoders not available. Install x265enc, vaapih265enc, nvh265enc, or v4l2h265enc for better compression."
                .to_string(),
        );
    }
    if available_av1_encoders.is_empty() {
        recommendations.push(
            "AV1 encoders not available. Install svtav1enc, vaapav1enc, or nvav1enc for best compression efficiency."
                .to_string(),
        );
    }
    if !missing_gstreamer_elements.is_empty() {
        recommendations.push(format!(
            "Install missing GStreamer elements: {}",
            missing_gstreamer_elements.join(", ")
        ));
    }

    // Check compositor and service status
    let compositor_name = detect_compositor_name();
    let pipewire_running = check_pipewire_running();
    let portal_service_running = check_portal_service_running();

    // Add recommendations for missing services
    if wayland_display && !pipewire_running {
        recommendations.push(
            "PipeWire service is not running. Start it with 'systemctl --user start pipewire'."
                .to_string(),
        );
    }
    if wayland_display && !portal_service_running {
        recommendations.push("xdg-desktop-portal service is not running. Start it or install the portal backend for your desktop environment.".to_string());
    }

    Ok(LinuxRuntimeDiagnostics {
        session_type: session_type.to_string(),
        wayland_display,
        x11_display,
        xdg_current_desktop: xdg_desktop,
        expected_portal_backends,
        expected_portal_descriptors,
        available_portal_descriptors,
        missing_expected_portal_descriptors,
        required_video_source: required_video_source.to_string(),
        required_video_source_available,
        available_audio_sources,
        available_h264_encoders,
        available_hevc_encoders,
        available_av1_encoders,
        missing_gstreamer_elements,
        recommendations,
        compositor_name,
        pipewire_running,
        portal_service_running,
    })
}

fn clamp_portal_dim(dim: i32) -> u16 {
    dim.clamp(1, u16::MAX as i32) as u16
}

fn x11_monitor_crop(display_id: u32) -> Result<Option<(u32, u32, u32, u32)>> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen_num].root;
    let screen = &conn.setup().roots[screen_num];

    let resources = conn.randr_get_screen_resources_current(root)?.reply()?;

    let mut monitors = Vec::new();
    for output in resources.outputs {
        let info = conn.randr_get_output_info(output, 0)?.reply()?;
        if info.connection != x11rb::protocol::randr::Connection::CONNECTED || info.crtc == 0 {
            continue;
        }
        let crtc = conn.randr_get_crtc_info(info.crtc, 0)?.reply()?;
        monitors.push((crtc.x, crtc.y, crtc.width, crtc.height));
    }

    let idx = display_id as usize;
    if idx >= monitors.len() {
        return Ok(None);
    }

    let (x, y, width, height) = monitors[idx];
    let x = x.max(0) as u32;
    let y = y.max(0) as u32;
    let width = width.max(1) as u32;
    let height = height.max(1) as u32;
    let screen_w = screen.width_in_pixels as u32;
    let screen_h = screen.height_in_pixels as u32;

    let right = screen_w.saturating_sub(x.saturating_add(width));
    let bottom = screen_h.saturating_sub(y.saturating_add(height));

    Ok(Some((x, right, y, bottom)))
}

async fn enumerate_wayland_displays_inner() -> Result<Vec<crate::DisplayInfo>> {
    let proxy = Screencast::new().await?;
    let session = proxy.create_session().await?;
    let restore_token = load_restore_token();
    proxy
        .select_sources(
            &session,
            CursorMode::Hidden,
            SourceType::Monitor.into(),
            true,
            restore_token.as_deref(),
            PersistMode::ExplicitlyRevoked,
        )
        .await?;

    let response = proxy.start(&session, None).await?.response()?;
    if let Some(token) = response.restore_token() {
        save_restore_token(token)?;
    }

    let mut displays = Vec::new();
    for stream in response.streams() {
        if let Some(source) = stream.source_type() {
            if source != SourceType::Monitor {
                continue;
            }
        }

        let idx = displays.len() as u32;
        let (width, height) = stream.size().unwrap_or((1, 1));
        let name = stream
            .id()
            .filter(|id| !id.trim().is_empty())
            .map(|id| format!("Display {}", id))
            .unwrap_or_else(|| format!("Display {}", idx));

        displays.push(crate::DisplayInfo {
            id: idx,
            name,
            resolution: crate::Resolution {
                width: clamp_portal_dim(width),
                height: clamp_portal_dim(height),
            },
        });
    }

    Ok(displays)
}

fn enumerate_wayland_displays() -> Result<Vec<crate::DisplayInfo>> {
    let join = std::thread::spawn(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to create temporary runtime for portal monitor probe")?;
        runtime.block_on(enumerate_wayland_displays_inner())
    });
    join.join()
        .map_err(|_| anyhow!("portal monitor probe thread panicked"))?
}

fn require_elements(names: &[&str]) -> Result<()> {
    let missing: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| !element_available(name))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "missing GStreamer elements: {}",
            missing.join(", ")
        ))
    }
}

fn decoder_candidates(codec: Codec) -> &'static [&'static str] {
    match codec {
        Codec::H264 => &["vaapih264dec", "avdec_h264", "openh264dec"],
        Codec::Hevc => &["vaapih265dec", "avdec_h265"],
        Codec::Av1 => &["vaapav1dec", "vaapivav1dec", "av1dec", "dav1ddec"],
    }
}

fn select_parser(codec: Codec) -> Result<&'static str> {
    let parser = match codec {
        Codec::Av1 => "av1parse",
        Codec::Hevc => "h265parse",
        Codec::H264 => "h264parse",
    };
    if !element_available(parser) {
        return Err(anyhow!("missing GStreamer parser element: {parser}"));
    }
    Ok(parser)
}

fn caps_for_codec(codec: Codec) -> &'static str {
    match codec {
        Codec::Av1 => "video/x-av1,stream-format=(string)obu-stream,alignment=(string)tu",
        Codec::Hevc => "video/x-h265,stream-format=(string)byte-stream,alignment=(string)au",
        Codec::H264 => "video/x-h264,stream-format=(string)byte-stream,alignment=(string)au",
    }
}

fn require_decoder(codec: Codec) -> Result<()> {
    let candidates = decoder_candidates(codec);
    if candidates.iter().any(|name| element_available(name)) {
        Ok(())
    } else {
        Err(anyhow!(
            "missing GStreamer decoder for {codec:?}. Tried: {}",
            candidates.join(", ")
        ))
    }
}

fn hardware_encoder_candidates(codec: Codec) -> &'static [&'static str] {
    match codec {
        Codec::H264 => &["vaapih264enc", "nvh264enc", "v4l2h264enc"],
        Codec::Hevc => &["vaapih265enc", "nvh265enc", "v4l2h265enc"],
        Codec::Av1 => &["vaapav1enc", "vaapivav1enc", "nvav1enc"],
    }
}

fn software_encoder_candidates(codec: Codec) -> &'static [&'static str] {
    match codec {
        Codec::H264 => &["x264enc", "openh264enc"],
        Codec::Hevc => &["x265enc"],
        Codec::Av1 => &["svtav1enc", "av1enc", "rav1enc"],
    }
}

fn encoder_input_format(encoder_name: &str, enable_10bit: bool) -> &'static str {
    if encoder_name.starts_with("vaapi")
        || encoder_name.starts_with("nv")
        || encoder_name.starts_with("v4l2")
    {
        if enable_10bit {
            "P010_10LE"
        } else {
            "NV12"
        }
    } else {
        "I420"
    }
}

fn hardware_encoder_available(codec: Codec) -> bool {
    hardware_encoder_candidates(codec)
        .iter()
        .any(|name| element_available(name))
}

fn software_encoder_available(codec: Codec) -> bool {
    software_encoder_candidates(codec)
        .iter()
        .any(|name| element_available(name))
}

fn select_encoder(codec: Codec, enable_10bit: bool) -> Result<(String, &'static str)> {
    let encoder = hardware_encoder_candidates(codec)
        .iter()
        .chain(software_encoder_candidates(codec).iter())
        .find(|name| element_available(name))
        .ok_or_else(|| anyhow!("missing GStreamer encoder element for {codec:?}"))?;
    let input_format = encoder_input_format(encoder, enable_10bit);
    Ok((encoder.to_string(), input_format))
}

fn configure_low_latency_encoder(
    encoder: &gst::Element,
    encoder_name: &str,
    bitrate_kbps: u32,
    keyframe_interval_frames: u32,
    enable_10bit: bool,
) -> Result<()> {
    fn set_if_exists<V: ToValue>(encoder: &gst::Element, name: &str, value: V) {
        if encoder.has_property(name, None) {
            encoder.set_property(name, &value);
        }
    }

    set_if_exists(encoder, "bitrate", bitrate_kbps);
    set_if_exists(encoder, "target-bitrate", bitrate_kbps);
    set_if_exists(encoder, "keyframe-period", keyframe_interval_frames);
    set_if_exists(encoder, "key-int-max", keyframe_interval_frames as i32);

    if encoder_name.contains("x264") {
        set_if_exists(encoder, "tune", "zerolatency");
        set_if_exists(encoder, "speed-preset", "ultrafast");
        set_if_exists(encoder, "bframes", 0i32);
    } else if encoder_name.contains("x265") {
        set_if_exists(encoder, "tune", "zerolatency");
        set_if_exists(encoder, "speed-preset", "ultrafast");
        set_if_exists(encoder, "bframes", 0i32);
        if enable_10bit {
            set_if_exists(encoder, "profile", "main10");
        }
    } else if encoder_name.contains("svtav1") {
        set_if_exists(encoder, "preset", 8i32);
        set_if_exists(encoder, "tune", 0i32);
    } else if encoder_name.contains("vaapi") {
        set_if_exists(encoder, "rate-control", "cbr");
        set_if_exists(encoder, "max-bframes", 0i32);
        set_if_exists(encoder, "cabac", false);
    } else if encoder_name.contains("nvh265") && enable_10bit {
        set_if_exists(encoder, "profile", "main10");
    }

    Ok(())
}

pub struct PipewireEncoder {
    _fd: Option<OwnedFd>,
    #[allow(dead_code)]
    pipeline: gst::Pipeline,
    appsink: gst_app::AppSink,
    encoder_element: gst::Element,
}

impl PipewireEncoder {
    pub async fn new(config: EncodeConfig) -> MediaResult<Self> {
        gst::init().map_err(|e| MediaError::GStreamerError(e.to_string()))?;
        let (encoder_name, input_format) = select_encoder(config.codec, config.enable_10bit)
            .map_err(|e| MediaError::Unsupported(e.to_string()))?;
        let parser =
            select_parser(config.codec).map_err(|e| MediaError::GStreamerError(e.to_string()))?;
        require_elements(&["videoconvert", "queue", "appsink"])
            .map_err(|e| MediaError::GStreamerError(e.to_string()))?;
        if !element_available(&encoder_name) {
            return Err(MediaError::GStreamerError(format!(
                "missing GStreamer encoder element: {}",
                encoder_name
            )));
        }
        if !element_available(parser) {
            return Err(MediaError::GStreamerError(format!(
                "missing GStreamer parser element: {}",
                parser
            )));
        }

        let keyframe_interval_frames =
            ((config.fps as u32 * config.keyframe_interval_ms) / 1000).max(1);

        // Try PipeWire portal first, fallback to X11 capture if available.
        let portal_stream = open_portal_stream(config.display_id).await;

        let (pipeline_str, fd_opt) = match portal_stream {
            Ok((fd, node_id)) => {
                require_elements(&["pipewiresrc"])
                    .map_err(|e| MediaError::GStreamerError(e.to_string()))?;
                let pipeline_str = format!(
                    "pipewiresrc fd={} path={} do-timestamp=true ! videoconvert ! video/x-raw,format={},width={},height={},framerate={}/1 ! queue max-size-buffers=1 leaky=downstream ! {} name=encoder ! {} config-interval=-1 ! appsink name=sink max-buffers=1 drop=true sync=false",
                    fd.as_raw_fd(),
                    node_id,
                    input_format,
                    config.resolution.width,
                    config.resolution.height,
                    config.fps,
                    encoder_name,
                    parser,
                );
                (pipeline_str, Some(fd))
            }
            Err(err) => {
                if has_wayland_display() {
                    let backend_hint = portal_backend_hint_message();
                    return Err(MediaError::PortalUnavailable(format!(
                        "Wayland screencast portal failed: {}. Ensure xdg-desktop-portal + a desktop-specific portal backend are running, PipeWire is active, and screen capture permission is granted. {}",
                        err,
                        backend_hint
                    )));
                }

                if has_x11_display() {
                    log::warn!(
                        "PipeWire portal failed, falling back to X11 capture: {}",
                        err
                    );
                    let mut crop = None;
                    if let Some(display_id) = config.display_id {
                        match x11_monitor_crop(display_id) {
                            Ok(found) => crop = found,
                            Err(err) => {
                                log::warn!("Failed to resolve X11 display {}: {}", display_id, err)
                            }
                        }
                    }

                    let mut required = vec![
                        "ximagesrc",
                        "videoconvert",
                        "videoscale",
                        "queue",
                        "appsink",
                    ];
                    if crop.is_some() {
                        required.push("videocrop");
                    }
                    require_elements(&required)
                        .map_err(|e| MediaError::GStreamerError(e.to_string()))?;

                    let crop_str = if let Some((left, right, top, bottom)) = crop {
                        format!(
                            "videocrop left={} right={} top={} bottom={} ! ",
                            left, right, top, bottom
                        )
                    } else {
                        String::new()
                    };

                    let pipeline_str = format!(
                        "ximagesrc use-damage=0 ! videoconvert ! {}videoscale ! video/x-raw,format={},width={},height={},framerate={}/1 ! queue max-size-buffers=1 leaky=downstream ! {} name=encoder ! {} config-interval=-1 ! appsink name=sink max-buffers=1 drop=true sync=false",
                        crop_str,
                        input_format,
                        config.resolution.width,
                        config.resolution.height,
                        config.fps,
                        encoder_name,
                        parser,
                    );
                    (pipeline_str, None)
                } else {
                    return Err(MediaError::PlatformError(err.to_string()));
                }
            }
        };

        let pipeline = gst::parse::launch(&pipeline_str)
            .map_err(|e| MediaError::GStreamerError(e.to_string()))?
            .downcast::<gst::Pipeline>()
            .map_err(|_| MediaError::GStreamerError("failed to downcast pipeline".to_string()))?;

        let appsink = pipeline
            .by_name("sink")
            .ok_or_else(|| MediaError::GStreamerError("appsink not found".to_string()))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| MediaError::GStreamerError("appsink type mismatch".to_string()))?;

        let encoder_element = pipeline
            .by_name("encoder")
            .ok_or_else(|| MediaError::GStreamerError("encoder element not found".to_string()))?;

        configure_low_latency_encoder(
            &encoder_element,
            &encoder_name,
            config.bitrate_kbps,
            keyframe_interval_frames,
            config.enable_10bit,
        )
        .map_err(|e| MediaError::GStreamerError(e.to_string()))?;

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| MediaError::GStreamerError(e.to_string()))?;

        Ok(Self {
            _fd: fd_opt,
            pipeline,
            appsink,
            encoder_element,
        })
    }

    fn check_bus_errors(&self) -> MediaResult<()> {
        let bus = self
            .pipeline
            .bus()
            .ok_or_else(|| MediaError::GStreamerError("failed to get pipeline bus".to_string()))?;
        if let Some(msg) = bus.pop_filtered(&[gst::MessageType::Error]) {
            if let Some(err) = msg.view().error() {
                let err_msg = format!(
                    "GStreamer error: {} ({})",
                    err.error(),
                    err.debug().unwrap_or_default()
                );
                let err_str = err.error().to_string();

                if err_str.contains("Protocol Error") || err_str.contains("Error 71") {
                    return Err(MediaError::ProtocolViolation(err_msg));
                }
                if err_str.contains("Compositor") || err_str.contains("display connection") {
                    return Err(MediaError::CompositorDisconnect(err_msg));
                }
                if err_str.contains("PipeWire") || err_str.contains("node") {
                    return Err(MediaError::StreamNodeLoss(err_msg));
                }

                return Err(MediaError::GStreamerError(err_msg));
            }
        }
        Ok(())
    }

    pub fn next_frame(&mut self) -> MediaResult<EncodedFrame> {
        let sample = match self.appsink.pull_sample() {
            Ok(s) => s,
            Err(_) => {
                self.check_bus_errors()?;
                return Err(MediaError::GStreamerError(
                    "failed to pull sample (no bus error found)".to_string(),
                ));
            }
        };
        let buffer = sample
            .buffer()
            .ok_or_else(|| MediaError::GStreamerError("missing buffer".to_string()))?;
        let map = buffer
            .map_readable()
            .map_err(|_| MediaError::GStreamerError("buffer map failed".to_string()))?;
        let pts = buffer.pts().map(|t| t.nseconds() / 1_000).unwrap_or(0);
        let keyframe = !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);
        Ok(EncodedFrame {
            timestamp_us: pts,
            keyframe,
            data: map.as_slice().to_vec(),
            capture_duration_us: 0,
            encode_duration_us: 0,
        })
    }

    /// Update encoder bitrate at runtime.
    /// VAAPI encoders support dynamic bitrate changes via the "bitrate" property.
    pub fn set_bitrate(&mut self, bitrate_kbps: u32) -> Result<()> {
        if self.encoder_element.has_property("bitrate", None) {
            self.encoder_element.set_property("bitrate", bitrate_kbps);
        } else if self.encoder_element.has_property("target-bitrate", None) {
            self.encoder_element
                .set_property("target-bitrate", bitrate_kbps);
        }
        log::debug!("Linux encoder bitrate updated to {} kbps", bitrate_kbps);
        Ok(())
    }
}

pub struct GstVideoRenderer {
    #[allow(dead_code)]
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
}

impl GstVideoRenderer {
    pub fn new(config: DecodeConfig) -> Result<Self> {
        gst::init()?;
        let parser = select_parser(config.codec)?;
        require_elements(&[
            "appsrc",
            parser,
            "decodebin",
            "videoconvert",
            "autovideosink",
        ])?;
        require_decoder(config.codec)?;

        let pipeline_str = format!(
            "appsrc name=src is-live=true format=time do-timestamp=true ! {} ! decodebin ! videoconvert ! autovideosink sync=false",
            parser
        );
        let pipeline = gst::parse::launch(&pipeline_str)?
            .downcast::<gst::Pipeline>()
            .map_err(|_| anyhow!("failed to downcast pipeline"))?;
        let appsrc = pipeline
            .by_name("src")
            .ok_or_else(|| anyhow!("appsrc not found"))?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| anyhow!("appsrc type mismatch"))?;

        let caps_str = caps_for_codec(config.codec);
        let caps = gst::Caps::from_str(caps_str)?;
        appsrc.set_caps(Some(&caps));

        pipeline.set_state(gst::State::Playing)?;

        Ok(Self { pipeline, appsrc })
    }

    pub fn push(&self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        let mut buffer = gst::Buffer::with_size(payload.len())?;
        {
            let buffer = buffer
                .get_mut()
                .ok_or_else(|| anyhow!("buffer mut failed"))?;
            buffer.copy_from_slice(0, payload).map_err(|copied| {
                anyhow!("failed to copy buffer slice, copied {} bytes", copied)
            })?;
            buffer.set_pts(gst::ClockTime::from_nseconds(timestamp_us * 1_000));
        }
        self.appsrc.push_buffer(buffer)?;
        Ok(())
    }
}

impl Renderer for GstVideoRenderer {
    fn render(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        self.push(payload, timestamp_us)
    }
}

pub struct PipewireAudioCapturer {
    _fd: Option<OwnedFd>,
    #[allow(dead_code)]
    pipeline: gst::Pipeline,
    appsink: gst_app::AppSink,
}

impl PipewireAudioCapturer {
    pub async fn new() -> MediaResult<Self> {
        Self::new_system_mix().await
    }

    pub async fn new_system_mix() -> MediaResult<Self> {
        Self::new_with_route_linux(PipewireAudioRoute::SystemMix).await
    }

    pub async fn new_microphone() -> MediaResult<Self> {
        Self::new_with_route_linux(PipewireAudioRoute::Microphone).await
    }

    pub async fn new_application(app_name: String) -> MediaResult<Self> {
        Self::new_with_route_linux(PipewireAudioRoute::Application(app_name)).await
    }

    async fn new_with_route_linux(route: PipewireAudioRoute) -> MediaResult<Self> {
        gst::init().map_err(|e| MediaError::GStreamerError(e.to_string()))?;
        let (pipeline_str, fd_opt) = match route {
            PipewireAudioRoute::SystemMix => {
                let portal = open_audio_portal_stream().await;
                match portal {
                    Ok((fd, node_id)) => {
                        require_elements(&[
                            "pipewiresrc",
                            "audioconvert",
                            "audioresample",
                            "opusenc",
                            "appsink",
                        ])
                        .map_err(|e| MediaError::GStreamerError(e.to_string()))?;
                        let pipeline_str = format!(
                            "pipewiresrc fd={} path={} do-timestamp=true ! audioconvert ! audioresample ! opusenc bitrate=128000 frame-size=5 ! appsink name=sink max-buffers=4 drop=true sync=false",
                            fd.as_raw_fd(),
                            node_id
                        );
                        (pipeline_str, Some(fd))
                    }
                    Err(err) => {
                        if element_available("pulsesrc") {
                            require_elements(&[
                                "pulsesrc",
                                "audioconvert",
                                "audioresample",
                                "opusenc",
                                "appsink",
                            ])
                            .map_err(|e| MediaError::GStreamerError(e.to_string()))?;
                            log::warn!(
                                "PipeWire audio portal failed, falling back to PulseAudio: {}",
                                err
                            );
                            let pipeline_str = "pulsesrc ! audioconvert ! audioresample ! opusenc bitrate=128000 frame-size=5 ! appsink name=sink max-buffers=4 drop=true sync=false".to_string();
                            (pipeline_str, None)
                        } else {
                            return Err(MediaError::PortalUnavailable(format!(
                                "audio portal failed and pulsesrc unavailable: {}",
                                err
                            )));
                        }
                    }
                }
            }
            PipewireAudioRoute::Microphone => {
                if element_available("pulsesrc") {
                    require_elements(&[
                        "pulsesrc",
                        "audioconvert",
                        "audioresample",
                        "opusenc",
                        "appsink",
                    ])
                    .map_err(|e| MediaError::GStreamerError(e.to_string()))?;
                    let pipeline_str = "pulsesrc ! audioconvert ! audioresample ! opusenc bitrate=128000 frame-size=5 ! appsink name=sink max-buffers=4 drop=true sync=false".to_string();
                    (pipeline_str, None)
                } else if element_available("autoaudiosrc") {
                    require_elements(&[
                        "autoaudiosrc",
                        "audioconvert",
                        "audioresample",
                        "opusenc",
                        "appsink",
                    ])
                    .map_err(|e| MediaError::GStreamerError(e.to_string()))?;
                    let pipeline_str = "autoaudiosrc ! audioconvert ! audioresample ! opusenc bitrate=128000 frame-size=5 ! appsink name=sink max-buffers=4 drop=true sync=false".to_string();
                    (pipeline_str, None)
                } else {
                    return Err(MediaError::GStreamerError(
                        "no supported microphone source element found (expected pulsesrc or autoaudiosrc)".to_string()
                    ));
                }
            }
            PipewireAudioRoute::Application(app_name) => {
                if element_available("pulsesrc") {
                    require_elements(&[
                        "pulsesrc",
                        "audioconvert",
                        "audioresample",
                        "opusenc",
                        "appsink",
                    ])
                    .map_err(|e| MediaError::GStreamerError(e.to_string()))?;
                    match resolve_pulse_monitor_for_application(&app_name) {
                        Ok(source_name) => {
                            let escaped = gst_escape_property_value(&source_name);
                            let pipeline_str = format!(
                                "pulsesrc device=\"{}\" ! audioconvert ! audioresample ! opusenc bitrate=128000 frame-size=5 ! appsink name=sink max-buffers=4 drop=true sync=false",
                                escaped
                            );
                            log::info!(
                                "application audio route '{}' resolved to Pulse source '{}'",
                                app_name,
                                source_name
                            );
                            (pipeline_str, None)
                        }
                        Err(err) => {
                            log::warn!(
                                "application audio route '{}' resolution failed ({}), using system mix",
                                app_name,
                                err
                            );
                            let portal = open_audio_portal_stream().await;
                            match portal {
                                Ok((fd, node_id)) => {
                                    require_elements(&[
                                        "pipewiresrc",
                                        "audioconvert",
                                        "audioresample",
                                        "opusenc",
                                        "appsink",
                                    ])
                                    .map_err(|e| MediaError::GStreamerError(e.to_string()))?;
                                    let pipeline_str = format!(
                                        "pipewiresrc fd={} path={} do-timestamp=true ! audioconvert ! audioresample ! opusenc bitrate=128000 frame-size=5 ! appsink name=sink max-buffers=4 drop=true sync=false",
                                        fd.as_raw_fd(),
                                        node_id
                                    );
                                    (pipeline_str, Some(fd))
                                }
                                Err(portal_err) => {
                                    if element_available("pulsesrc") {
                                        require_elements(&[
                                            "pulsesrc",
                                            "audioconvert",
                                            "audioresample",
                                            "opusenc",
                                            "appsink",
                                        ])
                                        .map_err(|e| MediaError::GStreamerError(e.to_string()))?;
                                        log::warn!(
                                            "PipeWire audio portal failed, falling back to PulseAudio: {}",
                                            portal_err
                                        );
                                        let pipeline_str = "pulsesrc ! audioconvert ! audioresample ! opusenc bitrate=128000 frame-size=5 ! appsink name=sink max-buffers=4 drop=true sync=false".to_string();
                                        (pipeline_str, None)
                                    } else {
                                        return Err(MediaError::PortalUnavailable(format!(
                                            "audio portal failed and pulsesrc unavailable: {}",
                                            portal_err
                                        )));
                                    }
                                }
                            }
                        }
                    }
                } else {
                    return Err(MediaError::GStreamerError(
                        "application route requires pulsesrc but it is unavailable".to_string(),
                    ));
                }
            }
        };

        let pipeline = gst::parse::launch(&pipeline_str)
            .map_err(|e| MediaError::GStreamerError(e.to_string()))?
            .downcast::<gst::Pipeline>()
            .map_err(|_| MediaError::GStreamerError("failed to downcast pipeline".to_string()))?;

        let appsink = pipeline
            .by_name("sink")
            .ok_or_else(|| MediaError::GStreamerError("appsink not found".to_string()))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| MediaError::GStreamerError("appsink type mismatch".to_string()))?;

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| MediaError::GStreamerError(e.to_string()))?;

        Ok(Self {
            _fd: fd_opt,
            pipeline,
            appsink,
        })
    }

    pub fn next_packet(&mut self) -> MediaResult<EncodedFrame> {
        let sample = self.appsink.pull_sample().map_err(|_| {
            // Check bus for specific errors if possible
            let bus = self.pipeline.bus();
            if let Some(bus) = bus {
                if let Some(msg) = bus.pop_filtered(&[gst::MessageType::Error]) {
                    if let Some(err) = msg.view().error() {
                        return MediaError::StreamNodeLoss(format!(
                            "Audio capture failed: {} ({})",
                            err.error(),
                            err.debug().unwrap_or_default()
                        ));
                    }
                }
            }
            MediaError::GStreamerError("failed to pull audio sample".to_string())
        })?;
        let buffer = sample
            .buffer()
            .ok_or_else(|| MediaError::GStreamerError("missing audio buffer".to_string()))?;
        let map = buffer
            .map_readable()
            .map_err(|_| MediaError::GStreamerError("audio buffer map failed".to_string()))?;
        let pts = buffer.pts().map(|t| t.nseconds() / 1_000).unwrap_or(0);

        Ok(EncodedFrame {
            timestamp_us: pts,
            keyframe: true, // Audio packets are essentially all keyframes in Opus
            data: map.as_slice().to_vec(),
            capture_duration_us: 0,
            encode_duration_us: 0,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PipewireAudioRoute {
    SystemMix,
    Microphone,
    Application(String),
}

fn gst_escape_property_value(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

fn run_pactl(args: &[&str]) -> Result<String> {
    let output = Command::new("pactl")
        .args(args)
        .output()
        .with_context(|| format!("failed to execute pactl {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "pactl {} failed: {}",
            args.join(" "),
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn line_value_after_colon(line: &str, key: &str) -> Option<String> {
    let trimmed = line.trim();
    let value = trimmed.strip_prefix(key)?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_block_index(line: &str, prefix: &str) -> Option<u32> {
    let trimmed = line.trim();
    let value = trimmed.strip_prefix(prefix)?;
    value.trim().parse::<u32>().ok()
}

fn parse_app_property_value(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let eq_idx = trimmed.find('=')?;
    let rhs = trimmed[(eq_idx + 1)..].trim();
    if rhs.is_empty() {
        return None;
    }
    Some(rhs.trim_matches('"').to_string())
}

fn find_sink_index_for_application_from_sink_inputs(
    sink_inputs: &str,
    target: &str,
) -> Option<u32> {
    let mut current_sink_id: Option<u32> = None;
    let mut current_matches = false;
    let mut matched_sink_id: Option<u32> = None;

    for line in sink_inputs.lines() {
        if parse_block_index(line, "Sink Input #").is_some() {
            if current_matches && current_sink_id.is_some() {
                matched_sink_id = current_sink_id;
                break;
            }
            current_sink_id = None;
            current_matches = false;
            continue;
        }

        if let Some(sink_id) = line_value_after_colon(line, "Sink:").and_then(|s| s.parse().ok()) {
            current_sink_id = Some(sink_id);
            continue;
        }

        let trimmed = line.trim();
        if trimmed.starts_with("application.name")
            || trimmed.starts_with("application.process.binary")
            || trimmed.starts_with("application.process.id")
            || trimmed.starts_with("media.name")
        {
            if let Some(value) = parse_app_property_value(line) {
                if value.to_ascii_lowercase().contains(target) {
                    current_matches = true;
                }
            }
        }
    }

    if matched_sink_id.is_none() && current_matches && current_sink_id.is_some() {
        matched_sink_id = current_sink_id;
    }
    matched_sink_id
}

fn find_monitor_source_for_sink_from_sinks(sinks: &str, sink_id: u32) -> Option<String> {
    let mut in_target_sink = false;
    for line in sinks.lines() {
        if let Some(idx) = parse_block_index(line, "Sink #") {
            in_target_sink = idx == sink_id;
            continue;
        }
        if !in_target_sink {
            continue;
        }
        if let Some(source) = line_value_after_colon(line, "Monitor Source:") {
            return Some(source);
        }
    }
    None
}

fn resolve_pulse_monitor_for_application(app_name: &str) -> Result<String> {
    let target = app_name.trim().to_ascii_lowercase();
    if target.is_empty() {
        return Err(anyhow!("application route cannot be empty"));
    }

    let sink_inputs = run_pactl(&["list", "sink-inputs"])?;
    let sink_id = find_sink_index_for_application_from_sink_inputs(&sink_inputs, &target)
        .ok_or_else(|| {
            anyhow!(
                "no matching PulseAudio sink input found for application '{}'",
                app_name
            )
        })?;

    let sinks = run_pactl(&["list", "sinks"])?;
    if let Some(source) = find_monitor_source_for_sink_from_sinks(&sinks, sink_id) {
        return Ok(source);
    }

    Err(anyhow!(
        "unable to resolve monitor source for application '{}' (sink #{})",
        app_name,
        sink_id
    ))
}

pub struct GstAudioRenderer {
    appsrc: gst_app::AppSrc,
    pipeline: gst::Pipeline,
}

impl GstAudioRenderer {
    pub fn new() -> Result<Self> {
        gst::init()?;
        require_elements(&[
            "appsrc",
            "opusdec",
            "audioconvert",
            "audioresample",
            "autoaudiosink",
        ])?;
        let pipeline_str = "appsrc name=src is-live=true format=time ! opusdec ! audioconvert ! audioresample ! autoaudiosink sync=false";
        let pipeline = gst::parse::launch(pipeline_str)?
            .downcast::<gst::Pipeline>()
            .map_err(|_| anyhow!("failed to downcast audio pipeline"))?;

        let appsrc = pipeline
            .by_name("src")
            .ok_or_else(|| anyhow!("appsrc not found"))?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| anyhow!("appsrc type mismatch"))?;

        let caps = gst::Caps::builder("audio/x-opus")
            .field("rate", 48_000i32)
            .field("channels", 2i32)
            .build();
        appsrc.set_caps(Some(&caps));

        pipeline.set_state(gst::State::Playing)?;

        Ok(Self { appsrc, pipeline })
    }
}

impl Renderer for GstAudioRenderer {
    fn render(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        let mut buffer = gst::Buffer::from_mut_slice(payload.to_vec());
        let pts = gst::ClockTime::from_useconds(timestamp_us);
        if let Some(buffer_ref) = buffer.get_mut() {
            buffer_ref.set_pts(pts);
        }
        self.appsrc
            .push_buffer(buffer)
            .map_err(|_| anyhow!("failed to push audio buffer"))?;
        Ok(())
    }
}

impl Drop for GstAudioRenderer {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

pub struct LinuxProbe;

impl crate::CapabilityProbe for LinuxProbe {
    fn supported_encoders(&self) -> Result<Vec<crate::Codec>> {
        Ok(self
            .encoder_capabilities()?
            .into_iter()
            .map(|cap| cap.codec)
            .collect())
    }

    fn encoder_capabilities(&self) -> Result<Vec<crate::VideoCodecCapability>> {
        gst::init()?;
        let mut caps = Vec::new();
        for codec in [Codec::Av1, Codec::Hevc, Codec::H264] {
            let hardware_accelerated = hardware_encoder_available(codec);
            let software_available = software_encoder_available(codec);
            if hardware_accelerated || software_available {
                let supports_hdr10 =
                    hardware_accelerated && matches!(codec, Codec::Av1 | Codec::Hevc);
                caps.push(crate::VideoCodecCapability {
                    codec,
                    hardware_accelerated,
                    supports_10bit: supports_hdr10,
                    supports_hdr10,
                });
            }
        }
        Ok(caps)
    }

    fn supported_decoders(&self) -> Result<Vec<crate::Codec>> {
        gst::init()?;
        let mut codecs = Vec::new();
        for codec in [Codec::Av1, Codec::Hevc, Codec::H264] {
            if decoder_available(codec) {
                codecs.push(codec);
            }
        }
        Ok(codecs)
    }

    fn enumerate_displays(&self) -> Result<Vec<crate::DisplayInfo>> {
        if has_wayland_display() {
            match enumerate_wayland_displays() {
                Ok(displays) if !displays.is_empty() => return Ok(displays),
                Ok(_) => {
                    return Err(anyhow!(
                        "Wayland session detected but portal monitor probe returned no displays. Check xdg-desktop-portal permissions and PipeWire session availability."
                    ));
                }
                Err(err) => {
                    let backend_hint = portal_backend_hint_message();
                    return Err(anyhow!(
                        "Wayland session detected but portal monitor probe failed: {}. Ensure xdg-desktop-portal (with desktop backend) and PipeWire are running. {}",
                        err,
                        backend_hint
                    ));
                }
            }
        }

        if !has_x11_display() {
            return Ok(Vec::new());
        }

        let (conn, screen_num) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen_num].root;
        let resources = conn.randr_get_screen_resources_current(root)?.reply()?;

        let mut displays = Vec::new();
        for output in resources.outputs {
            let info = conn.randr_get_output_info(output, 0)?.reply()?;
            if info.connection != x11rb::protocol::randr::Connection::CONNECTED || info.crtc == 0 {
                continue;
            }
            let crtc = conn.randr_get_crtc_info(info.crtc, 0)?.reply()?;

            let idx = displays.len() as u32;
            let output_name = String::from_utf8_lossy(&info.name).trim().to_string();
            let name = if output_name.is_empty() {
                format!("Display {}", idx)
            } else {
                output_name
            };

            displays.push(crate::DisplayInfo {
                id: idx,
                name,
                resolution: crate::Resolution {
                    width: crtc.width.max(1),
                    height: crtc.height.max(1),
                },
            });
        }

        Ok(displays)
    }
}

fn decoder_available(codec: Codec) -> bool {
    let candidates: &[&str] = match codec {
        Codec::H264 => &["vaapih264dec", "avdec_h264", "openh264dec"],
        Codec::Hevc => &["vaapih265dec", "avdec_h265"],
        Codec::Av1 => &["vaapav1dec", "vaapivav1dec", "av1dec", "dav1ddec"],
    };
    candidates.iter().any(|name| element_available(name))
}

async fn open_audio_portal_stream() -> Result<(OwnedFd, u32)> {
    with_portal_retry("audio", open_audio_portal_stream_inner).await
}

async fn open_audio_portal_stream_inner() -> Result<(OwnedFd, u32)> {
    // Note: This uses the same Screencast portal but with different sources
    // In a real implementation, we might need the Device portal or
    // simply use the Screencast portal with 'Audio' capability.
    let proxy = Screencast::new().await?;
    let session = proxy.create_session().await?;
    proxy
        .select_sources(
            &session,
            CursorMode::Hidden,
            SourceType::Virtual.into(), // Virtual source for system audio capture
            true,                       // Enable audio
            None,
            PersistMode::DoNot,
        )
        .await?;
    let response = proxy.start(&session, None).await?.response()?;
    let fd = proxy.open_pipe_wire_remote(&session).await?;
    // Find the audio stream
    let stream = response
        .streams()
        .iter()
        .find(|s| s.pipe_wire_node_id() > 0) // Simplified check
        .ok_or_else(|| anyhow!("no audio streams returned"))?;
    Ok((fd, stream.pipe_wire_node_id()))
}

fn is_monitor_stream(stream: &Stream) -> bool {
    !matches!(
        stream.source_type(),
        Some(SourceType::Window) | Some(SourceType::Virtual)
    )
}

fn select_portal_monitor_stream(streams: &[Stream], display_id: Option<u32>) -> Result<&Stream> {
    let monitor_streams: Vec<&Stream> = streams
        .iter()
        .filter(|stream| is_monitor_stream(stream))
        .collect();
    if monitor_streams.is_empty() {
        return Err(anyhow!("no monitor streams returned"));
    }

    if let Some(id) = display_id {
        if let Some(stream) = monitor_streams.get(id as usize).copied() {
            return Ok(stream);
        }
        let fallback = monitor_streams[0];
        let fallback_label = fallback.id().unwrap_or("unknown");
        log::warn!(
            "Requested Wayland display {} unavailable ({} monitor streams); falling back to first stream '{}'",
            id,
            monitor_streams.len(),
            fallback_label
        );
        return Ok(fallback);
    }

    Ok(monitor_streams[0])
}

async fn open_portal_stream(display_id: Option<u32>) -> Result<(OwnedFd, u32)> {
    with_portal_retry("screencast", || open_portal_stream_inner(display_id)).await
}

async fn open_portal_stream_inner(display_id: Option<u32>) -> Result<(OwnedFd, u32)> {
    let proxy = Screencast::new().await?;
    let session = proxy.create_session().await?;
    let restore_token = load_restore_token();
    let allow_multiple = display_id.is_some();
    proxy
        .select_sources(
            &session,
            CursorMode::Metadata,
            SourceType::Monitor.into(),
            allow_multiple,
            restore_token.as_deref(),
            PersistMode::ExplicitlyRevoked,
        )
        .await?;
    let response = proxy.start(&session, None).await?.response()?;
    if let Some(token) = response.restore_token() {
        save_restore_token(token)?;
    }
    let fd = proxy.open_pipe_wire_remote(&session).await?;
    let stream = select_portal_monitor_stream(response.streams(), display_id)?;
    let (w, h) = stream.size().unwrap_or((0, 0));
    let label = stream.id().unwrap_or("unknown");
    log::info!(
        "Selected Wayland display stream '{}' ({}x{}, node={})",
        label,
        w,
        h,
        stream.pipe_wire_node_id()
    );
    Ok((fd, stream.pipe_wire_node_id()))
}

async fn with_portal_retry<F, Fut, T>(label: &str, mut op: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    const MAX_ATTEMPTS: usize = 4;
    const BASE_DELAY_MS: u64 = 350;

    let mut last_err = None;
    for attempt in 1..=MAX_ATTEMPTS {
        match op().await {
            Ok(result) => return Ok(result),
            Err(err) => {
                tracing::warn!(
                    "Portal {} attempt {}/{} failed: {}",
                    label,
                    attempt,
                    MAX_ATTEMPTS,
                    err
                );
                last_err = Some(err);
                if attempt < MAX_ATTEMPTS {
                    let delay = BASE_DELAY_MS * (1u64 << (attempt - 1));
                    sleep(Duration::from_millis(delay)).await;
                }
            }
        }
    }
    let err = last_err.unwrap_or_else(|| anyhow!("unknown portal error"));
    let mut hints = Vec::new();
    if has_wayland_display() {
        if !check_portal_service_running() {
            hints.push("xdg-desktop-portal process not detected".to_string());
        }
        if !check_pipewire_running() {
            hints.push("PipeWire process not detected".to_string());
        }
        hints.push(portal_backend_hint_message());
    }
    let hint_suffix = if hints.is_empty() {
        String::new()
    } else {
        format!(" Guidance: {}", hints.join(" | "))
    };
    Err(anyhow!(
        "Portal {} failed after {} attempts. Check permissions and portal availability: {}.{}",
        label,
        MAX_ATTEMPTS,
        err,
        hint_suffix
    ))
}

fn load_restore_token() -> Option<String> {
    let path = token_path()?;
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn save_restore_token(token: &str) -> Result<()> {
    let path = token_path().ok_or_else(|| anyhow!("missing config path"))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).ok();
    }
    fs::write(path, token).context("failed to write restore token")
}

fn token_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".config")))?;
    Some(base.join("wavry").join("portal_restore_token"))
}

#[cfg(test)]
mod tests {
    use super::{
        backend_to_portal_descriptor, expected_portal_backends_from_desktop,
        find_monitor_source_for_sink_from_sinks, find_sink_index_for_application_from_sink_inputs,
    };

    #[test]
    fn find_sink_index_for_application_matches_binary() {
        let sink_inputs = r#"
Sink Input #77
    Driver: PipeWire
    Sink: 2
    Properties:
        application.process.binary = "spotify"

Sink Input #80
    Driver: PipeWire
    Sink: 4
    Properties:
        application.process.binary = "discord"
"#;
        let sink = find_sink_index_for_application_from_sink_inputs(sink_inputs, "discord");
        assert_eq!(sink, Some(4));
    }

    #[test]
    fn find_monitor_source_for_sink_reads_target_block() {
        let sinks = r#"
Sink #2
    State: RUNNING
    Name: alsa_output.usb-foo.analog-stereo
    Monitor Source: alsa_output.usb-foo.analog-stereo.monitor

Sink #4
    State: RUNNING
    Name: alsa_output.pci-0000_00_1f.3.analog-stereo
    Monitor Source: alsa_output.pci-0000_00_1f.3.analog-stereo.monitor
"#;
        let source = find_monitor_source_for_sink_from_sinks(sinks, 4);
        assert_eq!(
            source.as_deref(),
            Some("alsa_output.pci-0000_00_1f.3.analog-stereo.monitor")
        );
    }

    #[test]
    fn expected_portal_backends_match_kde() {
        let backends = expected_portal_backends_from_desktop(Some("KDE"));
        assert_eq!(
            backends,
            vec!["xdg-desktop-portal-kde", "xdg-desktop-portal-gtk"]
        );
    }

    #[test]
    fn expected_portal_backends_match_hyprland() {
        let backends = expected_portal_backends_from_desktop(Some("Hyprland"));
        assert_eq!(
            backends,
            vec![
                "xdg-desktop-portal-hyprland",
                "xdg-desktop-portal-wlr",
                "xdg-desktop-portal-gtk"
            ]
        );
    }

    #[test]
    fn expected_portal_backends_default_when_unknown() {
        let backends = expected_portal_backends_from_desktop(Some("UnknownDesktop"));
        assert_eq!(
            backends,
            vec![
                "xdg-desktop-portal-kde",
                "xdg-desktop-portal-gnome",
                "xdg-desktop-portal-wlr",
                "xdg-desktop-portal-gtk"
            ]
        );
    }

    #[tokio::test]
    async fn test_wayland_capture_smoke() {
        if std::env::var("WAVRY_CI_WAYLAND_CAPTURE_TEST").is_err() {
            return;
        }

        use crate::{Codec, EncodeConfig, Resolution};
        let config = EncodeConfig {
            codec: Codec::H264,
            resolution: Resolution {
                width: 640,
                height: 480,
            },
            fps: 30,
            bitrate_kbps: 1000,
            keyframe_interval_ms: 2000,
            display_id: None,
            enable_10bit: false,
            enable_hdr: false,
        };

        let mut encoder = super::PipewireEncoder::new(config)
            .await
            .expect("Failed to create encoder");
        // Pull a few frames to ensure it's actually working
        for _ in 0..5 {
            let _frame = encoder.next_frame().expect("Failed to get frame");
        }
    }

    #[test]
    fn test_pulse_monitor_resolution_syntax() {
        // This test ensures the parsing logic for pactl output is correct.
        let sink_inputs = r#"
Sink Input #1
    Driver: module-protocol-native.c
    Owner Module: 15
    Client: 22
    Sink: 0
    Sample Specification: s16le 2ch 44100Hz
    Channel Map: front-left,front-right
    Format: pcm, format.sample_format = "\"s16le\""  format.rate = "44100"  format.channels = "2"  format.channel_map = "\"front-left,front-right\""
    Corked: no
    Mute: no
    Volume: front-left: 65536 / 100% / 0.00 dB,   front-right: 65536 / 100% / 0.00 dB
            balance 0.00
    Buffer Latency: 0 usec
    Sink Latency: 0 usec
    Resample method: n/a
    Properties:
        media.name = "Playback"
        application.name = "Spotify"
        application.process.id = "1234"
        application.process.binary = "spotify"
"#;
        let sink_id =
            super::find_sink_index_for_application_from_sink_inputs(sink_inputs, "spotify");
        assert_eq!(sink_id, Some(0));

        let sinks = r#"
Sink #0
    State: RUNNING
    Name: alsa_output.pci-0000_00_1f.3.analog-stereo
    Description: Built-in Audio Analog Stereo
    Driver: module-alsa-card.c
    Sample Specification: s16le 2ch 44100Hz
    Channel Map: front-left,front-right
    Owner Module: 7
    Mute: no
    Volume: front-left: 65536 / 100% / 0.00 dB,   front-right: 65536 / 100% / 0.00 dB
            balance 0.00
    Base Volume: 65536 / 100% / 0.00 dB
    Monitor Source: alsa_output.pci-0000_00_1f.3.analog-stereo.monitor
"#;
        let monitor = super::find_monitor_source_for_sink_from_sinks(sinks, 0);
        assert_eq!(
            monitor.as_deref(),
            Some("alsa_output.pci-0000_00_1f.3.analog-stereo.monitor")
        );
    }

    #[test]
    fn backend_descriptor_mapping_is_stable() {
        assert_eq!(
            backend_to_portal_descriptor("xdg-desktop-portal-kde"),
            Some("kde.portal")
        );
        assert_eq!(
            backend_to_portal_descriptor("xdg-desktop-portal-gnome"),
            Some("gnome.portal")
        );
        assert_eq!(
            backend_to_portal_descriptor("xdg-desktop-portal-wlr"),
            Some("wlr.portal")
        );
        assert_eq!(
            backend_to_portal_descriptor("xdg-desktop-portal-hyprland"),
            Some("hyprland.portal")
        );
        assert_eq!(
            backend_to_portal_descriptor("xdg-desktop-portal-gtk"),
            Some("gtk.portal")
        );
        assert_eq!(
            backend_to_portal_descriptor("xdg-desktop-portal-unknown"),
            None
        );
    }
}
