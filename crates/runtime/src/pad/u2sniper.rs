//! Uncharted 2, chapter 22 "The Monastery", tower-2 sniper fight fix (#430).
//!
//! Adapted from Mevlock/uncharted_pc-dev (`e8d4a6b`, `fc5be37`, `f0458df`, `76781d2`, GPL-2.0):
//! 1. Holds `(animate 'entrance-tunnel-door-1 'close)` and `(enable-nav-blocker-object
//!    'entrance-tunnel-door-1)` while any live bridge-wave NPC or `sniper-6`/`-7` is still on
//!    the tunnel side of the gate (within 30 m, up to 1 m out on the bridge side), then replays
//!    both calls once everyone is out.
//! 2. Moves `sniper-6` or `sniper-7`'s spawner to `zone-sniper-6-1` (outside the gate on the
//!    bridge side) if spawned after the gate has closed, restoring it on a checkpoint restart.
//!
//! `BB_U2_MONASTERY_SNIPER=0` disables the fix.

use super::hooks::GuestRegs;
use std::sync::{Mutex, OnceLock};

const SID_SNIPER_1: u32 = 0x7b76_5409;
const SID_SNIPER_6: u32 = 0x6531_040c;
const SID_SNIPER_7: u32 = 0x61f0_19bb;
const SID_POST: u32 = 0xaeb2_faca;
const SID_GATE: u32 = 0x818f_051d;
const SID_CLOSE: u32 = 0x9e92_0916;
const SID_HELD: u32 = 0x00bb_0001;

const GATE_POS: [f32; 3] = [-181.38, 20.11, -451.67];
const GATE_FWD: [f32; 2] = [0.703, -0.711];

const THROUGH_GATE: [(u32, &str); 12] = [
    (0x84a5_dd27, "ai-gt02-bridge-wave1-1"),
    (0x89e6_fbfe, "ai-gt02-bridge-wave1-2"),
    (0x8d27_e649, "ai-gt02-bridge-wave1-3"),
    (0x9360_b64c, "ai-gt02-bridge-wave1-4"),
    (0x97a1_abfb, "ai-gt02-bridge-wave1-5"),
    (0x9ae2_8d22, "ai-gt02-bridge-wave1-6"),
    (0x9e23_9095, "ai-gt02-bridge-wave1-7"),
    (0xa66c_2d28, "ai-gt02-bridge-wave1-8"),
    (0xa2ad_309f, "ai-gt02-bridge-wave1-9"),
    (0x0be7_dca2, "ai-gt02-bridge-wave1-10"),
    (SID_SNIPER_6, "ai-gt02-sniper-6"),
    (SID_SNIPER_7, "ai-gt02-sniper-7"),
];

#[derive(Clone, Copy)]
struct BuildOffsets {
    find: u64,
    table: u64,
    animate: u64,
    nav_on: u64,
    alive: u64,
}

const BUILDS: [BuildOffsets; 2] = [
    // 1.02
    BuildOffsets {
        find: 0x56_c7b0,
        table: 0x113_0390,
        animate: 0x39_7c50,
        nav_on: 0xa0_4f60,
        alive: 0x2d_d900,
    },
    // 1.00 (EU and US)
    BuildOffsets {
        find: 0x25_7e90,
        table: 0xf1_ddb0,
        animate: 0x10_0230,
        nav_on: 0x3f_f550,
        alive: 0x08_7d40,
    },
];

#[derive(Clone, Copy, Default)]
struct MovedSpawner {
    spawner: usize,
    was: [f32; 3],
    put: [f32; 3],
}

#[derive(Default)]
struct State {
    version: Option<usize>,
    image_base: u64,
    gate_closed: bool,
    hold: bool,
    closing: bool,
    held_argc: usize,
    held_argv: [u64; 8],
    moved: [MovedSpawner; 2],
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}

fn fix_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BB_U2_MONASTERY_SNIPER").is_ok_and(|e| e.starts_with('0')))
}

fn sniper_name(sid: u32) -> Option<&'static str> {
    match sid {
        SID_SNIPER_1 => Some("ai-gt02-sniper-1"),
        0x7635_72d0 => Some("ai-gt02-sniper-2"),
        0x72f4_6f67 => Some("ai-gt02-sniper-3"),
        0x6cb3_3f62 => Some("ai-gt02-sniper-4"),
        0x6872_22d5 => Some("ai-gt02-sniper-5"),
        SID_SNIPER_6 => Some("ai-gt02-sniper-6"),
        SID_SNIPER_7 => Some("ai-gt02-sniper-7"),
        0xf56c_dee1 => Some("ai-gt02-rpg-1"),
        _ => None,
    }
}

/// Returns `Some(distance_inside_m)` when `pos` is on the tunnel side of `entrance-tunnel-door-1`
/// (or up to 1 m out on the bridge side) within 30 m horizontally and 8 m vertically.
pub(crate) fn is_inside_tunnel_plane(pos: [f32; 3]) -> Option<f32> {
    let dx = pos[0] - GATE_POS[0];
    let dz = pos[2] - GATE_POS[2];
    let inside = dx * GATE_FWD[0] + dz * GATE_FWD[1];
    if inside > -1.0 && dx * dx + dz * dz < 30.0 * 30.0 && (pos[1] - GATE_POS[1]).abs() < 8.0 {
        Some(inside)
    } else {
        None
    }
}

unsafe fn find_spawner(version: usize, image: u64, sid: u32) -> *mut u8 {
    type SpawnerFind = unsafe extern "sysv64" fn(table: *const u8, sid: u32) -> *mut u8;
    let b = BUILDS[version];
    // SAFETY: caller ensures `image` is the mapped U2 executable matching `version`.
    let f: SpawnerFind = unsafe { std::mem::transmute((image + b.find) as usize) };
    unsafe { f((image + b.table) as *const u8, sid) }
}

unsafe fn spawner_npc(s: *const u8) -> *const u8 {
    if s.is_null() {
        return std::ptr::null();
    }
    // SAFETY: EntitySpawner layout: +0x30 process handle, +0x38 id.
    let h = unsafe { (s.add(0x30) as *const u64).read_unaligned() };
    let id = unsafe { (s.add(0x38) as *const u32).read_unaligned() };
    if h < 0x10000 || id == 0 {
        return std::ptr::null();
    }
    let obj = unsafe { (h as *const u64).read_unaligned() };
    if obj < 0x10000 {
        return std::ptr::null();
    }
    let oid = unsafe { ((obj + 0x44) as *const u32).read_unaligned() };
    if oid == id {
        obj as *const u8
    } else {
        std::ptr::null()
    }
}

unsafe fn npc_alive(version: usize, image: u64, sid: u32) -> bool {
    type DcNative = unsafe extern "sysv64" fn(argc: i32, argv: *mut u64) -> u64;
    let b = BUILDS[version];
    let mut argv = [u64::from(sid), 0];
    // SAFETY: `image + b.alive` is `DcNpcAliveP`.
    let f: DcNative = unsafe { std::mem::transmute((image + b.alive) as usize) };
    unsafe { (f(1, argv.as_mut_ptr()) & 0xff) != 0 }
}

unsafe fn still_inside(version: usize, image: u64) -> Option<(&'static str, f32)> {
    for (sid, name) in THROUGH_GATE {
        // SAFETY: reads guest spawner and NPC world position at +0x110.
        let s = unsafe { find_spawner(version, image, sid) };
        let n = unsafe { spawner_npc(s) };
        if n.is_null() || !unsafe { npc_alive(version, image, sid) } {
            continue;
        }
        let pos = unsafe { (n.add(0x110) as *const [f32; 3]).read_unaligned() };
        if let Some(side) = is_inside_tunnel_plane(pos) {
            return Some((name, side));
        }
    }
    None
}

unsafe fn on_spawn(version: usize, image: u64, argc: u64, argv: u64) {
    if (argc as i32) < 1 || argv < 0x10000 {
        return;
    }
    // SAFETY: `argv` points to at least one `u32` symbol id.
    let sid = unsafe { (argv as *const u32).read_unaligned() };
    let Some(name) = sniper_name(sid) else {
        return;
    };
    let mut st = state().lock().unwrap_or_else(|e| e.into_inner());
    st.version = Some(version);
    st.image_base = image;
    if sid == SID_SNIPER_1 {
        st.hold = false;
    }
    if (sid != SID_SNIPER_6 && sid != SID_SNIPER_7) || !fix_on() {
        return;
    }
    // SAFETY: `image` is mapped and `version` matches this executable.
    let s = unsafe { find_spawner(version, image, sid) };
    if s.is_null() {
        return;
    }
    let idx = usize::from(sid == SID_SNIPER_7);
    if !st.gate_closed {
        let entry = &mut st.moved[idx];
        if entry.spawner == s as usize {
            let cur = unsafe { (s as *const [f32; 3]).read_unaligned() };
            if cur == entry.put {
                unsafe { (s as *mut [f32; 3]).write_unaligned(entry.was) };
                println!("U2 monastery sniper: {name}'s spawner back at its own position (the gate is open)");
            }
        }
        st.moved[idx] = MovedSpawner::default();
        return;
    }
    let post = unsafe { find_spawner(version, image, SID_POST) };
    if post.is_null() {
        return;
    }
    let same_frame = unsafe {
        std::slice::from_raw_parts(s.add(0x58), 16)
            == std::slice::from_raw_parts(post.add(0x58), 16)
    };
    if !same_frame {
        return;
    }
    let cur = unsafe { (s as *const [f32; 3]).read_unaligned() };
    let post_pos = unsafe { (post as *const [f32; 3]).read_unaligned() };
    if st.moved[idx].spawner != s as usize || cur != st.moved[idx].put {
        st.moved[idx] = MovedSpawner {
            spawner: s as usize,
            was: cur,
            put: post_pos,
        };
    }
    unsafe { (s as *mut [f32; 3]).write_unaligned(post_pos) };
    println!(
        "U2 monastery sniper: {name} spawns behind entrance-tunnel-door-1 after it closed: spawned outside it at zone-sniper-6-1 (Monastery #309 fix)"
    );
}

unsafe fn on_animate(version: usize, image: u64, argc: u64, argv: u64) {
    let argc = argc as i32;
    if !(2..=8).contains(&argc) || argv < 0x10000 || !fix_on() {
        return;
    }
    let mut st = state().lock().unwrap_or_else(|e| e.into_inner());
    st.version = Some(version);
    st.image_base = image;
    if st.closing {
        return;
    }
    // SAFETY: `argv` holds `argc` 8-byte DC arguments.
    let args = unsafe { std::slice::from_raw_parts_mut(argv as *mut u64, argc as usize) };
    if args[0] as u32 != SID_GATE || args[1] as u32 != SID_CLOSE {
        return;
    }
    let Some((inside, side)) = (unsafe { still_inside(version, image) }) else {
        return;
    };
    st.held_argc = argc as usize;
    st.held_argv[..argc as usize].copy_from_slice(args);
    st.hold = true;
    args[0] = u64::from(SID_HELD);
    println!(
        "U2 monastery sniper: entrance-tunnel-door-1 not closed yet: {inside} is still inside ({side:.1} m behind the gate); it closes when everyone is out (Monastery #309 fix)"
    );
}

unsafe fn on_blocker(version: usize, image: u64, argc: u64, argv: u64, on: bool) {
    if (argc as i32) < 1 || argv < 0x10000 {
        return;
    }
    let mut st = state().lock().unwrap_or_else(|e| e.into_inner());
    st.version = Some(version);
    st.image_base = image;
    // SAFETY: `argv` points to at least one `u64` argument.
    let sid = unsafe { (argv as *const u64).read_unaligned() } as u32;
    if sid != SID_GATE {
        return;
    }
    if on && st.hold && !st.closing {
        unsafe { (argv as *mut u64).write_unaligned(u64::from(SID_HELD)) };
        return;
    }
    if !on {
        st.hold = false;
    }
    st.gate_closed = on;
}

/// Called from the U2 per-frame hook to replay a held gate close once all NPCs have exited.
pub(super) unsafe fn on_frame(image: u64, version: usize) {
    let (held_argc, mut held_argv) = {
        let mut st = state().lock().unwrap_or_else(|e| e.into_inner());
        if !st.hold || st.version != Some(version) || st.image_base != image {
            return;
        }
        if unsafe { still_inside(version, image) }.is_some() {
            return;
        }
        st.hold = false;
        st.closing = true;
        (st.held_argc, st.held_argv)
    };
    type DcNative = unsafe extern "sysv64" fn(argc: i32, argv: *mut u64) -> u64;
    let b = BUILDS[version];
    let animate: DcNative = unsafe { std::mem::transmute((image + b.animate) as usize) };
    let nav_on: DcNative = unsafe { std::mem::transmute((image + b.nav_on) as usize) };
    unsafe {
        animate(held_argc as i32, held_argv.as_mut_ptr());
        let mut nav_argv = [u64::from(SID_GATE), 0];
        nav_on(1, nav_argv.as_mut_ptr());
    }
    if let Ok(mut st) = state().lock() {
        st.closing = false;
    }
    println!("U2 monastery sniper: entrance-tunnel-door-1 closes now, everyone is out");
}

pub(super) unsafe extern "win64" fn spawn_102(
    rdi: u64,
    rsi: u64,
    image: u64,
    _: u64,
    _: *const GuestRegs,
) {
    unsafe { on_spawn(0, image, rdi, rsi) };
}

pub(super) unsafe extern "win64" fn spawn_100(
    rdi: u64,
    rsi: u64,
    image: u64,
    _: u64,
    _: *const GuestRegs,
) {
    unsafe { on_spawn(1, image, rdi, rsi) };
}

pub(super) unsafe extern "win64" fn animate_102(
    rdi: u64,
    rsi: u64,
    image: u64,
    _: u64,
    _: *const GuestRegs,
) {
    unsafe { on_animate(0, image, rdi, rsi) };
}

pub(super) unsafe extern "win64" fn animate_100(
    rdi: u64,
    rsi: u64,
    image: u64,
    _: u64,
    _: *const GuestRegs,
) {
    unsafe { on_animate(1, image, rdi, rsi) };
}

pub(super) unsafe extern "win64" fn gate_on_102(
    rdi: u64,
    rsi: u64,
    image: u64,
    _: u64,
    _: *const GuestRegs,
) {
    unsafe { on_blocker(0, image, rdi, rsi, true) };
}

pub(super) unsafe extern "win64" fn gate_off_102(
    rdi: u64,
    rsi: u64,
    image: u64,
    _: u64,
    _: *const GuestRegs,
) {
    unsafe { on_blocker(0, image, rdi, rsi, false) };
}

pub(super) unsafe extern "win64" fn gate_on_100(
    rdi: u64,
    rsi: u64,
    image: u64,
    _: u64,
    _: *const GuestRegs,
) {
    unsafe { on_blocker(1, image, rdi, rsi, true) };
}

pub(super) unsafe extern "win64" fn gate_off_100(
    rdi: u64,
    rsi: u64,
    image: u64,
    _: u64,
    _: *const GuestRegs,
) {
    unsafe { on_blocker(1, image, rdi, rsi, false) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_plane_detects_tunnel_side_and_ignores_outside_or_far_npcs() {
        // 2 m behind gate along GATE_FWD (0.703, -0.711)
        let inside = [
            GATE_POS[0] + 2.0 * GATE_FWD[0],
            GATE_POS[1],
            GATE_POS[2] + 2.0 * GATE_FWD[1],
        ];
        assert!(is_inside_tunnel_plane(inside).is_some_and(|d| (d - 2.0).abs() < 0.01));

        // 2 m outside on bridge side (beyond -1 m doorway allowance)
        let outside = [
            GATE_POS[0] - 2.0 * GATE_FWD[0],
            GATE_POS[1],
            GATE_POS[2] - 2.0 * GATE_FWD[1],
        ];
        assert_eq!(is_inside_tunnel_plane(outside), None);

        // > 30 m away or > 8 m vertically
        let far = [
            GATE_POS[0] + 35.0 * GATE_FWD[0],
            GATE_POS[1],
            GATE_POS[2] + 35.0 * GATE_FWD[1],
        ];
        assert_eq!(is_inside_tunnel_plane(far), None);
        let high = [inside[0], GATE_POS[1] + 9.0, inside[2]];
        assert_eq!(is_inside_tunnel_plane(high), None);
    }

    #[test]
    fn blocker_callback_holds_nav_on_while_gate_close_is_held() {
        {
            let mut st = state().lock().unwrap();
            *st = State::default();
            st.hold = true;
        }
        let mut argv = [u64::from(SID_GATE), 0];
        unsafe { on_blocker(0, 0x1000, 1, argv.as_mut_ptr() as u64, true) };
        assert_eq!(argv[0], u64::from(SID_HELD));
        assert!(!state().lock().unwrap().gate_closed);

        // Opening the gate drops the hold.
        let mut open_argv = [u64::from(SID_GATE), 0];
        unsafe { on_blocker(0, 0x1000, 1, open_argv.as_mut_ptr() as u64, false) };
        let st = state().lock().unwrap();
        assert!(!st.hold);
        assert!(!st.gate_closed);
    }
}
