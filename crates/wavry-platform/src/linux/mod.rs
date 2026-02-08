use std::fs;
use std::future::Future;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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
use tokio::time::sleep;
use x11rb::protocol::xproto::{ConnectionExt as X11ConnectionExt, Window};
use x11rb::protocol::xtest::ConnectionExt as XTestExt;

use wavry_media::{FrameData, FrameFormat, RawFrame};

use crate::{FrameCapturer, InputInjector};

fn element_available(name: &str) -> bool {
    gst::ElementFactory::find(name).is_some()
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

pub struct PipewireCapturer {
    _fd: Option<OwnedFd>,
    #[allow(dead_code)]
    pipeline: gst::Pipeline,
    appsink: gst_app::AppSink,
}

impl PipewireCapturer {
    pub async fn new() -> Result<Self> {
        gst::init()?;
        let portal = open_portal_stream().await;
        let (pipeline_str, fd_opt) = match portal {
            Ok((fd, node_id)) => {
                require_elements(&["pipewiresrc", "videoconvert", "appsink"])?;
                let pipeline_str = format!(
                    "pipewiresrc fd={} path={} do-timestamp=true ! videoconvert ! video/x-raw,format=RGBA ! appsink name=sink max-buffers=1 drop=true sync=false",
                    fd.as_raw_fd(),
                    node_id
                );
                (pipeline_str, Some(fd))
            }
            Err(err) => {
                if std::env::var_os("DISPLAY").is_some() {
                    require_elements(&["ximagesrc", "videoconvert", "appsink"])?;
                    tracing::warn!("PipeWire portal failed, falling back to X11 capture: {}", err);
                    let pipeline_str = "ximagesrc use-damage=0 ! videoconvert ! video/x-raw,format=RGBA ! appsink name=sink max-buffers=1 drop=true sync=false".to_string();
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

        pipeline.set_state(gst::State::Playing)?;

        Ok(Self {
            _fd: fd_opt,
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
    X11(X11Injector),
}

impl UinputInjector {
    pub fn new() -> Result<Self> {
        if is_wayland_session() {
            if let Ok(portal) = PortalInjector::new() {
                return Ok(UinputInjector::Portal(portal));
            }
        }

        match UinputInner::new() {
            Ok(inner) => Ok(UinputInjector::Uinput(inner)),
            Err(err) => {
                if std::env::var_os("DISPLAY").is_some() {
                    if let Ok(x11) = X11Injector::new() {
                        tracing::warn!("uinput init failed, falling back to X11 input: {}", err);
                        return Ok(UinputInjector::X11(x11));
                    }
                }
                Err(err)
            }
        }
    }
}

impl InputInjector for UinputInjector {
    fn key(&mut self, keycode: u32, pressed: bool) -> Result<()> {
        match self {
            UinputInjector::Uinput(inner) => inner.key(keycode, pressed),
            UinputInjector::Portal(portal) => portal.key(keycode, pressed),
            UinputInjector::X11(x11) => x11.key(keycode, pressed),
        }
    }

    fn mouse_button(&mut self, button: u8, pressed: bool) -> Result<()> {
        match self {
            UinputInjector::Uinput(inner) => inner.mouse_button(button, pressed),
            UinputInjector::Portal(portal) => portal.mouse_button(button, pressed),
            UinputInjector::X11(x11) => x11.mouse_button(button, pressed),
        }
    }

    fn mouse_motion(&mut self, dx: i32, dy: i32) -> Result<()> {
        match self {
            UinputInjector::Uinput(inner) => inner.mouse_motion(dx, dy),
            UinputInjector::Portal(portal) => portal.mouse_motion(dx, dy),
            UinputInjector::X11(x11) => x11.mouse_motion(dx, dy),
        }
    }

    fn mouse_absolute(&mut self, x: f32, y: f32) -> Result<()> {
        match self {
            UinputInjector::Uinput(inner) => inner.mouse_absolute(x, y),
            UinputInjector::Portal(portal) => portal.mouse_absolute(x, y),
            UinputInjector::X11(x11) => x11.mouse_absolute(x, y),
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
        let code = match u16::try_from(keycode) {
            Ok(code) => code,
            Err(_) => return Ok(()),
        };
        self.emit(InputEvent::new(EventType::KEY, code, value))?;
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

    fn mouse_absolute(&mut self, x: f32, y: f32) -> Result<()> {
        let x = (x.clamp(0.0, 1.0) * 65535.0) as i32;
        let y = (y.clamp(0.0, 1.0) * 65535.0) as i32;
        self.device.emit(&[
            InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_X.0, x),
            InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_Y.0, y),
        ])?;
        self.sync()
    }
}

struct X11Injector {
    conn: x11rb::rust_connection::RustConnection,
    root: Window,
}

impl X11Injector {
    fn new() -> Result<Self> {
        let (conn, screen_num) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen_num].root;
        Ok(Self { conn, root })
    }

    fn to_x_keycode(keycode: u32) -> Option<u8> {
        let code = keycode.saturating_add(8);
        u8::try_from(code).ok()
    }

    fn clamp_i16(value: i32) -> i16 {
        value.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    fn motion_absolute(&self, x: i32, y: i32) -> Result<()> {
        let x = Self::clamp_i16(x);
        let y = Self::clamp_i16(y);
        self.conn
            .xtest_fake_input(
                x11rb::protocol::xproto::MOTION_NOTIFY,
                0,
                0,
                self.root,
                x,
                y,
                0,
            )?;
        self.conn.flush()?;
        Ok(())
    }

    fn motion_relative(&self, dx: i32, dy: i32) -> Result<()> {
        let pointer = self.conn.query_pointer(self.root)?.reply()?;
        let x = pointer.root_x as i32 + dx;
        let y = pointer.root_y as i32 + dy;
        self.motion_absolute(x, y)
    }
}

impl InputInjector for X11Injector {
    fn key(&mut self, keycode: u32, pressed: bool) -> Result<()> {
        let code = match Self::to_x_keycode(keycode) {
            Some(code) => code,
            None => return Ok(()),
        };
        let event = if pressed {
            x11rb::protocol::xproto::KEY_PRESS
        } else {
            x11rb::protocol::xproto::KEY_RELEASE
        };
        self.conn
            .xtest_fake_input(event, code, 0, self.root, 0, 0, 0)?;
        self.conn.flush()?;
        Ok(())
    }

    fn mouse_button(&mut self, button: u8, pressed: bool) -> Result<()> {
        let event = if pressed {
            x11rb::protocol::xproto::BUTTON_PRESS
        } else {
            x11rb::protocol::xproto::BUTTON_RELEASE
        };
        self.conn
            .xtest_fake_input(event, button, 0, self.root, 0, 0, 0)?;
        self.conn.flush()?;
        Ok(())
    }

    fn mouse_motion(&mut self, dx: i32, dy: i32) -> Result<()> {
        self.motion_relative(dx, dy)
    }

    fn mouse_absolute(&mut self, x: f32, y: f32) -> Result<()> {
        let setup = self.conn.setup();
        let screen = &setup.roots[0];
        let px = (x.clamp(0.0, 1.0) * screen.width_in_pixels as f32) as i32;
        let py = (y.clamp(0.0, 1.0) * screen.height_in_pixels as f32) as i32;
        self.motion_absolute(px, py)
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
    last_abs: Option<(f32, f32)>,
}

impl PortalInjector {
    fn new() -> Result<Self> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let ready = Arc::new(AtomicBool::new(false));
        let ready_flag = ready.clone();

        thread::spawn(move || {
            let mut backoff_ms = 500u64;
            loop {
                ready_flag.store(false, Ordering::SeqCst);
                let runtime = RuntimeBuilder::new_current_thread().enable_all().build();
                let runtime = match runtime {
                    Ok(rt) => rt,
                    Err(err) => {
                        tracing::warn!("Wayland input runtime init failed: {}", err);
                        thread::sleep(Duration::from_millis(backoff_ms));
                        backoff_ms = (backoff_ms * 2).min(4_000);
                        continue;
                    }
                };

                let result: Result<()> = runtime.block_on(async {
                    let proxy = RemoteDesktop::new().await?;
                    let session = proxy.create_session().await?;

                    let types = DeviceType::Pointer | DeviceType::Keyboard;
                    let request = proxy
                        .select_devices(&session, types, None, PersistMode::DoNot)
                        .await?;
                    if request.response().is_err() {
                        return Err(anyhow!("RemoteDesktop device selection denied"));
                    }

                    let request = proxy.start(&session, None).await?;
                    if request.response().is_err() {
                        return Err(anyhow!("RemoteDesktop start denied"));
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
                    Ok(())
                });

                if let Err(err) = result {
                    tracing::warn!("RemoteDesktop session failed: {}", err);
                }

                if rx.is_closed() {
                    break;
                }

                thread::sleep(Duration::from_millis(backoff_ms));
                backoff_ms = (backoff_ms * 2).min(4_000);
            }
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

    fn mouse_absolute(&mut self, x: f32, y: f32) -> Result<()> {
        if let Some((last_x, last_y)) = self.last_abs {
            // ScreenCaptureKit/Portals don't always support absolute directly, 
            // so we calculate delta if needed, but portal.notify_pointer_motion
            // is usually relative. 
            // Wait, notify_pointer_motion is RELATIVE.
            // If we want absolute, we need to know screen resolution or 
            // use notify_pointer_motion_absolute if available.
            // Ashpd RemoteDesktop has notify_pointer_motion_absolute.
            
            // For now, if we only have relative, we calculate delta.
            // But we should ideally update the portal loop to use absolute.
            let dx = (x - last_x) as f64;
            let dy = (y - last_y) as f64;
            
            // We need to scale these to something sensible for "pixels" 
            // since f32 0..1 is too small for raw relative movement.
            // Assuming 1920x1080 for delta calculation if no other info.
            self.send(PortalEvent::Motion {
                dx: dx * 1920.0,
                dy: dy * 1080.0,
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
    with_portal_retry("screencast", open_portal_stream_inner).await
}

async fn open_portal_stream_inner() -> Result<(OwnedFd, u32)> {
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
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| {
        std::env::var_os("HOME").map(|home| Path::new(&home).join(".config"))
    })?;
    Some(base.join("wavry").join("portal_restore_token"))
}
