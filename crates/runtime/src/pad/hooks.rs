//! The guest callback table of `src/runtime_pad.c` (`runtime_guest_callbacks`): host functions
//! the loader runs at the start of matching guest functions (`probe.c`), and the byte patches
//! the same table carries. The guest code is found by `pattern`, not by a fixed address, so
//! every build of a game that contains it is covered: hex bytes separated by spaces, "??" for a
//! byte that differs between builds (RIP-relative displacements, call targets). The pattern
//! must match exactly once in the executable segments; the hook sits `skip` bytes into the
//! match, and `moved` bytes of whole position-independent instructions there move to the
//! thunk. Entries with the same name are variants (one per build of the code, or one per copy
//! of the code in a build): each one that matches is installed, in table order, and a site is
//! hooked once. A callback gets the guest's rdi, rsi and r14 there, the image base, and the
//! registers the thunk saved. Without a callback, `patch` (`moved` bytes) replaces the code at
//! the hook instead (a fix in the game's code).
//!
//! The callbacks read and write the game's own memory (its camera objects, joypad state and
//! music manager) at offsets measured in each build; every access is unsafe for that reason
//! and only reached from a pattern that matched the code that owns the object.

use super::camera::*;
use super::{camera, now_us};
use std::ffi::{c_char, CStr};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::OnceLock;

/// Set by `probe.c` when one of the "(raw mouse)" hooks (US 1.02 only) is installed. The raw
/// follow camera needs it too: U1's follow camera pattern matches 1.00 and 1.02 alike.
#[no_mangle]
pub static runtime_raw_mouse_hooks: AtomicI32 = AtomicI32::new(0);

/// The registers the thunk saved (`RuntimeGuestRegs`); the return address follows them.
#[repr(C)]
pub struct GuestRegs {
    r15: u64,
    r13: u64,
    r12: u64,
    rbx: u64,
    r11: u64,
    r10: u64,
    r9: u64,
    r8: u64,
    rdx: u64,
    rcx: u64,
    rax: u64,
    rflags: u64,
}

type Callback = unsafe extern "win64" fn(u64, u64, u64, u64, *const GuestRegs);

/// `RuntimeGuestCallback`.
#[repr(C)]
pub struct GuestCallback {
    name: *const c_char,
    pattern: *const c_char,
    skip: usize,
    moved: usize,
    callback: Option<Callback>,
    patch: *const u8,
}

// SAFETY: the pointers are to static strings and byte arrays.
unsafe impl Sync for GuestCallback {}

const fn hook(
    name: &'static CStr,
    pattern: &'static CStr,
    skip: usize,
    moved: usize,
    callback: Callback,
) -> GuestCallback {
    GuestCallback {
        name: name.as_ptr(),
        pattern: pattern.as_ptr(),
        skip,
        moved,
        callback: Some(callback),
        patch: std::ptr::null(),
    }
}

const fn fix(
    name: &'static CStr,
    pattern: &'static CStr,
    skip: usize,
    moved: usize,
    patch: &'static [u8],
) -> GuestCallback {
    GuestCallback {
        name: name.as_ptr(),
        pattern: pattern.as_ptr(),
        skip,
        moved,
        callback: None,
        patch: patch.as_ptr(),
    }
}

fn raw_wanted(c: &Camera) -> bool {
    c.input.mouse_raw && runtime_raw_mouse_hooks.load(Ordering::Relaxed) != 0
}

fn locked() -> std::sync::MutexGuard<'static, Camera> {
    camera()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// # Safety
/// `address` is a guest object with three floats there.
unsafe fn floats<'a>(address: u64) -> &'a mut [f32; 3] {
    // SAFETY: (caller) a live, aligned guest object.
    unsafe { &mut *(address as *mut [f32; 3]) }
}

fn follow(
    camera_object: u64,
    dir: u64,
    arc: u64,
    table: &ArcTable,
    raw: Option<f32>,
    raw_max: f32,
) {
    let now = now_us();
    let mut c = locked();
    let raw = raw.is_some() && raw_wanted(&c);
    // SAFETY: the hooked function's camera object: its direction and arc live at these offsets.
    let (dir, arc) = unsafe {
        (
            floats(camera_object + dir),
            (arc != 0).then(|| &mut *((camera_object + arc) as *mut f32)),
        )
    };
    c.follow_camera(now, camera_object, dir, arc, table, raw, raw_max);
}

/// A follow camera that never takes the raw path.
const NO_RAW: Option<f32> = None;
const RAW: Option<f32> = Some(0.0);

unsafe extern "win64" fn u1_follow_camera(
    camera: u64,
    _: u64,
    _: u64,
    _: u64,
    _: *const GuestRegs,
) {
    follow(camera, 0x1310, 0x12bc, &U1_ARC, RAW, 1.0);
}
unsafe extern "win64" fn u2_follow_camera_100(
    camera: u64,
    _: u64,
    _: u64,
    _: u64,
    _: *const GuestRegs,
) {
    follow(camera, 0x9ec0, 0x9e60, &U2_ARC, NO_RAW, 1.0); // build 100: never raw
}
unsafe extern "win64" fn u2_follow_camera(
    camera: u64,
    _: u64,
    _: u64,
    _: u64,
    _: *const GuestRegs,
) {
    follow(camera, 0x9ec0, 0x9e60, &U2_ARC, RAW, 0.75);
}
unsafe extern "win64" fn u3_follow_camera(
    camera: u64,
    _: u64,
    _: u64,
    _: u64,
    _: *const GuestRegs,
) {
    follow(camera, 0x78c0, 0x7874, &U3_ARC, NO_RAW, 1.0);
}

/// U1 and U2: rsi = the camera; the limits come from its settings (+0x8 up, +0xc down).
fn cover(camera_object: u64, dir: u64, pitch: u64, settings: u64) {
    let now = now_us();
    let mut c = locked();
    if !c.input.mouse_raw {
        return;
    }
    // SAFETY: the cover camera object: direction, pitch and a pointer to its settings.
    unsafe {
        let settings = ((camera_object + settings) as *const *const f32).read_unaligned();
        let limits = (!settings.is_null()).then(|| {
            [
                settings.add(2).read_unaligned(),
                settings.add(3).read_unaligned(),
            ]
        });
        c.cover_camera(
            now,
            camera_object,
            floats(camera_object + dir),
            &mut *((camera_object + pitch) as *mut f32),
            limits,
        );
    }
}
unsafe extern "win64" fn u1_cover_camera(_: u64, camera: u64, _: u64, _: u64, _: *const GuestRegs) {
    cover(camera, 0x240, 0x230, 0x1300);
}
unsafe extern "win64" fn u2_cover_camera(_: u64, camera: u64, _: u64, _: u64, _: *const GuestRegs) {
    cover(camera, 0x1c0, 0x1b0, 0x1290);
}

/// Raw mouse, aiming (U1 and U2, US 1.02). The aim controller (U1 0x276a10, U2 0x71a1b0;
/// rsi = the controller) gets the aiming camera's remembered aim direction as its 4th
/// argument (rcx). The hook sits at the function's first instruction, so the return address
/// lies right above the saved registers.
///
/// # Safety
/// `regs` is the thunk's frame.
unsafe fn raw_aim(
    game: &str,
    controller: u64,
    regs: *const GuestRegs,
    image: u64,
    main_return: u64,
) {
    let now = now_us();
    let mut c = locked();
    // SAFETY: the thunk's frame, with the return address after the registers.
    let (ret, dir) = unsafe {
        (
            regs.add(1)
                .cast::<u64>()
                .read_unaligned()
                .wrapping_sub(image),
            (*regs).rcx,
        )
    };
    // SAFETY: rcx is the remembered aim direction when the call is the camera's own.
    c.raw_aim(now, game, controller, ret, main_return, dir, || unsafe {
        floats(dir)
    });
}
unsafe extern "win64" fn u1_aim_controller(
    _: u64,
    controller: u64,
    image: u64,
    _: u64,
    regs: *const GuestRegs,
) {
    // The aiming camera's update (0xc43b0) calls it at 0xc4c52.
    // SAFETY: the thunk's frame.
    unsafe { raw_aim("U1", controller, regs, image, 0xc4c57) };
}
unsafe extern "win64" fn u2_aim_controller(
    _: u64,
    controller: u64,
    image: u64,
    _: u64,
    regs: *const GuestRegs,
) {
    // The aiming camera calls it at 0x45124b.
    // SAFETY: the thunk's frame.
    unsafe { raw_aim("U2", controller, regs, image, 0x451250) };
}

/// U1: the game's pad update (image+0x117200) fills the caller's joypad state (its first
/// argument): the sticks as floats lx ly rx ry at +0 and again at +0x10, converted from the
/// pad's bytes. Cameras other than the follow camera get the mouse as a right stick value
/// there, not limited to the stick's range.
unsafe extern "win64" fn u1_pad_update(
    state: u64,
    _: u64,
    image: u64,
    _: u64,
    _: *const GuestRegs,
) {
    let mut c = locked();
    if c.knobs.debug && c.joypad_state != state {
        super::note(&format!(
            "Mouse: joypad state {state:#x} (image {image:#x}, offset {:#x})",
            state.wrapping_sub(image)
        ));
    }
    c.joypad_state = state;
}

fn sticks(state: u64, aim: &AimCurve) {
    let now = now_us();
    let mut c = locked();
    // SAFETY: the game's float joypad state: eight floats at +0 (sticks twice).
    let pad = unsafe { std::slice::from_raw_parts_mut(state as *mut f32, 8) };
    c.mouse_sticks(now, pad, aim);
}

fn pad_zero(r14: u64, aim: &AimCurve) {
    let state = locked().joypad_state;
    if state == 0 || r14.wrapping_sub(0x62) != state {
        return; // pad 0 only
    }
    sticks(state, aim);
}
unsafe extern "win64" fn u1_joypad_sticks(_: u64, _: u64, _: u64, r14: u64, _: *const GuestRegs) {
    pad_zero(r14, &U1_100_AIM);
}
unsafe extern "win64" fn u1_joypad_sticks_102(
    _: u64,
    _: u64,
    _: u64,
    r14: u64,
    _: *const GuestRegs,
) {
    pad_zero(r14, &U1_102_AIM);
}

/// U2 and U3 fill the same float state (an array of pads in U3: pad 0 first, a pad every 0x64
/// bytes), in code of their own; the hooks sit after the stick values are stored, in each
/// build's registers.
unsafe extern "win64" fn u2_sticks_r14(_: u64, _: u64, _: u64, r14: u64, _: *const GuestRegs) {
    sticks(r14, &U23_100_AIM);
}
unsafe extern "win64" fn u2_sticks_r12(_: u64, _: u64, _: u64, _: u64, regs: *const GuestRegs) {
    // SAFETY: the thunk's frame.
    sticks(unsafe { (*regs).r12 }, &U23_102_AIM);
}
unsafe extern "win64" fn u3_sticks_r13(_: u64, _: u64, _: u64, _: u64, regs: *const GuestRegs) {
    // SAFETY: the thunk's frame.
    sticks(unsafe { (*regs).r13 }, &U23_100_AIM);
}
unsafe extern "win64" fn u3_sticks_pair_r13(
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    regs: *const GuestRegs,
) {
    // SAFETY: the thunk's frame.
    let regs = unsafe { &*regs };
    if regs.rcx == 0x64 {
        sticks(regs.r13, &U23_100_AIM); // the pad at r13 + rcx - 0x64
    }
}
unsafe extern "win64" fn u3_sticks_r12_rdx(_: u64, _: u64, _: u64, _: u64, regs: *const GuestRegs) {
    // SAFETY: the thunk's frame.
    let regs = unsafe { &*regs };
    if regs.rdx == 0 {
        sticks(regs.r12, &U23_102_AIM); // the pad at r12 + rdx
    }
}
unsafe extern "win64" fn u3_sticks_pair_r12(
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    regs: *const GuestRegs,
) {
    // SAFETY: the thunk's frame.
    let regs = unsafe { &*regs };
    if regs.rdx == 0u64.wrapping_sub(0x64) {
        sticks(regs.r12, &U23_102_AIM); // the pad at r12 + rdx + 0x64
    }
}

/// Concatenates byte runs into one array of exactly `N` bytes.
const fn join<const N: usize>(parts: &[&[u8]]) -> [u8; N] {
    let mut out = [0u8; N];
    let mut n = 0;
    let mut i = 0;
    while i < parts.len() {
        let part = parts[i];
        let mut j = 0;
        while j < part.len() {
            out[n] = part[j];
            n += 1;
            j += 1;
        }
        i += 1;
    }
    assert!(n == N, "patch size");
    out
}

const CC8: [u8; 8] = [0xcc; 8];

/// U2 UpdateFocus (player-base.cpp:1101): the camera's look-at spring went NaN once at 240 Hz
/// and the game's assertion ended it. The assertion's call now restarts the spring at the
/// player's target ([rbp-0x60]): position (+0x2b0, y +0x2a4) and velocities (+0x2c0, +0x2a8); a
/// NaN target still stops at the trap.
const U2_FOCUS_RESET: [u8; 56] = [
    0xc5, 0xf8, 0x28, 0x45, 0xa0, // vmovaps xmm0, [rbp-0x60] (the target)
    0x0f, 0x2e, 0xc0, 0x7a, 0x2c, // ucomiss xmm0, xmm0; jp trap
    0xc5, 0xf8, 0x29, 0x83, 0xb0, 0x02, 0x00, 0x00, // vmovaps [rbx+0x2b0], xmm0
    0xc4, 0xe3, 0x79, 0x17, 0x83, 0xa4, 0x02, 0x00, 0x00,
    0x01, // vextractps [rbx+0x2a4], xmm0, 1
    0xc5, 0xf8, 0x29, 0x45, 0xd0, // vmovaps [rbp-0x30], xmm0
    0xc5, 0xf0, 0x57, 0xc9, // vxorps xmm1, xmm1, xmm1
    0xc5, 0xf8, 0x29, 0x8b, 0xc0, 0x02, 0x00, 0x00, // vmovaps [rbx+0x2c0], xmm1
    0x83, 0xa3, 0xa8, 0x02, 0x00, 0x00, 0x00, // and dword [rbx+0x2a8], 0
    0xeb, 0x02, // jmp (store the focus)
    0xcd, 0x41, // trap: int 0x41
];
/// 58 bytes: a nop first.
static U2_FOCUS_RESET_102: [u8; 58] = join(&[&[0x66, 0x90], &U2_FOCUS_RESET]);

/// U2 game/character-leg-ik.cpp:391, assertion IsFinite(deltaTransform): the root delta the leg
/// IK gets from its caller can be NaN for a frame (train wreck, chapter 1: issues #88, #253)
/// and the game stopped. The leg IK only adds its y to two accumulators, so a non-finite delta
/// now counts as no movement: it is zeroed (also in the caller's vector at [r15]) and the
/// update goes on. Build 100 (US and EU 1.00) calls the assertion handler (5 bytes), build 102
/// loads it (7 bytes).
const fn leg_ik(jump: u8) -> [u8; 11] {
    [
        0xc5, 0xf8, 0x57, 0xc0, // vxorps xmm0, xmm0, xmm0
        0xc4, 0xc1, 0x78, 0x29, 0x07, // vmovaps [r15], xmm0
        0xeb, jump, // jmp (the update)
    ]
}
static U2_LEG_IK_ZERO_100: [u8; 51] = join(&[&leg_ik(0x2d), &CC8, &CC8, &CC8, &CC8, &CC8]);
static U2_LEG_IK_ZERO_102: [u8; 53] =
    join(&[&leg_ik(0x2f), &CC8, &CC8, &CC8, &CC8, &CC8, &[0xcc, 0xcc]]);

/// U1 and U2 "pAction != NULL" (moveactioncontroller.cpp:40), from Mevlock's dev fork: the
/// player's move actions come from a pool of 64 slots. Two move states swapping every frame
/// (landing into a fight, often right after a jump) keep one action each swap; at 144 FPS
/// bursts took 16-30 slots, and a longer one filled the pool and stopped the game. The slot
/// count passed where the character is set up becomes 256 (+144 KB from the character's heap).
/// One site in U1 and one in U2, in every build; U3 has no such pool.
static MOVE_POOL_256: [u8; 5] = [0xb9, 0x00, 0x01, 0x00, 0x00]; // mov ecx, 256

/// U3 Player::UpdateFocus (player.cpp:1897, assertion m_lookAt.X() == m_lookAt.X(): the crash
/// climbing the fort in chapter 8). The point Nate looks at ([player+0xef0], velocity +0xf00)
/// springs towards a target; once NaN it stays NaN, and the function's own reset (taken when
/// the point is far off) never runs for it: NaN compares false. The assertion's path now takes
/// that reset when the target is a number (the velocity register zeroed first, as on the
/// reset's usual way in); a NaN target still stops the game. BilalReloaded's fix for 1.02 (his
/// Uncharted port, GPL v2, GitHub #13 there), and the same for 1.00 (reset at 0x390779, target
/// at rbp-0xa0, zero in xmm4).
static U3_FOCUS_RESET_100: [u8; 51] = join(&[
    &[0xc5, 0xd8, 0x57, 0xe4], // vxorps xmm4, xmm4, xmm4
    &[0xc5, 0xf8, 0x28, 0x85, 0x60, 0xff, 0xff, 0xff], // vmovaps xmm0, [rbp-0xa0] (the target)
    &[0xc5, 0xf8, 0xc2, 0xc8, 0x00], // vcmpeqps xmm1, xmm0, xmm0
    &[0xc5, 0xf8, 0x50, 0xc1], // vmovmskps eax, xmm1
    &[0x83, 0xf8, 0x0f],       // cmp eax, 15
    &[0x0f, 0x84, 0xa3, 0xfe, 0xff, 0xff], // je the reset (0x390779)
    &[0xcd, 0x41],             // the target is NaN too: trap as before
    &CC8,
    &CC8,
    &[0xcc, 0xcc, 0xcc],
]);
static U3_FOCUS_RESET_102: [u8; 53] = join(&[
    &[
        0xc5, 0xd0, 0x57, 0xed, 0xc5, 0xf8, 0x28, 0x45, 0x80, 0xc5, 0xf8, 0xc2, 0xc8, 0x00,
    ],
    &[
        0xc5, 0xf8, 0x50, 0xc1, 0x83, 0xf8, 0x0f, 0x0f, 0x84, 0xa5, 0xfe, 0xff, 0xff, 0xcd, 0x41,
    ],
    &CC8,
    &CC8,
    &CC8,
]);

#[cfg(uncharted_loader)]
mod fiber {
    extern "C" {
        /// `runtime_fiber.c`: sets this thread's `runtime_ndgi_open`.
        pub fn runtime_ndgi_set(open: i32);
    }
}

/// Render command contexts (ndgi), from Mevlock's dev fork: Begin and End keep a per-thread
/// flag, set while a job worker has a context open (`runtime_ndgi_open`, cleared too when a
/// fiber leaves the thread, `runtime_fiber.c`), used by the job wait patch in `probe.c`. A
/// context records its worker at Begin (+0xe810) and asserts the same one at End. One site each
/// in all three games.
fn ndgi_open(open: bool) {
    #[cfg(uncharted_loader)]
    // SAFETY: sets a thread-local flag in the C runtime.
    unsafe {
        fiber::runtime_ndgi_set(i32::from(open))
    };
    #[cfg(not(uncharted_loader))]
    let _ = open;
}
unsafe extern "win64" fn ndgi_begin(_: u64, _: u64, _: u64, _: u64, _: *const GuestRegs) {
    ndgi_open(true);
}
unsafe extern "win64" fn ndgi_end(_: u64, _: u64, _: u64, _: u64, _: *const GuestRegs) {
    ndgi_open(false);
}

/// U2 Chapter Select weapons (issue #16, 2026-11-06): the menu's Chapter Select plays the
/// chapter's task group (PlayTask, mode 1). PlayTask first resets the player's settings to the
/// default set (459d0e4e: only the .45 Defender, 40 rounds) and then applies the weapon set of
/// the task it played or its parents; a group has none. The chapter's first checkpoint applies
/// its own set when it activates only if its tasks.bin flags carry 0x20, and the first
/// checkpoints of chapters 8 and 13 (war-temple-1-inside-temple, valley-start) do not: Nate
/// starts with the Defender, whose model those levels never load, so it is invisible and
/// silent. Here, after PlayTask(mode 1) of a task without data (a group), the first playable
/// task that activates gets its set as flag 0x20 would give it (when the set has a gun); once.
/// Continue (mode 2), checkpoint restarts and jumps to a checkpoint do not arm it. Chapters it
/// changes: 6-10, 13, 14, 18, 20, 23, 24, 26. `BB_CHAPTER_WEAPONS=0`: off.
static U2_CHAPTER_START: AtomicBool = AtomicBool::new(false);

fn u2_chapter_weapons_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BB_CHAPTER_WEAPONS").is_ok_and(|e| e.starts_with('0')))
}

static U2_APPLY_SETTINGS_EU: [u8; 24] = [
    0x55, 0x48, 0x89, 0xe5, 0x31, 0xf6, 0xe8, 0x25, 0x2c, 0xfd, 0xff, 0x48, 0x85, 0xc0, 0x74, 0x12,
    0x48, 0x8d, 0x3d, 0xf1, 0xd9, 0xb9, 0x00, 0xb9,
];
static U2_APPLY_SETTINGS_US: [u8; 24] = [
    0x55, 0x48, 0x89, 0xe5, 0xe8, 0x97, 0x4e, 0xfb, 0xff, 0x48, 0x85, 0xc0, 0x74, 0x17, 0x48, 0x8b,
    0x30, 0x48, 0x85, 0xf6, 0x74, 0x0f, 0x48, 0x8d,
];

unsafe extern "win64" fn u2_chapter_play_hook(
    task: u64,
    mode: u64,
    _: u64,
    _: u64,
    _: *const GuestRegs,
) {
    if !u2_chapter_weapons_on() || task < 0x10000 {
        return;
    }
    // SAFETY: a task object: its data pointer at +0x50.
    let group = unsafe { ((task + 0x50) as *const u64).read_unaligned() } == 0;
    U2_CHAPTER_START.store(mode as u32 == 1 && group, Ordering::SeqCst);
}

/// # Safety
/// `task` is the task the hooked function activates; `image + apply` the settings function.
unsafe fn u2_chapter_activate(task: u64, image: u64, apply: u64, code: &[u8]) {
    if !U2_CHAPTER_START.load(Ordering::SeqCst) || task < 0x10000 {
        return;
    }
    // SAFETY: the image is mapped; the function is compared before it is called.
    let there = unsafe { std::slice::from_raw_parts((image + apply) as *const u8, code.len()) };
    if there != code {
        U2_CHAPTER_START.store(false, Ordering::SeqCst);
        super::note(
            "Chapter weapons: the settings function is not where expected; nothing applied",
        );
        return;
    }
    let read32 = |address: u64| unsafe { (address as *const u32).read_unaligned() };
    // SAFETY: the task object and its data (+0x50, flags at +0xd0 there).
    let (data, flags, set) = unsafe {
        let data = ((task + 0x50) as *const u64).read_unaligned();
        if data == 0 || read32(data + 0xd0) & 6 == 0 {
            return; // not playable (a group): wait
        }
        (data, read32(task + 0x48), read32(data + 0xc8))
    };
    U2_CHAPTER_START.store(false, Ordering::SeqCst);
    if flags & 0x20 != 0 || set == 0 {
        return; // the game applies it itself
    }
    // Only a set with a gun: some first checkpoints name the empty set (65759d0e, the unarmed
    // openings'), where the Defender is better than nothing (chapter 12). The set: five slots
    // {name, weapon id +8, ammo +0x10} of 0x18 bytes; guns in the first three, f114b39d = none.
    // The lookup is the one the settings function calls (EU: (id, 0) -> set; 1.02: (id) -> &set).
    // SAFETY: the guest's lookup functions, called with the System V ABI at the addresses of the
    // two builds this runs for (the settings function matched above).
    let found: u64 = unsafe {
        if apply == 0x43b7e0 {
            let lookup: extern "sysv64" fn(u32, i32) -> u64 =
                std::mem::transmute((image + 0x40e410) as usize);
            lookup(set, 0)
        } else {
            let lookup: extern "sysv64" fn(u32) -> u64 =
                std::mem::transmute((image + 0xa15300) as usize);
            let pointer = lookup(set);
            if pointer == 0 {
                0
            } else {
                (pointer as *const u64).read_unaligned()
            }
        }
    };
    let gun = found != 0
        && (0..3).any(|i| {
            // The found weapon set: five slots of 0x18 bytes.
            let weapon = read32(found + 8 + 0x18 * i);
            weapon != 0 && weapon != 0xf114_b39d
        });
    let id = read32(task + 8);
    let _ = data;
    if !gun {
        super::note(&format!(
            "Chapter weapons: Chapter Select start at task {id:08x}: its set {set:08x} has no gun; the game's default kept"
        ));
        return;
    }
    super::note(&format!(
        "Chapter weapons: Chapter Select start at task {id:08x}: its weapon set {set:08x} applied (the game leaves the default .45 Defender there)"
    ));
    // SAFETY: the settings function, checked above.
    unsafe {
        let apply_settings: extern "sysv64" fn(u32) = std::mem::transmute((image + apply) as usize);
        apply_settings(set);
    }
}
unsafe extern "win64" fn u2_chapter_activate_eu(
    task: u64,
    _: u64,
    image: u64,
    _: u64,
    _: *const GuestRegs,
) {
    // SAFETY: the hooked TaskActivate's task and the image.
    unsafe { u2_chapter_activate(task, image, 0x43b7e0, &U2_APPLY_SETTINGS_EU) };
}
unsafe extern "win64" fn u2_chapter_activate_us(
    task: u64,
    _: u64,
    image: u64,
    _: u64,
    _: *const GuestRegs,
) {
    // SAFETY: the hooked TaskActivate's task and the image.
    unsafe { u2_chapter_activate(task, image, 0xa60460, &U2_APPLY_SETTINGS_US) };
}

/// U3, London Underground (#245, from spacepepperoni/uncharted_Linux#81): the title-screen
/// music played on into the level after Continue at the chapter's low-intensity checkpoint. The
/// menu playlist survives level loads, loops, and has no tempo; the level queues its playlist
/// 0x0c3dd4f5 to switch at the current track's next musical boundary, which the music manager
/// finds with fmodf(position, beat length): with no tempo that is NaN, so the boundary never
/// comes and the menu theme plays on. Only for that pending playlist, and only while the
/// playing track has no tempo, its copy in the manager is set to switch at once. rbx is the
/// manager: +0x2f5 a playlist is pending, +0x224 its id, +0x22f its flags (4: wait for a
/// boundary), +0x180 the playing track's beat length. `BB_CALLBACKS_SKIP=menu` off.
unsafe extern "win64" fn u3_menu_music_into_level(
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    regs: *const GuestRegs,
) {
    static TOLD: AtomicBool = AtomicBool::new(false);
    // SAFETY: rbx is the music manager of the matched code.
    unsafe {
        let manager = (*regs).rbx as *mut u8;
        if manager.add(0x2f5).read() == 0 || manager.add(0x22f).read() & 4 == 0 {
            return;
        }
        let pending = (manager.add(0x224) as *const u32).read_unaligned();
        let beat = (manager.add(0x180) as *const f32).read_unaligned();
        if pending != 0x0c3d_d4f5 || beat != 0.0 {
            return;
        }
        manager.add(0x22f).write(manager.add(0x22f).read() & !4);
    }
    if !TOLD.swap(true, Ordering::Relaxed) {
        super::note("U3 music: the London Underground music replaces the title music (#245)");
    }
}

/// Entries with one name are variants for the builds. Joypad sticks entries: the first two
/// tell the 1.00 and 1.02 builds of U1 apart by the displacement of a constant (unmasked), the
/// last one is any other build (aimed like 1.02). Entries are tried in order, a site is hooked
/// once.
static GUEST_CALLBACKS: [GuestCallback; 36] = [
    // U1 follow camera, build 100
    hook(c"U1 follow camera (mouse)",
        c"55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 ec 28 01 00 00 \
          44 89 8d 00 ff ff ff 49 89 cd 48 89 d3 49 89 f7 49 89 fc 4c \
          8b 35 ?? ?? ?? ??",
        0x0, 6, u1_follow_camera),
    // U1 pad update, build 100
    hook(c"U1 pad update (mouse)",
        c"55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 ec b8 00 00 00 \
          49 89 fe 4c 8b 2d ?? ?? ?? ?? 49 8b 45 00 48 89 45 d0 4c 8d \
          3d ?? ?? ?? ?? 41 8b 7f 70 48 8d 75 b0 e8 ?? ?? ?? ?? 89 c1 \
          85 c9 74 15 48 8d 3d ?? ?? ?? ?? 31 c0 89 ce e8 ?? ?? ?? ?? \
          e9 ?? ?? ?? ?? 44 8b 65 b4 41 c6 47 6e 00 80 7d bc 00 0f 84 \
          ?? ?? ?? ?? 41 8b 7f 70 48 8d b5 38 ff ff ff ba 01 00 00 00 \
          e8 ?? ?? ?? ?? 89 c1 85 c9 0f 88 ?? ?? ?? ?? 41 c6 47 6e 01 \
          41 c6 47 6c 01 41 c6 47 6d 01 85 c9 0f 84 ?? ?? ?? ?? 45 8b \
          6f 60 41 8a 5f 5d 48 8b 85 38 ff ff ff 41 89 47 60 41 c6 47 \
          5d 00 89 c1 c1 e9 1f 48 8d 15 ?? ?? ?? ?? 88 0a 48 89 c1",
        0x0, 6, u1_pad_update),
    // U1 joypad sticks
    hook(c"U1 joypad sticks (mouse)",
        c"c5 f9 6f 05 f0 f1 68 00 c5 f8 28 0d ?? ?? ?? ?? c5 f8 28 15 \
          ?? ?? ?? ?? 48 ba 00 00 00 00 00 00 80 bf c5 fa 10 1d ?? ?? \
          ?? ?? 66 66 66 66 66 2e 0f 1f 84 00 00 00 00 00 80 7c 01 6c \
          00 0f 84 ?? ?? ?? ?? c4 e2 79 31 64 01 50 c5 d9 db e0 c5 f8 \
          5b e4 c5 d8 59 e1 c5 d8 58 e2 c4 c1 78 11 66 9e c4 c1 78 11 \
          66 ae 80 7c 01 6d 00 74 3b c5 e2 59 64 01 40 c4 c1 7a 11 66 \
          be",
        102, 5, u1_joypad_sticks),
    hook(c"U1 joypad sticks (mouse)",
        c"c5 f9 6f 05 70 68 68 00 c5 f8 28 0d ?? ?? ?? ?? c5 f8 28 15 \
          ?? ?? ?? ?? 48 ba 00 00 00 00 00 00 80 bf c5 fa 10 1d ?? ?? \
          ?? ?? 66 66 66 66 66 2e 0f 1f 84 00 00 00 00 00 80 7c 01 6c \
          00 0f 84 ?? ?? ?? ?? c4 e2 79 31 64 01 50 c5 d9 db e0 c5 f8 \
          5b e4 c5 d8 59 e1 c5 d8 58 e2 c4 c1 78 11 66 9e c4 c1 78 11 \
          66 ae 80 7c 01 6d 00 74 3b c5 e2 59 64 01 40 c4 c1 7a 11 66 \
          be",
        102, 5, u1_joypad_sticks_102),
    hook(c"U1 joypad sticks (mouse)",
        c"80 7c 01 6d 00 74 3b c5 e2 59 64 01 40 c4 c1 7a 11 66 be",
        0x0, 5, u1_joypad_sticks_102),
    // Raw mouse (mouse_raw=1), build 102: U1 aim controller and cover camera update
    hook(c"U1 aim controller (raw mouse)",
        c"55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 ec 08 03 00 00 \
          45 89 c4 48 89 8d 00 fd ff ff 49 89 d5 49 89 f7 48 89 bd 10 \
          fd ff ff 4c 8b 35 ?? ?? ?? ??",
        0x0, 6, u1_aim_controller),
    hook(c"U1 cover camera (raw mouse)",
        c"55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 ec 88 01 00 00 \
          49 89 d7 48 89 f3 49 89 fe 4c 8b 25 ?? ?? ?? ?? 49 8b 04 24 \
          48 89 45 d0 80 bb f4 01 00 00 00",
        0x0, 6, u1_cover_camera),
    // Raw mouse (mouse_raw=1), build 102: U2 aim controller and cover camera update
    hook(c"U2 aim controller (raw mouse)",
        c"55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 ec 18 01 00 00 \
          c5 fa 11 45 90 44 89 85 60 ff ff ff 49 89 ce 49 89 d5 49 89 \
          f7 48 89 fb 80 3d ?? ?? ?? ?? 00",
        0x0, 6, u2_aim_controller),
    hook(c"U2 cover camera (raw mouse)",
        c"55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 ec 68 01 00 00 \
          49 89 d7 48 89 f3 49 89 fe 4c 8b 2d ?? ?? ?? ?? 49 8b 45 00 \
          48 89 45 d0 8a 8b 74 01 00 00",
        0x0, 6, u2_cover_camera),
    // U2 follow camera, build 100
    hook(c"U2 follow camera (mouse)",
        c"55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 ec 48 01 00 00 \
          44 89 8d 18 ff ff ff 48 89 8d c8 fe ff ff 49 89 d5 4c 89 ad \
          38 ff ff ff 48 89 f3",
        0x0, 6, u2_follow_camera_100),
    // U2 follow camera, build 102
    hook(c"U2 follow camera (mouse)",
        c"55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 ec 28 01 00 00 \
          45 89 cc 48 89 8d f8 fe ff ff 49 89 d6 49 89 fd 48 8b 55 20 \
          4c 8b 7d 18 48 8b 45 28",
        0x0, 6, u2_follow_camera),
    // U3 follow camera, build 100
    hook(c"U3 follow camera (mouse)",
        c"55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 ec d8 01 00 00 \
          44 89 8d a0 fe ff ff 48 89 8d 48 fe ff ff 49 89 d5 48 89 f3 \
          49 89 ff 4c 8b 65 20",
        0x0, 6, u3_follow_camera),
    // U3 follow camera, build 102
    hook(c"U3 follow camera (mouse)",
        c"55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 ec b8 01 00 00 \
          45 89 cc 48 89 8d 68 fe ff ff 49 89 d6 49 89 fd 48 8b 55 20 \
          48 8b 45 28",
        0x0, 6, u3_follow_camera),
    // U2 joypad sticks, build 100
    hook(c"U2 joypad sticks (mouse)",
        c"c4 e2 79 31 40 50 c5 f9 db 05 ?? ?? ?? ?? c5 f8 5b c0 c5 f8 \
          5e 05 ?? ?? ?? ?? c5 f8 58 05 ?? ?? ?? ?? c4 c1 78 11 06 c4 \
          c1 78 11 46 10 80 78 6d 00 74 28 c5 fa 10 40 40 c4 c1 7a 11 \
          46 20 c5 fa 10 40 44 c4 c1 7a 11 46 24 c5 fa 10 40 48 c4 c1 \
          7a 11 46 28 c5 fa 10 40 4c eb 1a 48 b9 00 00 00 00 00 00 80 \
          bf 49 89 4e 20 41 c7 46 28 00 00 00 00 c5 f8 57 c0 c4 c1 7a \
          11 46 2c",
        0x75, 6, u2_sticks_r14),
    // U2 joypad sticks, build 102
    hook(c"U2 joypad sticks (mouse)",
        c"c4 c1 7a 11 74 24 08 c5 d2 59 db c4 c1 7a 11 5c 24 0c c4 c1 \
          7a 11 74 24 18 c4 c1 7a 11 5c 24 1c 80 7c 08 6d 00 74 36 c5 \
          fa 10 5c 08 40 c4 c1 7a 11 5c 24 20",
        0x20, 5, u2_sticks_r12),
    // U2 joypad sticks, build 102: the copy for one pad (the code above is the copy for
    // several pads, which the game does not use)
    hook(c"U2 joypad sticks (mouse)",
        c"c4 c1 7a 11 2c 24 c5 e2 59 de c4 c1 7a 11 5c 24 04 c4 c1 7a \
          11 6c 24 10 c4 c1 7a 11 5c 24 14 80 7c 08 6d 00 74 36 c5 fa \
          10 5c 08 40 c4 c1 7a 11 5c 24 20",
        0x1f, 5, u2_sticks_r12),
    // U3 joypad sticks, build 100
    hook(c"U3 joypad sticks (mouse)",
        c"c4 e2 79 31 40 50 c5 f9 db 05 ?? ?? ?? ?? c5 f8 5b c0 c5 f8 \
          5e 05 ?? ?? ?? ?? c5 f8 58 05 ?? ?? ?? ?? c4 c1 78 11 45 00 \
          c4 c1 78 11 45 10 80 78 75 00 74 28 c5 fa 10 40 40 c4 c1 7a \
          11 45 20 c5 fa 10 40 44 c4 c1 7a 11 45 24 c5 fa 10 40 48 c4 \
          c1 7a 11 45 28 c5 fa 10 40 4c eb 1a 48 b9 00 00 00 00 00 00 \
          80 bf 49 89 4d 20 41 c7 45 28 00 00 00 00 c5 f8 57 c0 c4 c1 \
          7a 11 45 2c",
        0x76, 6, u3_sticks_r13),
    // U3 joypad sticks, build 102
    hook(c"U3 joypad sticks (mouse)",
        c"80 79 55 00 74 33 c5 fa 10 41 20 c4 c1 7a 11 44 14 20 c5 fa \
          10 41 24 c4 c1 7a 11 44 14 24 c5 fa 10 41 28 c4 c1 7a 11 44 \
          14 28 c5 fa 10 71 2c eb 1a 0f 1f 84 00 00 00 00 00 4d 89 44 \
          14 20 41 c7 44 14 28 00 00 00 00 c5 c8 57 f6 c4 c1 7a 11 74 \
          14 2c",
        0x4b, 7, u3_sticks_r12_rdx),
    // U3 joypad sticks, two pads, build 100
    hook(c"U3 joypad sticks, two pads (mouse)",
        c"c4 e2 79 31 58 30 c5 e1 db d8 c5 f8 5b db c5 e0 5e d9 c5 e0 \
          58 da c4 c1 78 11 5c 0d 9c c4 c1 78 11 5c 0d ac 80 78 55 00 \
          74 2c c5 fa 10 58 20 c4 c1 7a 11 5c 0d bc c5 fa 10 58 24 c4 \
          c1 7a 11 5c 0d c0 c5 fa 10 58 28 c4 c1 7a 11 5c 0d c4 c5 fa \
          10 58 2c eb 13 90 49 89 54 0d bc 41 c7 44 0d c4 00 00 00 00 \
          c5 e0 57 db c4 c1 7a 11 5c 0d c8",
        0x68, 7, u3_sticks_pair_r13),
    // U3 joypad sticks, two pads, build 102
    hook(c"U3 joypad sticks, two pads (mouse)",
        c"80 79 55 00 74 3e c5 fa 10 41 20 c4 c1 7a 11 84 14 84 00 00 \
          00 c5 fa 10 41 24 c4 c1 7a 11 84 14 88 00 00 00 c5 fa 10 41 \
          28 c4 c1 7a 11 84 14 8c 00 00 00 c5 fa 10 71 2c eb 22 66 2e \
          0f 1f 84 00 00 00 00 00 4d 89 84 14 84 00 00 00 41 c7 84 14 \
          8c 00 00 00 00 00 00 00 c5 c8 57 f6 c4 c1 7a 11 b4 14 90 00 \
          00 00",
        0x5c, 10, u3_sticks_pair_r12),
    // U2 leg IK NaN delta, build 100
    fix(c"U2 leg IK NaN delta",
        c"e8 ?? ?? ?? ?? 48 85 c0 74 27 48 8d 3d ?? ?? ?? ?? 48 8d 35 \
          ?? ?? ?? ?? 48 8d 15 ?? ?? ?? ?? 48 8d 0d ?? ?? ?? ?? 41 b8 \
          87 01 00 00 45 31 c9 ff d0 cd 41 c4 c1 78 28 07 c4 e3 79 04 c0 01",
        0x0, 51, &U2_LEG_IK_ZERO_100),
    // U2 leg IK NaN delta, build 102
    fix(c"U2 leg IK NaN delta",
        c"48 8b 05 ?? ?? ?? ?? 48 85 c0 74 27 48 8d 3d ?? ?? ?? ?? 48 \
          8d 35 ?? ?? ?? ?? 48 8d 15 ?? ?? ?? ?? 48 8d 0d ?? ?? ?? ?? \
          41 b8 87 01 00 00 45 31 c9 ff d0 cd 41 c4 c1 78 28 07 c4 e3 79 04 c0 01",
        0x0, 53, &U2_LEG_IK_ZERO_102),
    // U1 and U2 move action pool: mov edx,6; mov ecx,64 (slots); mov r8d,32
    fix(c"U1/U2 move action pool 256 slots",
        c"ba 06 00 00 00 b9 40 00 00 00 41 b8 20 00 00 00",
        0x5, 5, &MOVE_POOL_256),
    // U3 player focus NaN reset, build 100
    fix(c"U3 player focus NaN reset",
        c"c4 e3 79 04 cb 00 c5 f0 c2 c9 00 c5 f8 50 c1 83 f8 0f 74 38 \
          e8 ?? ?? ?? ?? 48 85 c0 74 27 48 8d 3d ?? ?? ?? ?? 48 8d 35 \
          ?? ?? ?? ?? 48 8d 15 ?? ?? ?? ?? 48 8d 0d ?? ?? ?? ?? 41 b8 \
          69 07 00 00 45 31 c9 ff d0 cd 41 c5 f8 28 45 c0",
        0x14, 51, &U3_FOCUS_RESET_100),
    // U3 player focus NaN reset, build 102 (BilalReloaded)
    fix(c"U3 player focus NaN reset",
        c"c4 e3 79 04 cb 00 c5 f0 c2 c9 00 c5 f8 50 c1 83 f8 0f 74 3a \
          48 8b 05 ?? ?? ?? ?? 48 85 c0 74 27 48 8d 3d ?? ?? ?? ?? 48 \
          8d 35 ?? ?? ?? ?? 48 8d 15 ?? ?? ?? ?? 48 8d 0d ?? ?? ?? ?? \
          41 b8 6a 07 00 00 45 31 c9 ff d0 cd 41",
        0x14, 53, &U3_FOCUS_RESET_102),
    // U2 camera focus NaN reset, build 100
    fix(c"U2 camera focus NaN reset",
        c"e8 ?? ?? ?? ?? 48 85 c0 74 27 48 8d 3d ?? ?? ?? ?? 48 8d 35 \
          ?? ?? ?? ?? 48 8d 15 ?? ?? ?? ?? 48 8d 0d ?? ?? ?? ?? 41 b8 \
          4d 04 00 00 45 31 c9 ff d0 cd 41 c5 f8 28 45 d0",
        0x0, 56, &U2_FOCUS_RESET),
    // U2 camera focus NaN reset, build 102
    fix(c"U2 camera focus NaN reset",
        c"48 8b 05 ?? ?? ?? ?? 48 85 c0 74 27 48 8d 3d ?? ?? ?? ?? 48 \
          8d 35 ?? ?? ?? ?? 48 8d 15 ?? ?? ?? ?? 48 8d 0d ?? ?? ?? ?? \
          41 b8 4d 04 00 00 45 31 c9 ff d0 cd 41 c5 f8 28 45 d0",
        0x0, 58, &U2_FOCUS_RESET_102),
    // U3 title music into the London Underground level (#245), build 102: cmp byte
    // [rbx+0x2f5],0; je; test byte [rbx+0x22f],4; sete al; or al,r15b; jne (NdMusicManager
    // update, pending playlist switch)
    hook(c"U3 menu music into London Underground",
        c"80 bb f5 02 00 00 00 74 61 f6 83 2f 02 00 00 04 0f 94 c0 44 08 f8 75 0a",
        0x0, 7, u3_menu_music_into_level),
    // Render context Begin, build 100: mov [r12+0xe810], rax
    hook(c"Job render context begin", c"49 89 84 24 10 e8 00 00", 0x0, 8, ndgi_begin),
    // build 102: mov [r15+0xe810], r12
    hook(c"Job render context begin", c"4d 89 a7 10 e8 00 00", 0x0, 7, ndgi_begin),
    // Render context End, build 100: mov rbx,[r14+0xe810]; call GetCurrentWorkerThreadIndex;
    // cmp rbx,rax; je
    hook(c"Job render context end", c"49 8b 9e 10 e8 00 00 e8 ?? ?? ?? ?? 48 39 c3 74", 0x0, 7, ndgi_end),
    // build 102: cmp byte [r15+0xe7dc],0; je; mov r12,[r15+0xe810] (the index call is inlined)
    hook(c"Job render context end",
        c"41 80 bf dc e7 00 00 00 0f 84 ?? ?? ?? ?? 4d 8b a7 10 e8 00 00",
        14, 7, ndgi_end),
    // U2 Chapter Select weapons (issue #16, Mevlock): PlayTask and TaskActivate by their first
    // 40 bytes (EU 1.00, 1.02); the settings function is checked before it is called.
    hook(c"U2 Chapter Select weapons: PlayTask",
        c"55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 e4 e0 ff ff ff 48 81 ec 60 01 00 00 89 f3 49 89 fd 4c 8b 3d 11 c0 a2 00 49",
        0x0, 6, u2_chapter_play_hook),
    hook(c"U2 Chapter Select weapons: TaskActivate",
        c"55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 e4 e0 ff ff ff 48 81 ec a0 02 00 00 49 89 fd 48 8b 05 73 81 a2 00 48 8b 00",
        0x0, 6, u2_chapter_activate_eu),
    hook(c"U2 Chapter Select weapons: PlayTask",
        c"55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 e4 e0 ff ff ff 48 81 ec 80 01 00 00 41 89 f6 48 89 fb 4c 8b 3d 18 2e 5f 00",
        0x0, 6, u2_chapter_play_hook),
    hook(c"U2 Chapter Select weapons: TaskActivate",
        c"55 48 89 e5 41 57 41 56 41 55 41 54 53 48 81 e4 e0 ff ff ff 48 81 ec a0 02 00 00 48 89 fb 48 8b 05 bb e8 5e 00 48 8b 00",
        0x0, 6, u2_chapter_activate_us),
];

/// `const RuntimeGuestCallback *runtime_guest_callbacks(size_t *count)`.
///
/// # Safety
/// `count` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn runtime_guest_callbacks(count: *mut usize) -> *const GuestCallback {
    if !count.is_null() {
        // SAFETY: writable (caller).
        unsafe { count.write(GUEST_CALLBACKS.len()) };
    }
    GUEST_CALLBACKS.as_ptr()
}

/// The table and its patches, for the tests.
#[cfg(test)]
pub(super) fn table() -> &'static [GuestCallback] {
    &GUEST_CALLBACKS
}

#[cfg(test)]
impl GuestCallback {
    pub(super) fn parts(&self) -> (String, String, usize, usize, bool, bool) {
        // SAFETY: static C strings.
        let text = |p: *const c_char| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        (
            text(self.name),
            text(self.pattern),
            self.skip,
            self.moved,
            self.callback.is_some(),
            !self.patch.is_null(),
        )
    }
}
