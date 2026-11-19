//! The tests of the deleted `tests/test_pad.c`, over a fake device instead of SDL's virtual
//! joystick, plus the motion sensors and the module's layouts.

use super::camera::*;
use super::host::*;
use super::input::*;
use super::read::Pad;
use super::{PadData, PadTouch};
use crate::benchmark_run::BenchmarkPad;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

struct FakePad {
    name: String,
    buttons: u32,
    axes: [i16; 6],
    fingers: Vec<Finger>,
    gyro: Option<[f32; 3]>,
    accel: Option<[f32; 3]>,
    sensors: bool,
    prompts: i32,
}

impl Default for FakePad {
    fn default() -> FakePad {
        FakePad {
            name: String::new(),
            buttons: 0,
            axes: [0; 6],
            fingers: Vec::new(),
            gyro: None,
            accel: None,
            sensors: false,
            prompts: PROMPTS_PLAYSTATION,
        }
    }
}

#[derive(Default)]
struct Fake {
    pads: BTreeMap<u32, FakePad>,
    open: Vec<u32>,
    rumbles: Vec<(u32, u16, u16)>,
    rumble_fails: bool,
    overlay: bool,
    benchmark: Option<BenchmarkPad>,
    keys: Option<Vec<bool>>,
    prompts: Vec<i32>,
    leds: Vec<(u8, u8, u8)>,
    mouse: MouseSample,
}

#[derive(Clone, Default)]
struct FakeHost(Arc<Mutex<Fake>>);

impl FakeHost {
    fn state(&self) -> std::sync::MutexGuard<'_, Fake> {
        self.0.lock().unwrap()
    }

    fn attach(&self, id: u32, name: &str) {
        self.state().pads.insert(
            id,
            FakePad {
                name: name.into(),
                ..FakePad::default()
            },
        );
    }

    fn press(&self, id: u32, button: i32, down: bool) {
        let mut state = self.state();
        let pad = state.pads.get_mut(&id).unwrap();
        pad.buttons = (pad.buttons & !(1 << button)) | (u32::from(down) << button);
    }

    fn attach_family(&self, id: u32, name: &str, prompts: i32) {
        self.attach(id, name);
        self.state().pads.get_mut(&id).unwrap().prompts = prompts;
    }

    fn set_axis(&self, id: u32, axis: i32, value: i16) {
        self.state().pads.get_mut(&id).unwrap().axes[axis as usize] = value;
    }
}

const KEYS: [(&str, i32); 23] = [
    ("Space", 44),
    ("C", 6),
    ("F", 9),
    ("E", 8),
    ("Q", 20),
    ("G", 10),
    ("Left Shift", 225),
    ("V", 25),
    ("Escape", 41),
    ("Return", 40),
    ("Tab", 43),
    ("Up", 82),
    ("Down", 81),
    ("Left", 80),
    ("Right", 79),
    ("W", 26),
    ("S", 22),
    ("A", 4),
    ("D", 7),
    ("I", 12),
    ("K", 14),
    ("J", 13),
    ("L", 15),
];
const BUTTONS: [(&str, i32); 26] = [
    ("a", 0),
    ("b", 1),
    ("x", 2),
    ("y", 3),
    ("back", 4),
    ("guide", 5),
    ("start", 6),
    ("leftstick", 7),
    ("rightstick", 8),
    ("leftshoulder", 9),
    ("rightshoulder", 10),
    ("dpup", 11),
    ("dpdown", 12),
    ("dpleft", 13),
    ("dpright", 14),
    ("misc1", 15),
    ("paddle1", 16),
    ("paddle2", 17),
    ("paddle3", 18),
    ("paddle4", 19),
    ("touchpad", 20),
    ("misc2", 21),
    ("misc3", 22),
    ("misc4", 23),
    ("misc5", 24),
    ("misc6", 25),
];

impl Host for FakeHost {
    fn pads_ready(&self) -> bool {
        true
    }
    fn init_pads(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn pads(&mut self) -> Vec<PadId> {
        self.state().pads.keys().copied().collect()
    }
    fn open(&mut self, id: PadId) -> bool {
        let mut state = self.state();
        let known = state.pads.contains_key(&id);
        if known {
            state.open.push(id);
        }
        known
    }
    fn close(&mut self, id: PadId) {
        self.state().open.retain(|&open| open != id);
    }
    fn connected(&self, id: PadId) -> bool {
        self.state().pads.contains_key(&id)
    }
    fn name(&self, id: PadId) -> String {
        self.state()
            .pads
            .get(&id)
            .map(|p| p.name.clone())
            .unwrap_or_default()
    }
    fn button(&self, id: PadId, button: i32) -> bool {
        self.state()
            .pads
            .get(&id)
            .is_some_and(|p| p.buttons & (1 << button) != 0)
    }
    fn axis(&self, id: PadId, axis: i32) -> i16 {
        self.state()
            .pads
            .get(&id)
            .map_or(0, |p| p.axes[axis as usize])
    }
    fn fingers(&self, id: PadId) -> Vec<Finger> {
        self.state()
            .pads
            .get(&id)
            .map(|p| p.fingers.clone())
            .unwrap_or_default()
    }
    fn has_rumble(&self, id: PadId) -> bool {
        self.connected(id)
    }
    fn rumble(&mut self, id: PadId, low: u16, high: u16, _ms: u32) -> bool {
        let mut state = self.state();
        state.rumbles.push((id, low, high));
        !state.rumble_fails
    }
    fn set_led(&mut self, _id: PadId, r: u8, g: u8, b: u8) {
        self.state().leds.push((r, g, b));
    }
    fn prompts_device(&self, id: PadId) -> i32 {
        self.state()
            .pads
            .get(&id)
            .map_or(PROMPTS_XBOX, |p| p.prompts)
    }
    fn sensors(&mut self, id: PadId, enable: bool) {
        if let Some(pad) = self.state().pads.get_mut(&id) {
            pad.sensors = enable;
        }
    }
    fn gyro(&self, id: PadId) -> Option<[f32; 3]> {
        self.state()
            .pads
            .get(&id)
            .filter(|p| p.sensors)
            .and_then(|p| p.gyro)
    }
    fn accel(&self, id: PadId) -> Option<[f32; 3]> {
        self.state()
            .pads
            .get(&id)
            .filter(|p| p.sensors)
            .and_then(|p| p.accel)
    }
    fn keyboard(&self) -> Option<Vec<bool>> {
        self.state().keys.clone()
    }
    fn scancode_from_name(&self, name: &str) -> Option<i32> {
        KEYS.iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|&(_, code)| code)
    }
    fn keycode_from_name(&self, name: &str) -> Option<i32> {
        name.bytes()
            .next()
            .map(|b| i32::from(b.to_ascii_lowercase()))
    }
    fn scancode_from_keycode(&self, key: i32) -> Option<i32> {
        (97..=122).contains(&key).then(|| 4 + key - 97)
    }
    fn key_name(&self, key: i32) -> String {
        char::from_u32(key as u32)
            .map(String::from)
            .unwrap_or_default()
    }
    fn scancode_name(&self, code: i32) -> String {
        format!("scancode {code}")
    }
    fn button_from_name(&self, name: &str) -> Option<i32> {
        BUTTONS
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|&(_, b)| b)
    }
    fn mouse_configure(&mut self, _look: bool) {}
    fn mouse_sample(&mut self) -> MouseSample {
        self.state().mouse
    }
    fn overlay_captures_input(&self) -> bool {
        self.state().overlay
    }
    fn prompts_controller(&mut self, device: i32) {
        self.state().prompts.push(device);
    }
    fn benchmark_pad(&self) -> Option<BenchmarkPad> {
        self.state().benchmark
    }
}

fn temp(name: &str) -> String {
    std::env::temp_dir()
        .join(format!("uncharted-pad-test-{}-{name}", std::process::id()))
        .to_string_lossy()
        .into()
}

/// Writes a file and waits out the module's 20 ms check of it.
fn write(path: &str, text: &str) {
    std::fs::write(path, text).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(25));
}

struct Rig {
    host: FakeHost,
    pad: Pad,
    camera: Arc<Mutex<Camera>>,
    script: String,
    config: String,
}

fn rig(name: &str, config_text: &str, scripted: bool) -> Rig {
    let host = FakeHost::default();
    let script = temp(&format!("{name}-pad"));
    let config = temp(&format!("{name}-input"));
    write(&config, config_text);
    write(&script, "");
    let settings = Settings {
        input_config: Some(config.clone()),
        pad_file: scripted.then(|| script.clone()),
        pad_host: true,
        ..Settings::default()
    };
    let camera = Arc::new(Mutex::new(Camera::new(Knobs::default())));
    let mut pad = Pad::new(Box::new(host.clone()), settings, camera.clone());
    assert_eq!(pad.init(), 0);
    assert_eq!(pad.open(1, 0, 0), 1);
    Rig {
        host,
        pad,
        camera,
        script,
        config,
    }
}

impl Rig {
    fn read(&mut self) -> PadData {
        self.pad.read_state(1).unwrap()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.script);
        let _ = std::fs::remove_file(&self.config);
    }
}

#[test]
fn layouts_match_the_c_abi() {
    assert_eq!(std::mem::size_of::<PadData>(), 120);
    assert_eq!(std::mem::size_of::<PadTouch>(), 8);
    assert_eq!(std::mem::offset_of!(PadData, touches), 60);
    assert_eq!(std::mem::offset_of!(PadData, timestamp), 80);
    assert_eq!(std::mem::size_of::<super::ControllerInfo>(), 28);
    let neutral = PadData::neutral();
    assert_eq!(
        (neutral.left_x, neutral.right_y, neutral.orientation[3]),
        (128, 128, 1.0)
    );
}

#[test]
fn handle_checks_follow_the_c_module() {
    let mut pad = Pad::new(
        Box::new(FakeHost::default()),
        Settings::default(),
        Arc::new(Mutex::new(Camera::new(Knobs::default()))),
    );
    assert_eq!(pad.open(1, 0, 0), super::read::ERR_NOT_INITIALIZED);
    pad.init();
    assert_eq!(pad.open(2, 0, 0), super::read::ERR_INVALID_ARG);
    assert_eq!(pad.open(1, 1, 0), super::read::ERR_INVALID_ARG);
    assert_eq!(pad.open(1, 2, 1), super::read::ERR_INVALID_ARG);
    assert_eq!(pad.open(1, 2, 0), 1);
    assert_eq!(pad.open(1, 0, 0), super::read::ERR_ALREADY_OPENED);
    assert_eq!(pad.handle_ok(7), super::read::ERR_INVALID_HANDLE);
    let info = pad.info(1).unwrap();
    assert_eq!(
        (info.resolution_x, info.resolution_y, info.connected_count),
        (1920, 943, 1)
    );
    assert_eq!(pad.close(1), 0);
    assert_eq!(pad.close(1), super::read::ERR_INVALID_HANDLE);
}

#[test]
fn scripted_input_touchpad_vibration_and_second_pad() {
    let mut rig = rig("main", "pad_vibration=0\npad_vibration_strength=50\n", true);
    write(&rig.script, "cross l3 touchpad_left");
    let data = rig.read();
    assert!(!rig.pad.input.pad_vibration);
    assert_eq!(rig.pad.input.pad_vibration_strength, 50);
    let want = button::CROSS | button::L3 | button::TOUCHPAD;
    assert_eq!(data.buttons & want, want);
    assert!(data.touch_count == 1 && data.touches[0].x == 480 && data.touches[0].y == 471);
    write(&rig.script, "touchpad_right");
    let data = rig.read();
    assert!(data.touch_count == 1 && data.touches[0].x == 1440);
    write(&rig.script, "");
    let data = rig.read();
    assert!(data.buttons == 0 && data.touch_count == 0);

    // A pad with two fingers on its touch surface and the touchpad click.
    rig.host.attach(10, "test controller");
    rig.host.state().pads.get_mut(&10).unwrap().fingers = vec![
        Finger {
            down: true,
            x: 0.75,
            y: 0.5,
        },
        Finger {
            down: true,
            x: 0.25,
            y: 1.0,
        },
    ];
    rig.host.press(10, BUTTON_TOUCHPAD, true);
    let data = rig.read();
    assert_eq!(rig.pad.current, Some(10));
    assert!(data.touch_count == 2 && data.buttons & button::TOUCHPAD != 0);
    assert_eq!(
        data.touches[0],
        PadTouch {
            x: 1439,
            y: 471,
            id: 0,
            reserve: [0; 3]
        }
    );
    assert_eq!(
        data.touches[1],
        PadTouch {
            x: 480,
            y: 942,
            id: 1,
            reserve: [0; 3]
        }
    );

    // Rumble: off by `pad_vibration=0`, then scaled by the strength.
    rig.pad.vibrate(64, 192);
    assert!(rig.host.state().rumbles.is_empty());
    rig.pad.input.pad_vibration = true;
    rig.pad.vibrate(64, 192);
    assert_eq!(rig.host.state().rumbles, [(10, 8224, 24672)]);
    rig.pad.input.pad_vibration_strength = 100;
    rig.pad.vibrate(64, 192);
    assert_eq!(rig.host.state().rumbles[1], (10, 64 * 257, 192 * 257));

    // The settings menu open: neutral input.
    rig.host.state().overlay = true;
    let data = rig.read();
    assert!(data.touch_count == 0 && data.buttons == 0);
    rig.host.state().overlay = false;

    // Benchmark mode replaces the whole read: the real pad and the replay file are ignored.
    rig.host.state().benchmark = Some(BenchmarkPad {
        buttons: button::CROSS,
        left_x: 10,
        left_y: 20,
        right_x: 30,
        right_y: 40,
    });
    let data = rig.read();
    assert!(data.buttons == button::CROSS && data.l2 == 0 && data.r2 == 0);
    assert_eq!(
        (data.left_x, data.left_y, data.right_x, data.right_y),
        (10, 20, 30, 40)
    );
    rig.host.state().benchmark = None;
    assert_ne!(rig.read().buttons, button::CROSS); // the real pad again

    // Fingers lifted: the click alone is a left-side touch.
    rig.host.state().pads.get_mut(&10).unwrap().fingers.clear();
    let data = rig.read();
    assert!(data.touch_count == 1 && data.touches[0].x == 480);

    // A second gamepad; pressing a button on it makes it the one the game reads.
    rig.host.press(10, BUTTON_TOUCHPAD, false);
    rig.host.attach(11, "second test controller");
    rig.pad.listed_us = 0;
    let data = rig.read();
    assert!(data.buttons & button::CROSS == 0 && rig.pad.pads.len() == 2);
    rig.host.press(11, BUTTON_SOUTH, true);
    assert!(rig.read().buttons & button::CROSS != 0);
    assert_eq!(rig.pad.current, Some(11));

    // Hot-plug: the pad in use leaves, the other one is read.
    rig.host.state().pads.remove(&11);
    rig.pad.listed_us = 0;
    rig.read();
    assert_eq!(
        (rig.pad.current, rig.pad.pads.clone()),
        (Some(10), vec![10])
    );
    assert!(rig.pad.report_line().contains("test controller"));
}

#[test]
fn motion_sensors_are_reported_once_the_game_enables_them() {
    let mut rig = rig("motion", "", false);
    rig.host.attach(5, "motion pad");
    {
        let mut state = rig.host.state();
        let pad = state.pads.get_mut(&5).unwrap();
        pad.gyro = Some([1.0, -2.0, 0.5]);
        pad.accel = Some([0.0, 9.80665, 19.6133]);
    }
    let off = rig.read();
    assert_eq!(
        (off.angular_velocity, off.acceleration),
        ([0.0; 3], [0.0; 3])
    );
    assert!(!rig.host.state().pads[&5].sensors);
    rig.pad.set_motion(true);
    let on = rig.read();
    assert_eq!(on.angular_velocity, [1.0, -2.0, 0.5]);
    assert!((on.acceleration[1] - 1.0).abs() < 1e-6 && (on.acceleration[2] - 2.0).abs() < 1e-6);
    assert!(rig.host.state().pads[&5].sensors);
    rig.pad.set_motion(false);
    assert!(!rig.host.state().pads[&5].sensors);
    assert_eq!(rig.read().angular_velocity, [0.0; 3]);
}

#[test]
fn keyboard_binds_and_the_controller_work_together() {
    let mut rig = rig(
        "keys",
        "key.cross=Space\npad.circle=none\nkey.triangle=Z\n",
        false,
    );
    let mut keys = vec![false; SCANCODE_COUNT];
    keys[44] = true; // Space: Cross
    keys[26] = true; // W: left stick up
    rig.host.state().keys = Some(keys);
    // No pad: the keys count once the mirror window is not in the way (no pad, no filter).
    let data = rig.read();
    assert!(data.buttons & button::CROSS != 0);
    assert_eq!(data.left_y, 0);
    assert_eq!(rig.pad.input.keys[3][0], Bind::Layout(122)); // "Z", by layout
    assert_eq!(rig.pad.input.pad[1], -1); // pad.circle=none
}

#[test]
fn mirrored_keys_and_wheel_steps_are_filtered() {
    let mut rig = rig("mirror", "", false);
    rig.host.attach(2, "pad");
    let pad_id = Some(2);
    let t = 1_000_000_000_000u64;
    let mut keys = Some(vec![false; SCANCODE_COUNT]);
    let (mut mouse, mut wheel) = (0u32, 0i32);
    let p = &mut rig.pad;
    p.filter_mirrored_input(pad_id, t, &mut keys, &mut mouse, &mut wheel);
    // The stick moves, and an arrow key follows 5 ms later: the key is a copy of it.
    rig.host.state().pads.get_mut(&2).unwrap().axes[axis::LEFT_Y as usize] = 30000;
    p.filter_mirrored_input(pad_id, t + 1_000_000, &mut keys, &mut mouse, &mut wheel);
    let down = |down: bool| {
        let mut keys = vec![false; SCANCODE_COUNT];
        keys[81] = down;
        keys[26] = false;
        Some(keys)
    };
    keys = down(true);
    p.filter_mirrored_input(pad_id, t + 1_005_000, &mut keys, &mut mouse, &mut wheel);
    assert!(!keys.as_ref().unwrap()[81]);
    keys = down(true);
    p.filter_mirrored_input(pad_id, t + 1_200_000, &mut keys, &mut mouse, &mut wheel);
    assert!(!keys.as_ref().unwrap()[81]); // held on: still ignored
    keys = down(false);
    wheel = 1; // the trackpad's wheel step, then the stick returns 10 ms later
    p.filter_mirrored_input(pad_id, t + 1_300_000, &mut keys, &mut mouse, &mut wheel);
    assert_eq!(wheel, 0);
    rig.host.state().pads.get_mut(&2).unwrap().axes[axis::LEFT_Y as usize] = 0;
    let mut at = t + 1_310_000;
    while at <= t + 1_500_000 {
        wheel = 0;
        keys = down(false);
        p.filter_mirrored_input(pad_id, at, &mut keys, &mut mouse, &mut wheel);
        assert_eq!(wheel, 0);
        at += 10_000;
    }
    // The real keyboard, the controller untouched: counts after the window.
    let w = |on: bool| {
        let mut keys = vec![false; SCANCODE_COUNT];
        keys[26] = on;
        Some(keys)
    };
    keys = w(true);
    p.filter_mirrored_input(pad_id, t + 3_000_000, &mut keys, &mut mouse, &mut wheel);
    assert!(!keys.as_ref().unwrap()[26]);
    keys = w(true);
    p.filter_mirrored_input(pad_id, t + 3_060_000, &mut keys, &mut mouse, &mut wheel);
    assert!(keys.as_ref().unwrap()[26]);
    keys = w(false);
    wheel = -1; // the real wheel
    p.filter_mirrored_input(pad_id, t + 4_000_000, &mut keys, &mut mouse, &mut wheel);
    assert_eq!(wheel, 0);
    p.filter_mirrored_input(pad_id, t + 4_060_000, &mut keys, &mut mouse, &mut wheel);
    assert_eq!(wheel, -1);
    wheel = 0;
    p.filter_mirrored_input(pad_id, t + 4_070_000, &mut keys, &mut mouse, &mut wheel);
    assert_eq!(wheel, 0); // one step, once
                          // Mouse buttons are filtered like keys.
    mouse = 1;
    p.filter_mirrored_input(pad_id, t + 6_000_000, &mut keys, &mut mouse, &mut wheel);
    assert_eq!(mouse, 0);
    mouse = 1;
    p.filter_mirrored_input(pad_id, t + 6_060_000, &mut keys, &mut mouse, &mut wheel);
    assert_eq!(mouse, 1);
}

#[test]
fn prompts_follow_the_device_in_use() {
    let mut rig = rig("prompts", "", false);
    rig.host.attach(3, "ps pad");
    rig.read();
    rig.host.press(3, BUTTON_SOUTH, true);
    rig.read();
    assert_eq!(rig.host.state().prompts, [1]);
}

#[test]
fn prompts_report_the_controller_family_and_follow_switches() {
    let mut rig = rig("prompt-families", "", false);
    rig.host.attach_family(1, "family pad", PROMPTS_PLAYSTATION);
    rig.read();
    for prompts in [
        PROMPTS_PLAYSTATION,
        PROMPTS_NINTENDO,
        PROMPTS_XBOX,
        PROMPTS_PLAYSTATION,
    ] {
        rig.host.state().pads.get_mut(&1).unwrap().prompts = prompts;
        rig.host.press(1, BUTTON_SOUTH, true);
        assert_ne!(rig.read().buttons & button::CROSS, 0);
        assert_eq!(
            rig.host.state().prompts.last().copied(),
            Some(prompts),
            "family {prompts}"
        );
        rig.host.press(1, BUTTON_SOUTH, false);
        rig.read();
    }
    assert_eq!(
        rig.host.state().prompts,
        [
            PROMPTS_PLAYSTATION,
            PROMPTS_NINTENDO,
            PROMPTS_XBOX,
            PROMPTS_PLAYSTATION
        ]
    );
}

fn family_rig(key: &str, name: &str, prompts: i32) -> Rig {
    let rig = rig(key, "", false);
    rig.host.attach_family(1, name, prompts);
    rig
}

/// Every game input over one controller family: the face buttons by position (the bottom one is
/// the game's Cross), both shoulders, both stick clicks, both triggers, both sticks, the d-pad,
/// OPTIONS and the touchpad, plus the buttons the game does not use.
fn assert_every_input_maps_by_position(key: &str, name: &str, prompts: i32) {
    let mut rig = family_rig(key, name, prompts);
    let id = 1;
    rig.read();
    const BUTTON_ACTIONS: [(i32, u32); 14] = [
        (0, button::CROSS),
        (1, button::CIRCLE),
        (2, button::SQUARE),
        (3, button::TRIANGLE),
        (9, button::L1),
        (10, button::R1),
        (7, button::L3),
        (8, button::R3),
        (6, button::OPTIONS),
        (4, button::TOUCHPAD),
        (11, button::UP),
        (12, button::DOWN),
        (13, button::LEFT),
        (14, button::RIGHT),
    ];
    for (sdl, game) in BUTTON_ACTIONS {
        rig.host.press(id, sdl, true);
        let data = rig.read();
        assert_ne!(data.buttons & game, 0, "{name}: SDL button {sdl}");
        rig.host.press(id, sdl, false);
        rig.read();
    }
    for sdl in [5, 15, 16, 17, 18, 19, 21, 22, 23, 24, 25] {
        rig.host.press(id, sdl, true);
        assert_eq!(rig.read().buttons, 0, "{name}: unused SDL button {sdl}");
        rig.host.press(id, sdl, false);
        rig.read();
    }
    // The physical touchpad click, next to the touchpad action on BACK.
    rig.host.press(id, BUTTON_TOUCHPAD, true);
    assert_ne!(
        rig.read().buttons & button::TOUCHPAD,
        0,
        "{name}: touchpad click"
    );
    rig.host.press(id, BUTTON_TOUCHPAD, false);
    rig.read();
    // Both triggers.
    rig.host.set_axis(id, axis::LEFT_TRIGGER, i16::MAX);
    let data = rig.read();
    assert_ne!(data.buttons & button::L2, 0, "{name}: left trigger");
    assert_eq!(data.l2, 255, "{name}: left trigger value");
    rig.host.set_axis(id, axis::LEFT_TRIGGER, 0);
    rig.read();
    rig.host.set_axis(id, axis::RIGHT_TRIGGER, i16::MAX);
    let data = rig.read();
    assert_ne!(data.buttons & button::R2, 0, "{name}: right trigger");
    assert_eq!(data.r2, 255, "{name}: right trigger value");
    rig.host.set_axis(id, axis::RIGHT_TRIGGER, 0);
    rig.read();
    // Both sticks, each end of both axes.
    type AxisField = fn(&PadData) -> u8;
    let sticks: [(i32, AxisField); 4] = [
        (axis::LEFT_X, |d| d.left_x),
        (axis::LEFT_Y, |d| d.left_y),
        (axis::RIGHT_X, |d| d.right_x),
        (axis::RIGHT_Y, |d| d.right_y),
    ];
    for (a, value) in sticks {
        rig.host.set_axis(id, a, i16::MIN);
        assert_eq!(value(&rig.read()), 0, "{name}: stick axis {a} low");
        rig.host.set_axis(id, a, i16::MAX);
        assert_eq!(value(&rig.read()), 255, "{name}: stick axis {a} high");
        rig.host.set_axis(id, a, 0);
        rig.read();
    }
    // The bottom face button is the game's Cross on every pad, and the family reaches the prompts.
    rig.host.press(id, BUTTON_SOUTH, true);
    assert_ne!(rig.read().buttons & button::CROSS, 0, "{name}: bottom face");
    assert_eq!(
        rig.host.state().prompts.last().copied(),
        Some(prompts),
        "{name}: prompt family"
    );
    rig.host.press(id, BUTTON_SOUTH, false);
    rig.read();
}

#[test]
fn xbox_pad_maps_every_input_by_position() {
    assert_every_input_maps_by_position("xbox", "Xbox Series Controller", PROMPTS_XBOX);
}

#[test]
fn dualshock4_pad_maps_every_input_by_position() {
    assert_every_input_maps_by_position("ds4", "DualShock 4", PROMPTS_PLAYSTATION);
}

#[test]
fn dualsense_pad_maps_every_input_by_position() {
    assert_every_input_maps_by_position("dualsense", "DualSense", PROMPTS_PLAYSTATION);
}

#[test]
fn switch_pro_pad_maps_every_input_by_position() {
    assert_every_input_maps_by_position("switch-pro", "Switch Pro Controller", PROMPTS_NINTENDO);
}

#[test]
fn joycon_pair_maps_every_input_by_position() {
    assert_every_input_maps_by_position("joycon-pair", "Joy-Con (L/R)", PROMPTS_NINTENDO);
}

#[test]
fn joycon_single_maps_every_input_by_position() {
    assert_every_input_maps_by_position("joycon-single", "Joy-Con (L)", PROMPTS_NINTENDO);
}

#[test]
fn generic_sdl3_pad_maps_every_input_by_position() {
    assert_every_input_maps_by_position("generic", "Generic Gamepad", PROMPTS_XBOX);
}

#[test]
fn controller_tool_reports_and_rumbles() {
    let host = FakeHost::default();
    let mut boxed = host.clone();
    assert_eq!(super::tool(&mut boxed, true, 50), 0);
    assert!(host.state().rumbles.is_empty());
    host.attach(1, "tool pad");
    assert_eq!(super::tool(&mut boxed, true, 50), 0);
    assert_eq!(host.state().rumbles, [(1, 32768, 32768), (1, 0, 0)]);
    assert!(host.state().open.is_empty());
}

#[test]
fn controller_tool_reports_a_refused_rumble_start() {
    let host = FakeHost::default();
    let mut boxed = host.clone();
    host.attach(1, "tool pad");
    host.state().rumble_fails = true;
    let (code, lines) = super::tool_lines(&mut boxed, true, 100);
    assert_eq!(code, 0);
    assert_eq!(lines.last().map(String::as_str), Some("tested=0"));
    assert_eq!(host.state().rumbles, [(1, 65535, 65535), (1, 0, 0)]);
}

#[test]
fn benchmark_replaces_the_whole_read() {
    let mut rig = rig("benchmark-whole", "", false);
    rig.host.attach(3, "motion pad");
    {
        let mut state = rig.host.state();
        let pad = state.pads.get_mut(&3).unwrap();
        pad.fingers = vec![Finger {
            down: true,
            x: 0.5,
            y: 0.5,
        }];
        pad.gyro = Some([1.0, 2.0, 3.0]);
        pad.accel = Some([9.80665, 0.0, 0.0]);
        state.mouse = MouseSample {
            captured: true,
            dx: 50.0,
            dy: -25.0,
            buttons: 1,
            wheel: 1,
        };
    }
    rig.pad.set_motion(true);
    rig.read(); // the player's pad, touch, motion and mouse reach the camera queues
    let sample_before = rig.camera.lock().unwrap().sample;
    rig.host.state().benchmark = Some(BenchmarkPad {
        buttons: button::CROSS,
        left_x: 10,
        left_y: 20,
        right_x: 30,
        right_y: 40,
    });
    let data = rig.read();
    assert_eq!(data.buttons, button::CROSS);
    assert_eq!(
        (data.left_x, data.left_y, data.right_x, data.right_y),
        (10, 20, 30, 40)
    );
    assert_eq!((data.l2, data.r2), (0, 0));
    assert_eq!(data.touch_count, 0);
    assert_eq!(data.touches, [PadTouch::default(); 2]);
    assert_eq!(data.angular_velocity, [0.0; 3]);
    assert_eq!(data.acceleration, [0.0; 3]);
    assert_eq!(data.orientation, [0.0, 0.0, 0.0, 1.0]);
    // No host sample was taken, so no mouse motion entered the camera queues.
    assert_eq!(rig.camera.lock().unwrap().sample, sample_before);
}

#[test]
fn settings_and_light_bar() {
    let mut rig = rig(
        "led",
        "mouse_aim_sensitivity=9\nmouse_sensitivity=-1\npad_vibration_strength=500\nmouse_raw=1\n",
        false,
    );
    rig.host.attach(4, "led pad");
    rig.read();
    let camera = rig.camera.lock().unwrap().input;
    assert_eq!(
        (
            camera.mouse_aim_sensitivity,
            camera.mouse_sensitivity,
            camera.mouse_raw
        ),
        (3.0, 1.0, true)
    );
    assert_eq!(rig.pad.input.pad_vibration_strength, 100);
    rig.pad.set_led(1, 2, 3);
    assert_eq!(rig.host.state().leds, [(1, 2, 3)]);
    assert_eq!(
        (
            atoi(" -12x"),
            atoi("x"),
            atof("1.5e1z"),
            atof("-.5"),
            atof("e5")
        ),
        (-12, 0, 15.0, -0.5, 0.0)
    );
}

#[test]
fn injected_mouse_reaches_raw_aim_while_armed() {
    let mut rig = rig("aim", "mouse_raw=1\n", true);
    write(&rig.script, "l2");
    rig.read();
    assert!(rig.camera.lock().unwrap().aim_held);
    rig.camera.lock().unwrap().aim_direct_us = super::now_us();
    write(&rig.script, "l2 mx=1000");
    rig.read();
    assert!(rig.camera.lock().unwrap().aim_dx > 1.0);
    write(&rig.script, "");
    rig.read();
    assert!(!rig.camera.lock().unwrap().aim_held);
}

fn camera() -> Camera {
    Camera::new(Knobs::default())
}

fn heading_error(dir: &[f32; 3], expected: f32) -> bool {
    (heading_of(dir) - expected).abs() < 0.01
}

#[test]
fn aim_controller_handoff_gives_each_controller_every_turn_once() {
    let mut c = camera();
    c.input.mouse_raw = true;
    c.aim_held = true;
    c.aim_press_us = 100;
    let (mut a, mut b) = ([0.0, 0.0, -1.0f32], [0.0, 0.0, -1.0f32]);
    const R: u64 = 0xc4c57;
    c.aim_dx = 10.0;
    c.raw_aim(1000, "U1", 0x1000, R, R, 0, || &mut a);
    c.raw_aim(1000, "U1", 0x2000, R, R, 0, || &mut b);
    assert!(heading_error(&a, -1.5) && heading_error(&b, -1.5));
    // Repeated calls get no duplicate turn; later motion reaches both controllers once.
    c.raw_aim(1000, "U1", 0x2000, R, R, 0, || &mut b);
    assert!(heading_error(&b, -1.5));
    c.aim_dx = 10.0;
    c.raw_aim(1000, "U1", 0x2000, R, R, 0, || &mut b);
    c.raw_aim(1000, "U1", 0x1000, R, R, 0, || &mut a);
    assert!(heading_error(&a, -3.0) && heading_error(&b, -3.0));
}

#[test]
fn aim_next_press_starts_over() {
    let mut c = camera();
    c.input.mouse_raw = true;
    const R: u64 = 0xc4c57;
    let (mut a, mut b) = ([0.0, 0.0, -1.0f32], [0.0, 0.0, -1.0f32]);
    c.aim_held = false;
    c.raw_aim(1000, "U1", 0x1000, R, R, 0, || &mut a);
    c.aim_held = true;
    c.aim_press_us += 1;
    c.raw_aim(1000, "U1", 0x1000, R, R, 0, || &mut a);
    c.raw_aim(1000, "U1", 0x3000, R, R, 0, || &mut b);
    assert!(heading_error(&a, 0.0) && heading_error(&b, 0.0));
    c.aim_dx = 4.0;
    c.raw_aim(1000, "U1", 0x3000, R, R, 0, || &mut b);
    c.raw_aim(1000, "U1", 0x1000, R, R, 0, || &mut a);
    assert!(heading_error(&a, -0.6) && heading_error(&b, -0.6));
}

#[test]
fn raw_aim_leaves_other_callers_and_raw_off_alone() {
    let mut c = camera();
    let mut aim = [0.0, 0.0, -1.0f32];
    c.input.mouse_raw = true;
    c.aim_held = true;
    c.aim_press_us = 5;
    c.aim_dx = 100.0;
    c.raw_aim(10, "U1", 0, 0x1234, 0xc4c57, 0, || &mut aim); // a wrong return address
    assert!(aim == [0.0, 0.0, -1.0] && c.aim_dx == 100.0);
    c.raw_aim(10, "U1", 0, 0xc4c57, 0xc4c57, 0, || &mut aim);
    assert!((wrap_degrees(heading_of(&aim) + 15.0)).abs() < 0.05 && c.aim_dx == 0.0);
    c.input.mouse_raw = false; // off: no turn
    c.aim_dx = 100.0;
    let before = aim[0];
    c.raw_aim(10, "U1", 0, 0xc4c57, 0xc4c57, 0, || &mut aim);
    assert_eq!(aim[0], before);
    // Armed only when the hook ran in this press, and within the last second.
    c.input.mouse_raw = true;
    c.aim_held = true;
    c.aim_press_us = 5_000_000;
    c.aim_direct_us = 4_900_000;
    assert!(!c.raw_aim_armed(5_500_000));
    c.aim_direct_us = 5_000_010;
    assert!(c.raw_aim_armed(5_500_000));
    assert!(!c.raw_aim_armed(6_100_000));
    c.aim_held = false;
    assert!(!c.raw_aim_armed(5_500_000));
}

#[test]
fn frame_motion_hands_each_camera_its_share_once() {
    let mut c = camera();
    c.dx[0] = 10.0;
    assert_eq!(c.frame_motion(0x1000, 1_000_000), (10.0, 0.0));
    assert_eq!(c.dx[0], 0.0);
    c.dx[0] = 20.0;
    assert_eq!(c.frame_motion(0x2000, 1_016_667), (20.0, 0.0));
    c.dx[0] += 30.0;
    assert_eq!(c.frame_motion(0x2000, 1_033_334), (30.0, 0.0));
    assert_eq!(c.frame_motion(0x2000, 1_033_335), (0.0, 0.0));
    // The other half of a blend still receives each turn once when it returns.
    assert_eq!(c.frame_motion(0x1000, 1_033_336), (50.0, 0.0));
    assert_eq!(c.frame_motion(0x1000, 1_033_337), (0.0, 0.0));
    c.sample += 1; // next frame's input sample has no motion
    assert_eq!(c.frame_motion(0x4000, 1_050_001), (0.0, 0.0));
    assert_eq!(c.dx[0], 0.0); // no stale share for a new owner
}

#[test]
fn frame_motion_survives_leaving_and_returning() {
    let mut c = camera();
    let mut now = 2_000_000u64;
    let near =
        |got: (f32, f32), x: f32, y: f32| (got.0 - x).abs() < 0.01 && (got.1 - y).abs() < 0.01;
    c.sample += 1;
    c.dx[0] = 5.0;
    c.dy[0] = -2.0;
    assert!(near(c.frame_motion(0x1000, now), 5.0, -2.0));
    for _ in 0..120 {
        c.sample += 1;
        now += 16_667;
        c.dx[0] = 5.0;
        c.dy[0] = -2.0;
        assert!(near(c.frame_motion(0x3000, now), 5.0, -2.0));
        assert!(c.dx[0] == 0.0 && c.dy[0] == 0.0);
    }
    c.sample += 1;
    now += 16_667;
    c.dx[0] = 5.0;
    c.dy[0] = -2.0;
    assert!(near(c.frame_motion(0x1000, now), 5.0, -2.0));
    // Both cameras share this frame once, including when their call order reverses.
    assert!(near(c.frame_motion(0x3000, now + 1), 5.0, -2.0));
    assert!(near(c.frame_motion(0x1000, now + 2), 0.0, 0.0));
    assert!(near(c.frame_motion(0x3000, now + 3), 0.0, 0.0));
    c.sample += 1;
    now += 16_667;
    c.dx[0] = 5.0;
    c.dy[0] = -2.0;
    assert!(near(c.frame_motion(0x3000, now), 5.0, -2.0));
    assert!(near(c.frame_motion(0x1000, now + 1), 5.0, -2.0));
    assert!(near(c.frame_motion(0x3000, now + 2), 0.0, 0.0));
    assert!(near(c.frame_motion(0x1000, now + 3), 0.0, 0.0));
}

#[test]
fn frame_motion_split_reads_give_both_cameras_the_same_total() {
    for extra in [0.0f32, 4.0] {
        let mut c = camera();
        let mut now = 5_000_000 + (extra as u64) * 100_000;
        c.sample += 1;
        c.frame_motion(0x1000, now - 16_667);
        c.frame_motion(0x2000, now - 16_666);
        let (mut a, mut b) = ((0.0f32, 0.0f32), (0.0f32, 0.0f32));
        for frame in 0..2 {
            c.sample += 1;
            c.dx[0] = if frame == 0 { 10.0 } else { 0.0 };
            c.dy[0] = if frame == 0 { -5.0 } else { 0.0 };
            let got = c.frame_motion(0x1000, now);
            a = (a.0 + got.0, a.1 + got.1);
            assert!(c.dx[0] == 0.0 && c.dy[0] == 0.0);
            if frame == 0 {
                c.sample += 1; // extra pad read between A and B
                c.dx[0] = extra;
                c.dy[0] = -extra / 2.0;
            }
            let got = c.frame_motion(0x2000, now + 1);
            b = (b.0 + got.0, b.1 + got.1);
            assert!(c.dx[0] == 0.0 && c.dy[0] == 0.0);
            now += 16_667;
        }
        assert!((a.0 - (10.0 + extra)).abs() < 0.01);
        assert!((b.0 - a.0).abs() < 0.01 && (b.1 - a.1).abs() < 0.01);
        assert!((a.1 - (-5.0 - extra / 2.0)).abs() < 0.01);
    }
}

#[test]
fn frame_motion_next_press_clears_the_shares() {
    let mut c = camera();
    c.dx[0] = 8.0;
    assert_eq!(c.frame_motion(0x4000, 1_050_002), (8.0, 0.0));
    c.aim_held = true;
    c.aim_press_us += 1;
    assert_eq!(c.frame_motion(0x3000, 1_050_003), (0.0, 0.0));
    c.dx[0] = 4.0;
    c.dy[0] = -2.0;
    assert_eq!(c.frame_motion(0x3000, 1_066_670), (4.0, -2.0));
    assert_eq!(c.frame_motion(0x2000, 1_066_671), (4.0, -2.0)); // a camera joining this blend
    assert_eq!(c.frame_motion(0x2000, 1_066_672), (0.0, 0.0));
}

#[test]
fn follow_camera_keeps_one_state_per_camera_only_when_raw() {
    let mut c = camera();
    let mut dir = [0.0, 0.0, -1.0f32];
    let mut arc = 0.0f32;
    let h0 = heading_of(&dir);
    // mouse_raw off: the old single state (slot 0), the turn is the plain mouse turn.
    c.dx[0] = 100.0;
    c.follow_camera(
        1_000_000,
        0x9000,
        &mut dir,
        Some(&mut arc),
        &U2_ARC,
        false,
        0.75,
    );
    assert!((wrap_degrees(h0 - heading_of(&dir)) - 15.0).abs() < 0.05);
    assert!(c
        .follow_states
        .iter()
        .all(|s| s.camera == 0 && s.shown_us == 0));
    // The raw path keeps a state of its own per camera.
    c.follow_camera(
        1_100_000,
        0x9000,
        &mut dir,
        Some(&mut arc),
        &U2_ARC,
        true,
        0.75,
    );
    assert_eq!(
        c.follow_states
            .iter()
            .filter(|s| s.camera == 0x9000)
            .count(),
        1
    );
    // Slot reuse: the least recently used slot goes first.
    let mut c = camera();
    let first = c.follow_state(10, 100);
    let second = c.follow_state(11, 200);
    c.follow_state(12, 300);
    c.follow_state(13, 400);
    assert!(first != second && c.follow_state(10, 500) == first);
    let fifth = c.follow_state(14, 600);
    assert!(fifth == second && c.follow_states[fifth].camera == 14);
    assert_eq!(c.follow_state(10, 700), first);
}

#[test]
fn mouse_sticks_ask_the_game_for_the_turn_the_mouse_made() {
    let mut c = camera();
    let mut pad = [0.0f32; 8];
    c.dx[1] = 20.0;
    c.mouse_sticks(1_000_000, &mut pad, &U1_100_AIM);
    assert!(pad[2] > 0.0 && pad[2] == pad[6] && pad[3] == 0.0);
    assert_eq!(c.sticks_hook_us, 1_000_000);
    assert_eq!(mouse_axis(0.0), 128);
    assert!(mouse_axis(100.0) > 128 && mouse_axis(-100.0) < 128);
    assert!(stick_for_rate(&U1_102_AIM.h, -45.0) < 0.0);
    assert!((U2_ARC.pitch_to_arc(U2_ARC.arc_to_pitch(0.7)) - 0.7).abs() < 1e-4);
}

#[cfg(target_arch = "x86_64")]
#[test]
fn guest_callback_table_and_exports_are_complete() {
    let table = super::hooks::table();
    assert_eq!(table.len(), 36);
    let mut hooks = 0;
    for entry in table {
        let (name, pattern, skip, moved, callback, patch) = entry.parts();
        assert!(callback != patch, "{name}: a callback or a patch");
        hooks += usize::from(callback);
        let bytes: Vec<&str> = pattern.split(' ').collect();
        assert!(
            bytes
                .iter()
                .all(|b| *b == "??" || (b.len() == 2 && u8::from_str_radix(b, 16).is_ok())),
            "{name}"
        );
        assert!(skip + moved <= bytes.len(), "{name}");
    }
    assert_eq!(hooks, 29);
    let mut count = 0usize;
    // SAFETY: a writable count.
    let pointer = unsafe { super::hooks::runtime_guest_callbacks(&mut count) };
    assert!(!pointer.is_null() && count == 36);
    for symbol in [
        "scePadInit",
        "scePadOpen",
        "scePadClose",
        "scePadReadState",
        "scePadRead",
        "scePadGetControllerInformation",
        "scePadSetVibration",
        "scePadResetOrientation",
        "scePadSetAngularVelocityDeadbandState",
        "scePadSetTiltCorrectionState",
        "scePadSetMotionSensorState",
        "scePadSetLightBar",
        "scePadResetLightBar",
    ] {
        assert!(super::guest::export(symbol).is_some(), "{symbol}");
    }
    assert!(super::guest::export("scePadNothing").is_none());
    // Exercise the guest import path: a private export alone cannot enable the sensors.
    assert_eq!(
        super::resolve(c"clVvL4ZDntw#A#B"),
        super::guest::export("scePadSetMotionSensorState")
    );
}
