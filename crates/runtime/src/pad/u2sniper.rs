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

struct GuestOps {
    version: usize,
    image: u64,
    find_spawner: unsafe fn(usize, u64, u32) -> *mut u8,
    npc_alive: unsafe fn(usize, u64, u32) -> bool,
    animate: unsafe fn(usize, u64, i32, *mut u64),
    nav_on: unsafe fn(usize, u64, i32, *mut u64),
}

impl GuestOps {
    fn for_build(version: usize, image: u64) -> Self {
        Self {
            version,
            image,
            find_spawner,
            npc_alive,
            animate: call_animate,
            nav_on: call_nav_on,
        }
    }
}

unsafe fn call_animate(version: usize, image: u64, argc: i32, argv: *mut u64) {
    type DcNative = unsafe extern "sysv64" fn(argc: i32, argv: *mut u64) -> u64;
    let b = BUILDS[version];
    // SAFETY: `image + b.animate` is the U2 DC `animate` native.
    let f: DcNative = unsafe { std::mem::transmute((image + b.animate) as usize) };
    unsafe { f(argc, argv) };
}

unsafe fn call_nav_on(version: usize, image: u64, argc: i32, argv: *mut u64) {
    type DcNative = unsafe extern "sysv64" fn(argc: i32, argv: *mut u64) -> u64;
    let b = BUILDS[version];
    // SAFETY: `image + b.nav_on` is `enable-nav-blocker-object`.
    let f: DcNative = unsafe { std::mem::transmute((image + b.nav_on) as usize) };
    unsafe { f(argc, argv) };
}

unsafe fn still_inside_with(ops: &GuestOps) -> Option<(&'static str, f32)> {
    for (sid, name) in THROUGH_GATE {
        // SAFETY: reads guest spawner and NPC world position at +0x110.
        let s = unsafe { (ops.find_spawner)(ops.version, ops.image, sid) };
        let n = unsafe { spawner_npc(s) };
        if n.is_null() || !unsafe { (ops.npc_alive)(ops.version, ops.image, sid) } {
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
    let ops = GuestOps::for_build(version, image);
    unsafe { on_spawn_with(state(), &ops, argc, argv, fix_on()) };
}

unsafe fn on_spawn_with(
    st_lock: &Mutex<State>,
    ops: &GuestOps,
    argc: u64,
    argv: u64,
    fix_enabled: bool,
) {
    if (argc as i32) < 1 || argv < 0x10000 {
        return;
    }
    // SAFETY: `argv` points to at least one `u32` symbol id.
    let sid = unsafe { (argv as *const u32).read_unaligned() };
    let Some(name) = sniper_name(sid) else {
        return;
    };
    let mut st = st_lock.lock().unwrap_or_else(|e| e.into_inner());
    st.version = Some(ops.version);
    st.image_base = ops.image;
    if sid == SID_SNIPER_1 {
        st.hold = false;
    }
    if (sid != SID_SNIPER_6 && sid != SID_SNIPER_7) || !fix_enabled {
        return;
    }
    // SAFETY: `image` is mapped and `version` matches this executable.
    let s = unsafe { (ops.find_spawner)(ops.version, ops.image, sid) };
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
    let post = unsafe { (ops.find_spawner)(ops.version, ops.image, SID_POST) };
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
    let ops = GuestOps::for_build(version, image);
    unsafe { on_animate_with(state(), &ops, argc, argv, fix_on()) };
}

unsafe fn on_animate_with(
    st_lock: &Mutex<State>,
    ops: &GuestOps,
    argc: u64,
    argv: u64,
    fix_enabled: bool,
) {
    let argc = argc as i32;
    if !(2..=8).contains(&argc) || argv < 0x10000 || !fix_enabled {
        return;
    }
    let mut st = st_lock.lock().unwrap_or_else(|e| e.into_inner());
    st.version = Some(ops.version);
    st.image_base = ops.image;
    if st.closing {
        return;
    }
    // SAFETY: `argv` holds `argc` 8-byte DC arguments.
    let args = unsafe { std::slice::from_raw_parts_mut(argv as *mut u64, argc as usize) };
    if args[0] as u32 != SID_GATE || args[1] as u32 != SID_CLOSE {
        return;
    }
    let Some((inside, side)) = (unsafe { still_inside_with(ops) }) else {
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
    unsafe { on_blocker_with(state(), version, image, argc, argv, on) };
}

unsafe fn on_blocker_with(
    st_lock: &Mutex<State>,
    version: usize,
    image: u64,
    argc: u64,
    argv: u64,
    on: bool,
) {
    if (argc as i32) < 1 || argv < 0x10000 {
        return;
    }
    let mut st = st_lock.lock().unwrap_or_else(|e| e.into_inner());
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
    let ops = GuestOps::for_build(version, image);
    unsafe { on_frame_with(state(), &ops) };
}

unsafe fn on_frame_with(st_lock: &Mutex<State>, ops: &GuestOps) {
    let (held_argc, mut held_argv) = {
        let mut st = st_lock.lock().unwrap_or_else(|e| e.into_inner());
        if !st.hold || st.version != Some(ops.version) || st.image_base != ops.image {
            return;
        }
        if unsafe { still_inside_with(ops) }.is_some() {
            return;
        }
        st.hold = false;
        st.closing = true;
        (st.held_argc, st.held_argv)
    };
    unsafe {
        (ops.animate)(
            ops.version,
            ops.image,
            held_argc as i32,
            held_argv.as_mut_ptr(),
        );
        let mut nav_argv = [u64::from(SID_GATE), 0];
        (ops.nav_on)(ops.version, ops.image, 1, nav_argv.as_mut_ptr());
    }
    if let Ok(mut st) = st_lock.lock() {
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
    use std::cell::RefCell;
    use std::collections::HashMap;

    const TEST_IMAGE: u64 = 0x1000_0000;
    const SID_WAVE_8: u32 = 0xa66c_2d28;
    const SID_WAVE_9: u32 = 0xa2ad_309f;

    struct FakeEntity {
        spawner: Box<[u8; 0x70]>,
        handle_target: Box<u64>,
        npc: Box<[u8; 0x120]>,
        alive: bool,
    }

    impl FakeEntity {
        fn new(
            spawner_pos: [f32; 3],
            npc_pos: [f32; 3],
            alive: bool,
            id: u32,
            frame: [u8; 16],
        ) -> Self {
            let mut npc = Box::new([0u8; 0x120]);
            npc[0x44..0x48].copy_from_slice(&id.to_le_bytes());
            for (i, c) in npc_pos.iter().enumerate() {
                npc[0x110 + i * 4..0x114 + i * 4].copy_from_slice(&c.to_le_bytes());
            }
            let handle_target = Box::new(npc.as_ptr() as u64);
            let mut spawner = Box::new([0u8; 0x70]);
            for (i, c) in spawner_pos.iter().enumerate() {
                spawner[i * 4..(i + 1) * 4].copy_from_slice(&c.to_le_bytes());
            }
            let h = (&*handle_target as *const u64) as u64;
            spawner[0x30..0x38].copy_from_slice(&h.to_le_bytes());
            spawner[0x38..0x3c].copy_from_slice(&id.to_le_bytes());
            spawner[0x58..0x68].copy_from_slice(&frame);
            Self {
                spawner,
                handle_target,
                npc,
                alive,
            }
        }

        fn spawner_pos(&self) -> [f32; 3] {
            unsafe { (self.spawner.as_ptr() as *const [f32; 3]).read_unaligned() }
        }

        fn set_npc_pos(&mut self, pos: [f32; 3]) {
            for (i, c) in pos.iter().enumerate() {
                self.npc[0x110 + i * 4..0x114 + i * 4].copy_from_slice(&c.to_le_bytes());
            }
        }

        fn set_stale_npc_id(&mut self, stale_id: u32) {
            self.npc[0x44..0x48].copy_from_slice(&stale_id.to_le_bytes());
        }

        fn clear_handle(&mut self) {
            *self.handle_target = 0;
        }
    }

    #[derive(Default)]
    struct MockWorld {
        entities: HashMap<u32, FakeEntity>,
        animate_calls: Vec<(i32, Vec<u64>)>,
        nav_on_calls: Vec<(i32, Vec<u64>)>,
    }

    thread_local! {
        static MOCK: RefCell<MockWorld> = RefCell::new(MockWorld::default());
    }

    fn reset_mock() {
        MOCK.with(|m| *m.borrow_mut() = MockWorld::default());
    }

    unsafe fn mock_find_spawner(_: usize, _: u64, sid: u32) -> *mut u8 {
        MOCK.with(|m| {
            m.borrow_mut()
                .entities
                .get_mut(&sid)
                .map_or(std::ptr::null_mut(), |e| e.spawner.as_mut_ptr())
        })
    }

    unsafe fn mock_npc_alive(_: usize, _: u64, sid: u32) -> bool {
        MOCK.with(|m| m.borrow().entities.get(&sid).is_some_and(|e| e.alive))
    }

    unsafe fn mock_animate(_: usize, _: u64, argc: i32, argv: *mut u64) {
        let args = unsafe { std::slice::from_raw_parts(argv, argc as usize) }.to_vec();
        MOCK.with(|m| m.borrow_mut().animate_calls.push((argc, args)));
    }

    unsafe fn mock_nav_on(_: usize, _: u64, argc: i32, argv: *mut u64) {
        let args = unsafe { std::slice::from_raw_parts(argv, argc as usize) }.to_vec();
        MOCK.with(|m| m.borrow_mut().nav_on_calls.push((argc, args)));
    }

    fn mock_ops(version: usize) -> GuestOps {
        GuestOps {
            version,
            image: TEST_IMAGE,
            find_spawner: mock_find_spawner,
            npc_alive: mock_npc_alive,
            animate: mock_animate,
            nav_on: mock_nav_on,
        }
    }

    fn pos_along_tunnel(dist: f32) -> [f32; 3] {
        [
            GATE_POS[0] + dist * GATE_FWD[0],
            GATE_POS[1],
            GATE_POS[2] + dist * GATE_FWD[1],
        ]
    }

    #[test]
    fn gate_plane_detects_tunnel_side_and_ignores_outside_or_far_npcs() {
        // 2 m behind gate along GATE_FWD (0.703, -0.711)
        let inside = pos_along_tunnel(2.0);
        assert!(is_inside_tunnel_plane(inside).is_some_and(|d| (d - 2.0).abs() < 0.01));

        // Doorway allowance: -0.9 m is inside, -1.1 m is outside
        assert!(is_inside_tunnel_plane(pos_along_tunnel(-0.9)).is_some());
        assert_eq!(is_inside_tunnel_plane(pos_along_tunnel(-1.1)), None);

        // 2 m outside on bridge side (beyond -1 m doorway allowance)
        let outside = pos_along_tunnel(-2.0);
        assert_eq!(is_inside_tunnel_plane(outside), None);

        // > 30 m away or > 8 m vertically
        let far = pos_along_tunnel(35.0);
        assert_eq!(is_inside_tunnel_plane(far), None);
        let high = [inside[0], GATE_POS[1] + 9.0, inside[2]];
        assert_eq!(is_inside_tunnel_plane(high), None);
    }

    #[test]
    fn spawner_npc_validates_handle_and_matching_id() {
        reset_mock();
        assert!(unsafe { spawner_npc(std::ptr::null()) }.is_null());

        let mut e = FakeEntity::new([0.0; 3], pos_along_tunnel(3.0), true, 42, [1; 16]);
        assert!(!unsafe { spawner_npc(e.spawner.as_ptr()) }.is_null());

        e.set_stale_npc_id(99);
        assert!(unsafe { spawner_npc(e.spawner.as_ptr()) }.is_null());

        e.set_stale_npc_id(42);
        e.clear_handle();
        assert!(unsafe { spawner_npc(e.spawner.as_ptr()) }.is_null());
    }

    #[test]
    fn still_inside_skips_dead_bodies_and_detects_live_bridge_npcs() {
        reset_mock();
        let frame = [0x11; 16];
        MOCK.with(|m| {
            let mut w = m.borrow_mut();
            // wave1-8 is dead inside the tunnel: must NOT hold the gate (Mevlock 76781d2)
            w.entities.insert(
                SID_WAVE_8,
                FakeEntity::new([0.0; 3], pos_along_tunnel(2.0), false, 10, frame),
            );
            // wave1-9 is alive inside the tunnel: MUST hold the gate
            w.entities.insert(
                SID_WAVE_9,
                FakeEntity::new([0.0; 3], pos_along_tunnel(5.0), true, 11, frame),
            );
        });
        let ops = mock_ops(0);
        let res = unsafe { still_inside_with(&ops) };
        assert_eq!(res.map(|(n, _)| n), Some("ai-gt02-bridge-wave1-9"));

        // Move wave1-9 out onto the bridge; dead wave1-8 remains inside -> clear.
        MOCK.with(|m| {
            m.borrow_mut()
                .entities
                .get_mut(&SID_WAVE_9)
                .unwrap()
                .set_npc_pos(pos_along_tunnel(-3.0));
        });
        assert_eq!(unsafe { still_inside_with(&ops) }, None);
    }

    #[test]
    fn animate_holds_close_while_live_npc_inside_then_replays_on_frame() {
        reset_mock();
        let frame = [0x22; 16];
        MOCK.with(|m| {
            m.borrow_mut().entities.insert(
                SID_WAVE_8,
                FakeEntity::new([0.0; 3], pos_along_tunnel(4.0), true, 20, frame),
            );
        });
        let st = Mutex::new(State::default());
        let ops = mock_ops(0);

        // Non-gate animate is ignored.
        let mut other_argv = [u64::from(SID_SNIPER_6), u64::from(SID_CLOSE)];
        unsafe { on_animate_with(&st, &ops, 2, other_argv.as_mut_ptr() as u64, true) };
        assert_eq!(other_argv[0], u64::from(SID_SNIPER_6));
        assert!(!st.lock().unwrap().hold);

        // Gate close while wave1-8 is inside: held and object arg swapped to SID_HELD.
        let mut close_argv = [u64::from(SID_GATE), u64::from(SID_CLOSE), 0xdead];
        unsafe { on_animate_with(&st, &ops, 3, close_argv.as_mut_ptr() as u64, true) };
        assert_eq!(close_argv[0], u64::from(SID_HELD));
        {
            let guard = st.lock().unwrap();
            assert!(guard.hold);
            assert_eq!(guard.held_argc, 3);
            assert_eq!(guard.held_argv[0], u64::from(SID_GATE));
            assert_eq!(guard.held_argv[1], u64::from(SID_CLOSE));
            assert_eq!(guard.held_argv[2], 0xdead);
        }

        // Subsequent enable-nav-blocker-object is also held while hold is active.
        let mut nav_argv = [u64::from(SID_GATE), 0];
        unsafe { on_blocker_with(&st, 0, TEST_IMAGE, 1, nav_argv.as_mut_ptr() as u64, true) };
        assert_eq!(nav_argv[0], u64::from(SID_HELD));
        assert!(!st.lock().unwrap().gate_closed);

        // Frame tick while wave1-8 is still inside does not replay yet.
        unsafe { on_frame_with(&st, &ops) };
        MOCK.with(|m| assert!(m.borrow().animate_calls.is_empty()));
        assert!(st.lock().unwrap().hold);

        // Wave1-8 exits onto the bridge -> next frame replays animate + nav_on.
        MOCK.with(|m| {
            m.borrow_mut()
                .entities
                .get_mut(&SID_WAVE_8)
                .unwrap()
                .set_npc_pos(pos_along_tunnel(-2.5));
        });
        unsafe { on_frame_with(&st, &ops) };
        {
            let guard = st.lock().unwrap();
            assert!(!guard.hold);
            assert!(!guard.closing);
        }
        MOCK.with(|m| {
            let w = m.borrow();
            assert_eq!(
                w.animate_calls,
                vec![(3, vec![u64::from(SID_GATE), u64::from(SID_CLOSE), 0xdead])]
            );
            assert_eq!(w.nav_on_calls, vec![(1, vec![u64::from(SID_GATE)])]);
        });
    }

    #[test]
    fn blocker_callback_holds_nav_on_while_gate_close_is_held() {
        let st = Mutex::new(State {
            hold: true,
            ..State::default()
        });
        let mut argv = [u64::from(SID_GATE), 0];
        unsafe { on_blocker_with(&st, 0, 0x1000, 1, argv.as_mut_ptr() as u64, true) };
        assert_eq!(argv[0], u64::from(SID_HELD));
        assert!(!st.lock().unwrap().gate_closed);

        // When closing == true (frame hook replay), nav_on is allowed through.
        st.lock().unwrap().closing = true;
        let mut replay_argv = [u64::from(SID_GATE), 0];
        unsafe { on_blocker_with(&st, 0, 0x1000, 1, replay_argv.as_mut_ptr() as u64, true) };
        assert_eq!(replay_argv[0], u64::from(SID_GATE));
        assert!(st.lock().unwrap().gate_closed);
        st.lock().unwrap().closing = false;

        // Opening the gate drops the hold.
        st.lock().unwrap().hold = true;
        let mut open_argv = [u64::from(SID_GATE), 0];
        unsafe { on_blocker_with(&st, 0, 0x1000, 1, open_argv.as_mut_ptr() as u64, false) };
        let guard = st.lock().unwrap();
        assert!(!guard.hold);
        assert!(!guard.gate_closed);
    }

    #[test]
    fn sniper_spawn_relocates_to_bridge_post_when_gate_closed_and_restores_on_open() {
        reset_mock();
        let same_frame = [0x33; 16];
        let other_frame = [0x44; 16];
        let orig_s6 = [10.0, 20.0, 30.0];
        let orig_s7 = [12.0, 20.0, 32.0];
        let post_pos = [-183.0, 20.1, -449.0];

        MOCK.with(|m| {
            let mut w = m.borrow_mut();
            w.entities.insert(
                SID_POST,
                FakeEntity::new(post_pos, post_pos, false, 1, same_frame),
            );
            w.entities.insert(
                SID_SNIPER_6,
                FakeEntity::new(orig_s6, orig_s6, true, 2, same_frame),
            );
            w.entities.insert(
                SID_SNIPER_7,
                FakeEntity::new(orig_s7, orig_s7, true, 3, other_frame),
            );
        });

        let st = Mutex::new(State {
            gate_closed: true,
            hold: true,
            ..State::default()
        });
        let ops = mock_ops(0);

        // sniper-1 spawn clears any stale hold (fight restart).
        let mut s1_argv = [u64::from(SID_SNIPER_1)];
        unsafe { on_spawn_with(&st, &ops, 1, s1_argv.as_mut_ptr() as u64, true) };
        assert!(!st.lock().unwrap().hold);

        // sniper-7 in a different level frame is NOT moved.
        let mut s7_argv = [u64::from(SID_SNIPER_7)];
        unsafe { on_spawn_with(&st, &ops, 1, s7_argv.as_mut_ptr() as u64, true) };
        MOCK.with(|m| assert_eq!(m.borrow().entities[&SID_SNIPER_7].spawner_pos(), orig_s7));

        // sniper-6 in the same level frame with gate closed IS moved to zone-sniper-6-1.
        let mut s6_argv = [u64::from(SID_SNIPER_6)];
        unsafe { on_spawn_with(&st, &ops, 1, s6_argv.as_mut_ptr() as u64, true) };
        MOCK.with(|m| assert_eq!(m.borrow().entities[&SID_SNIPER_6].spawner_pos(), post_pos));

        // Gate opens (checkpoint restart): next sniper-6 spawn restores original position.
        st.lock().unwrap().gate_closed = false;
        unsafe { on_spawn_with(&st, &ops, 1, s6_argv.as_mut_ptr() as u64, true) };
        MOCK.with(|m| assert_eq!(m.borrow().entities[&SID_SNIPER_6].spawner_pos(), orig_s6));
    }

    #[test]
    fn guest_callback_table_has_exact_u2_sniper_patterns() {
        let table = super::super::hooks::table();
        let expected = [
            (
                "U2 monastery sniper: spawn (1.02)",
                "55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 ec 38 01 00 00 4c 8b 2d 35 54 d6 00",
            ),
            (
                "U2 monastery sniper: spawn (1.00)",
                "55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 ec 38 01 00 00 4c 8b 2d 8d ea dd 00",
            ),
            (
                "U2 monastery sniper: gate closes (1.02)",
                "55 48 89 e5 41 56 53 85 ff 7e 43 8b 1e 31 f6 89 df e8 2a e4 2d 00",
            ),
            (
                "U2 monastery sniper: gate opens (1.02)",
                "55 48 89 e5 41 56 53 85 ff 7e 57 8b 1e 31 f6 89 df e8 9a e4 2d 00",
            ),
            (
                "U2 monastery sniper: gate closes (1.00)",
                "55 48 89 e5 41 56 53 85 ff 7e 46 8b 1e 89 df e8 7c 8d 23 00",
            ),
            (
                "U2 monastery sniper: gate opens (1.00)",
                "55 48 89 e5 41 56 53 85 ff 7e 43 8b 1e 89 df e8 1c 8d 23 00",
            ),
            (
                "U2 monastery sniper: gate animate (1.02)",
                "55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 ec 68 01 00 00 49 89 f7 4c",
            ),
            (
                "U2 monastery sniper: gate animate (1.00)",
                "55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 ec 68 01 00 00 41 89 d4 49",
            ),
        ];
        for (name, pat) in expected {
            let found = table
                .iter()
                .map(|cb| cb.parts())
                .find(|(n, _, _, _, _, _)| n == name)
                .unwrap_or_else(|| panic!("missing callback {name}"));
            assert_eq!(found.1, pat, "pattern mismatch for {name}");
            assert!(found.4, "callback fn missing for {name}");
        }
    }
}
