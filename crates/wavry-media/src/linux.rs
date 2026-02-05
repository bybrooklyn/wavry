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

use crate::{Codec, DecodeConfig, EncodedFrame, EncodeConfig, Renderer};

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

        let encoder_name = match config.codec {
            Codec::Hevc => "vaapih265enc",
            Codec::H264 => "vaapih264enc",
        };
        let parser = match config.codec {
            Codec::Hevc => "h265parse",
            Codec::H264 => "h264parse",
        };

        if gst::ElementFactory::find(encoder_name).is_none() {
            return Err(anyhow!("missing GStreamer encoder element: {encoder_name}"));
        }
        if gst::ElementFactory::find(parser).is_none() {
            return Err(anyhow!("missing GStreamer parser element: {parser}"));
        }

        let keyframe_interval_frames = ((config.fps as u32 * config.keyframe_interval_ms) / 1000)
            .max(1);
        
        // Use named encoder element for later property access
        let pipeline_str = format!(
            "pipewiresrc fd={} path={} do-timestamp=true ! videoconvert ! video/x-raw,format=NV12,width={},height={},framerate={}/1 ! {} name=encoder bitrate={} keyframe-period={} ! {} config-interval=-1 ! appsink name=sink max-buffers=1 drop=true sync=false",
            fd.as_raw_fd(),
            node_id,
            config.resolution.width,
            config.resolution.height,
            config.fps,
            encoder_name,
            config.bitrate_kbps,
            keyframe_interval_frames,
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
        let map = buffer.map_readable().map_err(|_| anyhow!("buffer map failed"))?;
        let pts = buffer
            .pts()
            .map(|t| t.nseconds() / 1_000)
            .unwrap_or(0);
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
        // VAAPI encoder bitrate property is in kbps
        self.encoder_element.set_property("bitrate", bitrate_kbps);
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
        let parser = match config.codec {
            Codec::Hevc => "h265parse",
            Codec::H264 => "h264parse",
        };

        if gst::ElementFactory::find(parser).is_none() {
            return Err(anyhow!("missing GStreamer parser element: {parser}"));
        }

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

        let caps_str = match config.codec {
            Codec::Hevc => "video/x-h265,stream-format=(string)byte-stream,alignment=(string)au",
            Codec::H264 => "video/x-h264,stream-format=(string)byte-stream,alignment=(string)au",
        };
        let caps = gst::Caps::from_str(caps_str)?;
        appsrc.set_caps(Some(&caps));

        pipeline.set_state(gst::State::Playing)?;

        Ok(Self { pipeline, appsrc })
    }

    pub fn push(&self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        let mut buffer = gst::Buffer::with_size(payload.len())?;
        {
            let buffer = buffer.get_mut().ok_or_else(|| anyhow!("buffer mut failed"))?;
            buffer
                .copy_from_slice(0, payload)
                .map_err(|copied| anyhow!("failed to copy buffer slice, copied {} bytes", copied))?;
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
            "pipewiresrc fd={} path={} do-timestamp=true ! audioconvert ! audioresample ! opusenc bitrate=128000 ! appsink name=sink max-buffers=10 drop=true sync=false",
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
        let buffer = sample.buffer().ok_or_else(|| anyhow!("missing audio buffer"))?;
        let map = buffer.map_readable().map_err(|_| anyhow!("audio buffer map failed"))?;
        let pts = buffer
            .pts()
            .map(|t| t.nseconds() / 1_000)
            .unwrap_or(0);
        
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
            true, // Enable audio
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
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| {
        std::env::var_os("HOME").map(|home| Path::new(&home).join(".config"))
    })?;
    Some(base.join("wavry").join("portal_restore_token"))
}
