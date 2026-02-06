#![allow(dead_code)]
use crate::{InputEvent, MouseButton};
use anyhow::Result;
use std::ffi::c_void;
use std::ptr::null;

// Manual struct defs to avoid import issues
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreateMouseEvent(
        source: *const c_void, // CGEventSourceRef
        mouse_type: u32,       // CGEventType
        mouse_cursor_position: CGPoint,
        mouse_button: u32, // CGMouseButton
    ) -> *mut c_void; // CGEventRef

    fn CGEventCreateKeyboardEvent(
        source: *const c_void,
        keycode: u16, // CGKeyCode
        keydown: bool,
    ) -> *mut c_void;

    fn CGEventPost(
        tap: u32, // CGEventTapLocation
        event: *mut c_void,
    );
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

// CGEventType constants
const K_CG_EVENT_NULL: u32 = 0;
const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
const K_CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
const K_CG_EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
const K_CG_EVENT_RIGHT_MOUSE_UP: u32 = 4;
const K_CG_EVENT_MOUSE_MOVED: u32 = 5;
const K_CG_EVENT_LEFT_MOUSE_DRAGGED: u32 = 6;
const K_CG_EVENT_RIGHT_MOUSE_DRAGGED: u32 = 7;
const K_CG_EVENT_KEY_DOWN: u32 = 10;
const K_CG_EVENT_KEY_UP: u32 = 11;
const K_CG_EVENT_SCROLL_WHEEL: u32 = 22;

// CGMouseButton
const K_CG_MOUSE_BUTTON_LEFT: u32 = 0;
const K_CG_MOUSE_BUTTON_RIGHT: u32 = 1;
const K_CG_MOUSE_BUTTON_CENTER: u32 = 2;

// CGEventTapLocation
const K_CG_HID_EVENT_TAP: u32 = 0;
const K_CG_SESSION_EVENT_TAP: u32 = 1;

pub struct MacInputInjector {
    width: f32,
    height: f32,
    gamepad_deadzone: f32,
}

impl MacInputInjector {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width: width as f32,
            height: height as f32,
            gamepad_deadzone: 0.1,
        }
    }

    pub fn with_gamepad_deadzone(width: u32, height: u32, deadzone: f32) -> Self {
        Self {
            width: width as f32,
            height: height as f32,
            gamepad_deadzone: normalize_gamepad_deadzone(deadzone),
        }
    }

    pub fn set_gamepad_deadzone(&mut self, deadzone: f32) {
        self.gamepad_deadzone = normalize_gamepad_deadzone(deadzone);
    }
}

unsafe impl Send for MacInputInjector {}

fn normalize_gamepad_deadzone(deadzone: f32) -> f32 {
    deadzone.clamp(0.0, 0.95)
}

fn apply_gamepad_deadzone(value: f32, deadzone: f32) -> f32 {
    let deadzone = normalize_gamepad_deadzone(deadzone);
    let abs = value.abs();
    if abs <= deadzone {
        0.0
    } else {
        let scaled = (abs - deadzone) / (1.0 - deadzone);
        scaled.copysign(value).clamp(-1.0, 1.0)
    }
}

impl crate::InputInjector for MacInputInjector {
    fn inject(&mut self, event: InputEvent) -> Result<()> {
        unsafe {
            let event_ref = match event {
                InputEvent::MouseMove { x, y } => {
                    let point = CGPoint {
                        x: (x * self.width) as f64,
                        y: (y * self.height) as f64,
                    };
                    CGEventCreateMouseEvent(
                        null(),
                        K_CG_EVENT_MOUSE_MOVED,
                        point,
                        K_CG_MOUSE_BUTTON_LEFT,
                    )
                }
                InputEvent::MouseDown { button } => {
                    let type_ = match button {
                        MouseButton::Left => K_CG_EVENT_LEFT_MOUSE_DOWN,
                        MouseButton::Right => K_CG_EVENT_RIGHT_MOUSE_DOWN,
                        _ => K_CG_EVENT_NULL,
                    };
                    let btn = match button {
                        MouseButton::Left => K_CG_MOUSE_BUTTON_LEFT,
                        MouseButton::Right => K_CG_MOUSE_BUTTON_RIGHT,
                        _ => K_CG_MOUSE_BUTTON_CENTER,
                    };
                    CGEventCreateMouseEvent(null(), type_, CGPoint { x: 0.0, y: 0.0 }, btn)
                }
                InputEvent::MouseUp { button } => {
                    let type_ = match button {
                        MouseButton::Left => K_CG_EVENT_LEFT_MOUSE_UP,
                        MouseButton::Right => K_CG_EVENT_RIGHT_MOUSE_UP,
                        _ => K_CG_EVENT_NULL,
                    };
                    let btn = match button {
                        MouseButton::Left => K_CG_MOUSE_BUTTON_LEFT,
                        MouseButton::Right => K_CG_MOUSE_BUTTON_RIGHT,
                        _ => K_CG_MOUSE_BUTTON_CENTER,
                    };
                    CGEventCreateMouseEvent(null(), type_, CGPoint { x: 0.0, y: 0.0 }, btn)
                }
                InputEvent::KeyDown { key_code } => {
                    CGEventCreateKeyboardEvent(null(), key_code as u16, true)
                }
                InputEvent::KeyUp { key_code } => {
                    CGEventCreateKeyboardEvent(null(), key_code as u16, false)
                }
                InputEvent::Gamepad { axes, buttons, .. } => {
                    // macOS gamepad injection is still pending (Foohid/VHID), but we still
                    // apply deadzone filtering here so noisy axes are ignored consistently.
                    let has_axis_activity = axes.iter().any(|axis| {
                        apply_gamepad_deadzone(axis.value, self.gamepad_deadzone) != 0.0
                    });
                    let has_button_activity = buttons.iter().any(|button| button.pressed);
                    if has_axis_activity || has_button_activity {
                        log::debug!("macOS gamepad event received, injection not yet implemented");
                    }
                    std::ptr::null_mut()
                }
                _ => std::ptr::null_mut(),
            };

            if !event_ref.is_null() {
                CGEventPost(K_CG_HID_EVENT_TAP, event_ref);
                CFRelease(event_ref);
            }
        }
        Ok(())
    }
}
