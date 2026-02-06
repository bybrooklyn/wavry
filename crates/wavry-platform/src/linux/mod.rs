use std::fs;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use anyhow::{anyhow, Context, Result};
use ashpd::desktop::{
    remote_desktop::{DeviceType, KeyState, RemoteDesktop},
    screencast::{CursorMode, Screencast, SourceType},
    PersistMode,
};
use evdev::{
    uinput::VirtualDevice,
    uinput::VirtualDeviceBuilder,
    UinputAbsSetup,
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
use tokio::runtime::Builder as RuntimeBuilder;
use tokio::sync::mpsc;

use wavry_media::{FrameData, FrameFormat, RawFrame};

use crate::{FrameCapturer, InputInjector};

pub struct PipewireCapturer {
    _fd: OwnedFd,
    #[allow(dead_code)]
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

pub enum UinputInjector {
    Uinput(UinputInner),
    Portal(PortalInjector),
}

impl UinputInjector {
    pub fn new() -> Result<Self> {
        if is_wayland_session() {
            if let Ok(portal) = PortalInjector::new() {
                return Ok(UinputInjector::Portal(portal));
            }
        }
        Ok(UinputInjector::Uinput(UinputInner::new()?))
    }
}

impl InputInjector for UinputInjector {
    fn key(&mut self, keycode: u32, pressed: bool) -> Result<()> {
        match self {
            UinputInjector::Uinput(inner) => inner.key(keycode, pressed),
            UinputInjector::Portal(portal) => portal.key(keycode, pressed),
        }
    }

    fn mouse_button(&mut self, button: u8, pressed: bool) -> Result<()> {
        match self {
            UinputInjector::Uinput(inner) => inner.mouse_button(button, pressed),
            UinputInjector::Portal(portal) => portal.mouse_button(button, pressed),
        }
    }

    fn mouse_motion(&mut self, dx: i32, dy: i32) -> Result<()> {
        match self {
            UinputInjector::Uinput(inner) => inner.mouse_motion(dx, dy),
            UinputInjector::Portal(portal) => portal.mouse_motion(dx, dy),
        }
    }

    fn mouse_absolute(&mut self, x: i32, y: i32) -> Result<()> {
        match self {
            UinputInjector::Uinput(inner) => inner.mouse_absolute(x, y),
            UinputInjector::Portal(portal) => portal.mouse_absolute(x, y),
        }
    }
}

pub struct UinputInner {
    device: VirtualDevice,
}

impl UinputInner {
    pub fn new() -> Result<Self> {
        let mut keys = AttributeSet::<Key>::new();
        for code in 0u16..=255u16 {
            keys.insert(Key::new(code));
        }

        let mut rel_axes = AttributeSet::<RelativeAxisType>::new();
        rel_axes.insert(RelativeAxisType::REL_X);
        rel_axes.insert(RelativeAxisType::REL_Y);

        let abs_x = AbsInfo::new(0, 65535, 0, 0, 0, 0);
        let abs_y = AbsInfo::new(0, 65535, 0, 0, 0, 0);

        let device = VirtualDeviceBuilder::new()?
            .name("wavry-uinput")
            .with_keys(&keys)?
            .with_relative_axes(&rel_axes)?
            .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisType::ABS_X, abs_x))?
            .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisType::ABS_Y, abs_y))?
            .build()?;
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

impl InputInjector for UinputInner {
    fn key(&mut self, keycode: u32, pressed: bool) -> Result<()> {
        let value = if pressed { 1 } else { 0 };
        self.emit(InputEvent::new(EventType::KEY, keycode.try_into().unwrap(), value))?;
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

enum PortalEvent {
    Key { keycode: u32, pressed: bool },
    Button { button: i32, pressed: bool },
    Motion { dx: f64, dy: f64 },
    Axis { dx: f64, dy: f64 },
}

struct PortalInjector {
    tx: mpsc::UnboundedSender<PortalEvent>,
    ready: Arc<AtomicBool>,
    last_abs: Option<(i32, i32)>,
}

impl PortalInjector {
    fn new() -> Result<Self> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let ready = Arc::new(AtomicBool::new(false));
        let ready_flag = ready.clone();

        thread::spawn(move || {
            let runtime = RuntimeBuilder::new_current_thread()
                .enable_all()
                .build();
            let runtime = match runtime {
                Ok(rt) => rt,
                Err(err) => {
                    tracing::warn!("Wayland input runtime init failed: {}", err);
                    return;
                }
            };

            runtime.block_on(async move {
                let proxy = match RemoteDesktop::new().await {
                    Ok(proxy) => proxy,
                    Err(err) => {
                        tracing::warn!("RemoteDesktop init failed: {}", err);
                        return;
                    }
                };

                let session = match proxy.create_session().await {
                    Ok(session) => session,
                    Err(err) => {
                        tracing::warn!("RemoteDesktop session failed: {}", err);
                        return;
                    }
                };

                let types = DeviceType::Pointer | DeviceType::Keyboard;
                if let Ok(request) = proxy
                    .select_devices(&session, types, None, PersistMode::DoNot)
                    .await
                {
                    let _ = request.response();
                }

                match proxy.start(&session, None).await {
                    Ok(request) => {
                        if request.response().is_err() {
                            tracing::warn!("RemoteDesktop start denied");
                            return;
                        }
                    }
                    Err(err) => {
                        tracing::warn!("RemoteDesktop start failed: {}", err);
                        return;
                    }
                }

                ready_flag.store(true, Ordering::SeqCst);

                while let Some(event) = rx.recv().await {
                    match event {
                        PortalEvent::Key { keycode, pressed } => {
                            let state = if pressed { KeyState::Pressed } else { KeyState::Released };
                            let _ = proxy.notify_keyboard_keycode(&session, keycode as i32, state).await;
                        }
                        PortalEvent::Button { button, pressed } => {
                            let state = if pressed { KeyState::Pressed } else { KeyState::Released };
                            let _ = proxy.notify_pointer_button(&session, button, state).await;
                        }
                        PortalEvent::Motion { dx, dy } => {
                            let _ = proxy.notify_pointer_motion(&session, dx, dy).await;
                        }
                        PortalEvent::Axis { dx, dy } => {
                            let _ = proxy.notify_pointer_axis(&session, dx, dy, true).await;
                        }
                    }
                }
            });
        });

        Ok(Self {
            tx,
            ready,
            last_abs: None,
        })
    }

    fn ready(&self) -> Result<()> {
        if self.ready.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(anyhow!("Wayland portal input not ready"))
        }
    }

    fn send(&self, event: PortalEvent) -> Result<()> {
        self.ready()?;
        self.tx
            .send(event)
            .map_err(|_| anyhow!("Wayland portal input channel closed"))
    }

    fn pointer_button_code(button: u8) -> i32 {
        match button {
            1 => 0x110,
            2 => 0x112,
            3 => 0x111,
            _ => 0x110,
        }
    }
}

impl InputInjector for PortalInjector {
    fn key(&mut self, keycode: u32, pressed: bool) -> Result<()> {
        self.send(PortalEvent::Key { keycode, pressed })
    }

    fn mouse_button(&mut self, button: u8, pressed: bool) -> Result<()> {
        let code = Self::pointer_button_code(button);
        self.send(PortalEvent::Button { button: code, pressed })
    }

    fn mouse_motion(&mut self, dx: i32, dy: i32) -> Result<()> {
        self.last_abs = None;
        self.send(PortalEvent::Motion {
            dx: dx as f64,
            dy: dy as f64,
        })
    }

    fn mouse_absolute(&mut self, x: i32, y: i32) -> Result<()> {
        if let Some((last_x, last_y)) = self.last_abs {
            let dx = x - last_x;
            let dy = y - last_y;
            self.send(PortalEvent::Motion {
                dx: dx as f64,
                dy: dy as f64,
            })?;
        }
        self.last_abs = Some((x, y));
        Ok(())
    }
}

fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || matches!(std::env::var("XDG_SESSION_TYPE"), Ok(ref v) if v == "wayland")
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
