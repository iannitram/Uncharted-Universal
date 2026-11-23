//! Android KeyEvent / MotionEvent gamepad adapter for the Slint launcher (Stage 7.03).
//!
//! Translates Android NDK keycodes (`AKEYCODE_*`) and analog/hat axes (`AMOTION_EVENT_AXIS_*`)
//! into the integer `Input::from_code` callbacks (`0..=10`) consumed by `app/ui/app.slint` and
//! `app/src/screen.rs`.

use std::collections::HashMap;

/// Stick/hat deflection threshold matching `app/src/input.rs` (`STICK_THRESHOLD = 0.6`).
pub const STICK_THRESHOLD: f32 = 0.6;

/// Android NDK `AKEYCODE_*` constants from `<android/keycodes.h>`.
pub mod keycode {
    pub const BACK: i32 = 4;
    pub const DPAD_UP: i32 = 19;
    pub const DPAD_DOWN: i32 = 20;
    pub const DPAD_LEFT: i32 = 21;
    pub const DPAD_RIGHT: i32 = 22;
    pub const DPAD_CENTER: i32 = 23;
    pub const TAB: i32 = 61;
    pub const ENTER: i32 = 66;
    pub const PAGE_UP: i32 = 92;
    pub const PAGE_DOWN: i32 = 93;
    pub const BUTTON_A: i32 = 96;
    pub const BUTTON_B: i32 = 97;
    pub const BUTTON_X: i32 = 99;
    pub const BUTTON_Y: i32 = 100;
    pub const BUTTON_L1: i32 = 102;
    pub const BUTTON_R1: i32 = 103;
    pub const ESCAPE: i32 = 111;
}

/// Android NDK `AMOTION_EVENT_AXIS_*` constants from `<android/input.h>`.
pub mod axis {
    pub const X: i32 = 0;
    pub const Y: i32 = 1;
    pub const HAT_X: i32 = 15;
    pub const HAT_Y: i32 = 16;
}

/// Prompt glyph family matching `app/src/prompts.rs::Style`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PromptStyle {
    #[default]
    Xbox,
    PlayStation,
    Nintendo,
}

impl PromptStyle {
    /// Infers button glyph style from an Android `InputDevice` vendor ID and name/descriptor.
    pub fn from_android_device(vendor_id: Option<u16>, device_name: &str) -> Self {
        match vendor_id {
            Some(0x054c) => return Self::PlayStation,
            Some(0x057e) => return Self::Nintendo,
            Some(0x045e) => return Self::Xbox,
            _ => {}
        }
        let lower = device_name.to_ascii_lowercase();
        if [
            "playstation",
            "dualshock",
            "dualsense",
            "ps4 controller",
            "ps5 controller",
            "sony",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
        {
            return Self::PlayStation;
        }
        if ["nintendo", "joy-con", "joycon", "switch pro"]
            .iter()
            .any(|needle| lower.contains(needle))
        {
            return Self::Nintendo;
        }
        Self::Xbox
    }
}

/// Focus navigation direction matching `app/src/focus.rs::Nav`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
}

/// Launcher input event matching `app/src/screen.rs::Input`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LauncherInput {
    Move(Nav),
    Tab { back: bool },
    Activate,
    Back,
    PrevGame,
    NextGame,
    OtherSaves,
}

impl LauncherInput {
    /// Returns the exact `.slint` `input(int)` callback code defined in `app/src/screen.rs`.
    pub const fn slint_code(self) -> i32 {
        match self {
            Self::Move(Nav::Up) => 0,
            Self::Move(Nav::Down) => 1,
            Self::Move(Nav::Left) => 2,
            Self::Move(Nav::Right) => 3,
            Self::Tab { back: false } => 4,
            Self::Tab { back: true } => 5,
            Self::Activate => 6,
            Self::Back => 7,
            Self::PrevGame => 8,
            Self::NextGame => 9,
            Self::OtherSaves => 10,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AndroidKeyEvent {
    pub device_id: i32,
    pub key_code: i32,
    pub action: KeyAction,
    pub shift_held: bool,
    pub is_repeat: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AndroidAxisEvent {
    pub device_id: i32,
    pub axis: i32,
    pub value: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MappedPadEvent {
    pub input: LauncherInput,
    pub slint_code: i32,
    pub style: PromptStyle,
}

#[derive(Default)]
struct DeviceState {
    style: PromptStyle,
    stick_x_held: bool,
    stick_y_held: bool,
    hat_x_held: bool,
    hat_y_held: bool,
}

/// Stateful per-device Android gamepad-to-Slint input mapper.
#[derive(Default)]
pub struct AndroidGamepadAdapter {
    devices: HashMap<i32, DeviceState>,
}

impl AndroidGamepadAdapter {
    pub fn connect_device(&mut self, device_id: i32, vendor_id: Option<u16>, name: &str) {
        self.devices.insert(
            device_id,
            DeviceState {
                style: PromptStyle::from_android_device(vendor_id, name),
                stick_x_held: false,
                stick_y_held: false,
                hat_x_held: false,
                hat_y_held: false,
            },
        );
    }

    pub fn disconnect_device(&mut self, device_id: i32) {
        self.devices.remove(&device_id);
    }

    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    /// Maps an Android key event. Only initial key-down presses emit launcher inputs.
    pub fn handle_key_event(&self, event: AndroidKeyEvent) -> Option<MappedPadEvent> {
        if event.action != KeyAction::Down || event.is_repeat {
            return None;
        }
        let input = map_keycode(event.key_code, event.shift_held)?;
        let style = self
            .devices
            .get(&event.device_id)
            .map(|d| d.style)
            .unwrap_or_default();
        Some(MappedPadEvent {
            input,
            slint_code: input.slint_code(),
            style,
        })
    }

    /// Maps an Android motion axis event (left stick or D-pad hat).
    ///
    /// Note: unlike `gilrs` where +Y is up, Android `AXIS_Y` and `AXIS_HAT_Y` report `-1.0` up and
    /// `+1.0` down.
    pub fn handle_axis_event(&mut self, event: AndroidAxisEvent) -> Option<MappedPadEvent> {
        let device = self.devices.entry(event.device_id).or_default();
        let (held, is_horizontal) = match event.axis {
            axis::X => (&mut device.stick_x_held, true),
            axis::Y => (&mut device.stick_y_held, false),
            axis::HAT_X => (&mut device.hat_x_held, true),
            axis::HAT_Y => (&mut device.hat_y_held, false),
            _ => return None,
        };

        let nav = edge_trigger_axis(held, is_horizontal, event.value)?;
        let input = LauncherInput::Move(nav);
        Some(MappedPadEvent {
            input,
            slint_code: input.slint_code(),
            style: device.style,
        })
    }
}

pub const fn map_keycode(key_code: i32, shift_held: bool) -> Option<LauncherInput> {
    match key_code {
        keycode::DPAD_UP => Some(LauncherInput::Move(Nav::Up)),
        keycode::DPAD_DOWN => Some(LauncherInput::Move(Nav::Down)),
        keycode::DPAD_LEFT => Some(LauncherInput::Move(Nav::Left)),
        keycode::DPAD_RIGHT => Some(LauncherInput::Move(Nav::Right)),
        keycode::TAB => Some(LauncherInput::Tab { back: shift_held }),
        keycode::BUTTON_A | keycode::DPAD_CENTER | keycode::ENTER => Some(LauncherInput::Activate),
        keycode::BUTTON_B | keycode::BACK | keycode::ESCAPE => Some(LauncherInput::Back),
        keycode::BUTTON_L1 | keycode::PAGE_UP => Some(LauncherInput::PrevGame),
        keycode::BUTTON_R1 | keycode::PAGE_DOWN => Some(LauncherInput::NextGame),
        keycode::BUTTON_Y => Some(LauncherInput::OtherSaves),
        // West (BUTTON_X) is intentionally unbound on launcher navigation, matching `app/src/input.rs`.
        _ => None,
    }
}

fn edge_trigger_axis(held: &mut bool, is_horizontal: bool, value: f32) -> Option<Nav> {
    let was = *held;
    let now = value.is_finite() && value.abs() >= STICK_THRESHOLD;
    *held = now;
    if !now || was {
        return None;
    }
    Some(match (is_horizontal, value > 0.0) {
        (true, true) => Nav::Right,
        (true, false) => Nav::Left,
        (false, true) => Nav::Down,
        (false, false) => Nav::Up,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycodes_map_to_matching_slint_input_codes() {
        let adapter = AndroidGamepadAdapter::default();
        let cases = [
            (keycode::DPAD_UP, false, LauncherInput::Move(Nav::Up), 0),
            (keycode::DPAD_DOWN, false, LauncherInput::Move(Nav::Down), 1),
            (keycode::DPAD_LEFT, false, LauncherInput::Move(Nav::Left), 2),
            (
                keycode::DPAD_RIGHT,
                false,
                LauncherInput::Move(Nav::Right),
                3,
            ),
            (keycode::TAB, false, LauncherInput::Tab { back: false }, 4),
            (keycode::TAB, true, LauncherInput::Tab { back: true }, 5),
            (keycode::BUTTON_A, false, LauncherInput::Activate, 6),
            (keycode::DPAD_CENTER, false, LauncherInput::Activate, 6),
            (keycode::ENTER, false, LauncherInput::Activate, 6),
            (keycode::BUTTON_B, false, LauncherInput::Back, 7),
            (keycode::BACK, false, LauncherInput::Back, 7),
            (keycode::ESCAPE, false, LauncherInput::Back, 7),
            (keycode::BUTTON_L1, false, LauncherInput::PrevGame, 8),
            (keycode::PAGE_UP, false, LauncherInput::PrevGame, 8),
            (keycode::BUTTON_R1, false, LauncherInput::NextGame, 9),
            (keycode::PAGE_DOWN, false, LauncherInput::NextGame, 9),
            (keycode::BUTTON_Y, false, LauncherInput::OtherSaves, 10),
        ];

        for (code, shift, expected, slint_code) in cases {
            let mapped = adapter
                .handle_key_event(AndroidKeyEvent {
                    device_id: 1,
                    key_code: code,
                    action: KeyAction::Down,
                    shift_held: shift,
                    is_repeat: false,
                })
                .expect("expected mapped key");
            assert_eq!(mapped.input, expected);
            assert_eq!(mapped.slint_code, slint_code);
        }

        // West (X), key-up, and repeats are ignored.
        assert_eq!(
            adapter.handle_key_event(AndroidKeyEvent {
                device_id: 1,
                key_code: keycode::BUTTON_X,
                action: KeyAction::Down,
                shift_held: false,
                is_repeat: false,
            }),
            None
        );
        assert_eq!(
            adapter.handle_key_event(AndroidKeyEvent {
                device_id: 1,
                key_code: keycode::BUTTON_A,
                action: KeyAction::Up,
                shift_held: false,
                is_repeat: false,
            }),
            None
        );
        assert_eq!(
            adapter.handle_key_event(AndroidKeyEvent {
                device_id: 1,
                key_code: keycode::BUTTON_A,
                action: KeyAction::Down,
                shift_held: false,
                is_repeat: true,
            }),
            None
        );
    }

    #[test]
    fn android_stick_and_hat_axes_respect_inverted_y_and_debounce() {
        let mut adapter = AndroidGamepadAdapter::default();
        adapter.connect_device(7, Some(0x054c), "DualSense Wireless Controller");

        // Deadzone ignored.
        assert_eq!(
            adapter.handle_axis_event(AndroidAxisEvent {
                device_id: 7,
                axis: axis::Y,
                value: -0.4,
            }),
            None
        );

        // Android -Y is Up!
        let up = adapter
            .handle_axis_event(AndroidAxisEvent {
                device_id: 7,
                axis: axis::Y,
                value: -0.9,
            })
            .expect("up stick push");
        assert_eq!(up.input, LauncherInput::Move(Nav::Up));
        assert_eq!(up.slint_code, 0);
        assert_eq!(up.style, PromptStyle::PlayStation);

        // Holding past threshold does not retrigger.
        assert_eq!(
            adapter.handle_axis_event(AndroidAxisEvent {
                device_id: 7,
                axis: axis::Y,
                value: -1.0,
            }),
            None
        );

        // Release then push +Y (Down).
        assert_eq!(
            adapter.handle_axis_event(AndroidAxisEvent {
                device_id: 7,
                axis: axis::Y,
                value: 0.0,
            }),
            None
        );
        let down = adapter
            .handle_axis_event(AndroidAxisEvent {
                device_id: 7,
                axis: axis::Y,
                value: 0.85,
            })
            .expect("down stick push");
        assert_eq!(down.input, LauncherInput::Move(Nav::Down));
        assert_eq!(down.slint_code, 1);

        // D-pad hat X/Y works independently.
        let left = adapter
            .handle_axis_event(AndroidAxisEvent {
                device_id: 7,
                axis: axis::HAT_X,
                value: -1.0,
            })
            .expect("hat left");
        assert_eq!(left.input, LauncherInput::Move(Nav::Left));
        assert_eq!(left.slint_code, 2);

        let hat_down = adapter
            .handle_axis_event(AndroidAxisEvent {
                device_id: 7,
                axis: axis::HAT_Y,
                value: 1.0,
            })
            .expect("hat down");
        assert_eq!(hat_down.input, LauncherInput::Move(Nav::Down));
        assert_eq!(hat_down.slint_code, 1);

        adapter.disconnect_device(7);
        assert_eq!(adapter.device_count(), 0);
    }

    #[test]
    fn device_prompt_style_matches_vendor_and_name_heuristics() {
        assert_eq!(
            PromptStyle::from_android_device(Some(0x054c), "Wireless Controller"),
            PromptStyle::PlayStation
        );
        assert_eq!(
            PromptStyle::from_android_device(Some(0x057e), "Pro Controller"),
            PromptStyle::Nintendo
        );
        assert_eq!(
            PromptStyle::from_android_device(Some(0x045e), "Xbox Wireless Controller"),
            PromptStyle::Xbox
        );
        assert_eq!(
            PromptStyle::from_android_device(None, "DualShock 4"),
            PromptStyle::PlayStation
        );
        assert_eq!(
            PromptStyle::from_android_device(None, "Nintendo Joy-Con (L)"),
            PromptStyle::Nintendo
        );
        assert_eq!(
            PromptStyle::from_android_device(None, "Generic Backbone"),
            PromptStyle::Xbox
        );
    }
}
