#![forbid(unsafe_code)]

use anyhow::{bail, Result};
use wavry_media::RawFrame;

pub trait FrameCapturer: Send {
    fn capture(&mut self) -> Result<RawFrame>;
}

pub trait InputInjector: Send {
    fn key(&mut self, keycode: u32, pressed: bool) -> Result<()>;
    fn mouse_button(&mut self, button: u8, pressed: bool) -> Result<()>;
    fn mouse_motion(&mut self, dx: i32, dy: i32) -> Result<()>;
    fn mouse_absolute(&mut self, x: f32, y: f32) -> Result<()>;
    fn scroll(&mut self, dx: f32, dy: f32) -> Result<()>;
    fn gamepad(&mut self, gamepad_id: u32, axes: &[(u32, f32)], buttons: &[(u32, bool)]) -> Result<()>;
}

pub struct UnsupportedCapturer;

impl FrameCapturer for UnsupportedCapturer {
    fn capture(&mut self) -> Result<RawFrame> {
        bail!("frame capture is not implemented for this platform")
    }
}

pub struct UnsupportedInjector;

impl InputInjector for UnsupportedInjector {
    fn key(&mut self, _keycode: u32, _pressed: bool) -> Result<()> {
        bail!("input injection is not implemented for this platform")
    }

    fn mouse_button(&mut self, _button: u8, _pressed: bool) -> Result<()> {
        bail!("input injection is not implemented for this platform")
    }

    fn mouse_motion(&mut self, _dx: i32, _dy: i32) -> Result<()> {
        bail!("input injection is not implemented for this platform")
    }

    fn mouse_absolute(&mut self, _x: f32, _y: f32) -> Result<()> {
        bail!("input injection is not implemented for this platform")
    }

    fn scroll(&mut self, _dx: f32, _dy: f32) -> Result<()> {
        bail!("input injection is not implemented for this platform")
    }

    fn gamepad(&mut self, _gamepad_id: u32, _axes: &[(u32, f32)], _buttons: &[(u32, bool)]) -> Result<()> {
        bail!("input injection is not implemented for this platform")
    }
}

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{PipewireCapturer, UinputInjector};

#[cfg(target_os = "windows")]
mod windows_input_injector;

#[cfg(target_os = "windows")]
pub use windows_input_injector::WindowsInjector;

mod dummy;
pub use dummy::{DummyCapturer, DummyInjector};
