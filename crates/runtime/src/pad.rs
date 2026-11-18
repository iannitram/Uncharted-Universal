//! libScePad (piece 3.10, from the deleted `src/runtime_pad.c`): SDL3 gamepads, keyboard and
//! mouse, with the synthetic benchmark inputs of #173 beside them.
//!
//! SDL events are pumped by the window thread (`gpu/shim/window.cpp`); here state is only
//! sampled. The controller, keyboard and mouse work together; their bindings come from
//! `input.ini` ([`input`]). [`read`] is one pad read: the gamepad in use (selection, hot-plug, a
//! second pad taking over), touchpad, motion sensors, rumble, the mirrored-key filter, scripted
//! input and the `BB_BENCHMARK` path that replaces all of it. [`camera`] and [`hooks`] are the
//! mouse camera and the guest callback table. [`host`] is the SDL3 and GPU library side, a
//! trait the tests replace with a fake device.
//!
//! The synthetic schedule below is a pure schedule: the runtime integration ticks it at 60 Hz
//! and passes each sample through the game's ordinary pad callback ([`read`] asks
//! `benchmark_run`). It does not load a save or drive a game. [`EntryScript`] is the menu
//! lead-in that gets from the title screen into the level before [`BenchmarkPath`] starts
//! turning the camera; `benchmark_run` joins the two to the clock.

pub mod camera;
#[cfg(target_arch = "x86_64")]
pub mod hooks;
pub mod host;
pub mod input;
pub mod read;

#[cfg(test)]
mod device_tests;

pub use input::button;

use camera::{Camera, Knobs};
use read::Pad;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

/// One line on stdout, through the C stdio on x86-64 so it stays in order with the loader's.
pub(crate) fn note(line: &str) {
    #[cfg(target_arch = "x86_64")]
    crate::abi::host::out_line(line);
    #[cfg(not(target_arch = "x86_64"))]
    println!("{line}");
}

/// Microseconds on a monotonic clock, never 0 (0 means "not yet" in the module's timers).
pub fn now_us() -> u64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    let start = START.get_or_init(std::time::Instant::now);
    start.elapsed().as_micros() as u64 + 1_000_000
}

/// `OrbisPadTouch`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PadTouch {
    pub x: u16,
    pub y: u16,
    pub id: u8,
    pub reserve: [u8; 3],
}

/// `OrbisPadData`: 120 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PadData {
    pub buttons: u32,
    pub left_x: u8,
    pub left_y: u8,
    pub right_x: u8,
    pub right_y: u8,
    pub l2: u8,
    pub r2: u8,
    pub analog_padding: [u8; 2],
    pub orientation: [f32; 4],
    pub acceleration: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub touch_count: u8,
    pub touch_reserve: [u8; 3],
    pub touch_held_time: u32,
    pub touches: [PadTouch; 2],
    pub connected: u8,
    pub pad0: [u8; 3],
    pub timestamp: u64,
    pub extension: [u8; 16],
    pub connected_count: u8,
    pub reserve: [u8; 2],
    pub unique_length: u8,
    pub unique: [u8; 12],
}

/// `OrbisPadControllerInformation`: 28 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ControllerInfo {
    pub pixel_density: f32,
    pub resolution_x: u16,
    pub resolution_y: u16,
    pub dead_zone_left: u8,
    pub dead_zone_right: u8,
    pub connection_type: u8,
    pub connected_count: u8,
    pub connected: u8,
    pub pad: [u8; 3],
    pub device_class: i32,
    pub reserve: [u8; 8],
}

const _: () = assert!(std::mem::size_of::<PadData>() == 120);
const _: () = assert!(std::mem::size_of::<PadTouch>() == 8);
const _: () = assert!(std::mem::size_of::<ControllerInfo>() == 28);
const _: () = assert!(std::mem::offset_of!(PadData, touches) == 60);
const _: () = assert!(std::mem::offset_of!(PadData, timestamp) == 80);

impl PadData {
    /// Sticks centred, the identity orientation, nothing else.
    pub fn neutral() -> PadData {
        PadData {
            left_x: 128,
            left_y: 128,
            right_x: 128,
            right_y: 128,
            orientation: [0.0, 0.0, 0.0, 1.0],
            ..PadData::default()
        }
    }
}

/// The camera state the callbacks and the pad read share.
pub fn camera() -> &'static Arc<Mutex<Camera>> {
    static CAMERA: OnceLock<Arc<Mutex<Camera>>> = OnceLock::new();
    CAMERA.get_or_init(|| Arc::new(Mutex::new(Camera::new(Knobs::from_env()))))
}

static PAD: Mutex<Option<Pad>> = Mutex::new(None);

/// Runs `f` under the pad lock on the module's pad, made on first use.
fn with_pad<R>(f: impl FnOnce(&mut Pad) -> R) -> R {
    let mut guard: MutexGuard<'_, Option<Pad>> =
        PAD.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let pad = guard.get_or_insert_with(|| {
        Pad::new(
            host::system(),
            input::Settings::from_env(),
            camera().clone(),
        )
    });
    f(pad)
}

/// `runtime_pad_report`'s line.
pub fn report_line() -> String {
    with_pad(|pad| pad.report_line())
}

/// `--controller-info` and `--controller-test`, a loader command the launcher uses before the
/// game starts (no Vulkan or guest memory): what controller is there, and a rumble test.
/// Prints `connected=`, `name=`, `rumble=` and `tested=`; 1 when detection fails.
pub fn tool(host: &mut dyn host::Host, test: bool, strength: i32) -> i32 {
    let already_ready = host.pads_ready();
    if !already_ready {
        if let Err(error) = host.init_pads() {
            eprintln!("Controller detection failed: {error}");
            return 1;
        }
    }
    let quit = |host: &mut dyn host::Host| {
        if !already_ready {
            host.quit_pads();
        }
    };
    let id = host.pads().first().copied().filter(|&id| host.open(id));
    let Some(id) = id else {
        note("connected=0\nrumble=0");
        quit(host);
        return 0;
    };
    let name = host.name(id);
    let rumble = host.has_rumble(id);
    note("connected=1");
    note(&format!(
        "name={}",
        if name.is_empty() {
            "Game controller"
        } else {
            &name
        }
    ));
    note(&format!("rumble={}", i32::from(rumble)));
    if test {
        let amount = ((65535 * strength.clamp(0, 100) as u32 + 50) / 100) as u16;
        if rumble {
            host.rumble(id, amount, amount, 500);
            host.delay_ms(500);
        }
        host.rumble(id, 0, 0, 0);
        note(&format!("tested={}", i32::from(rumble)));
    }
    host.close(id);
    quit(host);
    0
}

/// `--controller-watch SECONDS`, for controller reports and tests: the gamepads SDL lists and,
/// while the player presses buttons, the one the game reads.
pub fn watch(host: Box<dyn host::Host>, seconds: i32) -> i32 {
    let mut pad = Pad::new(host, input::Settings::from_env(), camera().clone());
    pad.host.hints();
    if let Err(error) = pad.host.init_pads() {
        eprintln!("Controller detection failed: {error}");
        return 1;
    }
    pad.mark_ready();
    pad.host.delay_ms(500);
    pad.host.update_pads();
    let end = now_us() + u64::from(if seconds > 0 { seconds } else { 5 } as u32) * 1_000_000;
    let mut last = None;
    let mut presses = 0u32;
    let mut was_down = false;
    while now_us() < end {
        pad.host.update_pads();
        let g = pad.current_gamepad();
        if g != last {
            last = g;
            note(&format!(
                "reading={}",
                g.map_or_else(|| "none".to_string(), |id| pad.host.name(id))
            ));
        }
        let down = g.is_some_and(|id| pad.button_south(id));
        if down && !was_down {
            presses += 1;
        }
        was_down = down;
        pad.host.delay_ms(5);
    }
    note(&format!(
        "gamepads={}\npresses_read={presses}",
        pad.pads.len()
    ));
    0
}

/// The scePad* entry points, called by guest code with the System V ABI, and the C symbols of
/// `src/runtime_pad.c`.
#[cfg(target_arch = "x86_64")]
mod guest {
    use super::read::{ERR_INVALID_ARG, ERR_INVALID_HANDLE};
    use super::{with_pad, ControllerInfo, PadData};
    use std::ffi::{c_char, CStr};

    extern "sysv64" fn pad_init() -> i32 {
        with_pad(|pad| pad.init())
    }

    extern "sysv64" fn pad_open(user: i32, kind: i32, index: i32, _param: *const u8) -> i32 {
        with_pad(|pad| pad.open(user, kind, index))
    }

    extern "sysv64" fn pad_close(handle: i32) -> i32 {
        with_pad(|pad| pad.close(handle))
    }

    unsafe fn read_state(handle: i32, data: *mut PadData) -> i32 {
        if with_pad(|pad| pad.handle_ok(handle)) != 0 {
            return ERR_INVALID_HANDLE;
        }
        if data.is_null() {
            return ERR_INVALID_ARG;
        }
        // Low latency mode: a new frame starts here and may wait (outside the lock).
        super::host::gpu::input_sample();
        match with_pad(|pad| pad.read_state(handle)) {
            // SAFETY: the guest's 120-byte `OrbisPadData`.
            Ok(value) => unsafe { data.write_unaligned(value) },
            Err(error) => return error,
        }
        0
    }

    extern "sysv64" fn pad_read_state(handle: i32, data: *mut PadData) -> i32 {
        // SAFETY: the guest passes null or its `OrbisPadData`.
        unsafe { read_state(handle, data) }
    }

    /// Buffered read: the port samples once per call, so one entry is returned.
    extern "sysv64" fn pad_read(handle: i32, data: *mut PadData, count: i32) -> i32 {
        if with_pad(|pad| pad.handle_ok(handle)) != 0 {
            return ERR_INVALID_HANDLE;
        }
        if data.is_null() || !(1..=64).contains(&count) {
            return ERR_INVALID_ARG;
        }
        // SAFETY: as `pad_read_state`.
        unsafe { read_state(handle, data) };
        1
    }

    extern "sysv64" fn pad_info(handle: i32, info: *mut ControllerInfo) -> i32 {
        if with_pad(|pad| pad.handle_ok(handle)) != 0 {
            return ERR_INVALID_HANDLE;
        }
        if info.is_null() {
            return ERR_INVALID_ARG;
        }
        match with_pad(|pad| pad.info(handle)) {
            // SAFETY: the guest's 28-byte `OrbisPadControllerInformation`.
            Ok(value) => unsafe { info.write_unaligned(value) },
            Err(error) => return error,
        }
        0
    }

    extern "sysv64" fn pad_vibration(handle: i32, param: *const u8) -> i32 {
        if with_pad(|pad| pad.handle_ok(handle)) != 0 {
            return ERR_INVALID_HANDLE;
        }
        if param.is_null() {
            return ERR_INVALID_ARG;
        }
        // SAFETY: the guest's two motor strengths.
        let (small, large) = unsafe { (param.read(), param.add(1).read()) };
        with_pad(|pad| pad.vibrate(small, large));
        0
    }

    /// Light bar {r, g, b}: shown on controllers that have one (DualShock 4, DualSense).
    extern "sysv64" fn pad_light_bar(handle: i32, rgb: *const u8) -> i32 {
        if with_pad(|pad| pad.handle_ok(handle)) != 0 {
            return ERR_INVALID_HANDLE;
        }
        if rgb.is_null() {
            return ERR_INVALID_ARG;
        }
        // SAFETY: the guest's three colour bytes.
        let (r, g, b) = unsafe { (rgb.read(), rgb.add(1).read(), rgb.add(2).read()) };
        with_pad(|pad| pad.set_led(r, g, b));
        0
    }

    extern "sysv64" fn pad_reset_light_bar(handle: i32) -> i32 {
        if with_pad(|pad| pad.handle_ok(handle)) != 0 {
            return ERR_INVALID_HANDLE;
        }
        with_pad(|pad| pad.set_led(0, 0, 255)); // user 1 colour
        0
    }

    extern "sysv64" fn pad_ok_handle(handle: i32) -> i32 {
        with_pad(|pad| pad.handle_ok(handle))
    }

    extern "sysv64" fn pad_ok_handle_flag(handle: i32, _flag: u8) -> i32 {
        with_pad(|pad| pad.handle_ok(handle))
    }

    /// The game turns the gyroscope and accelerometer on and off here.
    extern "sysv64" fn pad_motion_sensor(handle: i32, enable: u8) -> i32 {
        with_pad(|pad| {
            let result = pad.handle_ok(handle);
            if result == 0 {
                pad.set_motion(enable != 0);
            }
            result
        })
    }

    pub(super) fn export(symbol: &str) -> Option<usize> {
        Some(match symbol {
            "scePadInit" => pad_init as *const () as usize,
            "scePadOpen" => pad_open as *const () as usize,
            "scePadClose" => pad_close as *const () as usize,
            "scePadReadState" => pad_read_state as *const () as usize,
            "scePadRead" => pad_read as *const () as usize,
            "scePadGetControllerInformation" => pad_info as *const () as usize,
            "scePadSetVibration" => pad_vibration as *const () as usize,
            "scePadResetOrientation" => pad_ok_handle as *const () as usize,
            "scePadSetAngularVelocityDeadbandState" | "scePadSetTiltCorrectionState" => {
                pad_ok_handle_flag as *const () as usize
            }
            "scePadSetMotionSensorState" => pad_motion_sensor as *const () as usize,
            "scePadSetLightBar" => pad_light_bar as *const () as usize,
            "scePadResetLightBar" => pad_reset_light_bar as *const () as usize,
            _ => return None,
        })
    }

    /// # Safety
    /// `name` is null or a NUL-terminated string.
    pub(super) unsafe fn name<'a>(name: *const c_char) -> Option<&'a CStr> {
        // SAFETY: (caller).
        (!name.is_null()).then(|| unsafe { CStr::from_ptr(name) })
    }
}

/// The address of the scePad function a guest import resolves to, or 0.
#[cfg(target_arch = "x86_64")]
pub fn resolve(name: &std::ffi::CStr) -> Option<usize> {
    name.to_str()
        .ok()
        .and_then(crate::abi::nid_name)
        .and_then(guest::export)
}

/// `uintptr_t runtime_pad_resolve(const char *name)`.
///
/// # Safety
/// `name` is null or a NUL-terminated string.
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub unsafe extern "C" fn runtime_pad_resolve(name: *const std::ffi::c_char) -> usize {
    // SAFETY: (caller).
    unsafe { guest::name(name) }.and_then(resolve).unwrap_or(0)
}

/// `void runtime_pad_report(void)`: the read counter and the gamepad in use.
#[no_mangle]
pub extern "C" fn runtime_pad_report() {
    note(&report_line());
}

/// `int runtime_pad_tool(int test, int strength)`.
#[no_mangle]
pub extern "C" fn runtime_pad_tool(test: i32, strength: i32) -> i32 {
    tool(host::system().as_mut(), test != 0, strength)
}

/// `int runtime_pad_watch(int seconds)`.
#[no_mangle]
pub extern "C" fn runtime_pad_watch(seconds: i32) -> i32 {
    watch(host::system(), seconds)
}

/// The synthetic camera input for one fixed 60 Hz tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub buttons: u32,
    pub left_x: u8,
    pub left_y: u8,
    pub right_x: u8,
    pub right_y: u8,
}

impl Sample {
    /// Nothing pressed, both sticks centred.
    pub const NEUTRAL: Sample = Sample {
        buttons: 0,
        left_x: 128,
        left_y: 128,
        right_x: 128,
        right_y: 128,
    };
}

/// The title-screen lead-in: confirm the first menu entry twice, then wait for the level to load.
///
/// The first entry of the title menu is Continue (the newest save) when a save exists and New
/// Game (the first chapter) when none does, so confirming it covers both starts the issue asks
/// for. The timings are `tools/restart.sh`'s (`press.sh` holds 0.3 s; 6 s between the presses)
/// with fixed waits where that script polls the frame statistics, because a fixed schedule
/// replays the same way on every machine. They are not measured against a running game yet.
#[derive(Debug, Clone, Copy, Default)]
pub struct EntryScript;

impl EntryScript {
    /// From the first pad read to the first confirm (the title screen settling).
    pub const TITLE_WAIT_MS: u64 = 20_000;
    /// How long each confirm is held.
    pub const PRESS_MS: u64 = 300;
    /// From the end of the first confirm to the start of the second.
    pub const CONFIRM_GAP_MS: u64 = 6_000;
    /// From the end of the second confirm until the level has loaded and the scene has settled.
    pub const LOAD_WAIT_MS: u64 = 60_000;
    pub const SECOND_PRESS_MS: u64 = Self::TITLE_WAIT_MS + Self::PRESS_MS + Self::CONFIRM_GAP_MS;
    /// The scripted camera turn starts, and the measured interval with it, at this offset.
    pub const DONE_MS: u64 = Self::SECOND_PRESS_MS + Self::PRESS_MS + Self::LOAD_WAIT_MS;

    /// The input `elapsed_ms` after the first pad read; the menu is only ever confirmed.
    pub fn sample_at_ms(self, elapsed_ms: u64) -> Sample {
        let pressed = |start: u64| (start..start + Self::PRESS_MS).contains(&elapsed_ms);
        if pressed(Self::TITLE_WAIT_MS) || pressed(Self::SECOND_PRESS_MS) {
            Sample {
                buttons: button::CROSS,
                ..Sample::NEUTRAL
            }
        } else {
            Sample::NEUTRAL
        }
    }
}

/// Two alternating eight-second camera turns, matching the existing soak script's stick
/// positions. Every tick maps to one fixed sample, independent of wall-clock timing.
#[derive(Debug, Clone, Copy, Default)]
pub struct BenchmarkPath;

impl BenchmarkPath {
    pub const TICKS_PER_TURN: u64 = 8 * 60;
    pub const PERIOD_TICKS: u64 = Self::TICKS_PER_TURN * 2;

    pub fn sample_at_tick(self, tick: u64) -> Sample {
        let turn = (tick % Self::PERIOD_TICKS) / Self::TICKS_PER_TURN;
        Sample {
            right_x: if turn == 0 { 60 } else { 195 },
            ..Sample::NEUTRAL
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_path_replays_identical_inputs_and_returns_to_start() {
        let path = BenchmarkPath;
        let run_a = (0..BenchmarkPath::PERIOD_TICKS * 2)
            .map(|tick| path.sample_at_tick(tick))
            .collect::<Vec<_>>();
        let run_b = (0..BenchmarkPath::PERIOD_TICKS * 2)
            .map(|tick| path.sample_at_tick(tick))
            .collect::<Vec<_>>();
        assert_eq!(run_a, run_b);
        assert!(run_a[..BenchmarkPath::TICKS_PER_TURN as usize]
            .iter()
            .all(|sample| sample.right_x == 60));
        assert!(run_a
            [BenchmarkPath::TICKS_PER_TURN as usize..BenchmarkPath::PERIOD_TICKS as usize]
            .iter()
            .all(|sample| sample.right_x == 195));
        assert_eq!(
            path.sample_at_tick(0),
            path.sample_at_tick(BenchmarkPath::PERIOD_TICKS)
        );
    }

    #[test]
    fn entry_script_confirms_twice_and_is_neutral_otherwise() {
        let script = EntryScript;
        let pressed: Vec<u64> = (0..EntryScript::DONE_MS + 1_000)
            .filter(|&ms| script.sample_at_ms(ms).buttons == button::CROSS)
            .collect();
        assert_eq!(pressed.len() as u64, 2 * EntryScript::PRESS_MS);
        assert_eq!(pressed[0], EntryScript::TITLE_WAIT_MS);
        assert_eq!(
            pressed[EntryScript::PRESS_MS as usize],
            EntryScript::SECOND_PRESS_MS
        );
        for ms in (0..EntryScript::DONE_MS + 1_000).filter(|ms| !pressed.contains(ms)) {
            assert_eq!(script.sample_at_ms(ms), Sample::NEUTRAL, "{ms} ms");
        }
        let again: Vec<u64> = (0..EntryScript::DONE_MS)
            .filter(|&ms| script.sample_at_ms(ms).buttons == button::CROSS)
            .collect();
        assert_eq!(pressed, again);
    }
}
