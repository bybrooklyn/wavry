use std::fs;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{anyhow, Context, Result};
use ashpd::desktop::{
    screencast::{CursorMode, Screencast, SourceType},
    PersistMode,
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gst::glib;

use crate::{Codec, DecodeConfig, EncodeConfig, EncodedFrame, Renderer};

fn element_available(name: &str) -> bool {
    gst::ElementFactory::find(name).is_some()
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

fn select_encoder(codec: Codec) -> Result<(String, &'static str)> {
    let candidates: &[&str] = match codec {
        Codec::H264 => &["vaapih264enc", "x264enc", "openh264enc"],
        Codec::Hevc => &["vaapih265enc", "x265enc"],
        Codec::Av1 => &["vaapav1enc", "vaapivav1enc", "svtav1enc", "av1enc", "rav1enc"],
    };
    let encoder = candidates
        .iter()
        .find(|name| element_available(name))
        .ok_or_else(|| anyhow!("missing GStreamer encoder element for {codec:?}"))?;
    let input_format = if encoder.starts_with("vaapi") { "NV12" } else { "I420" };
    Ok((encoder.to_string(), input_format))
}

fn configure_low_latency_encoder(
    encoder: &gst::Element,
    encoder_name: &str,
    bitrate_kbps: u32,
    keyframe_interval_frames: u32,
) -> Result<()> {
    let set_if_exists = |name: &str, value: impl glib::ToValue| {
        if encoder.has_property(name, None) {
            encoder.set_property(name, value);
        }
    };

    set_if_exists("bitrate", bitrate_kbps);
    set_if_exists("target-bitrate", bitrate_kbps);
    set_if_exists("keyframe-period", keyframe_interval_frames);
    set_if_exists("key-int-max", keyframe_interval_frames as i32);

    if encoder_name.contains("x264") {
        set_if_exists("tune", "zerolatency");
        set_if_exists("speed-preset", "ultrafast");
        set_if_exists("bframes", 0i32);
    } else if encoder_name.contains("x265") {
        set_if_exists("tune", "zerolatency");
        set_if_exists("speed-preset", "ultrafast");
        set_if_exists("bframes", 0i32);
    } else if encoder_name.contains("svtav1") {
        set_if_exists("preset", 8i32);
        set_if_exists("tune", 0i32);
    } else if encoder_name.contains("vaapi") {
        set_if_exists("rate-control", "cbr");
        set_if_exists("max-bframes", 0i32);
        set_if_exists("cabac", false);
    }

    Ok(())
}

pub struct PipewireEncoder {
    _fd: OwnedFd,
    #[allow(dead_code)]
    pipeline: gst::Pipeline,
    appsink: gst_app::AppSink,
    encoder_element: gst::Element,
}

impl PipewireEncoder {
    pub async fn new(config: EncodeConfig) -> Result<Self> {
        gst::init()?;
        let (fd, node_id) = open_portal_stream().await?;

        let (encoder_name, input_format) = select_encoder(config.codec)?;
        let parser = select_parser(config.codec)?;

        let keyframe_interval_frames =
            ((config.fps as u32 * config.keyframe_interval_ms) / 1000).max(1);

        // Use named encoder element for later property access
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
            encoder_name,
            config.bitrate_kbps,
            keyframe_interval_frames,
        )?;

        pipeline.set_state(gst::State::Playing)?;

        Ok(Self {
            _fd: fd,
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
            self.encoder_element.set_property("target-bitrate", bitrate_kbps);
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
    _fd: OwnedFd,
    #[allow(dead_code)]
    pipeline: gst::Pipeline,
    appsink: gst_app::AppSink,
}

impl PipewireAudioCapturer {
    pub async fn new() -> Result<Self> {
        gst::init()?;
        let (fd, node_id) = open_audio_portal_stream().await?;

        let pipeline_str = format!(
            "pipewiresrc fd={} path={} do-timestamp=true ! audioconvert ! audioresample ! opusenc bitrate=128000 frame-size=5 ! appsink name=sink max-buffers=4 drop=true sync=false",
            fd.as_raw_fd(),
            node_id
        );

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
            _fd: fd,
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
        let pipeline_str = "appsrc name=src is-live=true format=time ! opusdec ! audioconvert ! audioresample ! autoaudiosink sync=false";
        let pipeline = gst::parse::launch(pipeline_str)?
            .downcast::<gst::Pipeline>()
            .map_err(|_| anyhow!("failed to downcast audio pipeline"))?;

        let appsrc = pipeline
            .by_name("src")
            .ok_or_else(|| anyhow!("appsrc not found"))?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| anyhow!("appsrc type mismatch"))?;

        pipeline.set_state(gst::State::Playing)?;

        Ok(Self { appsrc, pipeline })
    }
}

impl Renderer for GstAudioRenderer {
    fn render(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        let mut buffer = gst::Buffer::from_mut_slice(payload.to_vec());
        let pts = gst::ClockTime::from_useconds(timestamp_us);
        {
            let buffer_ref = buffer.get_mut().unwrap();
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
        gst::init()?;
        let mut codecs = Vec::new();
        for codec in [Codec::Av1, Codec::Hevc, Codec::H264] {
            if select_encoder(codec).is_ok() {
                codecs.push(codec);
            }
        }
        Ok(codecs)
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
        Ok(Vec::new())
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

async fn open_portal_stream() -> Result<(OwnedFd, u32)> {
    let proxy = Screencast::new().await?;
    let session = proxy.create_session().await?;
    let restore_token = load_restore_token();
    proxy
        .select_sources(
            &session,
            CursorMode::Metadata,
            SourceType::Monitor.into(),
            false,
            restore_token.as_deref(),
            PersistMode::ExplicitlyRevoked,
        )
        .await?;
    let response = proxy.start(&session, None).await?.response()?;
    if let Some(token) = response.restore_token() {
        save_restore_token(token)?;
    }
    let fd = proxy.open_pipe_wire_remote(&session).await?;
    let stream = response
        .streams()
        .get(0)
        .ok_or_else(|| anyhow!("no screencast streams returned"))?;
    Ok((fd, stream.pipe_wire_node_id()))
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
