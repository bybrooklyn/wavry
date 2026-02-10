use openxr as xr;
use std::time::{Duration, Instant};
use wavry_vr::types::{GamepadAxis, GamepadButton, GamepadInput, HandPose, Pose, StreamConfig};
use wavry_vr::{VrError, VrResult};

pub const INPUT_SEND_INTERVAL: Duration = Duration::from_millis(20);
pub const AXIS_EPS: f32 = 0.01;
pub const STICK_DEADZONE: f32 = 0.05;

#[derive(Clone, Copy, Default)]
pub struct GamepadSnapshot {
    pub axes: [f32; 4],
    pub buttons: [bool; 2],
    pub active: bool,
}

pub struct InputActions {
    pub action_set: xr::ActionSet,
    pub trigger: xr::Action<f32>,
    pub trigger_click: xr::Action<bool>,
    pub grip: xr::Action<f32>,
    pub grip_click: xr::Action<bool>,
    pub stick: xr::Action<xr::Vector2f>,
    pub primary: xr::Action<bool>,
    pub secondary: xr::Action<bool>,
    pub left: xr::Path,
    pub right: xr::Path,
    pub last_sent: [GamepadSnapshot; 2],
    pub last_sent_at: [Instant; 2],
}

impl InputActions {
    pub fn new<G>(instance: &xr::Instance, session: &xr::Session<G>) -> VrResult<Self> {
        let action_set = instance
            .create_action_set("wavry", "Wavry", 0)
            .map_err(|e| VrError::Adapter(format!("OpenXR action set: {e:?}")))?;

        let left = instance
            .string_to_path("/user/hand/left")
            .map_err(|e| VrError::Adapter(format!("OpenXR path left: {e:?}")))?;
        let right = instance
            .string_to_path("/user/hand/right")
            .map_err(|e| VrError::Adapter(format!("OpenXR path right: {e:?}")))?;
        let subaction_paths = [left, right];

        let trigger = action_set
            .create_action("trigger", "Trigger", &subaction_paths)
            .map_err(|e| VrError::Adapter(format!("OpenXR action trigger: {e:?}")))?;
        let trigger_click = action_set
            .create_action("trigger_click", "Trigger Click", &subaction_paths)
            .map_err(|e| VrError::Adapter(format!("OpenXR action trigger_click: {e:?}")))?;
        let grip = action_set
            .create_action("grip", "Grip", &subaction_paths)
            .map_err(|e| VrError::Adapter(format!("OpenXR action grip: {e:?}")))?;
        let grip_click = action_set
            .create_action("grip_click", "Grip Click", &subaction_paths)
            .map_err(|e| VrError::Adapter(format!("OpenXR action grip_click: {e:?}")))?;
        let stick = action_set
            .create_action("thumbstick", "Thumbstick", &subaction_paths)
            .map_err(|e| VrError::Adapter(format!("OpenXR action thumbstick: {e:?}")))?;
        let primary = action_set
            .create_action("primary", "Primary", &subaction_paths)
            .map_err(|e| VrError::Adapter(format!("OpenXR action primary: {e:?}")))?;
        let secondary = action_set
            .create_action("secondary", "Secondary", &subaction_paths)
            .map_err(|e| VrError::Adapter(format!("OpenXR action secondary: {e:?}")))?;

        let profile_paths = [
            "/interaction_profiles/khr/simple_controller",
            "/interaction_profiles/oculus/touch_controller",
            "/interaction_profiles/valve/index_controller",
            "/interaction_profiles/microsoft/motion_controller",
            "/interaction_profiles/htc/vive_controller",
        ];

        for profile in profile_paths {
            let profile_path = instance
                .string_to_path(profile)
                .map_err(|e| VrError::Adapter(format!("OpenXR profile path: {e:?}")))?;
            let bindings = Self::bindings_for_profile(
                instance,
                profile,
                &trigger,
                &trigger_click,
                &grip,
                &grip_click,
                &stick,
                &primary,
                &secondary,
            )?;
            if let Err(err) = instance.suggest_interaction_profile_bindings(profile_path, &bindings)
            {
                eprintln!(
                    "OpenXR binding suggestion rejected for {}: {:?}",
                    profile, err
                );
            }
        }

        session
            .attach_action_sets(&[&action_set])
            .map_err(|e| VrError::Adapter(format!("OpenXR attach actions: {e:?}")))?;

        Ok(Self {
            action_set,
            trigger,
            trigger_click,
            grip,
            grip_click,
            stick,
            primary,
            secondary,
            left,
            right,
            last_sent: [GamepadSnapshot::default(), GamepadSnapshot::default()],
            last_sent_at: [Instant::now(), Instant::now()],
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn bindings_for_profile<'a>(
        instance: &'a xr::Instance,
        profile: &'a str,
        trigger: &'a xr::Action<f32>,
        trigger_click: &'a xr::Action<bool>,
        grip: &'a xr::Action<f32>,
        grip_click: &'a xr::Action<bool>,
        stick: &'a xr::Action<xr::Vector2f>,
        primary: &'a xr::Action<bool>,
        secondary: &'a xr::Action<bool>,
    ) -> VrResult<Vec<xr::Binding<'a>>> {
        let mut bindings = Vec::with_capacity(24);
        macro_rules! bind_f32 {
            ($action:expr, $path:expr) => {
                if let Ok(path) = instance.string_to_path($path) {
                    bindings.push(xr::Binding::new($action, path));
                }
            };
        }
        macro_rules! bind_vec2 {
            ($action:expr, $path:expr) => {
                if let Ok(path) = instance.string_to_path($path) {
                    bindings.push(xr::Binding::new($action, path));
                }
            };
        }
        macro_rules! bind_bool {
            ($action:expr, $path:expr) => {
                if let Ok(path) = instance.string_to_path($path) {
                    bindings.push(xr::Binding::new($action, path));
                }
            };
        }

        match profile {
            "/interaction_profiles/khr/simple_controller" => {
                bind_bool!(trigger_click, "/user/hand/left/input/select/click");
                bind_bool!(trigger_click, "/user/hand/right/input/select/click");
                bind_bool!(primary, "/user/hand/left/input/select/click");
                bind_bool!(primary, "/user/hand/right/input/select/click");
                bind_bool!(secondary, "/user/hand/left/input/menu/click");
                bind_bool!(secondary, "/user/hand/right/input/menu/click");
            }
            "/interaction_profiles/oculus/touch_controller" => {
                bind_f32!(trigger, "/user/hand/left/input/trigger/value");
                bind_f32!(trigger, "/user/hand/right/input/trigger/value");
                bind_f32!(grip, "/user/hand/left/input/squeeze/value");
                bind_f32!(grip, "/user/hand/right/input/squeeze/value");
                bind_vec2!(stick, "/user/hand/left/input/thumbstick");
                bind_vec2!(stick, "/user/hand/right/input/thumbstick");
                bind_bool!(primary, "/user/hand/left/input/x/click");
                bind_bool!(primary, "/user/hand/right/input/a/click");
                bind_bool!(secondary, "/user/hand/left/input/y/click");
                bind_bool!(secondary, "/user/hand/right/input/b/click");
            }
            "/interaction_profiles/valve/index_controller" => {
                bind_f32!(trigger, "/user/hand/left/input/trigger/value");
                bind_f32!(trigger, "/user/hand/right/input/trigger/value");
                bind_f32!(grip, "/user/hand/left/input/squeeze/value");
                bind_f32!(grip, "/user/hand/right/input/squeeze/value");
                bind_vec2!(stick, "/user/hand/left/input/thumbstick");
                bind_vec2!(stick, "/user/hand/right/input/thumbstick");
                bind_bool!(primary, "/user/hand/left/input/a/click");
                bind_bool!(primary, "/user/hand/right/input/a/click");
                bind_bool!(secondary, "/user/hand/left/input/b/click");
                bind_bool!(secondary, "/user/hand/right/input/b/click");
            }
            "/interaction_profiles/microsoft/motion_controller" => {
                bind_f32!(trigger, "/user/hand/left/input/trigger/value");
                bind_f32!(trigger, "/user/hand/right/input/trigger/value");
                bind_bool!(grip_click, "/user/hand/left/input/squeeze/click");
                bind_bool!(grip_click, "/user/hand/right/input/squeeze/click");
                bind_vec2!(stick, "/user/hand/left/input/thumbstick");
                bind_vec2!(stick, "/user/hand/right/input/thumbstick");
                bind_vec2!(stick, "/user/hand/left/input/trackpad");
                bind_vec2!(stick, "/user/hand/right/input/trackpad");
                bind_bool!(primary, "/user/hand/left/input/thumbstick/click");
                bind_bool!(primary, "/user/hand/right/input/thumbstick/click");
                bind_bool!(primary, "/user/hand/left/input/trackpad/click");
                bind_bool!(primary, "/user/hand/right/input/trackpad/click");
                bind_bool!(secondary, "/user/hand/left/input/menu/click");
                bind_bool!(secondary, "/user/hand/right/input/menu/click");
            }
            "/interaction_profiles/htc/vive_controller" => {
                bind_f32!(trigger, "/user/hand/left/input/trigger/value");
                bind_f32!(trigger, "/user/hand/right/input/trigger/value");
                bind_bool!(grip_click, "/user/hand/left/input/squeeze/click");
                bind_bool!(grip_click, "/user/hand/right/input/squeeze/click");
                bind_vec2!(stick, "/user/hand/left/input/trackpad");
                bind_vec2!(stick, "/user/hand/right/input/trackpad");
                bind_bool!(primary, "/user/hand/left/input/trackpad/click");
                bind_bool!(primary, "/user/hand/right/input/trackpad/click");
                bind_bool!(secondary, "/user/hand/left/input/menu/click");
                bind_bool!(secondary, "/user/hand/right/input/menu/click");
            }
            _ => {}
        }

        Ok(bindings)
    }

    pub fn poll<G>(
        &mut self,
        session: &xr::Session<G>,
        timestamp_us: u64,
    ) -> VrResult<Vec<GamepadInput>> {
        session
            .sync_actions(&[xr::ActiveActionSet::new(&self.action_set)])
            .map_err(|e| VrError::Adapter(format!("OpenXR sync actions: {e:?}")))?;

        let mut outputs = Vec::new();
        let now = Instant::now();
        let hands = [(self.left, 0usize), (self.right, 1usize)];

        for (path, index) in hands {
            let trigger = self.trigger.state(session, path).ok();
            let trigger_click = self.trigger_click.state(session, path).ok();
            let grip = self.grip.state(session, path).ok();
            let grip_click = self.grip_click.state(session, path).ok();
            let stick = self.stick.state(session, path).ok();
            let primary = self.primary.state(session, path).ok();
            let secondary = self.secondary.state(session, path).ok();

            let active = trigger.as_ref().map(|s| s.is_active).unwrap_or(false)
                || trigger_click.as_ref().map(|s| s.is_active).unwrap_or(false)
                || grip.as_ref().map(|s| s.is_active).unwrap_or(false)
                || grip_click.as_ref().map(|s| s.is_active).unwrap_or(false)
                || stick.as_ref().map(|s| s.is_active).unwrap_or(false)
                || primary.as_ref().map(|s| s.is_active).unwrap_or(false)
                || secondary.as_ref().map(|s| s.is_active).unwrap_or(false);

            let mut axes = [0.0f32; 4];
            let mut buttons = [false; 2];
            if active {
                let trigger_val = trigger.map(|s| s.current_state).unwrap_or(0.0).max(
                    if trigger_click.map(|s| s.current_state).unwrap_or(false) {
                        1.0
                    } else {
                        0.0
                    },
                );
                let grip_val = grip.map(|s| s.current_state).unwrap_or(0.0).max(
                    if grip_click.map(|s| s.current_state).unwrap_or(false) {
                        1.0
                    } else {
                        0.0
                    },
                );
                let stick_val = stick
                    .map(|s| s.current_state)
                    .unwrap_or(xr::Vector2f { x: 0.0, y: 0.0 });
                let stick_x = if stick_val.x.abs() < STICK_DEADZONE {
                    0.0
                } else {
                    stick_val.x
                };
                let stick_y = if stick_val.y.abs() < STICK_DEADZONE {
                    0.0
                } else {
                    stick_val.y
                };
                axes = [stick_x, stick_y, trigger_val, grip_val];
                buttons = [
                    primary.map(|s| s.current_state).unwrap_or(false),
                    secondary.map(|s| s.current_state).unwrap_or(false),
                ];
            }

            let snapshot = GamepadSnapshot {
                axes,
                buttons,
                active,
            };

            let should_send = Self::should_send(
                snapshot,
                self.last_sent[index],
                now,
                self.last_sent_at[index],
            );
            if should_send {
                self.last_sent[index] = snapshot;
                self.last_sent_at[index] = now;

                let axes_out = vec![
                    GamepadAxis {
                        axis: 0,
                        value: axes[0],
                    },
                    GamepadAxis {
                        axis: 1,
                        value: axes[1],
                    },
                    GamepadAxis {
                        axis: 2,
                        value: axes[2],
                    },
                    GamepadAxis {
                        axis: 3,
                        value: axes[3],
                    },
                ];
                let buttons_out = vec![
                    GamepadButton {
                        button: 0,
                        pressed: buttons[0],
                    },
                    GamepadButton {
                        button: 1,
                        pressed: buttons[1],
                    },
                ];
                outputs.push(GamepadInput {
                    timestamp_us,
                    gamepad_id: index as u32,
                    axes: axes_out,
                    buttons: buttons_out,
                });
            }
        }

        Ok(outputs)
    }

    fn should_send(
        current: GamepadSnapshot,
        last: GamepadSnapshot,
        now: Instant,
        last_sent_at: Instant,
    ) -> bool {
        if current.active || last.active {
            if now.duration_since(last_sent_at) >= INPUT_SEND_INTERVAL {
                return true;
            }
            for i in 0..current.axes.len() {
                if (current.axes[i] - last.axes[i]).abs() > AXIS_EPS {
                    return true;
                }
            }
            for i in 0..current.buttons.len() {
                if current.buttons[i] != last.buttons[i] {
                    return true;
                }
            }
        }
        false
    }
}

pub struct EyeLayout {
    pub eye_width: u32,
    pub eye_height: u32,
    pub is_sbs: bool,
}

pub fn eye_layout(cfg: StreamConfig) -> EyeLayout {
    let width = cfg.width as u32;
    let height = cfg.height as u32;
    let is_sbs = width >= height * 2 && width.is_multiple_of(2);
    let eye_width = if is_sbs { width / 2 } else { width };
    EyeLayout {
        eye_width,
        eye_height: height,
        is_sbs,
    }
}

pub fn to_pose(pose: xr::Posef) -> Pose {
    Pose {
        position: [pose.position.x, pose.position.y, pose.position.z],
        orientation: [
            pose.orientation.x,
            pose.orientation.y,
            pose.orientation.z,
            pose.orientation.w,
        ],
    }
}

pub struct HandTrackingState {
    pub left: xr::HandTracker,
    pub right: xr::HandTracker,
}

impl HandTrackingState {
    pub fn new<G>(session: &xr::Session<G>) -> VrResult<Self> {
        let left = session
            .create_hand_tracker(xr::Hand::LEFT)
            .map_err(|e| VrError::Adapter(format!("OpenXR create hand tracker left: {e:?}")))?;
        let right = session
            .create_hand_tracker(xr::Hand::RIGHT)
            .map_err(|e| VrError::Adapter(format!("OpenXR create hand tracker right: {e:?}")))?;
        Ok(Self { left, right })
    }

    pub fn poll(&self, reference_space: &xr::Space, time: xr::Time) -> Vec<HandPose> {
        let mut out = Vec::with_capacity(2);
        if let Ok(Some((locations, velocities))) =
            reference_space.relate_hand_joints(&self.left, time)
        {
            if let Some(hand) = hand_pose_from_joints(0, &locations, &velocities) {
                out.push(hand);
            }
        }
        if let Ok(Some((locations, velocities))) =
            reference_space.relate_hand_joints(&self.right, time)
        {
            if let Some(hand) = hand_pose_from_joints(1, &locations, &velocities) {
                out.push(hand);
            }
        }
        out
    }
}

fn hand_pose_from_joints(
    hand_id: u32,
    locations: &xr::HandJointLocations,
    velocities: &xr::HandJointVelocities,
) -> Option<HandPose> {
    let palm_location = locations[xr::HandJoint::PALM];
    let has_position = palm_location
        .location_flags
        .contains(xr::SpaceLocationFlags::POSITION_VALID);
    let has_orientation = palm_location
        .location_flags
        .contains(xr::SpaceLocationFlags::ORIENTATION_VALID);
    if !has_position || !has_orientation {
        return None;
    }

    let palm_velocity = velocities[xr::HandJoint::PALM];
    let linear_velocity = if palm_velocity
        .velocity_flags
        .contains(xr::SpaceVelocityFlags::LINEAR_VALID)
    {
        [
            palm_velocity.linear_velocity.x,
            palm_velocity.linear_velocity.y,
            palm_velocity.linear_velocity.z,
        ]
    } else {
        [0.0; 3]
    };
    let angular_velocity = if palm_velocity
        .velocity_flags
        .contains(xr::SpaceVelocityFlags::ANGULAR_VALID)
    {
        [
            palm_velocity.angular_velocity.x,
            palm_velocity.angular_velocity.y,
            palm_velocity.angular_velocity.z,
        ]
    } else {
        [0.0; 3]
    };

    Some(HandPose {
        hand_id,
        pose: to_pose(palm_location.pose),
        linear_velocity,
        angular_velocity,
    })
}
