use std::env;
use std::fs;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
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

use crate::{Codec, DecodeConfig, EncodeConfig, EncodedFrame, Renderer};

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
    pub async fn new(config: EncodeConfig) -> Result<Self> {
        gst::init()?;
        let (encoder_name, input_format) = select_encoder(config.codec, config.enable_10bit)?;
        let parser = select_parser(config.codec)?;
        require_elements(&["videoconvert", "queue", "appsink"])?;
        if !element_available(&encoder_name) {
            return Err(anyhow!(
                "missing GStreamer encoder element: {}",
                encoder_name
            ));
        }
        if !element_available(parser) {
            return Err(anyhow!("missing GStreamer parser element: {}", parser));
        }

        let keyframe_interval_frames =
            ((config.fps as u32 * config.keyframe_interval_ms) / 1000).max(1);

        // Try PipeWire portal first, fallback to X11 capture if available.
        let portal_stream = open_portal_stream(config.display_id).await;

        let (pipeline_str, fd_opt) = match portal_stream {
            Ok((fd, node_id)) => {
                require_elements(&["pipewiresrc"])?;
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
                    require_elements(&required)?;

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
                    return Err(err);
                }
            }
        };

        let pipeline = gst::parse::launch(&pipeline_str)?
            .downcast::<gst::Pipeline>()
            .map_err(|_| anyhow!("failed to downcast pipeline"))?;

        let appsink = pipeline
            .by_name("sink")
            .ok_or_else(|| anyhow!("appsink not found"))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| anyhow!("appsink type mismatch"))?;

        let encoder_element = pipeline
            .by_name("encoder")
            .ok_or_else(|| anyhow!("encoder element not found"))?;

        configure_low_latency_encoder(
            &encoder_element,
            &encoder_name,
            config.bitrate_kbps,
            keyframe_interval_frames,
            config.enable_10bit,
        )?;

        pipeline.set_state(gst::State::Playing)?;

        Ok(Self {
            _fd: fd_opt,
            pipeline,
            appsink,
            encoder_element,
        })
    }

    pub fn next_frame(&mut self) -> Result<EncodedFrame> {
        let sample = self
            .appsink
            .pull_sample()
            .map_err(|_| anyhow!("failed to pull sample"))?;
        let buffer = sample.buffer().ok_or_else(|| anyhow!("missing buffer"))?;
        let map = buffer
            .map_readable()
            .map_err(|_| anyhow!("buffer map failed"))?;
        let pts = buffer.pts().map(|t| t.nseconds() / 1_000).unwrap_or(0);
        let keyframe = !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);
        Ok(EncodedFrame {
            timestamp_us: pts,
            keyframe,
            data: map.as_slice().to_vec(),
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
    pub async fn new() -> Result<Self> {
        gst::init()?;
        let portal = open_audio_portal_stream().await;
        let (pipeline_str, fd_opt) = match portal {
            Ok((fd, node_id)) => {
                require_elements(&[
                    "pipewiresrc",
                    "audioconvert",
                    "audioresample",
                    "opusenc",
                    "appsink",
                ])?;
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
                    ])?;
                    log::warn!(
                        "PipeWire audio portal failed, falling back to PulseAudio: {}",
                        err
                    );
                    let pipeline_str = "pulsesrc ! audioconvert ! audioresample ! opusenc bitrate=128000 frame-size=5 ! appsink name=sink max-buffers=4 drop=true sync=false".to_string();
                    (pipeline_str, None)
                } else {
                    return Err(anyhow!(
                        "audio portal failed and pulsesrc unavailable: {}",
                        err
                    ));
                }
            }
        };

        let pipeline = gst::parse::launch(&pipeline_str)?
            .downcast::<gst::Pipeline>()
            .map_err(|_| anyhow!("failed to downcast pipeline"))?;

        let appsink = pipeline
            .by_name("sink")
            .ok_or_else(|| anyhow!("appsink not found"))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| anyhow!("appsink type mismatch"))?;

        pipeline.set_state(gst::State::Playing)?;

        Ok(Self {
            _fd: fd_opt,
            pipeline,
            appsink,
        })
    }

    pub fn next_packet(&mut self) -> Result<EncodedFrame> {
        let sample = self
            .appsink
            .pull_sample()
            .map_err(|_| anyhow!("failed to pull audio sample"))?;
        let buffer = sample
            .buffer()
            .ok_or_else(|| anyhow!("missing audio buffer"))?;
        let map = buffer
            .map_readable()
            .map_err(|_| anyhow!("audio buffer map failed"))?;
        let pts = buffer.pts().map(|t| t.nseconds() / 1_000).unwrap_or(0);

        Ok(EncodedFrame {
            timestamp_us: pts,
            keyframe: true, // Audio packets are essentially all keyframes in Opus
            data: map.as_slice().to_vec(),
        })
    }
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
                Ok(_) => log::warn!("Wayland portal monitor probe returned no monitors"),
                Err(err) => log::warn!("Wayland portal monitor probe failed: {}", err),
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

fn select_portal_monitor_stream(
    streams: &[Stream],
    display_id: Option<u32>,
) -> Result<&Stream> {
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
    const MAX_ATTEMPTS: usize = 3;
    const BASE_DELAY_MS: u64 = 500;

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
    Err(anyhow!(
        "Portal {} failed after {} attempts. Check permissions and portal availability: {}",
        label,
        MAX_ATTEMPTS,
        err
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
