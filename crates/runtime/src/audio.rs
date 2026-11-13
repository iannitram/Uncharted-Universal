//! libSceAudioOut on SDL3 audio streams (piece 3.07; order 7 in docs/code-map.md), ported from
//! the deleted `src/runtime_audio.c` and `src/runtime_speakers.h`. `tests/test_speakers.c`'s
//! cases are this module's tests now.
//!
//! `sceAudioOutOutput` returns once per buffer period on a steady clock, like PS4 hardware:
//! FMOD's output thread reads 256-frame slots out of its mixer's 512-frame ring, and returning
//! in bursts (whenever the device pulls a quantum) lets it overtake the mixer and play stale
//! halves, a click every 10.7 ms. The lowest SDL queue level of each window of 32 buffers is
//! steered to two buffers (silence prefill on underrun, period/32 cadence corrections), which
//! also absorbs device clock drift. Without an audio device (headless runs, `BB_AUDIO=none`, or
//! any build but the loader's: [`device`]) ports only follow the clock.
//!
//! Environment: `BB_AUDIO=none` (no device), `SDL_AUDIO_DRIVER` or any other `BB_AUDIO` value
//! (the backend is the user's choice: never switch away from it), `BB_SPEAKERS` and
//! `BB_AUDIO_STEREO` ([`speaker_layout`]), `BB_AUDIO_STATS` (queue rhythm every 5 s) and
//! `BB_AUDIO_DUMP=<path>` (the converted PCM of each port, raw).

// Only x86-64 hosts run guest code: elsewhere nothing calls the ports (the guest entry points
// are not built), and the module is here for its tests and the hosts to come.
#![cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]

mod device;

use device::{Spec, Stream};
#[cfg(target_arch = "x86_64")]
use std::ffi::{c_char, CStr};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

const PORTS: usize = 25;
const ERR_NOT_OPENED: i32 = 0x8026_0001_u32 as i32;
const ERR_INVALID_PORT: i32 = 0x8026_0003_u32 as i32;
const ERR_INVALID_POINTER: i32 = 0x8026_0004_u32 as i32;
const ERR_PORT_FULL: i32 = 0x8026_0005_u32 as i32;
const ERR_INVALID_SIZE: i32 = 0x8026_0006_u32 as i32;
const ERR_INVALID_FORMAT: i32 = 0x8026_0007_u32 as i32;
const ERR_INVALID_FREQ: i32 = 0x8026_0008_u32 as i32;
const ERR_INVALID_VOLUME: i32 = 0x8026_0009_u32 as i32;
const ERR_INVALID_TYPE: i32 = 0x8026_000A_u32 as i32;
const ERR_ALREADY_INIT: i32 = 0x8026_000E_u32 as i32;
const ERR_NOT_INIT: i32 = 0x8026_000F_u32 as i32;
const VOLUME_0DB: i32 = 32768;
/// The only rate the PS4 takes, and the rate every stream runs at.
const RATE: u32 = 48_000;
/// The largest buffer, in frames.
const MAX_FRAMES: usize = 2048;
/// Channels of the eight `param & 0xff` formats: S16 mono, stereo, 7.1; float mono, stereo,
/// 7.1; S16 7.1 and float 7.1 in the standard (SDL) channel order.
const FORMAT_CHANNELS: [usize; 8] = [1, 2, 8, 1, 2, 8, 8, 8];
/// PS4 7.1 (L R C LFE SL SR BL BR) to SDL 7.1 (FL FR FC LFE BL BR SL SR).
const REMAP: [usize; 8] = [0, 1, 2, 3, 6, 7, 4, 5];
/// The plain device formats tried after the game's own: stereo S16 at 48000 and 44100 Hz.
const PLAIN: [Spec; 2] = [
    Spec {
        float: false,
        channels: 2,
        freq: 48_000,
    },
    Spec {
        float: false,
        channels: 2,
        freq: 44_100,
    },
];
/// The queue level is controlled over this many buffers.
const WINDOW: u32 = 32;
/// `BB_AUDIO_STATS` prints one line per port this often.
const STATS_NS: u64 = 5_000_000_000;

/// `sceAudioOutGetPortState`'s answer, 32 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PortState {
    output: u16,
    channel: u8,
    reserved: u8,
    volume: i16,
    reroute: u16,
    flag: u64,
    reserved64: [u64; 2],
}
const _: () = assert!(std::mem::size_of::<PortState>() == 32);

/// One entry of `sceAudioOutOutputs`.
#[repr(C)]
#[derive(Clone, Copy)]
struct OutputParam {
    handle: i32,
    data: *const u8,
}

// ---------------------------------------------------------------------------------------------
// The pure parts: speaker layout, channel conversion, pacing and the queue level.

/// Channels the games are told (and the 8-channel ports are mixed to): 2, 6 (5.1) or 8 (7.1).
/// `mode`: `BB_SPEAKERS` (the launcher's Speakers setting): `stereo`/`2`, `5.1`/`6`,
/// `7.1`/`8`; anything else is auto. `force_stereo`: `BB_AUDIO_STEREO` is set (the Linux fork's
/// switch, kept: it wins over everything). `device`: channels of the default output device, 0
/// when there is none. Auto follows the device: 8 or more is 7.1, 6 or 7 is 5.1, anything less
/// (a stereo device, the Steam Deck's speakers, no device) is stereo.
pub fn speaker_layout(mode: Option<&str>, force_stereo: bool, device: i32) -> usize {
    match (force_stereo, mode) {
        (true, _) | (false, Some("2" | "stereo")) => 2,
        (false, Some("6" | "5.1")) => 6,
        (false, Some("8" | "7.1")) => 8,
        _ if device >= 8 => 8,
        _ if device >= 6 => 6,
        _ => 2,
    }
}

/// -3 dB as the C module wrote it (0.7071f), not `FRAC_1_SQRT_2`: the fold stays bit-exact.
#[allow(clippy::approx_constant)]
const MINUS_3_DB: f32 = 0.7071;

/// One frame of the game's 7.1 mix (SDL order: FL FR FC LFE BL BR SL SR) folded to `n` channels
/// (2, 6 or 8) in `o`: stereo gets FL/FR at full level and centre and surrounds at -3 dB (LFE
/// dropped, not scaled down by SDL's 0.3); 5.1 keeps FL FR FC LFE and adds the back pair to the
/// sides at -3 dB; 8 (or any other `n`) is unchanged. Samples are clamped to [-1, 1].
pub fn fold_frame(s: &[f32; 8], n: usize, o: &mut [f32; 8]) {
    let n = match n {
        6 => {
            o[..4].copy_from_slice(&s[..4]);
            o[4] = s[6] + MINUS_3_DB * s[4];
            o[5] = s[7] + MINUS_3_DB * s[5];
            6
        }
        2 => {
            o[0] = s[0] + MINUS_3_DB * (s[2] + s[4] + s[6]);
            o[1] = s[1] + MINUS_3_DB * (s[2] + s[5] + s[7]);
            2
        }
        _ => {
            *o = *s;
            8
        }
    };
    for sample in &mut o[..n] {
        *sample = sample.clamp(-1.0, 1.0);
    }
}

/// The sample layout of a port's buffers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Layout {
    channels: usize,
    float: bool,
    /// 7.1 already in SDL's order (formats 6 and 7): no remap.
    standard: bool,
}

impl Layout {
    fn of(format: u32) -> Self {
        Layout {
            channels: FORMAT_CHANNELS[format as usize],
            float: format >= 3 && format != 6,
            standard: format >= 6,
        }
    }

    fn sample_bytes(self) -> usize {
        if self.float {
            4
        } else {
            2
        }
    }
}

fn sample(data: &[u8], index: usize, float: bool) -> f32 {
    if float {
        f32::from_ne_bytes(data[index * 4..index * 4 + 4].try_into().unwrap())
    } else {
        f32::from(i16::from_ne_bytes(
            data[index * 2..index * 2 + 2].try_into().unwrap(),
        ))
    }
}

/// One guest buffer of `frames` frames with each channel's volume applied, 7.1 moved from the
/// PS4's order to SDL's (unless the format is already standard), in the guest's sample format.
/// 16-bit samples are truncated toward zero, as C's `(int16_t)(v*gain)`.
fn convert(data: &[u8], layout: Layout, frames: usize, volume: &[i32; 8], out: &mut Vec<u8>) {
    let channels = layout.channels;
    let bytes = layout.sample_bytes();
    out.clear();
    out.resize(frames * channels * bytes, 0);
    for frame in 0..frames {
        for channel in 0..channels {
            let target = if channels == 8 && !layout.standard {
                REMAP[channel]
            } else {
                channel
            };
            let from = frame * channels + channel;
            let to = (frame * channels + target) * bytes;
            let gain = volume[channel] as f32 / VOLUME_0DB as f32;
            let value = sample(data, from, layout.float) * gain;
            if layout.float {
                out[to..to + 4].copy_from_slice(&value.to_ne_bytes());
            } else {
                out[to..to + 2].copy_from_slice(&(value as i16).to_ne_bytes());
            }
        }
    }
}

/// A converted 7.1 buffer folded to `n` (2 or 6) channels with [`fold_frame`]; 16-bit samples go
/// through float (/32768 in, *32767 out).
fn fold(converted: &[u8], float: bool, frames: usize, n: usize, out: &mut Vec<u8>) {
    let bytes = if float { 4 } else { 2 };
    out.clear();
    out.resize(frames * n * bytes, 0);
    let mut folded = [0.0f32; 8];
    for frame in 0..frames {
        let mut s = [0.0f32; 8];
        for (channel, value) in s.iter_mut().enumerate() {
            *value = sample(converted, frame * 8 + channel, float);
            if !float {
                *value /= 32768.0;
            }
        }
        fold_frame(&s, n, &mut folded);
        for (channel, &value) in folded[..n].iter().enumerate() {
            let at = (frame * n + channel) * bytes;
            if float {
                out[at..at + 4].copy_from_slice(&value.to_ne_bytes());
            } else {
                out[at..at + 2].copy_from_slice(&((value * 32767.0) as i16).to_ne_bytes());
            }
        }
    }
}

/// The period of one buffer of `frames` frames, in nanoseconds.
fn period_ns(frames: usize) -> u64 {
    frames as u64 * 1_000_000_000 / u64::from(RATE)
}

/// The cadence of a paced output: `next` is the port's next return time (0 before the first).
/// The first buffer, or one more than 8 periods late (a stall), restarts the cadence at `now` and
/// returns at once; otherwise the caller sleeps until the returned deadline. Either way the next
/// deadline is one period later. The arithmetic wraps, as the C `uint64_t` did.
fn pace(next: &mut u64, now: u64, period: u64) -> Option<u64> {
    let wait = if *next == 0 || now > next.wrapping_add(8 * period) {
        *next = now;
        None
    } else {
        Some(*next)
    };
    *next = next.wrapping_add(period);
    wait
}

/// The SDL queue level control of one port. The device drains the queue in quanta (21 ms on
/// PipeWire), so the level is a sawtooth: its minimum over [`WINDOW`] buffers is what is
/// steered to two buffers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Level {
    /// Added to every deadline: +period/32 when the minimum stayed above four buffers (slow
    /// down), -period/32 below two (speed up), 0 between.
    adjust_ns: i64,
    window_min: i32,
    window_count: u32,
}

impl Level {
    /// Before queueing a buffer of `buffer` bytes with `queued` bytes waiting: the bytes of
    /// silence to queue first (less than one buffer queued: fill up to two, at most one buffer).
    fn step(&mut self, queued: i32, buffer: i32, period: u64) -> i32 {
        let low = 2 * buffer;
        let (silence, queued) = if queued < buffer {
            ((low - queued).min(buffer), low)
        } else {
            (0, queued)
        };
        if self.window_count == 0 || queued < self.window_min {
            self.window_min = queued;
        }
        self.window_count += 1;
        if self.window_count == WINDOW {
            let step = (period / u64::from(WINDOW)) as i64;
            self.adjust_ns = if self.window_min > low + 2 * buffer {
                step
            } else if self.window_min < low {
                -step
            } else {
                0
            };
            self.window_count = 0;
        }
        silence
    }
}

/// The ports of each `sceAudioOutOpen` type: main 0-7, BGM 8, voice 9-12, personal 13-16, pad
/// speaker 17-20, audio3d 21-22, aux 23-24.
fn port_range(kind: i32) -> Option<RangeInclusive<usize>> {
    Some(match kind {
        0 => 0..=7,
        1 => 8..=8,
        2 => 9..=12,
        3 => 13..=16,
        4 => 17..=20,
        126 => 21..=22,
        127 => 23..=24,
        _ => return None,
    })
}

/// `BB_AUDIO_DUMP`'s file for one port: `<base>.port<id>.<channels>ch.<f32|s16>`.
fn dump_path(base: &str, id: usize, layout: Layout) -> String {
    let format = if layout.float { "f32" } else { "s16" };
    format!("{base}.port{id}.{}ch.{format}", layout.channels)
}

// ---------------------------------------------------------------------------------------------
// Clock and output lines.

/// The host's monotonic clock in nanoseconds: `CLOCK_MONOTONIC`, the clock the C runtime's
/// kernel module gives the guest (`sceKernelGetLastOutputTime` shares its epoch).
#[cfg(unix)]
fn now_ns() -> u64 {
    #[repr(C)]
    struct Timespec {
        seconds: i64,
        nanoseconds: std::ffi::c_long,
    }
    extern "C" {
        fn clock_gettime(clock: std::ffi::c_int, time: *mut Timespec) -> std::ffi::c_int;
    }
    const CLOCK_MONOTONIC: std::ffi::c_int = if cfg!(target_vendor = "apple") { 6 } else { 1 };
    let mut time = Timespec {
        seconds: 0,
        nanoseconds: 0,
    };
    // SAFETY: `time` is writable and the clock exists on every Unix host.
    unsafe { clock_gettime(CLOCK_MONOTONIC, &mut time) };
    time.seconds as u64 * 1_000_000_000 + time.nanoseconds as u64
}

/// The host's monotonic clock in nanoseconds: the performance counter, rounded as winpthreads'
/// `clock_gettime(CLOCK_MONOTONIC)` does for the C modules of the Windows loader.
#[cfg(windows)]
fn now_ns() -> u64 {
    use windows_sys::Win32::System::Performance::{
        QueryPerformanceCounter, QueryPerformanceFrequency,
    };
    let (mut counter, mut frequency) = (0i64, 0i64);
    // SAFETY: both write one i64; they cannot fail on Windows XP or later.
    unsafe {
        QueryPerformanceFrequency(&mut frequency);
        QueryPerformanceCounter(&mut counter);
    }
    let frequency = frequency.max(1);
    let mut seconds = counter / frequency;
    let mut nanoseconds = ((counter % frequency) * 1_000_000_000 + (frequency >> 1)) / frequency;
    if nanoseconds >= 1_000_000_000 {
        seconds += 1;
        nanoseconds -= 1_000_000_000;
    }
    seconds as u64 * 1_000_000_000 + nanoseconds as u64
}

/// Sleeps until [`now_ns`] reaches `deadline`.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn sleep_until(deadline: u64) {
    #[repr(C)]
    struct Timespec {
        seconds: i64,
        nanoseconds: std::ffi::c_long,
    }
    extern "C" {
        fn clock_nanosleep(
            clock: std::ffi::c_int,
            flags: std::ffi::c_int,
            time: *const Timespec,
            remain: *mut Timespec,
        ) -> std::ffi::c_int;
    }
    const CLOCK_MONOTONIC: std::ffi::c_int = 1;
    const TIMER_ABSTIME: std::ffi::c_int = 1;
    let time = Timespec {
        seconds: (deadline / 1_000_000_000) as i64,
        nanoseconds: (deadline % 1_000_000_000) as std::ffi::c_long,
    };
    // SAFETY: an absolute deadline on the monotonic clock; an interrupted sleep goes again.
    while unsafe { clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &time, std::ptr::null_mut()) }
        != 0
    {}
}

/// Sleeps until [`now_ns`] reaches `deadline` (std's sleep is a high-resolution waitable timer
/// on Windows, as `compat_sleep_ns` was).
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn sleep_until(deadline: u64) {
    loop {
        let now = now_ns();
        if now >= deadline {
            return;
        }
        std::thread::sleep(std::time::Duration::from_nanos(deadline - now));
    }
}

/// One line on stdout (the loader's C stdio, in order with the C modules' lines).
fn note(line: &str) {
    #[cfg(target_arch = "x86_64")]
    crate::abi::host::out_line(line);
    #[cfg(not(target_arch = "x86_64"))]
    println!("{line}");
}

/// One line on stderr.
fn warn(line: &str) {
    #[cfg(target_arch = "x86_64")]
    crate::abi::host::err(&format!("{line}\n"));
    #[cfg(not(target_arch = "x86_64"))]
    eprintln!("{line}");
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every critical section leaves its state whole, so a poisoned lock is still usable.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------------------------
// The port table.

/// `BB_AUDIO_STATS` counters of one port.
struct Stats {
    start_ns: u64,
    last_ns: u64,
    max_gap_ns: u64,
    starved: u32,
    buffers: u32,
    min_queued: i32,
}

impl Stats {
    fn new() -> Self {
        Stats {
            start_ns: 0,
            last_ns: 0,
            max_gap_ns: 0,
            starved: 0,
            buffers: 0,
            min_queued: 1 << 30,
        }
    }
}

/// An open port, under its slot's mutex (held across the pacing sleep).
struct Port {
    layout: Layout,
    frames: usize,
    /// 7.1 ports: the channels sent to SDL (2, 6 or 8), the game's mix folded here; 0 without
    /// SDL.
    out_channels: usize,
    volume: [i32; 8],
    stream: Option<Stream>,
    /// The next return of `sceAudioOutOutput` ([`pace`]).
    next_deadline_ns: u64,
    level: Level,
    dump: Option<BufWriter<File>>,
    stats: Option<Stats>,
    converted: Vec<u8>,
    folded: Vec<u8>,
}

struct Slot {
    /// Open. Set and cleared under [`TABLE`], read without it.
    used: AtomicBool,
    /// Holds an SDL stream (the WASAPI fallback may only restart SDL when no port does).
    playing: AtomicBool,
    kind: AtomicI32,
    channels: AtomicUsize,
    /// When the last output returned, in microseconds of [`now_ns`].
    last_output_us: AtomicU64,
    port: Mutex<Option<Port>>,
}

/// What `src/runtime_audio.c` kept beside its table, under its table lock.
struct Table {
    /// SDL audio: `None` until the first port opens, then whether it runs.
    sdl: Option<bool>,
    /// `SDL_AUDIO_DRIVER` or `BB_AUDIO` names the backend: never switch away from it.
    explicit_driver: bool,
    /// The [`PLAIN`] format that worked: later ports start with it.
    plain: Option<usize>,
    /// SDL was moved to DirectSound once; never again.
    switched: bool,
}

static TABLE: Mutex<Table> = Mutex::new(Table {
    sdl: None,
    explicit_driver: false,
    plain: None,
    switched: false,
});
static SLOTS: [Slot; PORTS] = [const {
    Slot {
        used: AtomicBool::new(false),
        playing: AtomicBool::new(false),
        kind: AtomicI32::new(0),
        channels: AtomicUsize::new(0),
        last_output_us: AtomicU64::new(0),
        port: Mutex::new(None),
    }
}; PORTS];
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static BUFFERS_OUT: AtomicUsize = AtomicUsize::new(0);
static PORTS_OPENED: AtomicUsize = AtomicUsize::new(0);
/// The speaker layout once known (0 before), and the lock that keeps device queries apart from
/// the driver switch (after [`TABLE`]).
static SPEAKERS: Mutex<usize> = Mutex::new(0);

/// The speaker layout the games are told (`sceAudioOutGetPortState`'s channel) and that 7.1
/// ports are mixed down to, decided once ([`speaker_layout`]). The device is asked only when the
/// layout follows it; a device that reports no channels (WASAPI cannot read the format of some
/// sound cards) counts as stereo.
fn speaker_count() -> usize {
    let mut count = lock(&SPEAKERS);
    if *count != 0 {
        return *count;
    }
    let mode = std::env::var_os("BB_SPEAKERS").map(|mode| mode.to_string_lossy().into_owned());
    let mode = mode.as_deref();
    let force_stereo = std::env::var_os("BB_AUDIO_STEREO").is_some();
    let mut channels = 0;
    let queried = !force_stereo && speaker_layout(mode, false, 0) != speaker_layout(mode, false, 8);
    if queried {
        if let Some(reported) = device::default_channels() {
            channels = reported;
        }
    }
    *count = speaker_layout(mode, force_stereo, channels);
    note(&format!(
        "Runtime: speakers {} (output device {channels} channels): the game is told {} channels",
        mode.filter(|mode| !mode.is_empty()).unwrap_or("auto"),
        *count
    ));
    if queried && channels <= 0 {
        let error = device::error();
        note(&format!(
            "Runtime: speakers: the output device reports 0 channels ({}); treating it as stereo",
            if error.is_empty() {
                "format unknown"
            } else {
                &error
            }
        ));
    }
    *count
}

/// Starts SDL audio on the first open, unless `BB_AUDIO=none`.
fn sdl_audio(table: &mut Table) -> bool {
    if table.sdl.is_none() {
        let mode = std::env::var_os("BB_AUDIO");
        let none = mode.as_deref() == Some("none".as_ref());
        table.explicit_driver =
            device::driver_hint() || mode.as_ref().is_some_and(|mode| !mode.is_empty() && !none);
        let ready = !none && device::init();
        table.sdl = Some(ready);
        note(&format!(
            "Runtime: audio backend {}",
            if ready {
                device::current_driver().unwrap_or_else(|| "(null)".into())
            } else {
                "timer (silent)".into()
            }
        ));
    }
    table.sdl == Some(true)
}

fn format_name(spec: Spec) -> &'static str {
    if spec.float {
        "float"
    } else {
        "S16"
    }
}

/// The requested format first, then the [`PLAIN`] ones. Once a plain format worked, later ports
/// start with it.
fn open_formats(plain: &mut Option<usize>, id: usize, want: Spec) -> Option<Stream> {
    if plain.is_none() {
        if let Some(stream) = device::open(want, None) {
            return Some(stream);
        }
        note(&format!(
            "Runtime: audio port {id}: SDL could not open the device as {} ch {} {} Hz ({})",
            want.channels,
            format_name(want),
            want.freq,
            device::error()
        ));
    }
    for (index, spec) in PLAIN.iter().enumerate().skip(plain.unwrap_or(0)) {
        if let Some(stream) = device::open(*spec, Some(want)) {
            *plain = Some(index);
            note(&format!(
                "Runtime: audio port {id}: opened the device as stereo S16 {} Hz instead (SDL converts the game's audio)",
                spec.freq
            ));
            return Some(stream);
        }
        note(&format!(
            "Runtime: audio port {id}: SDL could not open the device as stereo S16 {} Hz either ({})",
            spec.freq,
            device::error()
        ));
    }
    None
}

/// The stream of a new port: the requested format, the plain formats, then (once per process,
/// when the user did not pick a backend) SDL's DirectSound driver in place of WASAPI. `None`:
/// the port runs on the timer sink.
fn open_stream(table: &mut Table, id: usize, want: Spec) -> Option<Stream> {
    let stream = open_formats(&mut table.plain, id, want);
    if stream.is_some()
        || table.switched
        || table.explicit_driver
        || device::current_driver().as_deref() != Some("wasapi")
    {
        return stream;
    }
    if let Some(other) = SLOTS
        .iter()
        .position(|slot| slot.playing.load(Ordering::Acquire))
    {
        note(&format!(
            "Runtime: audio port {id}: WASAPI failed, but port {other} is playing on it; keeping the driver"
        ));
        return None;
    }
    // Quitting SDL audio drops every stream and device, which is why no other port may hold one.
    table.switched = true;
    note(&format!(
        "Runtime: audio port {id}: WASAPI failed; switching SDL audio to the directsound driver"
    ));
    {
        let _device = lock(&SPEAKERS);
        device::quit();
        device::set_driver(Some(device::DIRECTSOUND));
        if !device::init() {
            note(&format!(
                "Runtime: audio: the directsound driver failed to start ({}); back to the default driver",
                device::error()
            ));
            device::set_driver(None);
            if !device::init() {
                note(&format!(
                    "Runtime: audio: SDL audio can not be restarted ({}); audio is silent",
                    device::error()
                ));
                table.sdl = Some(false);
            }
        } else {
            note(&format!(
                "Runtime: audio backend {} (directsound replaces wasapi for every port from now on)",
                device::current_driver().unwrap_or_else(|| "(null)".into())
            ));
        }
    }
    if table.sdl != Some(true) {
        return None;
    }
    table.plain = None;
    let stream = open_formats(&mut table.plain, id, want);
    if stream.is_some() {
        note(&format!(
            "Runtime: audio port {id}: sound works on the directsound driver"
        ));
    }
    stream
}

/// The port id of `handle`, or the error for it: `INVALID_PORT` for a handle that is not one,
/// `NOT_OPENED` for a closed port.
fn port_id(handle: i32) -> Result<usize, i32> {
    let id = (handle & 0xff) as usize;
    if handle & 0x3f00_0000 != 0x2000_0000 || id >= PORTS {
        return Err(ERR_INVALID_PORT);
    }
    if !SLOTS[id].used.load(Ordering::Acquire) {
        return Err(ERR_NOT_OPENED);
    }
    Ok(id)
}

/// [`port_id`] as the C module checked it, under the table lock.
fn checked_port(handle: i32) -> Result<usize, i32> {
    let _table = lock(&TABLE);
    port_id(handle)
}

/// `sceAudioOutInit`: once per process.
fn init() -> i32 {
    let _table = lock(&TABLE);
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        ERR_ALREADY_INIT
    } else {
        0
    }
}

/// `sceAudioOutOpen`: a port of `kind` for buffers of `length` frames (256..2048, a multiple of
/// 256) at 48000 Hz in format `param & 0xff`. Returns the handle,
/// `0x20000000 | kind << 16 | port`.
fn open(kind: i32, length: u32, freq: u32, param: u32) -> i32 {
    if !INITIALIZED.load(Ordering::Acquire) {
        return ERR_NOT_INIT;
    }
    if length == 0 || length as usize > MAX_FRAMES || length & 0xff != 0 {
        return ERR_INVALID_SIZE;
    }
    if freq != RATE {
        return ERR_INVALID_FREQ;
    }
    let format = param & 0xff;
    if format > 7 {
        return ERR_INVALID_FORMAT;
    }
    let Some(range) = port_range(kind) else {
        return ERR_INVALID_TYPE;
    };
    let layout = Layout::of(format);
    let mut table = lock(&TABLE);
    let Some(id) = range
        .into_iter()
        .find(|&id| !SLOTS[id].used.load(Ordering::Acquire))
    else {
        return ERR_PORT_FULL;
    };
    let mut port = Port {
        layout,
        frames: length as usize,
        out_channels: 0,
        volume: [VOLUME_0DB; 8],
        stream: None,
        next_deadline_ns: 0,
        level: Level::default(),
        dump: None,
        stats: None,
        converted: Vec::new(),
        folded: Vec::new(),
    };
    if sdl_audio(&mut table) {
        // A stereo-only game mix leaves the surround channels empty: SDL's own 7.1 to stereo
        // downmix scales the front pair by about 0.3 (far too quiet), so fold here instead.
        port.out_channels = if layout.channels == 8 {
            speaker_count()
        } else {
            layout.channels
        };
        let want = Spec {
            float: layout.float,
            channels: port.out_channels,
            freq: RATE as i32,
        };
        port.stream = open_stream(&mut table, id, want);
        match &port.stream {
            Some(stream) => stream.resume(),
            None => warn(&format!(
                "Runtime: SDL audio stream failed ({}); port {id} uses the timer sink",
                device::error()
            )),
        }
    }
    if std::env::var_os("BB_AUDIO_STATS").is_some() {
        port.stats = Some(Stats::new());
    }
    if let Some(base) = std::env::var_os("BB_AUDIO_DUMP") {
        let path = dump_path(&base.to_string_lossy(), id, layout);
        port.dump = File::create(path).ok().map(BufWriter::new);
    }
    let folded = match (layout.channels, port.out_channels) {
        (8, 2) => ", folded to stereo",
        (8, 6) => ", folded to 5.1",
        _ => "",
    };
    let slot = &SLOTS[id];
    slot.playing.store(port.stream.is_some(), Ordering::Release);
    slot.kind.store(kind, Ordering::Relaxed);
    slot.channels.store(layout.channels, Ordering::Relaxed);
    slot.last_output_us.store(0, Ordering::Relaxed);
    *lock(&slot.port) = Some(port);
    slot.used.store(true, Ordering::Release);
    PORTS_OPENED.fetch_add(1, Ordering::Relaxed);
    drop(table);
    note(&format!(
        "Runtime: audio port {id} opened (type {kind}, {} ch, {}, {length} frames{folded})",
        layout.channels,
        if layout.float { "float" } else { "s16" }
    ));
    (kind << 16) | id as i32 | 0x2000_0000
}

/// `sceAudioOutClose`: the stream and the dump file go with the port.
fn close(handle: i32) -> i32 {
    let _table = lock(&TABLE);
    let id = match port_id(handle) {
        Ok(id) => id,
        Err(error) => return error,
    };
    let slot = &SLOTS[id];
    *lock(&slot.port) = None;
    slot.playing.store(false, Ordering::Release);
    slot.used.store(false, Ordering::Release);
    0
}

/// One buffer of a port: converted, dumped, counted, paced (`pace`: wait for this port's next
/// period; `sceAudioOutOutputs` waits once for all its ports) and queued. Returns the samples of
/// a buffer, 0 for no data (only the output time moves), or an error.
///
/// # Safety
/// `data` is null or holds one buffer in the port's format (frames x channels samples).
unsafe fn output_port(handle: i32, data: *const u8, pace_port: bool) -> i32 {
    let id = match checked_port(handle) {
        Ok(id) => id,
        Err(error) => return error,
    };
    let slot = &SLOTS[id];
    let mut guard = lock(&slot.port);
    let Some(port) = guard.as_mut() else {
        return ERR_NOT_OPENED;
    };
    let layout = port.layout;
    let samples = port.frames * layout.channels;
    if !data.is_null() {
        // SAFETY: the caller's buffer holds `samples` samples of the port's format.
        let data = unsafe { std::slice::from_raw_parts(data, samples * layout.sample_bytes()) };
        output_buffer(id, port, data, pace_port);
        BUFFERS_OUT.fetch_add(1, Ordering::Relaxed);
    }
    slot.last_output_us
        .store(now_ns() / 1000, Ordering::Relaxed);
    if data.is_null() {
        0
    } else {
        samples as i32
    }
}

fn output_buffer(id: usize, port: &mut Port, data: &[u8], pace_port: bool) {
    let layout = port.layout;
    let period = period_ns(port.frames);
    let mut converted = std::mem::take(&mut port.converted);
    convert(data, layout, port.frames, &port.volume, &mut converted);
    if let Some(dump) = &mut port.dump {
        // A failed write only loses diagnostics.
        let _ = dump.write_all(&converted);
    }
    if let Some(stats) = &mut port.stats {
        let now = now_ns();
        if stats.start_ns == 0 {
            stats.start_ns = now;
            stats.min_queued = 1 << 30;
        } else if now - stats.last_ns > stats.max_gap_ns {
            stats.max_gap_ns = now - stats.last_ns;
        }
        stats.last_ns = now;
        stats.buffers += 1;
        if let Some(stream) = &port.stream {
            let queued = stream.queued();
            stats.min_queued = stats.min_queued.min(queued);
            if queued == 0 {
                stats.starved += 1;
            }
        }
        if now - stats.start_ns >= STATS_NS {
            note(&format!(
                "Audio stats port {id}: {} buffers/5s (expected {}), max gap {:.2} ms, min queued {} bytes, starved {}",
                stats.buffers,
                5 * RATE as usize / port.frames,
                stats.max_gap_ns as f64 / 1e6,
                stats.min_queued,
                stats.starved
            ));
            *stats = Stats {
                start_ns: now,
                last_ns: now,
                ..Stats::new()
            };
        }
    }
    if pace_port {
        if let Some(deadline) = pace(&mut port.next_deadline_ns, now_ns(), period) {
            sleep_until(deadline);
        }
    }
    if let Some(stream) = &port.stream {
        // Fewer speakers than the game's 7.1 mix: fold it (fold_frame).
        let queue: &[u8] = if layout.channels == 8 && port.out_channels < 8 {
            fold(
                &converted,
                layout.float,
                port.frames,
                port.out_channels,
                &mut port.folded,
            );
            &port.folded
        } else {
            &converted
        };
        let buffer = queue.len() as i32;
        let silence = port.level.step(stream.queued(), buffer, period);
        if silence > 0 {
            static SILENCE: [u8; MAX_FRAMES * 8 * 4] = [0; MAX_FRAMES * 8 * 4];
            stream.put(&SILENCE[..silence as usize]);
        }
        port.next_deadline_ns = port
            .next_deadline_ns
            .wrapping_add(port.level.adjust_ns as u64);
        stream.put(queue);
    }
    port.converted = converted;
}

/// `sceAudioOutOutputs`: one buffer for each of `count` ports, paced once for all of them (FMOD
/// feeds main and BGM together: pacing every port would halve the rate and starve both).
/// Returns the last port's result, or the first error.
///
/// # Safety
/// `params` is null or holds `count` entries whose buffers follow [`output_port`].
unsafe fn outputs(params: *const OutputParam, count: u32) -> i32 {
    if params.is_null() {
        return ERR_INVALID_POINTER;
    }
    if count == 0 || count as usize > PORTS {
        return ERR_PORT_FULL;
    }
    // SAFETY: the caller passes `count` entries.
    let params = unsafe { std::slice::from_raw_parts(params, count as usize) };
    let mut result = 0;
    for (index, param) in params.iter().enumerate() {
        // SAFETY: each entry's buffer follows `output_port` (caller).
        result = unsafe { output_port(param.handle, param.data, index == 0) };
        if result < 0 {
            return result;
        }
    }
    result
}

/// `sceAudioOutSetVolume`: each channel whose bit is set in `flags` takes its entry of `volume`
/// (0..=32768, 0 dB). An out-of-range entry stops there with `INVALID_VOLUME`, the channels
/// before it already set.
///
/// # Safety
/// `volume` is null or holds an entry for every channel whose bit is set.
unsafe fn set_volume(handle: i32, flags: i32, volume: *const i32) -> i32 {
    let id = match checked_port(handle) {
        Ok(id) => id,
        Err(error) => return error,
    };
    if volume.is_null() {
        return ERR_INVALID_POINTER;
    }
    let mut guard = lock(&SLOTS[id].port);
    let Some(port) = guard.as_mut() else {
        return ERR_NOT_OPENED;
    };
    for channel in (0..8).filter(|channel| flags & (1 << channel) != 0) {
        // SAFETY: the caller's array has this channel's entry.
        let value = unsafe { volume.add(channel).read_unaligned() };
        if !(0..=VOLUME_0DB).contains(&value) {
            return ERR_INVALID_VOLUME;
        }
        port.volume[channel] = value;
    }
    0
}

/// `sceAudioOutGetPortState`: voice and personal ports are on headphones (mono), pad speaker
/// ports on the pad (mono, volume 127), the others on the main output with the speaker layout
/// the game picks its mix (stereo, 5.1, 7.1) from.
fn port_state(handle: i32) -> Result<PortState, i32> {
    let id = checked_port(handle)?;
    let slot = &SLOTS[id];
    let channels = slot.channels.load(Ordering::Relaxed);
    let mut state = PortState::default();
    match slot.kind.load(Ordering::Relaxed) {
        2 | 3 => {
            state.output = 0x40;
            state.channel = 1;
        }
        4 => {
            state.output = 0x04;
            state.channel = 1;
            state.volume = 127;
        }
        _ => {
            state.output = 0x01;
            state.channel = if channels > 2 {
                speaker_count()
            } else {
                channels
            } as u8;
        }
    }
    Ok(state)
}

/// `sceAudioOutGetLastOutputTime`: when the port's last output returned, in microseconds.
fn last_output_time(handle: i32) -> Result<u64, i32> {
    let id = checked_port(handle)?;
    Ok(SLOTS[id].last_output_us.load(Ordering::Relaxed))
}

/// The host function behind the libSceAudioOut symbol `symbol`.
#[cfg(target_arch = "x86_64")]
fn export(symbol: &str) -> Option<usize> {
    use guest::*;
    Some(match symbol {
        "sceAudioOutInit" => audio_init as *const () as usize,
        "sceAudioOutOpen" => audio_open as *const () as usize,
        "sceAudioOutClose" => audio_close as *const () as usize,
        "sceAudioOutOutput" => audio_output as *const () as usize,
        "sceAudioOutOutputs" => audio_outputs as *const () as usize,
        "sceAudioOutSetVolume" => audio_volume as *const () as usize,
        "sceAudioOutGetPortState" => audio_state as *const () as usize,
        "sceAudioOutGetLastOutputTime" => audio_last_time as *const () as usize,
        _ => return None,
    })
}

/// Where the guest import `name` (a scoped NID) resolves to in this module, if anywhere.
#[cfg(target_arch = "x86_64")]
pub fn resolve(name: &CStr) -> Option<usize> {
    name.to_str()
        .ok()
        .and_then(crate::abi::nid_name)
        .and_then(export)
}

/// The libSceAudioOut entry points, called by guest code with the System V ABI.
#[cfg(target_arch = "x86_64")]
mod guest {
    use super::{OutputParam, PortState, ERR_INVALID_POINTER};

    pub(super) extern "sysv64" fn audio_init() -> i32 {
        super::init()
    }

    pub(super) extern "sysv64" fn audio_open(
        _user: i32,
        kind: i32,
        _index: i32,
        length: u32,
        freq: u32,
        param: u32,
    ) -> i32 {
        super::open(kind, length, freq, param)
    }

    pub(super) extern "sysv64" fn audio_close(handle: i32) -> i32 {
        super::close(handle)
    }

    pub(super) extern "sysv64" fn audio_output(handle: i32, data: *const u8) -> i32 {
        // SAFETY: the guest passes null or one buffer in the port's format.
        unsafe { super::output_port(handle, data, true) }
    }

    pub(super) extern "sysv64" fn audio_outputs(params: *const OutputParam, count: u32) -> i32 {
        // SAFETY: the guest passes null or `count` entries, each with null or one buffer.
        unsafe { super::outputs(params, count) }
    }

    pub(super) extern "sysv64" fn audio_volume(handle: i32, flags: i32, volume: *const i32) -> i32 {
        // SAFETY: the guest passes null or the 8-entry volume array.
        unsafe { super::set_volume(handle, flags, volume) }
    }

    pub(super) extern "sysv64" fn audio_state(handle: i32, state: *mut PortState) -> i32 {
        match super::port_state(handle) {
            Err(error) => error,
            Ok(_) if state.is_null() => ERR_INVALID_POINTER,
            Ok(value) => {
                // SAFETY: the guest's 32-byte state structure.
                unsafe { state.write_unaligned(value) };
                0
            }
        }
    }

    pub(super) extern "sysv64" fn audio_last_time(handle: i32, time: *mut u64) -> i32 {
        match super::last_output_time(handle) {
            Err(error) => error,
            Ok(_) if time.is_null() => ERR_INVALID_POINTER,
            Ok(value) => {
                // SAFETY: the guest's 64-bit output.
                unsafe { time.write_unaligned(value) };
                0
            }
        }
    }
}

/// The address of the libSceAudioOut function a guest import resolves to, or 0
/// (`uintptr_t runtime_audio_resolve(const char *name)`; `runtime_resolve` asks it in the C
/// order, after AJM).
///
/// # Safety
/// `name` is null or a NUL-terminated string.
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub unsafe extern "C" fn runtime_audio_resolve(name: *const c_char) -> usize {
    if name.is_null() {
        return 0;
    }
    // SAFETY: a NUL-terminated string (caller).
    resolve(unsafe { CStr::from_ptr(name) }).unwrap_or(0)
}

/// The audio counters on stdout (`runtime_report`).
#[no_mangle]
pub extern "C" fn runtime_audio_report() {
    note(&format!(
        "Runtime: audio ports opened={}, buffers output={}",
        PORTS_OPENED.load(Ordering::Relaxed),
        BUFFERS_OUT.load(Ordering::Relaxed)
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEAR: f32 = 1e-5;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < NEAR
    }

    // tests/test_speakers.c: the speaker layout.
    #[test]
    fn auto_layout_follows_the_device() {
        assert_eq!(speaker_layout(None, false, 0), 2, "no device: stereo");
        assert_eq!(speaker_layout(None, false, 1), 2, "mono device: stereo");
        assert_eq!(
            speaker_layout(None, false, 2),
            2,
            "the Steam Deck's speakers"
        );
        assert_eq!(speaker_layout(None, false, 6), 6, "5.1 device");
        assert_eq!(speaker_layout(None, false, 7), 6, "6.1 device: 5.1");
        assert_eq!(speaker_layout(None, false, 8), 8, "7.1 device");
        assert_eq!(
            speaker_layout(Some("auto"), false, 8),
            8,
            "BB_SPEAKERS=auto"
        );
        assert_eq!(speaker_layout(Some(""), false, 6), 6, "empty BB_SPEAKERS");
    }

    #[test]
    fn named_layout_wins_over_the_device() {
        assert_eq!(speaker_layout(Some("stereo"), false, 8), 2);
        assert_eq!(speaker_layout(Some("2"), false, 6), 2);
        assert_eq!(speaker_layout(Some("5.1"), false, 2), 6);
        assert_eq!(speaker_layout(Some("6"), false, 8), 6);
        assert_eq!(speaker_layout(Some("7.1"), false, 2), 8);
        assert_eq!(speaker_layout(Some("8"), false, 0), 8);
    }

    #[test]
    fn audio_stereo_forces_stereo_over_everything() {
        assert_eq!(speaker_layout(None, true, 8), 2);
        assert_eq!(speaker_layout(Some("7.1"), true, 8), 2);
    }

    // tests/test_speakers.c: the fold-down.
    #[test]
    fn fold_keeps_a_stereo_mix_at_full_level() {
        // The games' stereo-only mix (front pair only) must come out at full level, not at the
        // ~0.3 SDL's own 7.1 to stereo downmix gave.
        let front = [0.5, -0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let mut o = [0.0; 8];
        fold_frame(&front, 2, &mut o);
        assert!(near(o[0], 0.5) && near(o[1], -0.25));
        fold_frame(&front, 6, &mut o);
        assert!(near(o[0], 0.5) && near(o[1], -0.25));
        assert!(o[2..6].iter().all(|&v| near(v, 0.0)));
        fold_frame(&front, 8, &mut o);
        assert!(near(o[0], 0.5) && near(o[1], -0.25));
    }

    #[test]
    fn fold_mixes_centre_and_surrounds_at_minus_3_db() {
        // FL FR FC LFE BL BR SL SR
        let full = [0.1, 0.2, 0.4, 0.8, 0.1, 0.2, 0.3, 0.5];
        let mut o = [0.0; 8];
        fold_frame(&full, 2, &mut o);
        assert!(
            near(o[0], 0.1 + MINUS_3_DB * (0.4 + 0.1 + 0.3)),
            "LFE dropped"
        );
        assert!(near(o[1], 0.2 + MINUS_3_DB * (0.4 + 0.2 + 0.5)));
        fold_frame(&full, 6, &mut o);
        assert!(
            near(o[2], 0.4) && near(o[3], 0.8),
            "5.1 keeps centre and LFE"
        );
        assert!(near(o[4], 0.3 + MINUS_3_DB * 0.1) && near(o[5], 0.5 + MINUS_3_DB * 0.2));
        fold_frame(&full, 8, &mut o);
        assert_eq!(o, full, "7.1 unchanged");
    }

    #[test]
    fn fold_clamps() {
        let loud = [0.9, -0.9, 0.9, 0.0, 0.9, 0.9, 0.9, 0.9];
        let mut o = [0.0; 8];
        fold_frame(&loud, 2, &mut o);
        assert!(near(o[0], 1.0));
        let right: f32 = -0.9 + MINUS_3_DB * (0.9 + 0.9 + 0.9);
        assert!(near(o[1], right.min(1.0)));
        fold_frame(&loud, 6, &mut o);
        assert!(near(o[4], 1.0) && near(o[5], 1.0));
    }

    fn s16(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_ne_bytes()).collect()
    }

    fn f32s(samples: &[f32]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_ne_bytes()).collect()
    }

    #[test]
    fn convert_moves_ps4_7_1_to_sdl_order() {
        // PS4: L R C LFE SL SR BL BR; SDL: FL FR FC LFE BL BR SL SR.
        let frame: Vec<i16> = (1..=8).collect();
        let mut out = Vec::new();
        convert(&s16(&frame), Layout::of(2), 1, &[VOLUME_0DB; 8], &mut out);
        assert_eq!(out, s16(&[1, 2, 3, 4, 7, 8, 5, 6]));
        // Formats 6 and 7 are already in the standard order.
        convert(&s16(&frame), Layout::of(6), 1, &[VOLUME_0DB; 8], &mut out);
        assert_eq!(out, s16(&frame));
        let frame: Vec<f32> = (1..=8).map(|v| v as f32 / 10.0).collect();
        convert(&f32s(&frame), Layout::of(5), 1, &[VOLUME_0DB; 8], &mut out);
        assert_eq!(out, f32s(&[0.1, 0.2, 0.3, 0.4, 0.7, 0.8, 0.5, 0.6]));
        convert(&f32s(&frame), Layout::of(7), 1, &[VOLUME_0DB; 8], &mut out);
        assert_eq!(out, f32s(&frame));
    }

    #[test]
    fn convert_applies_each_channels_volume() {
        let mut volume = [VOLUME_0DB; 8];
        volume[1] = VOLUME_0DB / 2;
        let mut out = Vec::new();
        // Two stereo S16 frames; -3 * 0.5 truncates toward zero, as C's (int16_t) cast.
        convert(
            &s16(&[1000, -3, 32767, 1001]),
            Layout::of(1),
            2,
            &volume,
            &mut out,
        );
        assert_eq!(out, s16(&[1000, -1, 32767, 500]));
        volume[0] = 0;
        convert(&f32s(&[0.5, 0.5]), Layout::of(4), 1, &volume, &mut out);
        assert_eq!(out, f32s(&[0.0, 0.25]));
        // The volume follows the guest's channel, not the remapped one: SL (PS4 4) goes to SDL 6.
        let mut volume = [VOLUME_0DB; 8];
        volume[4] = 0;
        convert(&s16(&[100; 8]), Layout::of(2), 1, &volume, &mut out);
        assert_eq!(out, s16(&[100, 100, 100, 100, 100, 100, 0, 100]));
    }

    #[test]
    fn fold_turns_16_bit_7_1_into_stereo_and_5_1() {
        // One SDL-order frame: FL 16384 (0.5), FR -8192 (-0.25), the rest silent.
        let frame = s16(&[16384, -8192, 0, 0, 0, 0, 0, 0]);
        let mut out = Vec::new();
        fold(&frame, false, 1, 2, &mut out);
        assert_eq!(out, s16(&[16383, -8191]), "x/32768 then *32767, truncated");
        fold(&frame, false, 1, 6, &mut out);
        assert_eq!(out, s16(&[16383, -8191, 0, 0, 0, 0]));
        let frame = f32s(&[0.5, -0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        fold(&frame, true, 1, 2, &mut out);
        assert_eq!(out, f32s(&[0.5, -0.25]));
    }

    #[test]
    fn pace_returns_once_per_period() {
        let period = period_ns(256);
        assert_eq!(period, 5_333_333);
        let mut next = 0;
        // The first buffer starts the cadence and returns at once.
        assert_eq!(pace(&mut next, 1_000, period), None);
        assert_eq!(next, 1_000 + period);
        // Early buffers wait for their deadline, one period apart, whenever they arrive.
        assert_eq!(pace(&mut next, 1_500, period), Some(1_000 + period));
        assert_eq!(pace(&mut next, 1_600, period), Some(1_000 + 2 * period));
        // A late one (less than 8 periods) does not wait long but keeps the cadence.
        let late = 1_000 + 5 * period;
        assert_eq!(pace(&mut next, late, period), Some(1_000 + 3 * period));
        assert_eq!(next, 1_000 + 4 * period);
    }

    #[test]
    fn pace_restarts_after_a_stall() {
        let period = period_ns(256);
        let mut next = 0;
        pace(&mut next, 1_000, period);
        let stalled = 1_000 + period + 8 * period + 1;
        assert_eq!(pace(&mut next, stalled, period), None);
        assert_eq!(next, stalled + period);
        // Exactly 8 periods late is not a stall yet.
        let mut next = 0;
        pace(&mut next, 1_000, period);
        let deadline = 1_000 + period;
        assert_eq!(
            pace(&mut next, deadline + 8 * period, period),
            Some(deadline)
        );
    }

    #[test]
    fn level_prefills_silence_on_underrun() {
        let buffer = 1024; // 256 frames of S16 stereo
        let period = period_ns(256);
        let mut level = Level::default();
        assert_eq!(level.step(0, buffer, period), buffer, "empty: one buffer");
        assert_eq!(
            level.step(1000, buffer, period),
            1024,
            "never more than one"
        );
        assert_eq!(
            level.step(1500, buffer, period),
            0,
            "a buffer or more: none"
        );
        let mut level = Level::default();
        assert_eq!(level.step(100, 4096, period), 4096);
        assert_eq!(level.step(5000, 4096, period), 0);
        // Up to two buffers is always more than one below a buffer: the cap decides.
        assert_eq!(level.step(3000, 4096, period), 4096);
    }

    #[test]
    fn level_steers_the_window_minimum_to_two_buffers() {
        let buffer = 1024;
        let period = period_ns(256);
        let step = (period / 32) as i64;
        let run = |queued: i32| {
            let mut level = Level::default();
            for _ in 0..31 {
                level.step(queued, buffer, period);
                assert_eq!(level.adjust_ns, 0, "decided once per window");
            }
            level.step(queued, buffer, period);
            level
        };
        assert_eq!(run(5 * buffer).adjust_ns, step, "too full: slow down");
        assert_eq!(run(4 * buffer).adjust_ns, 0, "four buffers: keep");
        assert_eq!(run(2 * buffer).adjust_ns, 0, "two buffers: keep");
        assert_eq!(run(buffer + 1).adjust_ns, -step, "too low: speed up");
        // An underrun counts as two buffers (the prefill), not as empty.
        assert_eq!(run(0).adjust_ns, 0);
        // The minimum decides: one low buffer in a full window speeds up.
        let mut level = Level::default();
        for index in 0..32 {
            let queued = if index == 7 { buffer + 10 } else { 8 * buffer };
            level.step(queued, buffer, period);
        }
        assert_eq!(level.adjust_ns, -step);
        assert_eq!(level.window_count, 0, "a new window starts");
    }

    #[test]
    fn ports_ranges_and_dump_names() {
        assert_eq!(port_range(0), Some(0..=7));
        assert_eq!(port_range(1), Some(8..=8));
        assert_eq!(port_range(127), Some(23..=24));
        assert_eq!(port_range(5), None);
        assert_eq!(
            dump_path("/tmp/a", 3, Layout::of(2)),
            "/tmp/a.port3.8ch.s16"
        );
        assert_eq!(dump_path("x", 8, Layout::of(4)), "x.port8.2ch.f32");
        assert_eq!(std::mem::size_of::<PortState>(), 32);
        assert_eq!(std::mem::offset_of!(PortState, volume), 4);
        assert_eq!(std::mem::offset_of!(PortState, flag), 8);
        assert_eq!(std::mem::size_of::<OutputParam>(), 16);
    }

    /// The port table is process state: these tests take turns and start from no open port.
    static TABLE_TESTS: Mutex<()> = Mutex::new(());

    fn fresh_table() -> MutexGuard<'static, ()> {
        let guard = lock(&TABLE_TESTS);
        let _table = lock(&TABLE);
        for slot in &SLOTS {
            *lock(&slot.port) = None;
            slot.playing.store(false, Ordering::Release);
            slot.used.store(false, Ordering::Release);
        }
        INITIALIZED.store(false, Ordering::Release);
        guard
    }

    #[test]
    fn open_checks_its_arguments_in_the_c_order() {
        let _guard = fresh_table();
        assert_eq!(open(0, 256, RATE, 1), ERR_NOT_INIT);
        assert_eq!(init(), 0);
        assert_eq!(init(), ERR_ALREADY_INIT);
        for length in [0, 100, 2304, 4096] {
            assert_eq!(open(0, length, RATE, 1), ERR_INVALID_SIZE, "{length}");
        }
        assert_eq!(open(0, 256, 44_100, 1), ERR_INVALID_FREQ);
        assert_eq!(open(0, 256, RATE, 8), ERR_INVALID_FORMAT);
        assert_eq!(open(5, 256, RATE, 1), ERR_INVALID_TYPE);
        // Only the low byte of param is the format.
        let handle = open(1, 2048, RATE, 0x0100);
        assert_eq!(handle, 0x2001_0008, "BGM is port 8, type in bits 16-23");
        assert_eq!(open(1, 256, RATE, 1), ERR_PORT_FULL);
        assert_eq!(close(handle), 0);
        assert_eq!(close(handle), ERR_NOT_OPENED);
        assert_eq!(close(0x1000_0008), ERR_INVALID_PORT);
        assert_eq!(
            close(0x2000_0019),
            ERR_INVALID_PORT,
            "port 25 does not exist"
        );
    }

    #[test]
    fn main_ports_fill_in_order() {
        let _guard = fresh_table();
        init();
        let handles: Vec<i32> = (0..8).map(|_| open(0, 256, RATE, 1)).collect();
        assert_eq!(
            handles,
            (0..8).map(|id| 0x2000_0000 | id).collect::<Vec<_>>()
        );
        assert_eq!(open(0, 256, RATE, 1), ERR_PORT_FULL);
        close(handles[3]);
        assert_eq!(open(0, 256, RATE, 1), handles[3], "the lowest free port");
        assert_eq!(open(127, 256, RATE, 1), 0x207f_0017);
    }

    #[test]
    fn output_counts_samples_and_paces_on_the_timer() {
        let _guard = fresh_table();
        init();
        let stereo = open(0, 256, RATE, 1);
        let surround = open(0, 512, RATE, 5);
        let buffer = s16(&[0; 512]);
        let before = BUFFERS_OUT.load(Ordering::Relaxed);
        let start = now_ns();
        for _ in 0..3 {
            // SAFETY: one 256-frame stereo S16 buffer.
            assert_eq!(unsafe { output_port(stereo, buffer.as_ptr(), true) }, 512);
        }
        // Three buffers return once per period: the third at least two periods after the first.
        assert!(now_ns() - start >= 2 * period_ns(256));
        assert_eq!(BUFFERS_OUT.load(Ordering::Relaxed) - before, 3);
        let time = last_output_time(stereo).unwrap();
        assert!(time > 0 && time <= now_ns() / 1000);
        // No data: nothing is queued, only the output time moves.
        // SAFETY: a null buffer.
        assert_eq!(unsafe { output_port(stereo, std::ptr::null(), true) }, 0);
        assert_eq!(BUFFERS_OUT.load(Ordering::Relaxed) - before, 3);
        assert!(last_output_time(stereo).unwrap() >= time);
        let wide = f32s(&[0.0; 512 * 8]);
        let params = [
            OutputParam {
                handle: stereo,
                data: buffer.as_ptr(),
            },
            OutputParam {
                handle: surround,
                data: wide.as_ptr(),
            },
        ];
        // SAFETY: two entries with buffers of their ports' formats.
        unsafe {
            assert_eq!(
                outputs(params.as_ptr(), 2),
                512 * 8,
                "the last port's samples"
            );
            assert_eq!(outputs(std::ptr::null(), 1), ERR_INVALID_POINTER);
            assert_eq!(outputs(params.as_ptr(), 0), ERR_PORT_FULL);
            assert_eq!(outputs(params.as_ptr(), 26), ERR_PORT_FULL);
            assert_eq!(
                output_port(0x2000_0005, buffer.as_ptr(), true),
                ERR_NOT_OPENED
            );
        }
        let closed = [OutputParam {
            handle: 0x2000_0009,
            data: buffer.as_ptr(),
        }];
        // SAFETY: one entry; the port is not open, so its buffer is never read.
        assert_eq!(unsafe { outputs(closed.as_ptr(), 1) }, ERR_NOT_OPENED);
        assert_eq!(last_output_time(0x2000_0009), Err(ERR_NOT_OPENED));
    }

    #[test]
    fn volume_and_state_follow_the_port() {
        let _guard = fresh_table();
        init();
        let main = open(0, 256, RATE, 1);
        let voice = open(2, 256, RATE, 0);
        let personal = open(3, 256, RATE, 0);
        let pad = open(4, 256, RATE, 0);
        let surround = open(0, 256, RATE, 2);
        let volume = [0, 16384, VOLUME_0DB + 1, 0, 0, 0, 0, 0];
        // SAFETY: an 8-entry array, or null.
        unsafe {
            assert_eq!(set_volume(main, 0b11, volume.as_ptr()), 0);
            assert_eq!(set_volume(main, 0b100, volume.as_ptr()), ERR_INVALID_VOLUME);
            assert_eq!(set_volume(main, 1, std::ptr::null()), ERR_INVALID_POINTER);
            assert_eq!(set_volume(0x2000_0007, 1, volume.as_ptr()), ERR_NOT_OPENED);
            assert_eq!(set_volume(-1, 1, std::ptr::null()), ERR_INVALID_PORT);
        }
        let port = lock(&SLOTS[0].port);
        assert_eq!(port.as_ref().unwrap().volume[..3], [0, 16384, VOLUME_0DB]);
        drop(port);
        let state = |handle| port_state(handle).unwrap();
        assert_eq!((state(main).output, state(main).channel), (1, 2));
        assert_eq!((state(voice).output, state(voice).channel), (0x40, 1));
        assert_eq!((state(personal).output, state(personal).channel), (0x40, 1));
        assert_eq!(
            (state(pad).output, state(pad).channel, state(pad).volume),
            (0x04, 1, 127)
        );
        let speakers = state(surround).channel as usize;
        assert!(matches!(speakers, 2 | 6 | 8));
        assert_eq!(speakers, speaker_count());
        assert_eq!(port_state(0x2000_0006), Err(ERR_NOT_OPENED));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn resolves_the_audio_imports_by_nid() {
        assert_ne!(resolve(c"JfEPXVxhFqA#G#H").unwrap(), 0, "sceAudioOutInit");
        assert_ne!(
            resolve(c"w3PdaSTSwGE#G#H").unwrap(),
            0,
            "sceAudioOutOutputs"
        );
        assert_ne!(
            resolve(c"ekNvsT22rsY#G#H"),
            resolve(c"QOQtbeDqsT4#G#H"),
            "Open and Output differ"
        );
        assert_eq!(resolve(c"bzQExy189ZI#q#q"), None, "_init_env is not audio");
        assert_eq!(resolve(c"unknown"), None);
        // SAFETY: NUL-terminated literals or null.
        unsafe {
            assert_eq!(runtime_audio_resolve(std::ptr::null()), 0);
            assert_eq!(
                runtime_audio_resolve(c"GrQ9s4IrNaQ#G#H".as_ptr()),
                resolve(c"GrQ9s4IrNaQ#G#H").unwrap()
            );
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn guest_entry_points_check_their_pointers() {
        let _guard = fresh_table();
        let get = |nid: &CStr| resolve(nid).unwrap();
        // SAFETY: the libSceAudioOut contracts, called as guest code calls them.
        unsafe {
            let init: extern "sysv64" fn() -> i32 = std::mem::transmute(get(c"JfEPXVxhFqA#G#H"));
            let open: extern "sysv64" fn(i32, i32, i32, u32, u32, u32) -> i32 =
                std::mem::transmute(get(c"ekNvsT22rsY#G#H"));
            let state: extern "sysv64" fn(i32, *mut PortState) -> i32 =
                std::mem::transmute(get(c"GrQ9s4IrNaQ#G#H"));
            let close: extern "sysv64" fn(i32) -> i32 =
                std::mem::transmute(get(c"s1--uE9mBFw#G#H"));
            assert_eq!(init(), 0);
            let handle = open(0, 2, 0, 256, RATE, 0);
            assert_eq!(handle, 0x2002_0009);
            let mut value = PortState {
                flag: 7,
                ..PortState::default()
            };
            assert_eq!(state(handle, &mut value), 0);
            assert_eq!(
                value,
                PortState {
                    output: 0x40,
                    channel: 1,
                    ..PortState::default()
                }
            );
            assert_eq!(state(handle, std::ptr::null_mut()), ERR_INVALID_POINTER);
            assert_eq!(close(handle), 0);
            assert_eq!(state(handle, std::ptr::null_mut()), ERR_NOT_OPENED);
            let last = export("sceAudioOutGetLastOutputTime").unwrap();
            let last: extern "sysv64" fn(i32, *mut u64) -> i32 = std::mem::transmute(last);
            assert_eq!(last(handle, std::ptr::null_mut()), ERR_NOT_OPENED);
        }
    }
}
