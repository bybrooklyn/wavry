use crate::helpers::now_us;
use anyhow::Result;
use gilrs::{Event, EventType as GilrsEventType, Gilrs};
use rift_core::InputMessage as ProtoInputMessage;
use std::thread;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::warn;

#[cfg(target_os = "linux")]
use evdev::{Device, EventType, Key, RelativeAxisType};

pub fn normalize_gamepad_deadzone(deadzone: f32) -> f32 {
    deadzone.clamp(0.0, 0.95)
}

pub fn apply_gamepad_deadzone(value: f32, deadzone: f32) -> f32 {
    let deadzone = normalize_gamepad_deadzone(deadzone);
    let abs = value.abs();
    if abs <= deadzone {
        0.0
    } else {
        let scaled = (abs - deadzone) / (1.0 - deadzone);
        scaled.copysign(value).clamp(-1.0, 1.0)
    }
}

#[cfg(target_os = "linux")]
pub fn spawn_input_threads(
    input_tx: mpsc::Sender<ProtoInputMessage>,
    gamepad_enabled: bool,
    gamepad_deadzone: f32,
) -> Result<()> {
    if gamepad_enabled {
        let tx_gamepad = input_tx.clone();
        let deadzone = normalize_gamepad_deadzone(gamepad_deadzone);
        thread::spawn(move || {
            let mut gilrs = match Gilrs::new() {
                Ok(g) => g,
                Err(e) => {
                    warn!("gilrs init failed: {}", e);
                    return;
                }
            };
            loop {
                while let Some(Event { id, event, .. }) = gilrs.next_event() {
                    let gamepad_id = Into::<usize>::into(id) as u32;
                    let mut msg = ProtoInputMessage {
                        timestamp_us: now_us(),
                        event: None,
                    };
                    match event {
                        GilrsEventType::ButtonPressed(button, _) => {
                            msg.event = Some(rift_core::input_message::Event::Gamepad(
                                rift_core::GamepadMessage {
                                    gamepad_id,
                                    buttons: vec![rift_core::GamepadButton {
                                        button: button as u32,
                                        pressed: true,
                                    }],
                                    axes: vec![],
                                },
                            ));
                        }
                        GilrsEventType::ButtonReleased(button, _) => {
                            msg.event = Some(rift_core::input_message::Event::Gamepad(
                                rift_core::GamepadMessage {
                                    gamepad_id,
                                    buttons: vec![rift_core::GamepadButton {
                                        button: button as u32,
                                        pressed: false,
                                    }],
                                    axes: vec![],
                                },
                            ));
                        }
                        GilrsEventType::AxisChanged(axis, value, _) => {
                            msg.event = Some(rift_core::input_message::Event::Gamepad(
                                rift_core::GamepadMessage {
                                    gamepad_id,
                                    axes: vec![rift_core::GamepadAxis {
                                        axis: axis as u32,
                                        value: apply_gamepad_deadzone(value, deadzone),
                                    }],
                                    buttons: vec![],
                                },
                            ));
                        }
                        _ => continue,
                    }
                    if tx_gamepad.blocking_send(msg).is_err() {
                        return;
                    }
                }
                thread::sleep(Duration::from_millis(8));
            }
        });
    }

    let keyboard = find_device(DeviceKind::Keyboard)?;
    if keyboard.is_none() {
        warn!("no keyboard input device found");
    }
    let mouse = find_device(DeviceKind::Mouse)?;
    if mouse.is_none() {
        warn!("no mouse input device found");
    }

    if let Some(mut keyboard) = keyboard {
        let tx = input_tx.clone();
        thread::spawn(move || loop {
            let mut had_events = false;
            if let Ok(events) = keyboard.fetch_events() {
                for event in events {
                    had_events = true;
                    if event.event_type() == EventType::KEY {
                        let keycode = event.code();
                        let pressed = event.value() != 0;
                        let input = ProtoInputMessage {
                            event: Some(rift_core::input_message::Event::Key(rift_core::Key {
                                keycode: keycode as u32,
                                pressed,
                            })),
                            timestamp_us: now_us(),
                        };
                        if tx.blocking_send(input).is_err() {
                            return;
                        }
                    }
                }
            }
            if !had_events {
                thread::sleep(Duration::from_millis(1));
            }
        });
    }

    if let Some(mut mouse) = mouse {
        let _tx = input_tx;
        thread::spawn(move || {
            loop {
                let mut had_events = false;
                if let Ok(events) = mouse.fetch_events() {
                    for _event in events {
                        had_events = true;
                        // ... simple mouse handling ...
                    }
                }
                if !had_events {
                    thread::sleep(Duration::from_millis(1));
                }
            }
        });
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn spawn_input_threads(
    input_tx: mpsc::Sender<ProtoInputMessage>,
    gamepad_enabled: bool,
    gamepad_deadzone: f32,
) -> Result<()> {
    if gamepad_enabled {
        let tx_gamepad = input_tx.clone();
        let deadzone = normalize_gamepad_deadzone(gamepad_deadzone);
        thread::spawn(move || {
            let mut gilrs = match Gilrs::new() {
                Ok(g) => g,
                Err(e) => {
                    warn!("gilrs init failed: {}", e);
                    return;
                }
            };
            loop {
                while let Some(Event { id, event, .. }) = gilrs.next_event() {
                    let gamepad_id = Into::<usize>::into(id) as u32;
                    let mut msg = ProtoInputMessage {
                        timestamp_us: now_us(),
                        event: None,
                    };
                    match event {
                        GilrsEventType::ButtonPressed(button, _) => {
                            msg.event = Some(rift_core::input_message::Event::Gamepad(
                                rift_core::GamepadMessage {
                                    gamepad_id,
                                    buttons: vec![rift_core::GamepadButton {
                                        button: button as u32,
                                        pressed: true,
                                    }],
                                    axes: vec![],
                                },
                            ));
                        }
                        GilrsEventType::ButtonReleased(button, _) => {
                            msg.event = Some(rift_core::input_message::Event::Gamepad(
                                rift_core::GamepadMessage {
                                    gamepad_id,
                                    buttons: vec![rift_core::GamepadButton {
                                        button: button as u32,
                                        pressed: false,
                                    }],
                                    axes: vec![],
                                },
                            ));
                        }
                        GilrsEventType::AxisChanged(axis, value, _) => {
                            msg.event = Some(rift_core::input_message::Event::Gamepad(
                                rift_core::GamepadMessage {
                                    gamepad_id,
                                    axes: vec![rift_core::GamepadAxis {
                                        axis: axis as u32,
                                        value: apply_gamepad_deadzone(value, deadzone),
                                    }],
                                    buttons: vec![],
                                },
                            ));
                        }
                        _ => continue,
                    }
                    if tx_gamepad.blocking_send(msg).is_err() {
                        return;
                    }
                }
                thread::sleep(Duration::from_millis(8));
            }
        });
    }

    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(2));
        let press = ProtoInputMessage {
            event: Some(rift_core::input_message::Event::Key(rift_core::Key {
                keycode: 30,
                pressed: true,
            })),
            timestamp_us: now_us(),
        };
        if input_tx.blocking_send(press).is_err() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
        let release = ProtoInputMessage {
            event: Some(rift_core::input_message::Event::Key(rift_core::Key {
                keycode: 30,
                pressed: false,
            })),
            timestamp_us: now_us(),
        };
        if input_tx.blocking_send(release).is_err() {
            break;
        }
    });
    Ok(())
}

#[cfg(target_os = "linux")]
enum DeviceKind {
    Keyboard,
    Mouse,
}

#[cfg(target_os = "linux")]
fn is_keyboard(device: &Device) -> bool {
    let keys = match device.supported_keys() {
        Some(keys) => keys,
        None => return false,
    };
    keys.contains(Key::KEY_A)
        || keys.contains(Key::KEY_Z)
        || keys.contains(Key::KEY_ENTER)
        || keys.contains(Key::KEY_SPACE)
}

#[cfg(target_os = "linux")]
fn is_mouse(device: &Device) -> bool {
    let rel = match device.supported_relative_axes() {
        Some(rel) => rel,
        None => return false,
    };
    let keys = device.supported_keys();
    let rel_ok = rel.contains(RelativeAxisType::REL_X) && rel.contains(RelativeAxisType::REL_Y);
    let btn_ok = keys
        .map(|k| k.contains(Key::BTN_LEFT) || k.contains(Key::BTN_RIGHT))
        .unwrap_or(false);
    rel_ok && btn_ok
}

#[cfg(target_os = "linux")]
fn find_device(kind: DeviceKind) -> Result<Option<Device>> {
    let mut fallback: Option<Device> = None;
    for (_path, device) in evdev::enumerate() {
        match kind {
            DeviceKind::Keyboard => {
                if is_keyboard(&device) {
                    return Ok(Some(device));
                }
                if fallback.is_none() && device.supported_keys().is_some() {
                    fallback = Some(device);
                }
            }
            DeviceKind::Mouse => {
                if is_mouse(&device) {
                    return Ok(Some(device));
                }
                if fallback.is_none() && device.supported_relative_axes().is_some() {
                    fallback = Some(device);
                }
            }
        }
    }
    Ok(fallback)
}
