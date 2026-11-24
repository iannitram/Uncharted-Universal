//! The devices behind [`super`]: SDL3 gamepads and keyboard (`sdl3-sys`) and the GPU library's
//! mouse and overlay calls in the loader build, nothing anywhere else.
//!
//! `sdl3-sys` is built with `no-link` (see `audio::device`): the loader already links SDL3 and
//! the GPU library, `cargo test` and the mobile builds link no C. The tests drive the module
//! over a fake [`Host`].

/// `SDL_GAMEPAD_BUTTON_COUNT`, `SDL_GAMEPAD_AXIS_COUNT` and `SDL_SCANCODE_COUNT`.
pub const BUTTON_COUNT: i32 = 26;
pub const SCANCODE_COUNT: usize = 512;
pub const BUTTON_SOUTH: i32 = 0;
pub const BUTTON_TOUCHPAD: i32 = 20;
pub const SCANCODE_F9: usize = 66;

/// `SDL_GamepadAxis`.
pub mod axis {
    pub const LEFT_X: i32 = 0;
    pub const LEFT_Y: i32 = 1;
    pub const RIGHT_X: i32 = 2;
    pub const RIGHT_Y: i32 = 3;
    pub const LEFT_TRIGGER: i32 = 4;
    pub const RIGHT_TRIGGER: i32 = 5;
    pub const COUNT: i32 = 6;
}

/// An SDL joystick instance id.
pub type PadId = u32;

/// The device values [`Host::prompts_controller`] takes: 0 keyboard, 1 PlayStation, 2 Xbox,
/// 3 Nintendo (Switch Pro and Joy-Con).
pub const PROMPTS_KEYBOARD: i32 = 0;
pub const PROMPTS_PLAYSTATION: i32 = 1;
pub const PROMPTS_XBOX: i32 = 2;
pub const PROMPTS_NINTENDO: i32 = 3;

/// Maps an SDL3 `SDL_GamepadType` value to the prompt family (`PROMPTS_PLAYSTATION`,
/// `PROMPTS_NINTENDO`, or `PROMPTS_XBOX`). Xbox 360/One, unknown, and standard pads map to
/// `PROMPTS_XBOX` unless `south_is_cross` is true.
#[allow(dead_code)]
pub(crate) const fn prompts_for_gamepad_type(kind: i32, south_is_cross: bool) -> i32 {
    match kind {
        // SDL_GAMEPAD_TYPE_PS3 (4), PS4 (5), PS5 (6)
        4..=6 => PROMPTS_PLAYSTATION,
        // SDL_GAMEPAD_TYPE_NINTENDO_SWITCH_PRO (7), JOYCON_LEFT (8), RIGHT (9), PAIR (10)
        7..=10 => PROMPTS_NINTENDO,
        _ if south_is_cross => PROMPTS_PLAYSTATION,
        _ => PROMPTS_XBOX,
    }
}

/// One finger of the pad's touch surface (0..1 on both axes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Finger {
    pub down: bool,
    pub x: f32,
    pub y: f32,
}

/// The window's mouse for one read: motion since the last, buttons, wheel steps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MouseSample {
    pub captured: bool,
    pub dx: f32,
    pub dy: f32,
    pub buttons: u32,
    pub wheel: i32,
}

/// What the pad module asks of the machine. Every call is made under the pad lock.
pub trait Host: Send {
    /// Sets the hints that must be set before the gamepad subsystem starts.
    fn hints(&mut self) {}
    /// Whether the gamepad subsystem is up already.
    fn pads_ready(&self) -> bool;
    /// Starts the gamepad subsystem; the error is SDL's text.
    fn init_pads(&mut self) -> Result<(), String>;
    fn quit_pads(&mut self) {}
    fn update_pads(&mut self) {}
    fn delay_ms(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    /// The ids of the gamepads SDL lists.
    fn pads(&mut self) -> Vec<PadId>;
    fn open(&mut self, id: PadId) -> bool;
    fn close(&mut self, id: PadId);
    fn connected(&self, id: PadId) -> bool;
    fn name(&self, id: PadId) -> String;
    fn button(&self, id: PadId, button: i32) -> bool;
    fn axis(&self, id: PadId, axis: i32) -> i16;
    /// The fingers of the first touch surface; empty for a pad without one.
    fn fingers(&self, id: PadId) -> Vec<Finger>;
    fn has_rumble(&self, id: PadId) -> bool;
    /// Starts both motors; false when SDL refused (the pad is gone or has no rumble).
    fn rumble(&mut self, id: PadId, low: u16, high: u16, ms: u32) -> bool;
    fn set_led(&mut self, id: PadId, r: u8, g: u8, b: u8);
    /// The button pictures for this pad: 1 PlayStation (DualShock, DualSense: the game's own),
    /// 2 Xbox, 3 Nintendo (Switch Pro and Joy-Con; shown as the Xbox layout until #439), by
    /// position (the bottom face button is the game's Cross on every pad).
    fn prompts_device(&self, id: PadId) -> i32;
    /// Turns the gyroscope and accelerometer on or off.
    fn sensors(&mut self, id: PadId, enable: bool);
    /// Angular velocity (rad/s) and acceleration (m/s^2), when the pad has the sensor on.
    fn gyro(&self, id: PadId) -> Option<[f32; 3]>;
    fn accel(&self, id: PadId) -> Option<[f32; 3]>;
    /// The keyboard state by scancode; `None` while the video subsystem is not up.
    fn keyboard(&self) -> Option<Vec<bool>>;
    fn scancode_from_name(&self, name: &str) -> Option<i32>;
    fn keycode_from_name(&self, name: &str) -> Option<i32>;
    fn scancode_from_keycode(&self, key: i32) -> Option<i32>;
    fn key_name(&self, key: i32) -> String;
    fn scancode_name(&self, code: i32) -> String;
    fn button_from_name(&self, name: &str) -> Option<i32>;
    // The GPU library's side.
    fn mouse_configure(&mut self, look: bool);
    fn mouse_sample(&mut self) -> MouseSample;
    fn overlay_captures_input(&self) -> bool;
    fn prompts_controller(&mut self, device: i32);
    /// The scripted menu and camera path of `BB_BENCHMARK` (`benchmark_run`), when one is on.
    fn benchmark_pad(&self) -> Option<crate::benchmark_run::BenchmarkPad> {
        let mut out = crate::benchmark_run::BenchmarkPad::default();
        // SAFETY: a writable `BenchmarkPad`.
        let owned = unsafe { crate::benchmark_run::uncharted_benchmark_pad(&mut out) } != 0;
        owned.then_some(out)
    }
}

/// The GPU library's camera yaw (debug output only); no GPU library, no yaw.
pub mod gpu {
    /// Low latency mode: a new frame starts here and may wait, so outside the pad lock.
    pub fn input_sample() {
        #[cfg(uncharted_loader)]
        // SAFETY: the GPU library's own call.
        unsafe {
            super::sdl::bbgpu_input_sample()
        };
    }

    /// The yaw (degrees) of the camera the game shows and its age (seconds).
    #[cfg(uncharted_loader)]
    pub fn camera_yaw() -> Option<(f32, f64)> {
        let (mut yaw, mut age) = (0.0f32, 0.0f64);
        // SAFETY: two writable outputs.
        let known = unsafe { super::sdl::bbgpu_camera_yaw(&mut yaw, &mut age) } != 0;
        known.then_some((yaw, age))
    }

    #[cfg(not(uncharted_loader))]
    pub fn camera_yaw() -> Option<(f32, f64)> {
        None
    }
}

/// The host of this build.
#[cfg(uncharted_loader)]
pub fn system() -> Box<dyn Host> {
    Box::new(sdl::Sdl::default())
}

/// No SDL, no GPU library: no pad, keyboard or mouse.
#[cfg(not(uncharted_loader))]
pub fn system() -> Box<dyn Host> {
    Box::new(Nothing)
}

#[cfg(not(uncharted_loader))]
use sdl3_sys as _;

#[cfg(not(uncharted_loader))]
struct Nothing;

#[cfg(not(uncharted_loader))]
impl Host for Nothing {
    fn pads_ready(&self) -> bool {
        false
    }
    fn init_pads(&mut self) -> Result<(), String> {
        Err("this build has no SDL".into())
    }
    fn pads(&mut self) -> Vec<PadId> {
        Vec::new()
    }
    fn open(&mut self, _: PadId) -> bool {
        false
    }
    fn close(&mut self, _: PadId) {}
    fn connected(&self, _: PadId) -> bool {
        false
    }
    fn name(&self, _: PadId) -> String {
        String::new()
    }
    fn button(&self, _: PadId, _: i32) -> bool {
        false
    }
    fn axis(&self, _: PadId, _: i32) -> i16 {
        0
    }
    fn fingers(&self, _: PadId) -> Vec<Finger> {
        Vec::new()
    }
    fn has_rumble(&self, _: PadId) -> bool {
        false
    }
    fn rumble(&mut self, _: PadId, _: u16, _: u16, _: u32) -> bool {
        false
    }
    fn set_led(&mut self, _: PadId, _: u8, _: u8, _: u8) {}
    fn prompts_device(&self, _: PadId) -> i32 {
        PROMPTS_XBOX
    }
    fn sensors(&mut self, _: PadId, _: bool) {}
    fn gyro(&self, _: PadId) -> Option<[f32; 3]> {
        None
    }
    fn accel(&self, _: PadId) -> Option<[f32; 3]> {
        None
    }
    fn keyboard(&self) -> Option<Vec<bool>> {
        None
    }
    fn scancode_from_name(&self, _: &str) -> Option<i32> {
        None
    }
    fn keycode_from_name(&self, _: &str) -> Option<i32> {
        None
    }
    fn scancode_from_keycode(&self, _: i32) -> Option<i32> {
        None
    }
    fn key_name(&self, _: i32) -> String {
        String::new()
    }
    fn scancode_name(&self, _: i32) -> String {
        String::new()
    }
    fn button_from_name(&self, _: &str) -> Option<i32> {
        None
    }
    fn mouse_configure(&mut self, _: bool) {}
    fn mouse_sample(&mut self) -> MouseSample {
        MouseSample::default()
    }
    fn overlay_captures_input(&self) -> bool {
        false
    }
    fn prompts_controller(&mut self, _: i32) {}
}

#[cfg(uncharted_loader)]
mod sdl {
    use super::{Finger, Host, MouseSample, PadId, PROMPTS_XBOX};
    use sdl3_sys::error::SDL_GetError;
    use sdl3_sys::gamepad::*;
    use sdl3_sys::hints::{SDL_SetHint, SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS};
    use sdl3_sys::init::{
        SDL_InitSubSystem, SDL_QuitSubSystem, SDL_WasInit, SDL_INIT_GAMEPAD, SDL_INIT_VIDEO,
    };
    use sdl3_sys::joystick::SDL_JoystickID;
    use sdl3_sys::keyboard::{
        SDL_GetKeyFromName, SDL_GetKeyName, SDL_GetKeyboardState, SDL_GetScancodeFromKey,
        SDL_GetScancodeFromName, SDL_GetScancodeName,
    };
    use sdl3_sys::keycode::SDL_Keycode;
    use sdl3_sys::properties::SDL_GetBooleanProperty;
    use sdl3_sys::scancode::SDL_Scancode;
    use sdl3_sys::sensor::{SDL_SENSOR_ACCEL, SDL_SENSOR_GYRO};
    use sdl3_sys::stdinc::SDL_free;
    use sdl3_sys::timer::SDL_Delay;
    use std::collections::HashMap;
    use std::ffi::{c_char, c_int, CStr, CString};
    use std::ptr::{null_mut, NonNull};

    extern "C" {
        pub fn bbgpu_overlay_captures_input() -> c_int;
        pub fn bbgpu_input_sample();
        pub fn bbgpu_prompts_controller(device: c_int);
        pub fn bbgpu_mouse_configure(look: c_int);
        pub fn bbgpu_camera_yaw(yaw_degrees: *mut f32, age_seconds: *mut f64) -> c_int;
        pub fn bbgpu_mouse_sample(
            dx: *mut f32,
            dy: *mut f32,
            buttons: *mut u32,
            wheel: *mut c_int,
        ) -> c_int;
    }

    fn text(pointer: *const c_char) -> String {
        if pointer.is_null() {
            return String::new();
        }
        // SAFETY: SDL returns a NUL-terminated string that lives until the next call.
        unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into()
    }

    /// The SDL gamepads this module opened, by instance id.
    #[derive(Default)]
    pub(super) struct Sdl {
        pads: HashMap<PadId, NonNull<SDL_Gamepad>>,
    }

    // SAFETY: SDL gamepads are used only under the pad lock.
    unsafe impl Send for Sdl {}

    impl Sdl {
        fn get(&self, id: PadId) -> Option<*mut SDL_Gamepad> {
            self.pads.get(&id).map(|pad| pad.as_ptr())
        }
    }

    impl Drop for Sdl {
        fn drop(&mut self) {
            for pad in self.pads.values() {
                // SAFETY: opened by `open`, closed once.
                unsafe { SDL_CloseGamepad(pad.as_ptr()) };
            }
        }
    }

    impl Host for Sdl {
        fn hints(&mut self) {
            // The controller keeps working while the game window does not have the keyboard
            // focus (issue #74). SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=0 in the environment still
            // turns it off.
            // SAFETY: NUL-terminated hint name and value.
            unsafe { SDL_SetHint(SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS, c"1".as_ptr()) };
        }

        fn pads_ready(&self) -> bool {
            // SAFETY: a plain query.
            unsafe { SDL_WasInit(SDL_INIT_GAMEPAD) != 0 }
        }

        fn init_pads(&mut self) -> Result<(), String> {
            // SAFETY: SDL counts subsystem starts.
            if unsafe { SDL_InitSubSystem(SDL_INIT_GAMEPAD) } {
                Ok(())
            } else {
                // SAFETY: SDL's error text.
                Err(text(SDL_GetError()))
            }
        }

        fn quit_pads(&mut self) {
            // SAFETY: as `init_pads`.
            unsafe { SDL_QuitSubSystem(SDL_INIT_GAMEPAD) };
        }

        fn update_pads(&mut self) {
            // SAFETY: a plain call.
            unsafe { SDL_UpdateGamepads() };
        }

        fn delay_ms(&self, ms: u64) {
            // SAFETY: a plain call.
            unsafe { SDL_Delay(ms as u32) };
        }

        fn pads(&mut self) -> Vec<PadId> {
            let mut count: c_int = 0;
            // SAFETY: a writable count; the array is SDL's, freed below.
            let ids = unsafe { SDL_GetGamepads(&mut count) };
            let list = if ids.is_null() {
                Vec::new()
            } else {
                // SAFETY: `count` ids.
                unsafe { std::slice::from_raw_parts(ids, count.max(0) as usize) }
                    .iter()
                    .map(|id| id.0)
                    .collect()
            };
            // SAFETY: SDL allocated the array (null is fine).
            unsafe { SDL_free(ids.cast()) };
            list
        }

        fn open(&mut self, id: PadId) -> bool {
            if self.pads.contains_key(&id) {
                return true;
            }
            // SAFETY: an id SDL listed.
            let pad = unsafe { SDL_OpenGamepad(SDL_JoystickID(id)) };
            NonNull::new(pad).is_some_and(|pad| {
                self.pads.insert(id, pad);
                true
            })
        }

        fn close(&mut self, id: PadId) {
            if let Some(pad) = self.pads.remove(&id) {
                // SAFETY: opened by `open`, closed once.
                unsafe { SDL_CloseGamepad(pad.as_ptr()) };
            }
        }

        fn connected(&self, id: PadId) -> bool {
            // SAFETY: a live gamepad.
            self.get(id)
                .is_some_and(|pad| unsafe { SDL_GamepadConnected(pad) })
        }

        fn name(&self, id: PadId) -> String {
            // SAFETY: a live gamepad; SDL owns the string.
            self.get(id)
                .map_or_else(String::new, |pad| text(unsafe { SDL_GetGamepadName(pad) }))
        }

        fn button(&self, id: PadId, button: i32) -> bool {
            // SAFETY: a live gamepad.
            self.get(id)
                .is_some_and(|pad| unsafe { SDL_GetGamepadButton(pad, SDL_GamepadButton(button)) })
        }

        fn axis(&self, id: PadId, axis: i32) -> i16 {
            // SAFETY: a live gamepad.
            self.get(id).map_or(0, |pad| unsafe {
                SDL_GetGamepadAxis(pad, SDL_GamepadAxis(axis))
            })
        }

        fn fingers(&self, id: PadId) -> Vec<Finger> {
            let Some(pad) = self.get(id) else {
                return Vec::new();
            };
            // SAFETY: a live gamepad.
            if unsafe { SDL_GetNumGamepadTouchpads(pad) } <= 0 {
                return Vec::new();
            }
            // SAFETY: as above.
            let count = unsafe { SDL_GetNumGamepadTouchpadFingers(pad, 0) };
            (0..count.max(0))
                .map(|finger| {
                    let (mut down, mut x, mut y) = (false, 0.0f32, 0.0f32);
                    // SAFETY: writable outputs; the pressure is optional (null).
                    let ok = unsafe {
                        SDL_GetGamepadTouchpadFinger(
                            pad,
                            0,
                            finger,
                            &mut down,
                            &mut x,
                            &mut y,
                            null_mut(),
                        )
                    };
                    Finger {
                        down: ok && down,
                        x,
                        y,
                    }
                })
                .collect()
        }

        fn has_rumble(&self, id: PadId) -> bool {
            // SAFETY: a live gamepad; its property set.
            self.get(id).is_some_and(|pad| unsafe {
                SDL_GetBooleanProperty(
                    SDL_GetGamepadProperties(pad),
                    SDL_PROP_GAMEPAD_CAP_RUMBLE_BOOLEAN,
                    false,
                )
            })
        }

        fn rumble(&mut self, id: PadId, low: u16, high: u16, ms: u32) -> bool {
            // SAFETY: a live gamepad.
            self.get(id)
                .is_some_and(|pad| unsafe { SDL_RumbleGamepad(pad, low, high, ms) })
        }

        fn set_led(&mut self, id: PadId, r: u8, g: u8, b: u8) {
            if let Some(pad) = self.get(id) {
                // SAFETY: a live gamepad.
                unsafe { SDL_SetGamepadLED(pad, r, g, b) };
            }
        }

        fn prompts_device(&self, id: PadId) -> i32 {
            let Some(pad) = self.get(id) else {
                return PROMPTS_XBOX;
            };
            // SAFETY: a live gamepad.
            let kind = unsafe { SDL_GetGamepadType(pad) };
            // SAFETY: as above.
            let label = unsafe { SDL_GetGamepadButtonLabel(pad, SDL_GAMEPAD_BUTTON_SOUTH) };
            super::prompts_for_gamepad_type(kind.into(), label == SDL_GAMEPAD_BUTTON_LABEL_CROSS)
        }

        fn sensors(&mut self, id: PadId, enable: bool) {
            if let Some(pad) = self.get(id) {
                // SAFETY: a live gamepad; pads without the sensor just refuse.
                unsafe {
                    SDL_SetGamepadSensorEnabled(pad, SDL_SENSOR_GYRO, enable);
                    SDL_SetGamepadSensorEnabled(pad, SDL_SENSOR_ACCEL, enable);
                }
            }
        }

        fn gyro(&self, id: PadId) -> Option<[f32; 3]> {
            self.sensor(id, SDL_SENSOR_GYRO)
        }

        fn accel(&self, id: PadId) -> Option<[f32; 3]> {
            self.sensor(id, SDL_SENSOR_ACCEL)
        }

        fn keyboard(&self) -> Option<Vec<bool>> {
            // SAFETY: plain queries; the state array is SDL's, `count` entries, copied.
            unsafe {
                if SDL_WasInit(SDL_INIT_VIDEO) == 0 {
                    return None;
                }
                let mut count: c_int = 0;
                let state = SDL_GetKeyboardState(&mut count);
                (!state.is_null()).then(|| {
                    let mut keys =
                        std::slice::from_raw_parts(state, count.max(0) as usize).to_vec();
                    keys.resize(super::SCANCODE_COUNT, false);
                    keys
                })
            }
        }

        fn scancode_from_name(&self, name: &str) -> Option<i32> {
            let name = CString::new(name).ok()?;
            // SAFETY: a NUL-terminated name.
            let code = unsafe { SDL_GetScancodeFromName(name.as_ptr()) };
            (code != SDL_Scancode::UNKNOWN).then_some(code.0)
        }

        fn keycode_from_name(&self, name: &str) -> Option<i32> {
            let name = CString::new(name).ok()?;
            // SAFETY: a NUL-terminated name.
            let key = unsafe { SDL_GetKeyFromName(name.as_ptr()) };
            (key.0 != 0).then_some(key.0 as i32)
        }

        fn scancode_from_keycode(&self, key: i32) -> Option<i32> {
            // SAFETY: the modifier output is optional (null).
            let code = unsafe { SDL_GetScancodeFromKey(SDL_Keycode(key as u32), null_mut()) };
            (code != SDL_Scancode::UNKNOWN).then_some(code.0)
        }

        fn key_name(&self, key: i32) -> String {
            // SAFETY: SDL owns the string.
            text(unsafe { SDL_GetKeyName(SDL_Keycode(key as u32)) })
        }

        fn scancode_name(&self, code: i32) -> String {
            // SAFETY: SDL owns the string.
            text(unsafe { SDL_GetScancodeName(SDL_Scancode(code)) })
        }

        fn button_from_name(&self, name: &str) -> Option<i32> {
            let name = CString::new(name).ok()?;
            // SAFETY: a NUL-terminated name.
            let button = unsafe { SDL_GetGamepadButtonFromString(name.as_ptr()) };
            (button != SDL_GAMEPAD_BUTTON_INVALID).then_some(button.0)
        }

        fn mouse_configure(&mut self, look: bool) {
            // SAFETY: the GPU library's own call.
            unsafe { bbgpu_mouse_configure(c_int::from(look)) };
        }

        fn mouse_sample(&mut self) -> MouseSample {
            let (mut dx, mut dy, mut buttons, mut wheel) = (0.0f32, 0.0f32, 0u32, 0 as c_int);
            // SAFETY: four writable outputs.
            let captured =
                unsafe { bbgpu_mouse_sample(&mut dx, &mut dy, &mut buttons, &mut wheel) } != 0;
            MouseSample {
                captured,
                dx,
                dy,
                buttons,
                wheel,
            }
        }

        fn overlay_captures_input(&self) -> bool {
            // SAFETY: the GPU library's own query.
            unsafe { bbgpu_overlay_captures_input() != 0 }
        }

        fn prompts_controller(&mut self, device: i32) {
            // SAFETY: the GPU library's own call.
            unsafe { bbgpu_prompts_controller(device) };
        }
    }

    impl Sdl {
        fn sensor(&self, id: PadId, kind: sdl3_sys::sensor::SDL_SensorType) -> Option<[f32; 3]> {
            let pad = self.get(id)?;
            let mut data = [0.0f32; 3];
            // SAFETY: a live gamepad and room for the three values.
            unsafe { SDL_GetGamepadSensorData(pad, kind, data.as_mut_ptr(), 3) }.then_some(data)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamepad_type_maps_every_sdl3_family() {
        // SDL_GAMEPAD_TYPE_* numeric values from SDL3 SDL_gamepad.h:
        // 0 Unknown, 1 Standard, 2 Xbox360, 3 XboxOne, 4 PS3, 5 PS4, 6 PS5,
        // 7 SwitchPro, 8 JoyconLeft, 9 JoyconRight, 10 JoyconPair
        assert_eq!(prompts_for_gamepad_type(2, false), PROMPTS_XBOX);
        assert_eq!(prompts_for_gamepad_type(3, false), PROMPTS_XBOX);
        assert_eq!(prompts_for_gamepad_type(4, false), PROMPTS_PLAYSTATION);
        assert_eq!(prompts_for_gamepad_type(5, false), PROMPTS_PLAYSTATION);
        assert_eq!(prompts_for_gamepad_type(6, false), PROMPTS_PLAYSTATION);
        assert_eq!(prompts_for_gamepad_type(7, false), PROMPTS_NINTENDO);
        assert_eq!(prompts_for_gamepad_type(8, false), PROMPTS_NINTENDO);
        assert_eq!(prompts_for_gamepad_type(9, false), PROMPTS_NINTENDO);
        assert_eq!(prompts_for_gamepad_type(10, false), PROMPTS_NINTENDO);
        assert_eq!(prompts_for_gamepad_type(0, false), PROMPTS_XBOX);
        assert_eq!(prompts_for_gamepad_type(1, false), PROMPTS_XBOX);
        assert_eq!(prompts_for_gamepad_type(1, true), PROMPTS_PLAYSTATION);
    }
}
