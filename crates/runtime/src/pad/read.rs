//! One pad read of `src/runtime_pad.c`: the gamepad in use, the keyboard and mouse, the
//! mirrored-key filter, the scripted input (`BB_PAD_FILE`, record and replay) and the
//! `BB_BENCHMARK` path that replaces all of it.

use super::camera::{mouse_axis, Camera, MOUSE_DEGREES_PER_PIXEL};
use super::host::{
    axis, Host, MouseSample, PadId, BUTTON_COUNT, BUTTON_SOUTH, BUTTON_TOUCHPAD, SCANCODE_COUNT,
    SCANCODE_F9,
};
use super::input::*;
use super::{note, now_us, PadData, PadTouch};
use std::io::Write as _;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

const MAX_GAMEPADS: usize = 8;

fn axis_byte(v: i16) -> u8 {
    ((i32::from(v) + 32768) >> 8).clamp(0, 255) as u8
}

fn trigger_byte(v: i16) -> u8 {
    (i32::from(v) >> 7).clamp(0, 255) as u8
}

fn touch_axis(v: f32, max: i32) -> u16 {
    (if v <= 0.0 {
        0
    } else if v >= 1.0 {
        max
    } else {
        (v * max as f32 + 0.5) as i32
    }) as u16
}

fn touch_click(d: &mut PadData, right: bool) {
    d.buttons |= button::TOUCHPAD;
    d.touch_count = 1;
    d.touches[0] = PadTouch {
        x: if right { 1440 } else { 480 },
        y: 471,
        id: 0,
        reserve: [0; 3],
    };
}

/// Keys, clicks and wheel steps that a desktop layout makes from the controller, and the
/// gamepad changes that tell them apart. See [`Pad::filter_mirrored_input`].
struct Mirror {
    pad_prev: u64,
    pad_changes: [u64; 8],
    pad_change_next: usize,
    held: Vec<bool>,
    muted: Vec<bool>,
    out: Vec<bool>,
    down_at: Vec<u64>,
    wheel_at: [u64; 2],
}

const MIRROR_SOURCES: usize = SCANCODE_COUNT + 8;

impl Mirror {
    fn new() -> Mirror {
        Mirror {
            pad_prev: 0,
            pad_changes: [0; 8],
            pad_change_next: 0,
            held: vec![false; MIRROR_SOURCES],
            muted: vec![false; MIRROR_SOURCES],
            out: vec![false; SCANCODE_COUNT],
            down_at: vec![0; MIRROR_SOURCES],
            wheel_at: [0; 2],
        }
    }

    fn near_change(&self, at: u64, window: u64) -> bool {
        self.pad_changes
            .iter()
            .any(|&t| t != 0 && t + window >= at && t <= at + window)
    }
}

/// The scripted input of `BB_PAD_FILE`.
struct Injected {
    buttons: u32,
    stick: [i32; 4],
    touch_side: i32,
    mouse: [f32; 2],
}

#[derive(Clone, Copy, PartialEq)]
struct PadSample {
    ms: u32,
    buttons: u32,
    axes: [u8; 4],
    l2: u8,
    r2: u8,
}

/// The state of the module under the pad lock.
pub struct Pad {
    pub host: Box<dyn Host>,
    pub settings: Settings,
    pub camera: Arc<Mutex<Camera>>,
    pub input: Input,
    loaded: bool,
    /// 0 not tried, 1 up, -1 failed.
    ready: i8,
    pub pads: Vec<PadId>,
    pub current: Option<PadId>,
    pub listed_us: u64,
    pub connected_count: u8,
    pub reads: usize,
    mouse_vx: f32,
    mouse_vy: f32,
    mouse_last_us: u64,
    wheel_until_us: [u64; 2],
    mirror: Mirror,
    layout_logged: bool,
    prompts_device: i32,
    deck_touched: bool,
    motion: bool,
    sensors_on: Option<PadId>,
    injected: Injected,
    inject_last_check: u64,
    inject_mtime: Option<(SystemTime, u64)>,
    replay_armed: u8,
    replay_start: u64,
    replay: Vec<PadSample>,
    replay_next: usize,
    replay_loaded: bool,
    record_file: Option<std::fs::File>,
    record_start: u64,
    record_last: Option<PadSample>,
    f9_was_down: bool,
    mouse_inject_us: u64,
    hold_after_capture: bool,
    pub opened: bool,
    pub initialized: bool,
}

impl Pad {
    pub fn new(host: Box<dyn Host>, settings: Settings, camera: Arc<Mutex<Camera>>) -> Pad {
        Pad {
            host,
            settings,
            camera,
            input: Input::unloaded(),
            loaded: false,
            ready: 0,
            pads: Vec::new(),
            current: None,
            listed_us: 0,
            connected_count: 0,
            reads: 0,
            mouse_vx: 0.0,
            mouse_vy: 0.0,
            mouse_last_us: 0,
            wheel_until_us: [0; 2],
            mirror: Mirror::new(),
            layout_logged: false,
            prompts_device: -1,
            deck_touched: false,
            motion: false,
            sensors_on: None,
            injected: Injected {
                buttons: 0,
                stick: [-1; 4],
                touch_side: -1,
                mouse: [0.0; 2],
            },
            inject_last_check: 0,
            inject_mtime: None,
            replay_armed: 0,
            replay_start: 0,
            replay: Vec::new(),
            replay_next: 0,
            replay_loaded: false,
            record_file: None,
            record_start: 0,
            record_last: None,
            f9_was_down: false,
            mouse_inject_us: 0,
            hold_after_capture: false,
            opened: false,
            initialized: false,
        }
    }

    fn camera(&self) -> std::sync::MutexGuard<'_, Camera> {
        self.camera
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The gamepad subsystem was started by the caller (`--controller-watch`).
    pub fn mark_ready(&mut self) {
        self.ready = 1;
    }

    /// Reads `input.ini` on the first read and gives the camera its part.
    pub fn load_input(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.input = Input::load(self.host.as_mut(), &self.settings);
        self.camera().input = self.input.camera;
        self.host.mouse_configure(self.input.mouse_look);
    }

    /// Every connected gamepad stays open and the game reads the one in use: the last that had
    /// a button pressed or a stick or trigger pushed past half way, else the first. Only the
    /// first was read before, so the controller in the player's hands was ignored whenever
    /// another one came first in SDL's list (a Steam Deck's built-in controller next to a pad,
    /// or a handheld's hidden controller next to the virtual one of Bazzite's InputPlumber;
    /// issues #60, #61).
    fn list_gamepads(&mut self) {
        let mut i = 0;
        while i < self.pads.len() {
            let id = self.pads[i];
            if self.host.connected(id) {
                i += 1;
                continue;
            }
            if self.current == Some(id) {
                self.current = None;
            }
            self.host.close(id);
            self.pads.swap_remove(i);
        }
        for id in self.host.pads() {
            if self.pads.len() >= MAX_GAMEPADS {
                break;
            }
            if self.pads.contains(&id) || !self.host.open(id) {
                continue;
            }
            self.pads.push(id);
            self.connected_count = self.connected_count.wrapping_add(1);
            note(&format!(
                "Runtime: gamepad connected: {}",
                self.host.name(id)
            ));
        }
    }

    fn gamepad_active(&self, id: PadId) -> bool {
        (0..BUTTON_COUNT).any(|b| self.host.button(id, b))
            || (0..axis::COUNT).any(|a| {
                let v = i32::from(self.host.axis(id, a));
                !(-16384..=16384).contains(&v)
            })
    }

    /// The pad the game reads (`current_gamepad`). Starts the gamepad subsystem on first use.
    pub fn current_gamepad(&mut self) -> Option<PadId> {
        if self.ready == 0 {
            self.host.hints();
            self.ready = if self.host.pads_ready() || self.host.init_pads().is_ok() {
                1
            } else {
                -1
            };
        }
        if self.ready < 0 {
            return None;
        }
        self.pick_gamepad(now_us())
    }

    pub fn pick_gamepad(&mut self, now: u64) -> Option<PadId> {
        if self.current.is_none_or(|id| !self.host.connected(id))
            || now.wrapping_sub(self.listed_us) > 500_000
        {
            self.listed_us = now;
            self.list_gamepads();
        }
        if self.current.is_none() {
            self.current = self.pads.first().copied();
        }
        if let (true, Some(current)) = (self.pads.len() > 1, self.current) {
            if !self.gamepad_active(current) {
                if let Some(&other) = self
                    .pads
                    .iter()
                    .find(|&&id| id != current && self.gamepad_active(id))
                {
                    self.current = Some(other);
                    note(&format!(
                        "Runtime: gamepad in use: {}",
                        self.host.name(other)
                    ));
                }
            }
        }
        self.current
    }

    /// Where the layout put each character key the player chose, once the keyboard is up.
    fn log_layout_keys(&self) {
        for (a, action) in ACTION_TABLE.iter().enumerate() {
            for bind in &self.input.keys[a] {
                if let Bind::Layout(key) = *bind {
                    let code = self.host.scancode_from_keycode(key);
                    note(&format!(
                        "Runtime: key.{} {}: the key labelled so in this keyboard layout (US position {})",
                        action.name,
                        self.host.key_name(key),
                        code.map_or_else(|| "none".to_string(), |c| self.host.scancode_name(c))
                    ));
                }
            }
        }
    }

    fn bind_down(&self, bind: &Bind, keys: Option<&[bool]>, mouse: u32, now: u64) -> bool {
        match *bind {
            Bind::None => false,
            Bind::Key(code) => keys.is_some_and(|k| k.get(code as usize).copied().unwrap_or(false)),
            Bind::Layout(key) => keys.is_some_and(|k| {
                self.host
                    .scancode_from_keycode(key)
                    .is_some_and(|code| k.get(code as usize).copied().unwrap_or(false))
            }),
            Bind::Mouse(mask) => mouse & mask != 0,
            Bind::Wheel(step) => now < self.wheel_until_us[usize::from(step <= 0)],
        }
    }

    fn action_down(&self, a: usize, keys: Option<&[bool]>, mouse: u32, now: u64) -> bool {
        self.input.keys[a]
            .iter()
            .any(|bind| self.bind_down(bind, keys, mouse, now))
    }

    /// A desktop that turns controller buttons into keys and clicks while the game reads the
    /// controller itself (Steam's desktop layout on a Steam Deck in Desktop Mode: B is Escape,
    /// Y is Enter, the triggers are mouse clicks) gave every press twice: B for cover also
    /// opened the game's menu (issue #66). Such a copy goes down within a few milliseconds of
    /// the real button, so while a gamepad is open a key or mouse button counts only once it
    /// has been held for `BB_KEY_MIRROR_MS` (default 60) with no gamepad button changing around
    /// its press; a mirrored one stays ignored until released. The sticks count as gamepad
    /// changes too (each axis in steps of a quarter past a dead zone), and a wheel step is held
    /// back as long and dropped the same way: a layout that turns the stick into arrow keys or
    /// the trackpad into the wheel gave the d-pad (the weapon selection) while Drake left cover
    /// (issue #75). `BB_KEY_MIRROR_MS=0`: off.
    ///
    /// `keys` is replaced by the filtered keys when there is a keyboard.
    pub fn filter_mirrored_input(
        &mut self,
        pad: Option<PadId>,
        now: u64,
        keys: &mut Option<Vec<bool>>,
        mouse: &mut u32,
        wheel: &mut i32,
    ) {
        let window = u64::from(self.settings.mirror_ms.max(0) as u32) * 1000;
        let Some(pad) = pad.filter(|_| window != 0) else {
            return;
        };
        // When the gamepad's buttons, triggers or sticks last changed (a few times, for presses
        // that arrive just after).
        let mut state = 0u64;
        for b in 0..BUTTON_COUNT.min(32) {
            if self.host.button(pad, b) {
                state |= 1 << b;
            }
        }
        if self.host.axis(pad, axis::LEFT_TRIGGER) > 8000 {
            state |= 1 << 62;
        }
        if self.host.axis(pad, axis::RIGHT_TRIGGER) > 8000 {
            state |= 1 << 63;
        }
        for (a, stick) in [axis::LEFT_X, axis::LEFT_Y, axis::RIGHT_X, axis::RIGHT_Y]
            .into_iter()
            .enumerate()
        {
            let v = i32::from(self.host.axis(pad, stick));
            let size = v.unsigned_abs() as u64;
            let step = if size < 10000 {
                0
            } else {
                1 + (size - 10000) / 6000
            }; // 0..4
            state |= (step | if v < 0 { 8 } else { 0 }) << (32 + 4 * a);
        }
        let m = &mut self.mirror;
        if state != m.pad_prev {
            m.pad_prev = state;
            m.pad_changes[m.pad_change_next % 8] = now;
            m.pad_change_next += 1;
        }
        let mouse_in = *mouse;
        let mut mouse_out = 0u32;
        for i in 0..MIRROR_SOURCES {
            let down = if i < SCANCODE_COUNT {
                keys.as_ref()
                    .is_some_and(|k| k.get(i).copied().unwrap_or(false))
            } else {
                (mouse_in >> (i - SCANCODE_COUNT)) & 1 != 0
            };
            let mut pass = false;
            if !down {
                m.held[i] = false;
                m.muted[i] = false;
            } else {
                if !m.held[i] {
                    m.held[i] = true;
                    m.muted[i] = false;
                    m.down_at[i] = now;
                }
                if !m.muted[i] && m.near_change(m.down_at[i], window) {
                    m.muted[i] = true;
                }
                pass = !m.muted[i] && now.wrapping_sub(m.down_at[i]) >= window;
            }
            if i < SCANCODE_COUNT {
                m.out[i] = pass;
            } else if pass {
                mouse_out |= 1 << (i - SCANCODE_COUNT);
            }
        }
        if keys.is_some() {
            *keys = Some(m.out.clone());
        }
        *mouse = mouse_out;
        // Wheel steps: up, down. One waits BB_KEY_MIRROR_MS; a gamepad change around it drops it.
        let w = *wheel;
        *wheel = 0;
        for d in 0..2 {
            if (if d == 1 { w < 0 } else { w > 0 }) && m.wheel_at[d] == 0 {
                m.wheel_at[d] = now;
            }
            if m.wheel_at[d] == 0 {
                continue;
            }
            if m.near_change(m.wheel_at[d], window) {
                m.wheel_at[d] = 0;
            } else if now.wrapping_sub(m.wheel_at[d]) >= window {
                m.wheel_at[d] = 0;
                *wheel = if d == 1 { -1 } else { 1 };
            }
        }
    }

    /// The host's side of one read: controller, keyboard and mouse together.
    fn sample_host(&mut self, d: &mut PadData) {
        *d = PadData::neutral();
        d.connected = 1;
        d.connected_count = self.connected_count.max(1);
        d.timestamp = now_us();
        self.load_input();
        let g = self.current_gamepad();
        let MouseSample {
            captured,
            dx: mdx,
            dy: mdy,
            buttons: mut mouse,
            mut wheel,
        } = self.host.mouse_sample();
        if self.host.overlay_captures_input() {
            return; // settings menu open: neutral input
        }
        // Scripted runs (BB_PAD_FILE) ignore the player's controller and keyboard: a pad in
        // use on the same PC reached the test window (BB_PAD_HOST=1 keeps them).
        let scripted = self
            .settings
            .pad_file
            .as_deref()
            .is_some_and(|f| !f.is_empty())
            && !self.settings.pad_host;
        if scripted {
            return;
        }
        let now = d.timestamp;
        let mut keys = self.host.keyboard();
        // Steam's desktop layout (issues #66, #75).
        self.filter_mirrored_input(g, now, &mut keys, &mut mouse, &mut wheel);
        if wheel > 0 {
            self.wheel_until_us[0] = now + 80_000;
        }
        if wheel < 0 {
            self.wheel_until_us[1] = now + 80_000;
        }
        if keys.is_some() && !self.layout_logged {
            self.layout_logged = true;
            self.log_layout_keys();
        }
        let k = keys.as_deref();
        if let Some(g) = g {
            for a in 0..16 {
                let b = self.input.pad[a];
                let ps = ACTION_TABLE[a].ps;
                if b >= 0 && self.host.button(g, b) {
                    d.buttons |= ps;
                } else if b == PAD_TRIGGER_LEFT || b == PAD_TRIGGER_RIGHT {
                    let v = trigger_byte(self.host.axis(
                        g,
                        if b == PAD_TRIGGER_LEFT {
                            axis::LEFT_TRIGGER
                        } else {
                            axis::RIGHT_TRIGGER
                        },
                    ));
                    if v > 30 {
                        d.buttons |= ps;
                    }
                    if ps == button::L2 && v > d.l2 {
                        d.l2 = v;
                    }
                    if ps == button::R2 && v > d.r2 {
                        d.r2 = v;
                    }
                }
            }
            if self.host.button(g, BUTTON_TOUCHPAD) {
                d.buttons |= button::TOUCHPAD; // DualSense/DS4 click
            }
            if d.buttons & button::L2 != 0 && d.l2 == 0 {
                d.l2 = 255;
            }
            if d.buttons & button::R2 != 0 && d.r2 == 0 {
                d.r2 = 255;
            }
            d.left_x = axis_byte(self.host.axis(g, axis::LEFT_X));
            d.left_y = axis_byte(self.host.axis(g, axis::LEFT_Y));
            d.right_x = axis_byte(self.host.axis(g, axis::RIGHT_X));
            d.right_y = axis_byte(self.host.axis(g, axis::RIGHT_Y));
            if self.input.pad_invert_y {
                d.right_y = 255 - d.right_y;
            }
            for (finger, f) in self.host.fingers(g).into_iter().enumerate() {
                if d.touch_count >= 2 {
                    break;
                }
                if f.down {
                    d.touches[usize::from(d.touch_count)] = PadTouch {
                        x: touch_axis(f.x, 1919),
                        y: touch_axis(f.y, 942),
                        id: finger as u8,
                        reserve: [0; 3],
                    };
                    d.touch_count += 1;
                }
            }
            if self.motion {
                self.read_motion(g, d);
            }
        }
        // The device used last picks the button prompts: the game's PlayStation buttons or
        // pictures of the bound keys. A stick counts past half way, the mouse when it moves
        // more than a few pixels.
        let far = |v: u8| (i32::from(v) - 128).abs() > 64;
        let pad_used = g.is_some()
            && (d.buttons != 0
                || d.l2 > 60
                || d.r2 > 60
                || far(d.left_x)
                || far(d.left_y)
                || far(d.right_x)
                || far(d.right_y));
        let mut keys_used = mouse != 0 || wheel != 0 || (captured && mdx.abs() + mdy.abs() > 3.0);
        for a in 0..ACTIONS {
            if keys_used {
                break;
            }
            keys_used = self.action_down(a, k, mouse, now);
        }
        // Steam Deck mode (BB_DECK_PROMPTS=1) starts with the active controller's pictures (the
        // Deck's own, or a PlayStation or other pad connected over USB or Bluetooth) instead of
        // the keys, until the player uses a controller or the keyboard and mouse.
        if self.settings.deck_prompts && !self.deck_touched {
            if pad_used || keys_used {
                self.deck_touched = true;
            } else {
                let device = g.map_or(2, |g| self.host.prompts_device(g));
                if device != self.prompts_device {
                    self.prompts_device = device;
                    self.host.prompts_controller(device);
                }
            }
        }
        if pad_used != keys_used {
            let device = match g {
                Some(g) if pad_used => self.host.prompts_device(g),
                _ => 0,
            };
            if device != self.prompts_device {
                self.prompts_device = device;
                self.host.prompts_controller(device);
            }
        }
        // Keyboard and mouse, together with the controller.
        for (a, action) in ACTION_TABLE.iter().enumerate().take(16) {
            if self.action_down(a, k, mouse, now) {
                d.buttons |= action.ps;
            }
        }
        if d.buttons & button::L2 != 0 {
            d.l2 = 255;
        }
        if d.buttons & button::R2 != 0 {
            d.r2 = 255;
        }
        // The touchpad button without a finger on a touch surface is a left-side click.
        if d.buttons & button::TOUCHPAD != 0 && d.touch_count == 0 {
            touch_click(d, false);
        }
        let dir = |this: &Pad, plus: usize, minus: usize| {
            i32::from(this.action_down(plus, k, mouse, now))
                - i32::from(this.action_down(minus, k, mouse, now))
        };
        let lx = dir(self, A_LS_RIGHT, A_LS_LEFT);
        let ly = dir(self, A_LS_DOWN, A_LS_UP);
        if lx != 0 {
            d.left_x = if lx < 0 { 0 } else { 255 };
        }
        if ly != 0 {
            d.left_y = if ly < 0 { 0 } else { 255 };
        }
        let rx = dir(self, A_RS_RIGHT, A_RS_LEFT);
        let ry = dir(self, A_RS_DOWN, A_RS_UP);
        if rx != 0 {
            d.right_x = if rx < 0 { 0 } else { 255 };
        }
        if ry != 0 {
            d.right_y = if ry < 0 { 0 } else { 255 };
        }
        self.route_mouse(d, now, captured, mdx, mdy, rx != 0, ry != 0);
    }

    /// Gyro and accelerometer, once the game turned the motion sensor on. The game's
    /// acceleration is in g, SDL's in m/s^2; both report angular velocity in rad/s.
    fn read_motion(&mut self, g: PadId, d: &mut PadData) {
        if self.sensors_on != Some(g) {
            if let Some(old) = self.sensors_on {
                self.host.sensors(old, false);
            }
            self.host.sensors(g, true);
            self.sensors_on = Some(g);
        }
        if let Some(gyro) = self.host.gyro(g) {
            d.angular_velocity = gyro;
        }
        if let Some(accel) = self.host.accel(g) {
            d.acceleration = accel.map(|v| v / 9.80665);
        }
    }

    /// `scePadSetMotionSensorState`.
    pub fn set_motion(&mut self, on: bool) {
        self.motion = on;
        if !on {
            if let Some(old) = self.sensors_on.take() {
                self.host.sensors(old, false);
            }
        }
    }

    /// Where the game's camera or joypad state takes the mouse directly (their callbacks ran in
    /// the last 100 ms), the motion goes there: the follow camera turns with the mouse
    /// (vertically too where its arc is known), with the right stick held a little the same way
    /// (`follow_push`); aiming, other cameras and a follow camera that keeps no turns get the
    /// mouse as a right stick value in the joypad state (all three games). Where a build's
    /// hooks were not found, the mouse is a right stick byte here.
    #[allow(clippy::too_many_arguments)]
    fn route_mouse(
        &mut self,
        d: &mut PadData,
        now: u64,
        captured: bool,
        mdx: f32,
        mdy: f32,
        rx: bool,
        ry: bool,
    ) {
        let (direct_x, direct_y) = {
            let mut c = self.camera();
            let recent = |at: u64| at != 0 && now.wrapping_sub(at) < 100_000;
            let following = recent(c.camera_hook_us);
            let joypad = recent(c.sticks_hook_us);
            let cover = recent(c.cover_hook_us); // raw mouse only
            let raw_aiming = c.raw_aim_armed(now); // raw mouse only
            c.sample += 1;
            for q in 0..2 {
                if captured && (following || joypad || cover) {
                    c.dx[q] += mdx;
                    c.dy[q] += mdy;
                } else {
                    c.dx[q] = 0.0;
                    c.dy[q] = 0.0;
                }
            }
            // Kept while aiming: at low frame rates the aim controller runs only every 100-200 ms.
            if captured && raw_aiming {
                c.aim_dx += mdx;
                c.aim_dy += mdy;
            } else {
                c.aim_dx = 0.0;
                c.aim_dy = 0.0;
            }
            let held = c.aim_held;
            (
                joypad || raw_aiming || (cover && !held) || (following && !held && c.follow_direct),
                joypad
                    || raw_aiming
                    || (cover && !held)
                    || (following && !held && c.follow_direct && c.follow_has_arc),
            )
        };
        if captured && direct_x && direct_y {
            self.mouse_vx = 0.0;
            self.mouse_vy = 0.0;
            self.mouse_last_us = 0;
        } else if captured {
            let mut dt = if self.mouse_last_us != 0 {
                now.wrapping_sub(self.mouse_last_us) as f32 * 1e-6
            } else {
                0.0
            };
            self.mouse_last_us = now;
            let (sensitivity, invert) = {
                let c = self.camera();
                (c.input.mouse_sensitivity, c.input.mouse_invert_y)
            };
            if dt > 0.0 {
                dt = dt.min(0.1);
                // Turn rate in degrees/s, smoothed over 12 ms (mouse reports arrive in bursts).
                let alpha = 1.0 - (-dt / 0.012).exp();
                let scale = MOUSE_DEGREES_PER_PIXEL * sensitivity;
                self.mouse_vx += alpha * (mdx / dt * scale - self.mouse_vx);
                self.mouse_vy +=
                    alpha * ((if invert { -mdy } else { mdy }) / dt * scale - self.mouse_vy);
                if mdx == 0.0 && self.mouse_vx > -1.0 && self.mouse_vx < 1.0 {
                    self.mouse_vx = 0.0;
                }
                if mdy == 0.0 && self.mouse_vy > -1.0 && self.mouse_vy < 1.0 {
                    self.mouse_vy = 0.0;
                }
            }
            if self.mouse_vx != 0.0 && !rx && !direct_x {
                d.right_x = mouse_axis(self.mouse_vx);
            }
            if self.mouse_vy != 0.0 && !ry && !direct_y {
                d.right_y = mouse_axis(self.mouse_vy);
            }
        } else {
            self.mouse_vx = 0.0;
            self.mouse_vy = 0.0;
            self.mouse_last_us = 0;
        }
    }

    /// `BB_PAD_FILE=<file>`: scripted input for automated runs. The file holds
    /// whitespace-separated tokens, re-read when it changes: button names (cross circle square
    /// triangle l1 r1 l2 r2 l3 r3 options touchpad touchpad_left touchpad_right up down left
    /// right) are held while listed; touchpad defaults to a left-side click; lx= ly= rx= ry=
    /// (0..255) override the sticks; mx= my= move the mouse (pixels/s). An empty file releases
    /// everything.
    fn read_inject(&mut self) {
        let Some(path) = self.settings.pad_file.clone().filter(|p| !p.is_empty()) else {
            return;
        };
        let now = now_us();
        if now.wrapping_sub(self.inject_last_check) < 20_000 {
            return;
        }
        self.inject_last_check = now;
        let Ok(meta) = std::fs::metadata(&path) else {
            return;
        };
        // Whole-second mtimes on some systems: the size tells two edits within a second apart.
        let stamp = (
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
        );
        if self.inject_mtime == Some(stamp) {
            return;
        }
        self.inject_mtime = Some(stamp);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        const NAMES: [(&str, u32); 16] = [
            ("cross", button::CROSS),
            ("circle", button::CIRCLE),
            ("square", button::SQUARE),
            ("triangle", button::TRIANGLE),
            ("l1", button::L1),
            ("r1", button::R1),
            ("l2", button::L2),
            ("r2", button::R2),
            ("l3", button::L3),
            ("r3", button::R3),
            ("options", button::OPTIONS),
            ("touchpad", button::TOUCHPAD),
            ("up", button::UP),
            ("down", button::DOWN),
            ("left", button::LEFT),
            ("right", button::RIGHT),
        ];
        let inj = &mut self.injected;
        *inj = Injected {
            buttons: 0,
            stick: [-1; 4],
            touch_side: -1,
            mouse: [0.0; 2],
        };
        for token in text.split_whitespace() {
            if token == "replay" && self.replay_armed != 1 {
                self.replay_armed = 1; // BB_PAD_REPLAY
                self.replay_start = 0;
            }
            if token == "touchpad_left" || token == "touchpad_right" {
                inj.buttons |= button::TOUCHPAD;
                inj.touch_side = i32::from(token == "touchpad_right");
            }
            for (name, ps) in NAMES {
                if token == name {
                    inj.buttons |= ps;
                }
            }
            for (i, prefix) in ["lx=", "ly=", "rx=", "ry="].into_iter().enumerate() {
                if let Some(value) = token.strip_prefix(prefix) {
                    inj.stick[i] = atoi(value).clamp(0, 255);
                }
            }
            if let Some(value) = token.strip_prefix("mx=") {
                inj.mouse[0] = atof(value) as f32;
            }
            if let Some(value) = token.strip_prefix("my=") {
                inj.mouse[1] = atof(value) as f32;
            }
        }
        note(&format!(
            "Runtime: pad file: buttons {:#x} sticks {} {} {} {} mouse {} {} (t={:.4})",
            inj.buttons,
            inj.stick[0],
            inj.stick[1],
            inj.stick[2],
            inj.stick[3],
            inj.mouse[0],
            inj.mouse[1],
            now as f64 * 1e-6
        ));
    }

    /// `BB_PAD_RECORD=<file>`: F9 starts and stops recording the pad state (gamepad or
    /// keyboard) with the time since F9. Lines: ms buttons lx ly rx ry l2 r2, written when the
    /// state changes.
    fn record_sample(&mut self, d: &PadData) {
        let Some(path) = self.settings.record.clone().filter(|p| !p.is_empty()) else {
            return;
        };
        if self.ready <= 0 {
            return;
        }
        let keys = self.host.keyboard();
        let f9 = keys.as_ref().is_some_and(|k| k[SCANCODE_F9]);
        if f9 && !self.f9_was_down {
            if self.record_file.take().is_some() {
                note(&format!("Runtime: pad recording stopped ({path})"));
            } else if let Ok(file) = std::fs::File::create(&path) {
                self.record_file = Some(file);
                self.record_start = now_us();
                self.record_last = None;
                note(&format!(
                    "Runtime: pad recording started ({path}, F9 stops)"
                ));
            }
        }
        self.f9_was_down = f9;
        let Some(file) = self.record_file.as_mut() else {
            return;
        };
        let s = PadSample {
            ms: (now_us().wrapping_sub(self.record_start) / 1000) as u32,
            buttons: d.buttons,
            axes: [d.left_x, d.left_y, d.right_x, d.right_y],
            l2: d.l2,
            r2: d.r2,
        };
        if let Some(last) = self.record_last {
            if (s.buttons, s.axes, s.l2, s.r2) == (last.buttons, last.axes, last.l2, last.r2) {
                return;
            }
        }
        self.record_last = Some(s);
        let _ = writeln!(
            file,
            "{} {} {} {} {} {} {} {}",
            s.ms, s.buttons, s.axes[0], s.axes[1], s.axes[2], s.axes[3], s.l2, s.r2
        );
        let _ = file.flush();
    }

    /// `BB_PAD_REPLAY=<file>` plays such a recording back, started by the token "replay" in
    /// `BB_PAD_FILE` (scripted tests repeat a route the player ran once).
    fn replay_sample(&mut self, d: &mut PadData) {
        if self.replay_armed == 0 {
            return;
        }
        if self.replay_start == 0 {
            if !self.replay_loaded {
                self.replay_loaded = true;
                let path = self.settings.replay.clone();
                let text = path
                    .as_deref()
                    .and_then(|p| std::fs::read_to_string(p).ok());
                for line in text.iter().flat_map(|t| t.lines()) {
                    let v: Vec<u32> = line
                        .split_whitespace()
                        .map_while(|t| t.parse().ok())
                        .collect();
                    if v.len() < 8 {
                        break;
                    }
                    self.replay.push(PadSample {
                        ms: v[0],
                        buttons: v[1],
                        axes: [v[2] as u8, v[3] as u8, v[4] as u8, v[5] as u8],
                        l2: v[6] as u8,
                        r2: v[7] as u8,
                    });
                }
                note(&format!(
                    "Runtime: pad replay of {} samples from {}",
                    self.replay.len(),
                    path.as_deref().unwrap_or("(unset)")
                ));
            }
            self.replay_start = now_us();
            self.replay_next = 0;
        }
        let ms = (now_us().wrapping_sub(self.replay_start) / 1000) as u32;
        while self.replay_next < self.replay.len() && self.replay[self.replay_next].ms <= ms {
            self.replay_next += 1;
        }
        if self.replay_next == 0 {
            return;
        }
        if self.replay_next == self.replay.len()
            && ms > self.replay[self.replay.len() - 1].ms.saturating_add(500)
        {
            if self.replay_armed == 1 {
                note("Runtime: pad replay finished");
                self.replay_armed = 2;
            }
            return;
        }
        let s = self.replay[self.replay_next - 1];
        d.buttons = s.buttons;
        d.left_x = s.axes[0];
        d.left_y = s.axes[1];
        d.right_x = s.axes[2];
        d.right_y = s.axes[3];
        d.l2 = s.l2;
        d.r2 = s.r2;
    }

    /// `BB_BENCHMARK`: the scripted menu and camera path are the only input
    /// (`benchmark_run.rs`). It replaces the whole read: no host pad, keyboard or mouse sample,
    /// so touches, motion and the mouse camera queues get nothing from the player.
    fn benchmark_sample(&mut self, d: &mut PadData, benchmark: crate::benchmark_run::BenchmarkPad) {
        *d = PadData::neutral();
        d.connected = 1;
        d.connected_count = self.connected_count.max(1);
        d.timestamp = now_us();
        d.buttons = benchmark.buttons;
        d.left_x = benchmark.left_x;
        d.left_y = benchmark.left_y;
        d.right_x = benchmark.right_x;
        d.right_y = benchmark.right_y;
        d.l2 = 0;
        d.r2 = 0;
        self.camera().aim_held = false;
    }

    /// One whole read (`sample`).
    pub fn sample(&mut self, d: &mut PadData) {
        if let Some(benchmark) = self.host.benchmark_pad() {
            // The bindings and camera settings still load; no host pad, keyboard or mouse is read.
            self.load_input();
            self.benchmark_sample(d, benchmark);
            return;
        }
        self.sample_host(d);
        if self.host.overlay_captures_input() {
            // After the menu or the text dialog closes, buttons still held (the Cross that
            // accepted a name) stay hidden until released: the game would take them as a new press.
            self.hold_after_capture = true;
            return;
        }
        self.record_sample(d);
        self.read_inject();
        // Scripted mouse motion (mx= my=) goes where the player's mouse would.
        let inject_now = now_us();
        if (self.injected.mouse[0] != 0.0 || self.injected.mouse[1] != 0.0)
            && self.mouse_inject_us != 0
        {
            let dt = (inject_now.wrapping_sub(self.mouse_inject_us) as f32 * 1e-6).min(0.1);
            let [mx, my] = self.injected.mouse;
            let mut c = self.camera();
            for q in 0..2 {
                c.dx[q] += mx * dt;
                c.dy[q] += my * dt;
            }
            if c.raw_aim_armed(inject_now) {
                c.aim_dx += mx * dt;
                c.aim_dy += my * dt;
            }
        }
        self.mouse_inject_us = inject_now;
        self.replay_sample(d);
        d.buttons |= self.injected.buttons;
        {
            let held = d.buttons & button::L2 != 0;
            let mut c = self.camera();
            if held && !c.aim_held {
                c.aim_press_us = now_us();
            }
            c.aim_held = held;
        }
        if self.injected.touch_side >= 0 {
            touch_click(d, self.injected.touch_side != 0);
        } else if d.buttons & button::TOUCHPAD != 0 && d.touch_count == 0 {
            touch_click(d, false);
        }
        if self.injected.buttons & button::L2 != 0 {
            d.l2 = 255;
        }
        if self.injected.buttons & button::R2 != 0 {
            d.r2 = 255;
        }
        let injected = self.injected.stick;
        for (value, slot) in
            injected
                .iter()
                .zip([&mut d.left_x, &mut d.left_y, &mut d.right_x, &mut d.right_y])
        {
            if *value >= 0 {
                *slot = *value as u8;
            }
        }
        // Builds without a joypad callback: the right stick held a little while the mouse turns
        // the follow camera's direction (follow_push), unless the stick is in use.
        let now = now_us();
        {
            let c = self.camera();
            let recent = |at: u64| at != 0 && now.wrapping_sub(at) < 100_000;
            if c.follow_push != 0.0
                && recent(c.camera_hook_us)
                && !recent(c.sticks_hook_us)
                && !c.aim_held
                && (118..=138).contains(&d.right_x)
            {
                d.right_x = (128.0 + c.follow_push * 127.5) as u8;
            }
        }
        if self.hold_after_capture {
            if d.buttons != 0 {
                d.buttons = 0;
            } else {
                self.hold_after_capture = false;
            }
        }
    }

    /// `scePadSetVibration`: the two motor strengths, scaled by `pad_vibration_strength`.
    pub fn vibrate(&mut self, small: u8, large: u8) {
        let pad = if self.input.pad_vibration {
            self.current_gamepad()
        } else {
            None
        };
        if let Some(pad) = pad {
            let strength = self.input.pad_vibration_strength as u32;
            let scale = |v: u8| ((u32::from(v) * 257 * strength + 50) / 100) as u16;
            self.host.rumble(pad, scale(small), scale(large), 1000);
        }
    }

    pub fn set_led(&mut self, r: u8, g: u8, b: u8) {
        if let Some(pad) = self.current_gamepad() {
            self.host.set_led(pad, r, g, b);
        }
    }

    /// The first gamepad is the one `--controller-info` reports.
    pub fn button_south(&self, pad: PadId) -> bool {
        self.host.button(pad, BUTTON_SOUTH)
    }
}

pub const ERR_INVALID_ARG: i32 = 0x8092_0001u32 as i32;
pub const ERR_INVALID_HANDLE: i32 = 0x8092_0003u32 as i32;
pub const ERR_ALREADY_OPENED: i32 = 0x8092_0004u32 as i32;
pub const ERR_NOT_INITIALIZED: i32 = 0x8092_0005u32 as i32;
pub const PAD_HANDLE: i32 = 1;

impl Pad {
    pub fn init(&mut self) -> i32 {
        self.initialized = true;
        0
    }

    /// `scePadOpen`: user 1, the standard or special port, index 0, once.
    pub fn open(&mut self, user: i32, kind: i32, index: i32) -> i32 {
        if !self.initialized {
            return ERR_NOT_INITIALIZED;
        }
        if user != 1 || (kind != 0 && kind != 2) || index != 0 {
            return ERR_INVALID_ARG;
        }
        if std::mem::replace(&mut self.opened, true) {
            return ERR_ALREADY_OPENED;
        }
        note("Runtime: pad opened for user 1 (SDL gamepad or keyboard)");
        PAD_HANDLE
    }

    pub fn close(&mut self, handle: i32) -> i32 {
        if self.handle_ok(handle) != 0 {
            return ERR_INVALID_HANDLE;
        }
        self.opened = false;
        0
    }

    /// 0 for the open handle, else the error.
    pub fn handle_ok(&self, handle: i32) -> i32 {
        if handle == PAD_HANDLE && self.opened {
            0
        } else {
            ERR_INVALID_HANDLE
        }
    }

    /// `scePadReadState`: one sample.
    pub fn read_state(&mut self, handle: i32) -> Result<PadData, i32> {
        match self.handle_ok(handle) {
            0 => {
                let mut data = PadData::neutral();
                self.sample(&mut data);
                self.reads += 1;
                Ok(data)
            }
            error => Err(error),
        }
    }

    pub fn info(&mut self, handle: i32) -> Result<ControllerInfo, i32> {
        match self.handle_ok(handle) {
            0 => {
                self.current_gamepad();
                Ok(ControllerInfo {
                    pixel_density: 44.86,
                    resolution_x: 1920,
                    resolution_y: 943,
                    dead_zone_left: 2,
                    dead_zone_right: 2,
                    connection_type: 0,
                    connected_count: self.connected_count.max(1),
                    connected: 1,
                    pad: [0; 3],
                    device_class: 0,
                    reserve: [0; 8],
                })
            }
            error => Err(error),
        }
    }

    pub fn report_line(&self) -> String {
        let name = self
            .current
            .map_or_else(|| "none".to_string(), |id| self.host.name(id));
        format!("Runtime: pad reads={}, gamepad={}", self.reads, name)
    }
}

use super::ControllerInfo;
