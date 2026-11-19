//! The bindings of `input.ini` (`BB_INPUT_CONFIG`, written by the launcher):
//!
//! - `key.<action>=<key or mouse name>[|<second>]`: SDL key names ("Space", "Left Shift", "W"),
//!   "Mouse Left", "Mouse Right", "Mouse Middle", "Mouse 4", "Mouse 5", "Wheel Up",
//!   "Wheel Down"; actions: cross circle square triangle l1 r1 l2 r2 l3 r3 options touchpad up
//!   down left right, ls_up ls_down ls_left ls_right, rs_up rs_down rs_left rs_right.
//! - `pad.<action>=<controller button>`: SDL names (a b x y leftshoulder rightshoulder
//!   lefttrigger righttrigger leftstick rightstick back start dpup dpdown dpleft dpright
//!   touchpad misc1 paddle1..paddle4) or "none", for the button actions above.
//! - `mouse_look=1 mouse_sensitivity=1.0 mouse_invert_y=0 pad_invert_y=0 mouse_raw=0
//!   mouse_aim_sensitivity=1.0 pad_vibration=1 pad_vibration_strength=100`.
//!
//! Defaults (no file): WASD move, mouse look, left button R2 (fire), right button L2 (aim),
//! Space Cross, C Circle, F Square, E Triangle, Q L1, G R1, Left Shift L3, V R3, Escape/Enter
//! Options, Tab touchpad, arrow keys and the wheel the d-pad, IJKL the right stick.

use super::camera::CameraInput;
use super::host::Host;
use super::note;

/// PlayStation button bits (the C `BTN_*` enum).
pub mod button {
    pub const L3: u32 = 0x2;
    pub const R3: u32 = 0x4;
    pub const OPTIONS: u32 = 0x8;
    pub const UP: u32 = 0x10;
    pub const RIGHT: u32 = 0x20;
    pub const DOWN: u32 = 0x40;
    pub const LEFT: u32 = 0x80;
    pub const L2: u32 = 0x100;
    pub const R2: u32 = 0x200;
    pub const L1: u32 = 0x400;
    pub const R1: u32 = 0x800;
    pub const TRIANGLE: u32 = 0x1000;
    pub const CIRCLE: u32 = 0x2000;
    pub const CROSS: u32 = 0x4000;
    pub const SQUARE: u32 = 0x8000;
    pub const TOUCHPAD: u32 = 0x100000;
}
use button::*;

/// `BIND_KEY`: a physical key (SDL scancode, US position). `BIND_LAYOUT`: a character key the
/// player chose, found in the current keyboard layout (SDL keycode), so "Z" is the key
/// labelled Z on an AZERTY keyboard too. The default letters stay positions: WASD is ZQSD on AZERTY.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Bind {
    #[default]
    None,
    Key(i32),
    Mouse(u32),
    /// 1 wheel up, -1 wheel down.
    Wheel(i32),
    Layout(i32),
}

pub struct Action {
    pub name: &'static str,
    pub ps: u32,
    pub keys: &'static str,
    pub pad: Option<&'static str>,
}

pub const A_LS_UP: usize = 16;
pub const A_LS_DOWN: usize = 17;
pub const A_LS_LEFT: usize = 18;
pub const A_LS_RIGHT: usize = 19;
pub const A_RS_UP: usize = 20;
pub const A_RS_DOWN: usize = 21;
pub const A_RS_LEFT: usize = 22;
pub const A_RS_RIGHT: usize = 23;
pub const ACTIONS: usize = 24;

const fn action(
    name: &'static str,
    ps: u32,
    keys: &'static str,
    pad: Option<&'static str>,
) -> Action {
    Action {
        name,
        ps,
        keys,
        pad,
    }
}

pub static ACTION_TABLE: [Action; ACTIONS] = [
    action("cross", CROSS, "Space", Some("a")),
    action("circle", CIRCLE, "C", Some("b")),
    action("square", SQUARE, "F", Some("x")),
    action("triangle", TRIANGLE, "E", Some("y")),
    action("l1", L1, "Q", Some("leftshoulder")),
    action("r1", R1, "G", Some("rightshoulder")),
    action("l2", L2, "Mouse Right", Some("lefttrigger")),
    action("r2", R2, "Mouse Left", Some("righttrigger")),
    action("l3", L3, "Left Shift", Some("leftstick")),
    action("r3", R3, "V", Some("rightstick")),
    action("options", OPTIONS, "Escape|Return", Some("start")),
    action("touchpad", TOUCHPAD, "Tab", Some("back")),
    action("up", UP, "Up", Some("dpup")),
    action("down", DOWN, "Down", Some("dpdown")),
    action("left", LEFT, "Left|Wheel Up", Some("dpleft")),
    action("right", RIGHT, "Right|Wheel Down", Some("dpright")),
    action("ls_up", 0, "W", None),
    action("ls_down", 0, "S", None),
    action("ls_left", 0, "A", None),
    action("ls_right", 0, "D", None),
    action("rs_up", 0, "I", None),
    action("rs_down", 0, "K", None),
    action("rs_left", 0, "J", None),
    action("rs_right", 0, "L", None),
];

pub const PAD_TRIGGER_LEFT: i32 = -2;
pub const PAD_TRIGGER_RIGHT: i32 = -3;

#[derive(Clone, Debug)]
pub struct Input {
    pub keys: [[Bind; 2]; ACTIONS],
    /// A gamepad button, `PAD_TRIGGER_*`, or -1.
    pub pad: [i32; 16],
    pub mouse_look: bool,
    pub pad_invert_y: bool,
    pub pad_vibration: bool,
    pub pad_vibration_strength: i32,
    pub camera: CameraInput,
}

/// The environment the module reads, collected once (tests build it by hand).
#[derive(Clone, Debug)]
pub struct Settings {
    /// `BB_INPUT_CONFIG`.
    pub input_config: Option<String>,
    /// `BB_MOUSE_RAW`, `BB_MOUSE_AIM_SENSITIVITY`.
    pub mouse_raw: Option<String>,
    pub mouse_aim: Option<String>,
    /// `BB_PAD_FILE`, and `BB_PAD_HOST=1`.
    pub pad_file: Option<String>,
    pub pad_host: bool,
    /// `BB_KEY_MIRROR_MS` (default 60).
    pub mirror_ms: i32,
    /// `BB_DECK_PROMPTS=1`.
    pub deck_prompts: bool,
    /// `BB_PAD_RECORD`, `BB_PAD_REPLAY`.
    pub record: Option<String>,
    pub replay: Option<String>,
}

impl Settings {
    pub fn from_env() -> Settings {
        let var = |name: &str| std::env::var(name).ok();
        Settings {
            input_config: var("BB_INPUT_CONFIG"),
            mouse_raw: var("BB_MOUSE_RAW"),
            mouse_aim: var("BB_MOUSE_AIM_SENSITIVITY"),
            pad_file: var("BB_PAD_FILE"),
            pad_host: var("BB_PAD_HOST").is_some_and(|v| v.starts_with('1')),
            mirror_ms: var("BB_KEY_MIRROR_MS")
                .filter(|v| !v.is_empty())
                .map_or(60, |v| atoi(&v)),
            deck_prompts: var("BB_DECK_PROMPTS").is_some_and(|v| v.starts_with('1')),
            record: var("BB_PAD_RECORD"),
            replay: var("BB_PAD_REPLAY"),
        }
    }
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            input_config: None,
            mouse_raw: None,
            mouse_aim: None,
            pad_file: None,
            pad_host: false,
            mirror_ms: 60,
            deck_prompts: false,
            record: None,
            replay: None,
        }
    }
}

/// C `atoi`: leading blanks, a sign, digits; 0 when there are none.
pub fn atoi(text: &str) -> i32 {
    let text = text.trim_start();
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let mut value: i64 = 0;
    for byte in digits.bytes().take_while(u8::is_ascii_digit) {
        value = (value * 10 + i64::from(byte - b'0')).min(i64::from(i32::MAX) + 1);
    }
    (if negative { -value } else { value }).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// C `atof`: the longest number at the start of the text; 0 when there is none.
pub fn atof(text: &str) -> f64 {
    let text = text.trim_start();
    let bytes = text.as_bytes();
    let mut end = 0;
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        end = 1;
    }
    let whole = digits(end);
    end += whole;
    let mut fraction = 0;
    if bytes.get(end) == Some(&b'.') {
        fraction = digits(end + 1);
        end += 1 + fraction;
    }
    if whole + fraction == 0 {
        return 0.0;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let exponent = digits(end + 1 + sign);
        if exponent > 0 {
            end += 1 + sign + exponent;
        }
    }
    text[..end].parse().unwrap_or(0.0)
}

/// Whether `name` is one of the keys in `defaults` ("Escape|Return").
fn default_key(name: &str, defaults: &str) -> bool {
    defaults
        .split('|')
        .any(|key| key.eq_ignore_ascii_case(name))
}

fn parse_bind(host: &dyn Host, name: &str, defaults: &str) -> Bind {
    const MOUSE: [(&str, Bind); 7] = [
        ("Mouse Left", Bind::Mouse(1)),
        ("Mouse Right", Bind::Mouse(2)),
        ("Mouse Middle", Bind::Mouse(4)),
        ("Mouse 4", Bind::Mouse(8)),
        ("Mouse 5", Bind::Mouse(16)),
        ("Wheel Up", Bind::Wheel(1)),
        ("Wheel Down", Bind::Wheel(-1)),
    ];
    if let Some((_, bind)) = MOUSE.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return *bind;
    }
    let single = name.len() == 1 && name.as_bytes()[0] > b' ' && name.as_bytes()[0] < 127;
    if single && !default_key(name, defaults) {
        if let Some(key) = host.keycode_from_name(name) {
            return Bind::Layout(key);
        }
    }
    host.scancode_from_name(name).map_or(Bind::None, Bind::Key)
}

fn set_keys(host: &dyn Host, keys: &mut [Bind; 2], action: usize, value: &str) {
    let mut end = value.len().min(127);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let text = &value[..end];
    *keys = [Bind::None; 2];
    let (first, second) = match text.split_once('|') {
        Some((first, second)) => (first, Some(second)),
        None => (text, None),
    };
    let defaults = ACTION_TABLE[action].keys;
    if !first.is_empty() {
        keys[0] = parse_bind(host, first, defaults);
    }
    if let Some(second) = second.filter(|s| !s.is_empty()) {
        keys[1] = parse_bind(host, second, defaults);
    }
}

fn parse_pad(host: &dyn Host, value: &str) -> i32 {
    if value.eq_ignore_ascii_case("lefttrigger") {
        PAD_TRIGGER_LEFT
    } else if value.eq_ignore_ascii_case("righttrigger") {
        PAD_TRIGGER_RIGHT
    } else {
        host.button_from_name(value).unwrap_or(-1)
    }
}

impl Input {
    /// Before the first read: no bindings yet, the settings' defaults.
    pub fn unloaded() -> Input {
        Input {
            keys: [[Bind::None; 2]; ACTIONS],
            pad: [-1; 16],
            mouse_look: true,
            pad_invert_y: false,
            pad_vibration: true,
            pad_vibration_strength: 100,
            camera: CameraInput::default(),
        }
    }

    /// The defaults, then `input.ini`, then the environment the Slint app passes its settings in.
    pub fn load(host: &mut dyn Host, settings: &Settings) -> Input {
        let mut input = Input::unloaded();
        for (a, action) in ACTION_TABLE.iter().enumerate() {
            set_keys(host, &mut input.keys[a], a, action.keys);
            if let (true, Some(pad)) = (a < 16, action.pad) {
                input.pad[a] = parse_pad(host, pad);
            }
        }
        if let Some(path) = settings.input_config.as_deref().filter(|p| !p.is_empty()) {
            if let Ok(text) = std::fs::read_to_string(path) {
                for line in text.lines() {
                    let Some((key, value)) = line.split_once('=') else {
                        continue;
                    };
                    if line.starts_with('#') {
                        continue;
                    }
                    input.apply(host, key, value);
                }
                note(&format!(
                    "Runtime: controls from {path} (mouse look {}, sensitivity {:.2}, raw mouse {})",
                    if input.mouse_look { "on" } else { "off" },
                    input.camera.mouse_sensitivity,
                    if input.camera.mouse_raw { "on" } else { "off" }
                ));
            }
        }
        // After the file, so the Slint app can pass them through its settings (extra environment).
        if let Some(raw) = settings.mouse_raw.as_deref().filter(|v| !v.is_empty()) {
            input.camera.mouse_raw = atoi(raw) != 0;
        }
        if let Some(aim) = settings.mouse_aim.as_deref().filter(|v| !v.is_empty()) {
            input.camera.mouse_aim_sensitivity = atof(aim) as f32;
        }
        let camera = &mut input.camera;
        if camera.mouse_sensitivity <= 0.0 {
            camera.mouse_sensitivity = 1.0;
        }
        if camera.mouse_aim_sensitivity <= 0.0 {
            camera.mouse_aim_sensitivity = 1.0;
        }
        camera.mouse_aim_sensitivity = camera.mouse_aim_sensitivity.min(3.0);
        input.pad_vibration_strength = input.pad_vibration_strength.clamp(0, 100);
        input
    }

    fn apply(&mut self, host: &dyn Host, key: &str, value: &str) {
        let flag = || atoi(value) != 0;
        match key {
            "mouse_look" => self.mouse_look = flag(),
            "mouse_sensitivity" => self.camera.mouse_sensitivity = atof(value) as f32,
            "mouse_invert_y" => self.camera.mouse_invert_y = flag(),
            "mouse_raw" => self.camera.mouse_raw = flag(),
            "mouse_aim_sensitivity" => self.camera.mouse_aim_sensitivity = atof(value) as f32,
            "pad_invert_y" => self.pad_invert_y = flag(),
            "pad_vibration" => self.pad_vibration = flag(),
            "pad_vibration_strength" => self.pad_vibration_strength = atoi(value),
            _ => {
                for (a, action) in ACTION_TABLE.iter().enumerate() {
                    if key.strip_prefix("key.") == Some(action.name) {
                        set_keys(host, &mut self.keys[a], a, value);
                    } else if a < 16 && key.strip_prefix("pad.") == Some(action.name) {
                        self.pad[a] = parse_pad(host, value);
                    }
                }
            }
        }
    }
}
