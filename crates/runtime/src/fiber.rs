//! libSceFiber: user-mode fibers on guest-allocated stacks (piece 3.13, replacing
//! `src/runtime_fiber.c` on Linux).
//!
//! A fiber is suspended inside `fiber_swap` with its callee-saved registers (SysV), MXCSR and
//! the x87 control word pushed on its own stack; only that stack pointer is kept. Fibers move
//! between threads (a job system resumes them on any worker), so per-thread state is looked
//! up again after every resume and never cached across a switch.

use std::cell::UnsafeCell;
use std::ffi::{c_char, c_void, CStr};
use std::mem::size_of;
use std::ptr::{self, null_mut};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

const FIBER_ERROR_NULL: i32 = 0x8059_0001u32 as i32;
const FIBER_ERROR_ALIGNMENT: i32 = 0x8059_0002u32 as i32;
const FIBER_ERROR_RANGE: i32 = 0x8059_0003u32 as i32;
const FIBER_ERROR_INVALID: i32 = 0x8059_0004u32 as i32;
const FIBER_ERROR_PERMISSION: i32 = 0x8059_0005u32 as i32;
const FIBER_ERROR_STATE: i32 = 0x8059_0006u32 as i32;

const SIGNATURE_START: u32 = 0xdef1_649c;
const SIGNATURE_END: u32 = 0xb375_92a0;
const OPT_SIGNATURE: u32 = 0xbb40_e64d;
const STACK_SIGNATURE: u64 = 0x7149_f2ca_7149_f2ca;
const STACK_FILL: u64 = 0xdead_beef_dead_beef;
const CONTEXT_MINIMUM: u64 = 512;
const FLAG_CONTEXT_SIZE_CHECK: u32 = 0x10;
const FLAG_SET_FPU: u32 = 0x100;
const SDK_350: u32 = 0x0350_0000;

const STATE_RUN: u32 = 1;
const STATE_IDLE: u32 = 2;
const STATE_TERMINATED: u32 = 3;

type FiberEntry = unsafe extern "sysv64" fn(arg_on_initialize: u64, arg_on_run: u64);

/// The game reserves 256 bytes (8-byte aligned) for each fiber; the layout is ours.
#[repr(C)]
pub struct GuestFiber {
    magic_start: u32,
    state: AtomicU32,
    entry: Option<FiberEntry>,
    arg_on_initialize: u64,
    addr_context: *mut u8,
    size_context: u64,
    name: [u8; 32],
    saved_rsp: u64,
    flags: u32,
    magic_end: u32,
}

const _: () = assert!(size_of::<GuestFiber>() <= 256);

#[repr(C)]
pub struct GuestFiberInfo {
    size: u64,
    entry: Option<FiberEntry>,
    arg_on_initialize: u64,
    addr_context: *mut c_void,
    size_context: u64,
    name: [u8; 32],
    size_context_margin: u64,
    pad: [u8; 48],
}

const _: () = assert!(size_of::<GuestFiberInfo>() == 128);

struct FiberThread {
    current: *mut GuestFiber,
    previous: *mut GuestFiber,
    thread_rsp: u64,
    arg_on_run_to: u64,
    arg_on_return: u64,
    #[cfg(windows)]
    stack_base: u64,
    #[cfg(windows)]
    stack_limit: u64,
}

impl FiberThread {
    const fn new() -> Self {
        Self {
            current: null_mut(),
            previous: null_mut(),
            thread_rsp: 0,
            arg_on_run_to: 0,
            arg_on_return: 0,
            #[cfg(windows)]
            stack_base: 0,
            #[cfg(windows)]
            stack_limit: 0,
        }
    }
}

thread_local! {
    static FIBER_THREAD: UnsafeCell<FiberThread> = const { UnsafeCell::new(FiberThread::new()) };
}

static CONTEXT_SIZE_CHECK: AtomicU32 = AtomicU32::new(0);
static SWITCHES: AtomicU64 = AtomicU64::new(0);
static FIBERS_STARTED: AtomicU64 = AtomicU64::new(0);

/// Not inlined: a resumed fiber may be on another thread, so the TLS address is fetched again.
#[inline(never)]
fn this_thread() -> *mut FiberThread {
    FIBER_THREAD.with(|cell| cell.get())
}

extern "sysv64" {
    fn fiber_swap(save: *mut u64, load: u64);
    fn fiber_start();
}

core::arch::global_asm!(
    ".text",
    ".globl fiber_swap",
    "fiber_swap:",
    "    push rbp",
    "    push rbx",
    "    push r12",
    "    push r13",
    "    push r14",
    "    push r15",
    "    sub rsp, 8",
    "    stmxcsr [rsp]",
    "    fnstcw [rsp + 4]",
    "    mov [rdi], rsp",
    "    mov rsp, rsi",
    "    ldmxcsr [rsp]",
    "    fldcw [rsp + 4]",
    "    add rsp, 8",
    "    pop r15",
    "    pop r14",
    "    pop r13",
    "    pop r12",
    "    pop rbx",
    "    pop rbp",
    "    ret",
    ".globl fiber_start",
    "fiber_start:",
    "    mov rdi, rbx",
    "    and rsp, -16",
    "    call fiber_main",
    "    ud2",
);

thread_local! {
    static NDGI_OPEN: std::cell::Cell<std::ffi::c_int> = const { std::cell::Cell::new(0) };
}

/// Sets this thread's `runtime_ndgi_open` flag (used by `pad/hooks.rs` and `job_lock_wait`).
#[no_mangle]
pub extern "C" fn runtime_ndgi_set(open: std::ffi::c_int) {
    NDGI_OPEN.with(|flag| flag.set(open));
}

/// Returns this thread's `runtime_ndgi_open` flag.
#[no_mangle]
pub extern "C" fn runtime_ndgi_is_open() -> std::ffi::c_int {
    NDGI_OPEN.with(|flag| flag.get())
}

fn ndgi_leave() {
    runtime_ndgi_set(0);
}

/// For fault reports (no allocation, any thread): the guest fiber running here, or null.
#[no_mangle]
pub extern "C" fn runtime_fiber_name_here() -> *const c_char {
    // SAFETY: `this_thread()` points to the current thread's `FiberThread`.
    let current = unsafe { (*this_thread()).current };
    if current.is_null() {
        ptr::null()
    } else {
        // SAFETY: `current` is valid while running on this thread and `name` is NUL-terminated.
        unsafe { (*current).name.as_ptr().cast::<c_char>() }
    }
}

unsafe fn fatal(what: &str, fiber: *const GuestFiber) -> ! {
    let name = if fiber.is_null() {
        "?"
    } else {
        // SAFETY: `fiber` is non-null and `name` is NUL-terminated on initialization.
        unsafe { CStr::from_ptr((*fiber).name.as_ptr().cast()) }
            .to_str()
            .unwrap_or("?")
    };
    crate::abi::host::err(&format!("STOP: fiber '{name}': {what}\n"));
    crate::abi::libc::runtime_report();
    #[cfg(unix)]
    // SAFETY: `_exit` terminates the process immediately without running C++ destructors.
    unsafe {
        libc::_exit(24)
    }
    #[cfg(windows)]
    std::process::exit(24);
}

#[cfg(windows)]
unsafe fn use_stack(fiber: *const GuestFiber, t: *mut FiberThread) {
    unsafe {
        if (*t).stack_base == 0 {
            let mut base: u64;
            let mut limit: u64;
            core::arch::asm!("mov {}, gs:[0x08]", out(reg) base, options(nostack, readonly));
            core::arch::asm!("mov {}, gs:[0x10]", out(reg) limit, options(nostack, readonly));
            (*t).stack_base = base;
            (*t).stack_limit = limit;
        }
        let (base, limit) = if !fiber.is_null() && !(*fiber).addr_context.is_null() {
            (
                (*fiber).addr_context as u64 + (*fiber).size_context,
                (*fiber).addr_context as u64,
            )
        } else {
            ((*t).stack_base, (*t).stack_limit)
        };
        core::arch::asm!("mov gs:[0x08], {}", in(reg) base, options(nostack));
        core::arch::asm!("mov gs:[0x10], {}", in(reg) limit, options(nostack));
    }
}

#[cfg(not(windows))]
unsafe fn use_stack(_fiber: *const GuestFiber, _t: *mut FiberThread) {}

unsafe fn context_of(fiber: *mut GuestFiber, below: u64) -> u64 {
    // SAFETY: caller validated `fiber` and claimed `STATE_RUN`.
    let rsp = unsafe { (*fiber).saved_rsp };
    if rsp != 0 {
        unsafe { (*fiber).saved_rsp = 0 };
        return rsp;
    }
    let addr_context = unsafe { (*fiber).addr_context };
    let size_context = unsafe { (*fiber).size_context };
    let top = if !addr_context.is_null() {
        addr_context as u64 + size_context
    } else {
        below.wrapping_sub(512)
    } & !15u64;
    let frame = (top - 64) as *mut u64;
    let mut mxcsr: u32 = 0x9fc0;
    let mut fpucw: u16 = 0x037f;
    if (unsafe { (*fiber).flags } & FLAG_SET_FPU) == 0 {
        // SAFETY: reads the current thread's MXCSR and x87 control word.
        unsafe {
            core::arch::asm!("stmxcsr [{}]", in(reg) &mut mxcsr, options(nostack));
            core::arch::asm!("fnstcw [{}]", in(reg) &mut fpucw, options(nostack));
        }
    }
    // SAFETY: `frame` points to 64 bytes below the aligned stack top.
    unsafe {
        *frame.add(0) = (mxcsr as u64) | ((fpucw as u64) << 32);
        *frame.add(1) = 0; // r15
        *frame.add(2) = 0; // r14
        *frame.add(3) = 0; // r13
        *frame.add(4) = 0; // r12
        *frame.add(5) = fiber as u64; // rbx
        *frame.add(6) = 0; // rbp
        *frame.add(7) = fiber_start as *const () as usize as u64;
    }
    FIBERS_STARTED.fetch_add(1, Ordering::Relaxed);
    frame as u64
}

unsafe fn check_stack(fiber: *const GuestFiber) {
    if !fiber.is_null() {
        // SAFETY: `fiber` is a valid `GuestFiber` whose context stack begins with `STACK_SIGNATURE`.
        let addr = unsafe { (*fiber).addr_context };
        if !addr.is_null() && unsafe { *(addr as *const u64) } != STACK_SIGNATURE {
            unsafe { fatal("stack overflow (context size too small)", fiber) };
        }
    }
}

/// Runs first in every context that gains control: the previous fiber's context is saved.
unsafe fn arrived(t: *mut FiberThread) {
    let previous = unsafe { (*t).previous };
    unsafe { (*t).previous = null_mut() };
    if !previous.is_null() {
        unsafe { (*previous).state.store(STATE_IDLE, Ordering::Release) };
    }
}

#[no_mangle]
extern "sysv64" fn fiber_main(fiber: *mut GuestFiber) -> ! {
    let t = this_thread();
    // SAFETY: `fiber` is active on this thread and `t` is this thread's `FiberThread`.
    unsafe {
        arrived(t);
        if let Some(entry) = (*fiber).entry {
            entry((*fiber).arg_on_initialize, (*t).arg_on_run_to);
        }
        fatal("entry function returned", fiber);
    }
}

fn valid(fiber: *const GuestFiber) -> bool {
    // SAFETY: caller verified `fiber` is non-null and 8-byte aligned.
    unsafe { (*fiber).magic_start == SIGNATURE_START && (*fiber).magic_end == SIGNATURE_END }
}

fn check_fiber(fiber: *const GuestFiber) -> i32 {
    if fiber.is_null() {
        return FIBER_ERROR_NULL;
    }
    if !(fiber as usize).is_multiple_of(8) {
        return FIBER_ERROR_ALIGNMENT;
    }
    if valid(fiber) {
        0
    } else {
        FIBER_ERROR_INVALID
    }
}

unsafe fn prepare_context(fiber: *mut GuestFiber) {
    let addr = unsafe { (*fiber).addr_context };
    if addr.is_null() {
        return;
    }
    let stack = addr as *mut u64;
    unsafe { *stack = STACK_SIGNATURE };
    if (unsafe { (*fiber).flags } & FLAG_CONTEXT_SIZE_CHECK) != 0 {
        let words = unsafe { (*fiber).size_context } / 8;
        for i in 1..words {
            unsafe { *stack.add(i as usize) = STACK_FILL };
        }
    }
}

fn check_context(addr: *const c_void, size: u64) -> i32 {
    if !(addr as usize).is_multiple_of(16) {
        return FIBER_ERROR_ALIGNMENT;
    }
    if size != 0 && size < CONTEXT_MINIMUM {
        return FIBER_ERROR_RANGE;
    }
    if !size.is_multiple_of(16) || addr.is_null() != (size == 0) {
        return FIBER_ERROR_INVALID;
    }
    0
}

unsafe fn attach_context(fiber: *mut GuestFiber, addr: *mut c_void, size: u64) -> i32 {
    if addr.is_null() && size == 0 {
        return 0;
    }
    let e = check_context(addr, size);
    if e != 0 {
        return e;
    }
    if addr.is_null() || !unsafe { (*fiber).addr_context }.is_null() {
        return FIBER_ERROR_INVALID;
    }
    unsafe {
        (*fiber).addr_context = addr.cast::<u8>();
        (*fiber).size_context = size;
        prepare_context(fiber);
    }
    0
}

fn claim(fiber: *mut GuestFiber) -> bool {
    // SAFETY: caller validated `fiber`.
    unsafe {
        (*fiber)
            .state
            .compare_exchange(STATE_IDLE, STATE_RUN, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

unsafe extern "sysv64" fn fiber_initialize(
    fiber: *mut GuestFiber,
    name: *const c_char,
    entry: Option<FiberEntry>,
    arg: u64,
    addr: *mut c_void,
    size: u64,
    opt: *const u32,
    build: u32,
) -> i32 {
    if fiber.is_null() || name.is_null() || entry.is_none() {
        return FIBER_ERROR_NULL;
    }
    if !(fiber as usize).is_multiple_of(8) || (!opt.is_null() && !(opt as usize).is_multiple_of(8))
    {
        return FIBER_ERROR_ALIGNMENT;
    }
    let e = check_context(addr, size);
    if e != 0 {
        return e;
    }
    if !opt.is_null() && unsafe { *opt } != OPT_SIGNATURE {
        return FIBER_ERROR_INVALID;
    }
    unsafe {
        ptr::write_bytes(fiber, 0, 1);
        let bytes = CStr::from_ptr(name).to_bytes();
        let copy_len = bytes.len().min((*fiber).name.len() - 1);
        ptr::copy_nonoverlapping(bytes.as_ptr(), (*fiber).name.as_mut_ptr(), copy_len);
        (*fiber).entry = entry;
        (*fiber).arg_on_initialize = arg;
        (*fiber).addr_context = addr.cast::<u8>();
        (*fiber).size_context = size;
        (*fiber).flags = if build >= SDK_350 { FLAG_SET_FPU } else { 0 }
            | if CONTEXT_SIZE_CHECK.load(Ordering::Acquire) != 0 {
                FLAG_CONTEXT_SIZE_CHECK
            } else {
                0
            };
        prepare_context(fiber);
        (*fiber).magic_start = SIGNATURE_START;
        (*fiber).magic_end = SIGNATURE_END;
        (*fiber).state.store(STATE_IDLE, Ordering::Release);
    }
    0
}

unsafe extern "sysv64" fn fiber_opt_initialize(opt: *mut u32) -> i32 {
    if opt.is_null() {
        return FIBER_ERROR_NULL;
    }
    if !(opt as usize).is_multiple_of(8) {
        return FIBER_ERROR_ALIGNMENT;
    }
    unsafe { *opt = OPT_SIGNATURE };
    0
}

unsafe extern "sysv64" fn fiber_finalize(fiber: *mut GuestFiber) -> i32 {
    let e = check_fiber(fiber);
    if e != 0 {
        return e;
    }
    let ok = unsafe {
        (*fiber)
            .state
            .compare_exchange(
                STATE_IDLE,
                STATE_TERMINATED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    };
    if ok {
        0
    } else {
        FIBER_ERROR_STATE
    }
}

unsafe extern "sysv64" fn fiber_run_with(
    fiber: *mut GuestFiber,
    addr: *mut c_void,
    size: u64,
    arg: u64,
    ret: *mut u64,
) -> i32 {
    let e = check_fiber(fiber);
    if e != 0 {
        return e;
    }
    if !(addr as usize).is_multiple_of(16) {
        return FIBER_ERROR_ALIGNMENT;
    }
    let mut t = this_thread();
    if !unsafe { (*t).current }.is_null() {
        return FIBER_ERROR_PERMISSION;
    }
    let e = unsafe { attach_context(fiber, addr, size) };
    if e != 0 {
        return e;
    }
    if !claim(fiber) {
        return FIBER_ERROR_STATE;
    }
    unsafe {
        (*t).current = fiber;
        (*t).previous = null_mut();
        (*t).arg_on_run_to = arg;
        (*t).arg_on_return = 0;
        let here: u64;
        core::arch::asm!("mov {}, rsp", out(reg) here, options(nostack, nomem));
        let target = context_of(fiber, here);
        SWITCHES.fetch_add(1, Ordering::Relaxed);
        use_stack(fiber, t);
        fiber_swap(&mut (*t).thread_rsp, target);
        t = this_thread();
        use_stack(null_mut(), t);
        let last = (*t).current;
        (*t).current = null_mut();
        if !last.is_null() {
            (*last).state.store(STATE_IDLE, Ordering::Release);
        }
        if !ret.is_null() {
            *ret = (*t).arg_on_return;
        }
    }
    0
}

unsafe extern "sysv64" fn fiber_run(fiber: *mut GuestFiber, arg: u64, ret: *mut u64) -> i32 {
    unsafe { fiber_run_with(fiber, null_mut(), 0, arg, ret) }
}

unsafe extern "sysv64" fn fiber_switch_with(
    fiber: *mut GuestFiber,
    addr: *mut c_void,
    size: u64,
    arg: u64,
    arg_on_run: *mut u64,
) -> i32 {
    ndgi_leave();
    let e = check_fiber(fiber);
    if e != 0 {
        return e;
    }
    if !(addr as usize).is_multiple_of(16) {
        return FIBER_ERROR_ALIGNMENT;
    }
    let mut t = this_thread();
    let current = unsafe { (*t).current };
    if current.is_null() {
        return FIBER_ERROR_PERMISSION;
    }
    let e = unsafe { attach_context(fiber, addr, size) };
    if e != 0 {
        return e;
    }
    if !claim(fiber) {
        return FIBER_ERROR_STATE;
    }
    unsafe {
        check_stack(current);
        (*t).previous = current;
        (*t).current = fiber;
        (*t).arg_on_run_to = arg;
        let target = context_of(fiber, (*t).thread_rsp);
        SWITCHES.fetch_add(1, Ordering::Relaxed);
        use_stack(fiber, t);
        if (*current).addr_context.is_null() {
            let mut discarded = 0u64;
            fiber_swap(&mut discarded, target);
            std::hint::unreachable_unchecked();
        }
        fiber_swap(&mut (*current).saved_rsp, target);
        t = this_thread();
        arrived(t);
        if !arg_on_run.is_null() {
            *arg_on_run = (*t).arg_on_run_to;
        }
    }
    0
}

unsafe extern "sysv64" fn fiber_switch(
    fiber: *mut GuestFiber,
    arg: u64,
    arg_on_run: *mut u64,
) -> i32 {
    unsafe { fiber_switch_with(fiber, null_mut(), 0, arg, arg_on_run) }
}

unsafe extern "sysv64" fn fiber_return_to_thread(arg_on_return: u64, arg_on_run: *mut u64) -> i32 {
    ndgi_leave();
    let mut t = this_thread();
    let current = unsafe { (*t).current };
    if current.is_null() {
        return FIBER_ERROR_PERMISSION;
    }
    unsafe {
        check_stack(current);
        (*t).arg_on_return = arg_on_return;
        SWITCHES.fetch_add(1, Ordering::Relaxed);
        if (*current).addr_context.is_null() {
            let mut discarded = 0u64;
            fiber_swap(&mut discarded, (*t).thread_rsp);
            std::hint::unreachable_unchecked();
        }
        fiber_swap(&mut (*current).saved_rsp, (*t).thread_rsp);
        t = this_thread();
        arrived(t);
        if !arg_on_run.is_null() {
            *arg_on_run = (*t).arg_on_run_to;
        }
    }
    0
}

unsafe extern "sysv64" fn fiber_get_self(fiber: *mut *mut GuestFiber) -> i32 {
    if fiber.is_null() {
        return FIBER_ERROR_NULL;
    }
    let current = unsafe { (*this_thread()).current };
    if current.is_null() {
        return FIBER_ERROR_PERMISSION;
    }
    unsafe { *fiber = current };
    0
}

unsafe extern "sysv64" fn fiber_get_info(fiber: *mut GuestFiber, info: *mut GuestFiberInfo) -> i32 {
    if fiber.is_null() || info.is_null() {
        return FIBER_ERROR_NULL;
    }
    if !(fiber as usize).is_multiple_of(8) || !(info as usize).is_multiple_of(8) {
        return FIBER_ERROR_ALIGNMENT;
    }
    if unsafe { (*info).size } != size_of::<GuestFiberInfo>() as u64 || !valid(fiber) {
        return FIBER_ERROR_INVALID;
    }
    unsafe {
        (*info).entry = (*fiber).entry;
        (*info).arg_on_initialize = (*fiber).arg_on_initialize;
        (*info).addr_context = (*fiber).addr_context.cast();
        (*info).size_context = (*fiber).size_context;
        (*info).name = (*fiber).name;
        (*info).size_context_margin = u64::MAX;
        if ((*fiber).flags & FLAG_CONTEXT_SIZE_CHECK) != 0 && !(*fiber).addr_context.is_null() {
            let stack = (*fiber).addr_context as *const u64;
            let mut i = 1u64;
            let words = (*fiber).size_context / 8;
            while i < words && *stack.add(i as usize) == STACK_FILL {
                i += 1;
            }
            (*info).size_context_margin = (i - 1) * 8;
        }
    }
    0
}

unsafe extern "sysv64" fn fiber_rename(fiber: *mut GuestFiber, name: *const c_char) -> i32 {
    if fiber.is_null() || name.is_null() {
        return FIBER_ERROR_NULL;
    }
    let e = check_fiber(fiber);
    if e != 0 {
        return e;
    }
    unsafe {
        (*fiber).name.fill(0);
        let bytes = CStr::from_ptr(name).to_bytes();
        let copy_len = bytes.len().min((*fiber).name.len() - 1);
        ptr::copy_nonoverlapping(bytes.as_ptr(), (*fiber).name.as_mut_ptr(), copy_len);
    }
    0
}

extern "sysv64" fn fiber_start_size_check(flags: u32) -> i32 {
    if flags != 0 {
        return FIBER_ERROR_INVALID;
    }
    if CONTEXT_SIZE_CHECK
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        0
    } else {
        FIBER_ERROR_STATE
    }
}

extern "sysv64" fn fiber_stop_size_check() -> i32 {
    if CONTEXT_SIZE_CHECK
        .compare_exchange(1, 0, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        0
    } else {
        FIBER_ERROR_STATE
    }
}

pub(crate) fn resolve(name: &CStr) -> Option<usize> {
    let addr = match name.to_str().ok()? {
        "_sceFiberInitializeImpl" => fiber_initialize as *const () as usize,
        "sceFiberOptParamInitialize" => fiber_opt_initialize as *const () as usize,
        "sceFiberFinalize" => fiber_finalize as *const () as usize,
        "sceFiberRun" => fiber_run as *const () as usize,
        "sceFiberSwitch" => fiber_switch as *const () as usize,
        "_sceFiberAttachContextAndRun" => fiber_run_with as *const () as usize,
        "_sceFiberAttachContextAndSwitch" => fiber_switch_with as *const () as usize,
        "sceFiberGetSelf" => fiber_get_self as *const () as usize,
        "sceFiberReturnToThread" => fiber_return_to_thread as *const () as usize,
        "sceFiberGetInfo" => fiber_get_info as *const () as usize,
        "sceFiberRename" => fiber_rename as *const () as usize,
        "sceFiberStartContextSizeCheck" => fiber_start_size_check as *const () as usize,
        "sceFiberStopContextSizeCheck" => fiber_stop_size_check as *const () as usize,
        _ => return None,
    };
    Some(addr)
}

/// # Safety
/// `name` must be null or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn runtime_fiber_resolve(name: *const c_char) -> usize {
    if name.is_null() {
        return 0;
    }
    // SAFETY: caller guarantees `name` is a NUL-terminated C string.
    resolve(unsafe { CStr::from_ptr(name) }).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn runtime_fiber_report() {
    let started = FIBERS_STARTED.load(Ordering::Relaxed);
    let switches = SWITCHES.load(Ordering::Relaxed);
    println!("Runtime: fibers started={started}, switches={switches}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::MaybeUninit;

    #[repr(C, align(16))]
    struct AlignedStack([u8; 64 * 1024]);

    unsafe extern "sysv64" fn fiber_ping_pong(arg_init: u64, mut arg_run: u64) {
        let mut mxcsr: u32 = 0;
        core::arch::asm!("stmxcsr [{}]", in(reg) &mut mxcsr, options(nostack));
        assert_eq!(
            mxcsr, 0x9fc0,
            "SDK 3.50+ fiber must start with MXCSR 0x9fc0"
        );

        let mut self_ptr: *mut GuestFiber = null_mut();
        assert_eq!(fiber_get_self(&mut self_ptr), 0);
        assert!(!self_ptr.is_null());

        let mut next_arg = 0u64;
        assert_eq!(fiber_return_to_thread(arg_init + arg_run, &mut next_arg), 0);
        let mut resumed_mxcsr: u32 = 0;
        core::arch::asm!("stmxcsr [{}]", in(reg) &mut resumed_mxcsr, options(nostack));
        assert_eq!(
            resumed_mxcsr, 0x9fc0,
            "resumed fiber must preserve MXCSR 0x9fc0 across host threads"
        );
        arg_run = next_arg;
        assert_eq!(
            fiber_return_to_thread(arg_init * 10 + arg_run, &mut next_arg),
            0
        );
    }

    unsafe extern "sysv64" fn fiber_switch_target(arg_init: u64, arg_run: u64) {
        let _ = fiber_return_to_thread(arg_init + arg_run + 100, null_mut());
    }

    #[test]
    fn fiber_run_return_rename_info_and_size_check() {
        assert_eq!(fiber_start_size_check(0), 0);
        assert_eq!(fiber_start_size_check(0), FIBER_ERROR_STATE);

        let mut fiber = MaybeUninit::<GuestFiber>::uninit();
        let mut stack = AlignedStack([0; 64 * 1024]);
        let mut opt_aligned = 0u64;
        let opt = (&raw mut opt_aligned).cast::<u32>();
        unsafe {
            assert_eq!(fiber_opt_initialize(opt), 0);
            assert_eq!(
                fiber_initialize(
                    fiber.as_mut_ptr(),
                    c"worker".as_ptr(),
                    Some(fiber_ping_pong),
                    10,
                    stack.0.as_mut_ptr().cast(),
                    stack.0.len() as u64,
                    opt,
                    SDK_350,
                ),
                0
            );
            let f = fiber.as_mut_ptr();
            let mut ret = 0u64;
            assert_eq!(fiber_run(f, 5, &mut ret), 0);
            assert_eq!(ret, 15);

            // Resume on a different host thread to verify cross-thread resume and TLS re-fetch.
            let f_addr = f as usize;
            let handle = std::thread::spawn(move || {
                let mut ret2 = 0u64;
                assert_eq!(fiber_run(f_addr as *mut GuestFiber, 7, &mut ret2), 0);
                ret2
            });
            assert_eq!(handle.join().unwrap(), 107);

            assert_eq!(fiber_rename(f, c"renamed".as_ptr()), 0);
            let mut info = MaybeUninit::<GuestFiberInfo>::zeroed().assume_init();
            info.size = size_of::<GuestFiberInfo>() as u64;
            assert_eq!(fiber_get_info(f, &mut info), 0);
            assert_eq!(
                CStr::from_ptr(info.name.as_ptr().cast()).to_str().unwrap(),
                "renamed"
            );
            assert!(info.size_context_margin < stack.0.len() as u64);

            assert_eq!(fiber_finalize(f), 0);
            assert_eq!(fiber_finalize(f), FIBER_ERROR_STATE);
        }

        assert_eq!(fiber_stop_size_check(), 0);
        assert_eq!(fiber_stop_size_check(), FIBER_ERROR_STATE);
    }

    #[test]
    fn fiber_switch_between_two_fibers() {
        unsafe extern "sysv64" fn switcher(arg_init: u64, _arg_run: u64) {
            let target = arg_init as *mut GuestFiber;
            let _ = fiber_switch(target, 22, null_mut());
        }

        let mut f1 = MaybeUninit::<GuestFiber>::uninit();
        let mut f2 = MaybeUninit::<GuestFiber>::uninit();
        let mut s1 = AlignedStack([0; 64 * 1024]);
        let mut s2 = AlignedStack([0; 64 * 1024]);
        unsafe {
            assert_eq!(
                fiber_initialize(
                    f2.as_mut_ptr(),
                    c"f2".as_ptr(),
                    Some(fiber_switch_target),
                    3,
                    s2.0.as_mut_ptr().cast(),
                    s2.0.len() as u64,
                    ptr::null(),
                    SDK_350,
                ),
                0
            );
            assert_eq!(
                fiber_initialize(
                    f1.as_mut_ptr(),
                    c"f1".as_ptr(),
                    Some(switcher),
                    f2.as_mut_ptr() as u64,
                    s1.0.as_mut_ptr().cast(),
                    s1.0.len() as u64,
                    ptr::null(),
                    SDK_350,
                ),
                0
            );
            let mut ret = 0u64;
            assert_eq!(fiber_run(f1.as_mut_ptr(), 0, &mut ret), 0);
            assert_eq!(ret, 125);
        }
    }

    #[test]
    fn fiber_resolve_and_validation() {
        unsafe {
            assert_ne!(runtime_fiber_resolve(c"sceFiberRun".as_ptr()), 0);
            assert_eq!(runtime_fiber_resolve(c"unknownFiber".as_ptr()), 0);
            assert_eq!(fiber_run(null_mut(), 0, null_mut()), FIBER_ERROR_NULL);
            assert_eq!(
                fiber_initialize(
                    ptr::without_provenance_mut::<GuestFiber>(1),
                    c"bad".as_ptr(),
                    Some(fiber_switch_target),
                    0,
                    null_mut(),
                    0,
                    ptr::null(),
                    0,
                ),
                FIBER_ERROR_ALIGNMENT
            );
        }
    }
}
