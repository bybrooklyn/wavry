use anyhow::Result;
use crate::{InputInjector, FrameCapturer};
use wavry_media::RawFrame;
use tracing::info;

pub struct DummyInjector;

impl DummyInjector {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

impl InputInjector for DummyInjector {
    fn key(&mut self, keycode: u32, pressed: bool) -> Result<()> {
        info!("DummyInjector: Key {} {}", keycode, if pressed { "pressed" } else { "released" });
        Ok(())
    }

    fn mouse_button(&mut self, button: u8, pressed: bool) -> Result<()> {
        info!("DummyInjector: Mouse Button {} {}", button, if pressed { "pressed" } else { "released" });
        Ok(())
    }

    fn mouse_motion(&mut self, dx: i32, dy: i32) -> Result<()> {
        info!("DummyInjector: Mouse Motion {}, {}", dx, dy);
        Ok(())
    }

    fn mouse_absolute(&mut self, x: i32, y: i32) -> Result<()> {
        info!("DummyInjector: Mouse Absolute {}, {}", x, y);
        Ok(())
    }
}

pub struct DummyCapturer;

impl DummyCapturer {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

impl FrameCapturer for DummyCapturer {
    fn capture(&mut self) -> Result<RawFrame> {
        anyhow::bail!("DummyCapturer not implemented")
    }
}
