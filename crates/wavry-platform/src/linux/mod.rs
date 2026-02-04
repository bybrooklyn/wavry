use std::fs;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use ashpd::desktop::screencast::{CursorMode, PersistMode, Screencast, SourceType};
use evdev::{
    uinput::VirtualDevice,
    uinput::VirtualDeviceBuilder,
    AbsInfo,
    AbsoluteAxisType,
    AttributeSet,
    EventType,
    InputEvent,
    Key,
    RelativeAxisType,
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;

use wavry_media::{FrameData, FrameFormat, RawFrame};

use crate::{FrameCapturer, InputInjector};

pub struct PipewireCapturer {
    _fd: OwnedFd,
    pipeline: gst::Pipeline,
    appsink: gst_app::AppSink,
}

impl PipewireCapturer {
    pub async fn new() -> Result<Self> {
        gst::init()?;
        let (fd, node_id) = open_portal_stream().await?;
        let pipeline_str = format!(
            "pipewiresrc fd={} path={} do-timestamp=true ! videoconvert ! video/x-raw,format=RGBA ! appsink name=sink max-buffers=1 drop=true sync=false",
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
}

impl FrameCapturer for PipewireCapturer {
    fn capture(&mut self) -> Result<RawFrame> {
        let sample = self
            .appsink
            .pull_sample()
            .map_err(|_| anyhow!("failed to pull sample"))?;
        let buffer = sample.buffer().ok_or_else(|| anyhow!("missing buffer"))?;
        let map = buffer.map_readable().map_err(|_| anyhow!("buffer map failed"))?;
        let caps = sample.caps().ok_or_else(|| anyhow!("missing caps"))?;
        let info = gst_video::VideoInfo::from_caps(&caps)?;
        let pts = buffer
            .pts()
            .map(|t| t.nseconds() / 1_000)
            .unwrap_or(0);

        Ok(RawFrame {
            width: info.width() as u16,
            height: info.height() as u16,
            format: FrameFormat::Rgba8,
            timestamp_us: pts,
            data: FrameData::Cpu {
                bytes: map.as_slice().to_vec(),
                stride: info.stride()[0] as u32,
            },
        })
    }
}

pub struct UinputInjector {
    device: VirtualDevice,
}

impl UinputInjector {
    pub fn new() -> Result<Self> {
        let mut keys = AttributeSet::<Key>::new();
        for code in 0u16..=255u16 {
            keys.insert(Key::new(code));
        }

        let mut rel_axes = AttributeSet::<RelativeAxisType>::new();
        rel_axes.insert(RelativeAxisType::REL_X);
        rel_axes.insert(RelativeAxisType::REL_Y);

        let mut builder = VirtualDeviceBuilder::new()?;
        builder.name("wavry-uinput")?;
        builder.with_keys(&keys)?;
        builder.with_relative_axes(&rel_axes)?;

        let abs_x = AbsInfo::new(0, 65535, 0, 0, 0);
        let abs_y = AbsInfo::new(0, 65535, 0, 0, 0);
        builder.with_absolute_axis(AbsoluteAxisType::ABS_X, abs_x)?;
        builder.with_absolute_axis(AbsoluteAxisType::ABS_Y, abs_y)?;

        let device = builder.build()?;
        Ok(Self { device })
    }

    fn emit(&mut self, event: InputEvent) -> Result<()> {
        self.device.emit(&[event])?;
        Ok(())
    }
    
    /// Emit SYN_REPORT to synchronize input events.
    /// Required for proper input device operation on Linux.
    fn sync(&mut self) -> Result<()> {
        // SYN_REPORT = type 0, code 0, value 0
        self.device.emit(&[InputEvent::new(EventType::SYNCHRONIZATION, 0, 0)])?;
        Ok(())
    }
}

impl InputInjector for UinputInjector {
    fn key(&mut self, keycode: u32, pressed: bool) -> Result<()> {
        let value = if pressed { 1 } else { 0 };
        self.emit(InputEvent::new(EventType::KEY, keycode, value))?;
        self.sync()
    }

    fn mouse_button(&mut self, button: u8, pressed: bool) -> Result<()> {
        let value = if pressed { 1 } else { 0 };
        let code = match button {
            1 => 0x110,
            2 => 0x112,
            3 => 0x111,
            _ => 0x110,
        };
        self.emit(InputEvent::new(EventType::KEY, code, value))?;
        self.sync()
    }

    fn mouse_motion(&mut self, dx: i32, dy: i32) -> Result<()> {
        self.device.emit(&[
            InputEvent::new(EventType::RELATIVE, RelativeAxisType::REL_X.0, dx),
            InputEvent::new(EventType::RELATIVE, RelativeAxisType::REL_Y.0, dy),
        ])?;
        self.sync()
    }

    fn mouse_absolute(&mut self, x: i32, y: i32) -> Result<()> {
        self.device.emit(&[
            InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_X.0, x),
            InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_Y.0, y),
        ])?;
        self.sync()
    }
}

async fn open_portal_stream() -> Result<(OwnedFd, u32)> {
    let proxy = Screencast::new().await?;
    let session = proxy.create_session().await?;
    let restore_token = load_restore_token();
    proxy
        .select_sources(
            &session,
            CursorMode::Metadata,
            SourceType::Monitor,
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
