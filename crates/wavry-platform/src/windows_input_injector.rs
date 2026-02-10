use crate::InputInjector;
use anyhow::Result;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

pub struct WindowsInjector;

impl WindowsInjector {
    pub fn new() -> Self {
        Self
    }
}

impl InputInjector for WindowsInjector {
    fn key(&mut self, keycode: u32, pressed: bool) -> Result<()> {
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(keycode as u16),
                    wScan: 0,
                    dwFlags: if pressed {
                        KEYBD_EVENT_FLAGS(0)
                    } else {
                        KEYEVENTF_KEYUP
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
        Ok(())
    }

    fn mouse_button(&mut self, button: u8, pressed: bool) -> Result<()> {
        let flags = match (button, pressed) {
            (1, true) => MOUSEEVENTF_LEFTDOWN,
            (1, false) => MOUSEEVENTF_LEFTUP,
            (2, true) => MOUSEEVENTF_RIGHTDOWN,
            (2, false) => MOUSEEVENTF_RIGHTUP,
            (3, true) => MOUSEEVENTF_MIDDLEDOWN,
            (3, false) => MOUSEEVENTF_MIDDLEUP,
            _ => return Ok(()),
        };

        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
        Ok(())
    }

    fn mouse_motion(&mut self, dx: i32, dy: i32) -> Result<()> {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
        Ok(())
    }

    fn mouse_absolute(&mut self, x: f32, y: f32) -> Result<()> {
        // SendInput absolute coordinates are in the range 0..65535
        let ax = (x.clamp(0.0, 1.0) * 65535.0) as i32;
        let ay = (y.clamp(0.0, 1.0) * 65535.0) as i32;
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: ax,
                    dy: ay,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
        Ok(())
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> Result<()> {
        // Vertical scroll
        if dy.abs() > 0.001 {
            let wheel_delta = (dy * 120.0) as i32;
            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: wheel_delta as u32,
                        dwFlags: MOUSEEVENTF_WHEEL,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            unsafe {
                SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
            }
        }

        // Horizontal scroll
        if dx.abs() > 0.001 {
            let wheel_delta = (dx * 120.0) as i32;
            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: wheel_delta as u32,
                        dwFlags: MOUSEEVENTF_HWHEEL,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            unsafe {
                SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
            }
        }
        Ok(())
    }

    fn gamepad(
        &mut self,
        _gamepad_id: u32,
        _axes: &[(u32, f32)],
        _buttons: &[(u32, bool)],
    ) -> Result<()> {
        // Gamepad support on Windows would require XInput or Raw Input API
        // For now, we provide a stub that doesn't error but doesn't do anything
        // Future implementation: use XInput to inject gamepad input
        Ok(())
    }
}
