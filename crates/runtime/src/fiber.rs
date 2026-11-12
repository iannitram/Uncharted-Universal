//! libSceFiber, user-mode fibers on guest-allocated stacks (piece 3.13): the Linux x86-64 port
//! of `src/runtime_fiber.c`, which Windows still builds until piece 3.15.
//!
//! A fiber is suspended inside `uncharted_fiber_swap` with its callee-saved registers (System V),
//! MXCSR and the x87 control word pushed on its own stack; only that stack pointer is kept.
//! Fibers move between threads (a job system resumes them on any worker), so the per-thread
//! state is looked up again after every resume and never kept across a switch.
//!
//! A fiber becomes Idle only after its context has been saved: the fiber that runs next (or the
//! thread that gets control back) marks it, so another thread cannot resume a fiber whose stack
//! pointer is still being written.
//!
//! Fibers without a context of their own run on the calling thread's stack, below the frame of
//! `sceFiberRun`; they cannot be suspended and start again from their entry next time.

use crate::abi::host;
use std::cell::UnsafeCell;
use std::ffi::{c_char, c_void, CStr};
use std::ptr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

const ERROR_NULL: i32 = 0x8059_0001_u32 as i32;
const ERROR_ALIGNMENT: i32 = 0x8059_0002_u32 as i32;
const ERROR_RANGE: i32 = 0x8059_0003_u32 as i32;
const ERROR_INVALID: i32 = 0x8059_0004_u32 as i32;
const ERROR_PERMISSION: i32 = 0x8059_0005_u32 as i32;
const ERROR_STATE: i32 = 0x8059_0006_u32 as i32;

const SIGNATURE_START: u32 = 0xdef1_649c;
const SIGNATURE_END: u32 = 0xb375_92a0;
const OPT_SIGNATURE: u32 = 0xbb40_e64d;
const STACK_SIGNATURE: u64 = 0x7149_f2ca_7149_f2ca;
const STACK_FILL: u64 = 0xdead_beef_dead_beef;
const CONTEXT_MINIMUM: u64 = 512;
const FLAG_CONTEXT_SIZE_CHECK: u32 = 0x10;
/// Fibers of games built with SDK 3.50 or later start with the PS4's FPU state (0x9fc0, 0x037f)
/// instead of the creating thread's.
const FLAG_SET_FPU: u32 = 0x100;
const SDK_350: u32 = 0x0350_0000;

const STATE_RUN: u32 = 1;
const STATE_IDLE: u32 = 2;
const STATE_TERMINATED: u32 = 3;

/// A fiber's entry: `(arg_on_initialize, arg_on_run)`; it never returns.
type FiberEntry = unsafe extern "C" fn(u64, u64);

/// The game reserves 256 bytes (8-byte aligned) for each fiber; the layout is ours.
#[repr(C)]
struct GuestFiber {
    magic_start: u32,
    state: AtomicU32,
    entry: Option<FiberEntry>,
    arg_on_initialize: u64,
    addr_context: *mut u8,
    size_context: u64,
    name: [c_char; 32],
    /// The suspended context, 0 when the fiber starts from its entry.
    saved_rsp: u64,
    flags: u32,
    magic_end: u32,
}
const _: () = assert!(size_of::<GuestFiber>() <= 256, "OrbisFiber is 256 bytes");

#[repr(C)]
struct GuestFiberInfo {
    size: u64,
    entry: Option<FiberEntry>,
    arg_on_initialize: u64,
    addr_context: *mut c_void,
    size_context: u64,
    name: [c_char; 32],
    size_context_margin: u64,
    pad: [u8; 48],
}
const _: () = assert!(
    size_of::<GuestFiberInfo>() == 128,
    "OrbisFiberInfo is 128 bytes"
);

struct FiberThread {
    current: *mut GuestFiber,
    previous: *mut GuestFiber,
    /// The thread's context, suspended in `sceFiberRun`.
    thread_rsp: u64,
    arg_on_run_to: u64,
    arg_on_return: u64,
}

thread_local! {
    static FIBER_THREAD: UnsafeCell<FiberThread> = const {
        UnsafeCell::new(FiberThread {
            current: ptr::null_mut(),
            previous: ptr::null_mut(),
            thread_rsp: 0,
            arg_on_run_to: 0,
            arg_on_return: 0,
        })
    };
}

static CONTEXT_SIZE_CHECK: AtomicU32 = AtomicU32::new(0);
static SWITCHES: AtomicU64 = AtomicU64::new(0);
static FIBERS_STARTED: AtomicU64 = AtomicU64::new(0);

/// This thread's fiber state. Not inlined, and opaque to the optimizer: a resumed fiber may be
/// on another thread, so the address is fetched again after every swap.
#[inline(never)]
fn this_thread() -> *mut FiberThread {
    std::hint::black_box(FIBER_THREAD.with(UnsafeCell::get))
}

// `uncharted_fiber_swap(save, load)`: suspends here (`*save` = the stack pointer) and resumes
// the context whose stack pointer is `load`. `uncharted_fiber_start` begins a fresh context
// (`context_of`), with rbx the `GuestFiber`.
std::arch::global_asm!(
    ".text",
    ".p2align 4",
    ".globl uncharted_fiber_swap",
    ".hidden uncharted_fiber_swap",
    ".type uncharted_fiber_swap, @function",
    "uncharted_fiber_swap:",
    "push rbp",
    "push rbx",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "sub rsp, 8",
    "stmxcsr dword ptr [rsp]",
    "fnstcw word ptr [rsp + 4]",
    "mov qword ptr [rdi], rsp",
    "mov rsp, rsi",
    "ldmxcsr dword ptr [rsp]",
    "fldcw word ptr [rsp + 4]",
    "add rsp, 8",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbx",
    "pop rbp",
    "ret",
    ".size uncharted_fiber_swap, . - uncharted_fiber_swap",
    ".p2align 4",
    ".globl uncharted_fiber_start",
    ".hidden uncharted_fiber_start",
    ".type uncharted_fiber_start, @function",
    "uncharted_fiber_start:",
    "mov rdi, rbx",
    "and rsp, -16",
    "call {main}",
    "ud2",
    ".size uncharted_fiber_start, . - uncharted_fiber_start",
    main = sym fiber_main,
);

extern "C" {
    fn uncharted_fiber_swap(save: *mut u64, load: u64);
    fn uncharted_fiber_start();
}

/// For fault reports (no allocation, any thread): the fiber running here, or null.
#[no_mangle]
pub extern "C" fn runtime_fiber_name_here() -> *const c_char {
    // SAFETY: this thread's own state; `current` is a fiber that is running here.
    unsafe {
        let current = (*this_thread()).current;
        if current.is_null() {
            ptr::null()
        } else {
            (*current).name.as_ptr()
        }
    }
}

/// Ends the process on a fiber misuse the C ended it on (exit code 24).
fn fatal(what: &str, fiber: *const GuestFiber) -> ! {
    let name = if fiber.is_null() {
        std::borrow::Cow::Borrowed("?")
    } else {
        // SAFETY: a fiber is NUL-terminated within its 32 name bytes (`copy_name`).
        unsafe { CStr::from_ptr((*fiber).name.as_ptr()) }.to_string_lossy()
    };
    host::err(&format!("STOP: fiber '{name}': {what}\n"));
    crate::abi::libc::runtime_report();
    // SAFETY: flushing every C stream and ending the process take no Rust state.
    unsafe {
        libc::fflush(ptr::null_mut());
        libc::_exit(24)
    }
}

/// The calling thread's MXCSR and x87 control word.
fn host_fpu() -> (u32, u16) {
    let mut mxcsr = 0u32;
    let mut control = 0u16;
    // SAFETY: both store into locals and change nothing.
    unsafe {
        std::arch::asm!("stmxcsr [{}]", in(reg) &mut mxcsr, options(nostack, preserves_flags));
        std::arch::asm!("fnstcw [{}]", in(reg) &mut control, options(nostack, preserves_flags));
    }
    (mxcsr, control)
}

/// The context the target fiber resumes in: its saved one, or a fresh frame at the top of its
/// stack. A fiber without a context of its own starts on the thread stack below `below`, a
/// stack pointer under which nothing live remains.
///
/// # Safety
/// `fiber` is a valid fiber this thread claimed; its context, if any, is writable.
unsafe fn context_of(fiber: *mut GuestFiber, below: u64) -> u64 {
    // SAFETY: the caller guarantees a valid fiber and its writable context (or the free thread
    // stack below `below`).
    unsafe {
        let rsp = (*fiber).saved_rsp;
        if rsp != 0 {
            (*fiber).saved_rsp = 0;
            return rsp;
        }
        let top = if (*fiber).addr_context.is_null() {
            below - 512
        } else {
            (*fiber).addr_context as u64 + (*fiber).size_context
        } & !15;
        let frame = (top - 64) as *mut u64;
        let (mxcsr, control) = if (*fiber).flags & FLAG_SET_FPU != 0 {
            (0x9fc0, 0x037f)
        } else {
            host_fpu()
        };
        frame.write(u64::from(mxcsr) | u64::from(control) << 32);
        for register in 1..=4 {
            frame.add(register).write(0); // r15 r14 r13 r12
        }
        frame.add(5).write(fiber as u64); // rbx
        frame.add(6).write(0); // rbp: ends frame-pointer walks
        frame.add(7).write(uncharted_fiber_start as usize as u64);
        FIBERS_STARTED.fetch_add(1, Ordering::Relaxed);
        frame as u64
    }
}

/// # Safety
/// `fiber` is a valid fiber.
unsafe fn check_stack(fiber: *const GuestFiber) {
    // SAFETY: the caller guarantees a valid fiber; its context starts with the signature word.
    unsafe {
        let context = (*fiber).addr_context;
        if !context.is_null() && context.cast::<u64>().read() != STACK_SIGNATURE {
            fatal("stack overflow (context size too small)", fiber);
        }
    }
}

/// Runs first in every context that gains control: the previous fiber's context is saved.
///
/// # Safety
/// `t` is this thread's state.
unsafe fn arrived(t: *mut FiberThread) {
    // SAFETY: this thread's state; `previous` is the fiber that just suspended here.
    unsafe {
        let previous = (*t).previous;
        (*t).previous = ptr::null_mut();
        if !previous.is_null() {
            (*previous).state.store(STATE_IDLE, Ordering::Release);
        }
    }
}

/// The bottom of every fresh fiber context (`uncharted_fiber_start`).
unsafe extern "C" fn fiber_main(fiber: *mut GuestFiber) -> ! {
    let t = this_thread();
    // SAFETY: `context_of` put this fiber in rbx; it is valid and claimed by this thread.
    unsafe {
        arrived(t);
        if let Some(entry) = (*fiber).entry {
            entry((*fiber).arg_on_initialize, (*t).arg_on_run_to);
        }
    }
    fatal("entry function returned", fiber)
}

/// # Safety
/// `fiber` is readable.
unsafe fn valid(fiber: *const GuestFiber) -> bool {
    // SAFETY: the caller guarantees a readable fiber.
    unsafe { (*fiber).magic_start == SIGNATURE_START && (*fiber).magic_end == SIGNATURE_END }
}

/// # Safety
/// `fiber` is null or readable.
unsafe fn check_fiber(fiber: *const GuestFiber) -> i32 {
    if fiber.is_null() {
        ERROR_NULL
    } else if fiber as usize & 7 != 0 {
        ERROR_ALIGNMENT
    // SAFETY: the caller guarantees a readable fiber.
    } else if unsafe { valid(fiber) } {
        0
    } else {
        ERROR_INVALID
    }
}

/// # Safety
/// `fiber` is valid and its context, if any, writable.
unsafe fn prepare_context(fiber: *mut GuestFiber) {
    // SAFETY: the caller guarantees the fiber and its `size_context` writable bytes.
    unsafe {
        let stack = (*fiber).addr_context.cast::<u64>();
        if stack.is_null() {
            return;
        }
        stack.write(STACK_SIGNATURE);
        if (*fiber).flags & FLAG_CONTEXT_SIZE_CHECK != 0 {
            for i in 1..(*fiber).size_context / 8 {
                stack.add(i as usize).write(STACK_FILL);
            }
        }
    }
}

fn check_context(addr: *const c_void, size: u64) -> i32 {
    if addr as usize & 15 != 0 {
        ERROR_ALIGNMENT
    } else if size != 0 && size < CONTEXT_MINIMUM {
        ERROR_RANGE
    } else if size & 15 != 0 || addr.is_null() != (size == 0) {
        ERROR_INVALID
    } else {
        0
    }
}

/// # Safety
/// `fiber` is valid, `addr` null or `size` writable bytes.
unsafe fn attach_context(fiber: *mut GuestFiber, addr: *mut c_void, size: u64) -> i32 {
    if addr.is_null() && size == 0 {
        return 0;
    }
    let e = check_context(addr, size);
    if e != 0 {
        return e;
    }
    // SAFETY: the caller guarantees a valid fiber and the context.
    unsafe {
        if !(*fiber).addr_context.is_null() {
            return ERROR_INVALID;
        }
        (*fiber).addr_context = addr.cast();
        (*fiber).size_context = size;
        prepare_context(fiber);
    }
    0
}

/// `strncpy(name, source, 31)` into a zeroed name: at most 31 bytes, NUL-terminated.
///
/// # Safety
/// `name` is writable, `source` a NUL-terminated string.
unsafe fn copy_name(name: *mut [c_char; 32], source: *const c_char) {
    // SAFETY: the caller guarantees both.
    unsafe {
        let source = CStr::from_ptr(source).to_bytes();
        let length = source.len().min(31);
        let name = &mut *name;
        for (i, slot) in name.iter_mut().enumerate() {
            *slot = if i < length { source[i] as c_char } else { 0 };
        }
    }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn fiber_initialize(
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
        return ERROR_NULL;
    }
    if fiber as usize & 7 != 0 || opt as usize & 7 != 0 {
        return ERROR_ALIGNMENT;
    }
    let e = check_context(addr, size);
    if e != 0 {
        return e;
    }
    // SAFETY: the guest passes its 256-byte fiber, a name, the option word or null, and the
    // context's `size` bytes or null.
    unsafe {
        if !opt.is_null() && opt.read() != OPT_SIGNATURE {
            return ERROR_INVALID;
        }
        ptr::write_bytes(fiber, 0, 1);
        copy_name(ptr::addr_of_mut!((*fiber).name), name);
        (*fiber).entry = entry;
        (*fiber).arg_on_initialize = arg;
        (*fiber).addr_context = addr.cast();
        (*fiber).size_context = size;
        let set_fpu = if build >= SDK_350 { FLAG_SET_FPU } else { 0 };
        let size_check = if CONTEXT_SIZE_CHECK.load(Ordering::Acquire) != 0 {
            FLAG_CONTEXT_SIZE_CHECK
        } else {
            0
        };
        (*fiber).flags = set_fpu | size_check;
        prepare_context(fiber);
        (*fiber).magic_start = SIGNATURE_START;
        (*fiber).magic_end = SIGNATURE_END;
        (*fiber).state.store(STATE_IDLE, Ordering::Release);
    }
    0
}

unsafe extern "C" fn fiber_opt_initialize(opt: *mut u32) -> i32 {
    if opt.is_null() {
        return ERROR_NULL;
    }
    if opt as usize & 7 != 0 {
        return ERROR_ALIGNMENT;
    }
    // SAFETY: the guest passes its option block.
    unsafe { opt.write(OPT_SIGNATURE) };
    0
}

unsafe extern "C" fn fiber_finalize(fiber: *mut GuestFiber) -> i32 {
    // SAFETY: the guest passes its fiber or null.
    unsafe {
        let e = check_fiber(fiber);
        if e != 0 {
            return e;
        }
        match (*fiber).state.compare_exchange(
            STATE_IDLE,
            STATE_TERMINATED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => 0,
            Err(_) => ERROR_STATE,
        }
    }
}

/// # Safety
/// `fiber` is valid.
unsafe fn claim(fiber: *mut GuestFiber) -> bool {
    // SAFETY: the caller guarantees a valid fiber.
    unsafe {
        (*fiber)
            .state
            .compare_exchange(STATE_IDLE, STATE_RUN, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

/// The stack pointer of the caller's frame.
#[inline(always)]
fn stack_pointer() -> u64 {
    let rsp: u64;
    // SAFETY: reads rsp.
    unsafe {
        std::arch::asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack, preserves_flags))
    };
    rsp
}

/// `#[inline(never)]` keeps every switch in its own frame, so nothing of the caller's is kept in
/// registers across a resume on another thread.
#[inline(never)]
unsafe extern "C" fn fiber_run_with(
    fiber: *mut GuestFiber,
    addr: *mut c_void,
    size: u64,
    arg: u64,
    ret: *mut u64,
) -> i32 {
    // SAFETY: the guest passes its fiber, a context or null and a result slot or null; `t` is
    // this thread's state, fetched again after the swap.
    unsafe {
        let e = check_fiber(fiber);
        if e != 0 {
            return e;
        }
        if addr as usize & 15 != 0 {
            return ERROR_ALIGNMENT;
        }
        let t = this_thread();
        if !(*t).current.is_null() {
            return ERROR_PERMISSION;
        }
        let e = attach_context(fiber, addr, size);
        if e != 0 {
            return e;
        }
        if !claim(fiber) {
            return ERROR_STATE;
        }
        (*t).current = fiber;
        (*t).previous = ptr::null_mut();
        (*t).arg_on_run_to = arg;
        (*t).arg_on_return = 0;
        let target = context_of(fiber, stack_pointer());
        SWITCHES.fetch_add(1, Ordering::Relaxed);
        uncharted_fiber_swap(ptr::addr_of_mut!((*t).thread_rsp), target);
        // Back on this thread (it never left): a fiber returned to it.
        let t = this_thread();
        let last = (*t).current;
        (*t).current = ptr::null_mut();
        if !last.is_null() {
            (*last).state.store(STATE_IDLE, Ordering::Release);
        }
        if !ret.is_null() {
            ret.write((*t).arg_on_return);
        }
    }
    0
}

unsafe extern "C" fn fiber_run(fiber: *mut GuestFiber, arg: u64, ret: *mut u64) -> i32 {
    // SAFETY: forwarded from the guest.
    unsafe { fiber_run_with(fiber, ptr::null_mut(), 0, arg, ret) }
}

/// A render command context open on this thread (`src/runtime_tls.c`) leaves with the fiber.
#[cfg(uncharted_loader)]
fn ndgi_leave() {
    extern "C" {
        fn runtime_ndgi_leave();
    }
    // SAFETY: clears a thread-local flag.
    unsafe { runtime_ndgi_leave() };
}

#[cfg(not(uncharted_loader))]
fn ndgi_leave() {}

#[inline(never)]
unsafe extern "C" fn fiber_switch_with(
    fiber: *mut GuestFiber,
    addr: *mut c_void,
    size: u64,
    arg: u64,
    arg_on_run: *mut u64,
) -> i32 {
    ndgi_leave();
    // SAFETY: as `fiber_run_with`; `current` is the fiber running here.
    unsafe {
        let e = check_fiber(fiber);
        if e != 0 {
            return e;
        }
        if addr as usize & 15 != 0 {
            return ERROR_ALIGNMENT;
        }
        let t = this_thread();
        let current = (*t).current;
        if current.is_null() {
            return ERROR_PERMISSION;
        }
        let e = attach_context(fiber, addr, size);
        if e != 0 {
            return e;
        }
        if !claim(fiber) {
            return ERROR_STATE;
        }
        check_stack(current);
        (*t).previous = current;
        (*t).current = fiber;
        (*t).arg_on_run_to = arg;
        let target = context_of(fiber, (*t).thread_rsp);
        SWITCHES.fetch_add(1, Ordering::Relaxed);
        if (*current).addr_context.is_null() {
            let mut discarded = 0;
            uncharted_fiber_swap(&mut discarded, target); // no context to come back to
            std::process::abort();
        }
        uncharted_fiber_swap(ptr::addr_of_mut!((*current).saved_rsp), target);
        // Resumed later, possibly on another thread.
        let t = this_thread();
        arrived(t);
        if !arg_on_run.is_null() {
            arg_on_run.write((*t).arg_on_run_to);
        }
    }
    0
}

unsafe extern "C" fn fiber_switch(fiber: *mut GuestFiber, arg: u64, arg_on_run: *mut u64) -> i32 {
    // SAFETY: forwarded from the guest.
    unsafe { fiber_switch_with(fiber, ptr::null_mut(), 0, arg, arg_on_run) }
}

#[inline(never)]
unsafe extern "C" fn fiber_return_to_thread(arg_on_return: u64, arg_on_run: *mut u64) -> i32 {
    ndgi_leave();
    // SAFETY: this thread's state; `current` is the fiber running here.
    unsafe {
        let t = this_thread();
        let current = (*t).current;
        if current.is_null() {
            return ERROR_PERMISSION;
        }
        check_stack(current);
        (*t).arg_on_return = arg_on_return;
        SWITCHES.fetch_add(1, Ordering::Relaxed);
        if (*current).addr_context.is_null() {
            let mut discarded = 0;
            uncharted_fiber_swap(&mut discarded, (*t).thread_rsp);
            std::process::abort();
        }
        uncharted_fiber_swap(ptr::addr_of_mut!((*current).saved_rsp), (*t).thread_rsp);
        // Resumed later by sceFiberRun or sceFiberSwitch, possibly on another thread.
        let t = this_thread();
        arrived(t);
        if !arg_on_run.is_null() {
            arg_on_run.write((*t).arg_on_run_to);
        }
    }
    0
}

unsafe extern "C" fn fiber_get_self(fiber: *mut *mut GuestFiber) -> i32 {
    if fiber.is_null() {
        return ERROR_NULL;
    }
    // SAFETY: this thread's state, and the guest's slot.
    unsafe {
        let current = (*this_thread()).current;
        if current.is_null() {
            return ERROR_PERMISSION;
        }
        fiber.write(current);
    }
    0
}

unsafe extern "C" fn fiber_get_info(fiber: *mut GuestFiber, info: *mut GuestFiberInfo) -> i32 {
    if fiber.is_null() || info.is_null() {
        return ERROR_NULL;
    }
    if fiber as usize & 7 != 0 || info as usize & 7 != 0 {
        return ERROR_ALIGNMENT;
    }
    // SAFETY: the guest passes its fiber and a 128-byte info block; a valid fiber's context
    // holds `size_context` bytes.
    unsafe {
        if (*info).size != size_of::<GuestFiberInfo>() as u64 || !valid(fiber) {
            return ERROR_INVALID;
        }
        (*info).entry = (*fiber).entry;
        (*info).arg_on_initialize = (*fiber).arg_on_initialize;
        (*info).addr_context = (*fiber).addr_context.cast();
        (*info).size_context = (*fiber).size_context;
        (*info).name = (*fiber).name;
        (*info).size_context_margin = u64::MAX;
        let stack = (*fiber).addr_context.cast::<u64>();
        if (*fiber).flags & FLAG_CONTEXT_SIZE_CHECK != 0 && !stack.is_null() {
            let words = (*fiber).size_context / 8;
            let mut i = 1;
            while i < words && stack.add(i as usize).read() == STACK_FILL {
                i += 1;
            }
            (*info).size_context_margin = (i - 1) * 8;
        }
    }
    0
}

unsafe extern "C" fn fiber_rename(fiber: *mut GuestFiber, name: *const c_char) -> i32 {
    if fiber.is_null() || name.is_null() {
        return ERROR_NULL;
    }
    // SAFETY: the guest passes its fiber and a NUL-terminated name.
    unsafe {
        let e = check_fiber(fiber);
        if e != 0 {
            return e;
        }
        copy_name(ptr::addr_of_mut!((*fiber).name), name);
    }
    0
}

extern "C" fn fiber_start_size_check(flags: u32) -> i32 {
    if flags != 0 {
        return ERROR_INVALID;
    }
    match CONTEXT_SIZE_CHECK.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => 0,
        Err(_) => ERROR_STATE,
    }
}

extern "C" fn fiber_stop_size_check() -> i32 {
    match CONTEXT_SIZE_CHECK.compare_exchange(1, 0, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => 0,
        Err(_) => ERROR_STATE,
    }
}

/// The guest functions by symbol name, the C `exports` table.
fn exports() -> [(&'static str, usize); 13] {
    [
        (
            "_sceFiberInitializeImpl",
            fiber_initialize as *const () as usize,
        ),
        (
            "sceFiberOptParamInitialize",
            fiber_opt_initialize as *const () as usize,
        ),
        ("sceFiberFinalize", fiber_finalize as *const () as usize),
        ("sceFiberRun", fiber_run as *const () as usize),
        ("sceFiberSwitch", fiber_switch as *const () as usize),
        (
            "_sceFiberAttachContextAndRun",
            fiber_run_with as *const () as usize,
        ),
        (
            "_sceFiberAttachContextAndSwitch",
            fiber_switch_with as *const () as usize,
        ),
        ("sceFiberGetSelf", fiber_get_self as *const () as usize),
        (
            "sceFiberReturnToThread",
            fiber_return_to_thread as *const () as usize,
        ),
        ("sceFiberGetInfo", fiber_get_info as *const () as usize),
        ("sceFiberRename", fiber_rename as *const () as usize),
        (
            "sceFiberStartContextSizeCheck",
            fiber_start_size_check as *const () as usize,
        ),
        (
            "sceFiberStopContextSizeCheck",
            fiber_stop_size_check as *const () as usize,
        ),
    ]
}

/// The guest function for an import NID, or 0: the NID's symbol (`src/import_names.inc`) is
/// looked up by name, as `RUNTIME_LOOKUP` did.
///
/// # Safety
/// `name` is null or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn runtime_fiber_resolve(name: *const c_char) -> usize {
    if name.is_null() {
        return 0;
    }
    // SAFETY: the caller guarantees a NUL-terminated string.
    let Ok(nid) = unsafe { CStr::from_ptr(name) }.to_str() else {
        return 0;
    };
    let Some(symbol) = crate::abi::nid_name(nid) else {
        return 0;
    };
    exports()
        .iter()
        .find(|(name, _)| *name == symbol)
        .map_or(0, |&(_, function)| function)
}

/// The counter line at exit.
#[no_mangle]
pub extern "C" fn runtime_fiber_report() {
    crate::memory::say(&format!(
        "Runtime: fibers started={}, switches={}",
        FIBERS_STARTED.load(Ordering::Relaxed),
        SWITCHES.load(Ordering::Relaxed)
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::{fpu_state, serial};
    use std::sync::atomic::AtomicPtr;
    use std::sync::Mutex;

    /// A fiber context: 16-byte aligned guest memory.
    #[repr(C, align(16))]
    struct Context([u8; 64 * 1024]);

    fn context() -> Box<Context> {
        Box::new(Context([0; 64 * 1024]))
    }

    /// The 256 bytes the game reserves.
    #[repr(C, align(8))]
    struct Fiber([u8; 256]);

    fn fiber() -> Box<Fiber> {
        Box::new(Fiber([0; 256]))
    }

    fn raw(fiber: &mut Fiber) -> *mut GuestFiber {
        ptr::from_mut(fiber).cast()
    }

    /// Initializes `fiber` with `entry`, a 64 KiB context and SDK 3.50.
    fn init(fiber: &mut Fiber, name: &CStr, entry: FiberEntry, arg: u64, ctx: &mut Context) {
        // SAFETY: live locals of the right sizes.
        let result = unsafe {
            fiber_initialize(
                raw(fiber),
                name.as_ptr(),
                Some(entry),
                arg,
                ptr::from_mut(ctx).cast(),
                size_of::<Context>() as u64,
                ptr::null(),
                SDK_350,
            )
        };
        assert_eq!(result, 0);
    }

    fn run(fiber: &mut Fiber, arg: u64) -> u64 {
        let mut ret = 0;
        // SAFETY: an initialized fiber and a local slot.
        assert_eq!(unsafe { fiber_run(raw(fiber), arg, &mut ret) }, 0);
        ret
    }

    static LOG: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());

    /// Returns to the thread with `init + run`, then again with each next run's argument.
    unsafe extern "C" fn echo(init: u64, run: u64) {
        LOG.lock().unwrap().push((init, run));
        let mut next = 0;
        // SAFETY: running on a fiber; a local slot.
        unsafe { fiber_return_to_thread(init + run, &mut next) };
        loop {
            LOG.lock().unwrap().push((init, next));
            let value = next;
            // SAFETY: as above.
            unsafe { fiber_return_to_thread(value * 10, &mut next) };
        }
    }

    /// Run, return to the thread with a value, resume with another; state and counters.
    #[test]
    fn run_and_return_to_thread() {
        let _serial = serial();
        LOG.lock().unwrap().clear();
        let (mut f, mut ctx) = (fiber(), context());
        init(&mut f, c"echo", echo, 100, &mut ctx);
        let switches = SWITCHES.load(Ordering::Relaxed);
        assert_eq!(run(&mut f, 5), 105);
        // SAFETY: the fiber is suspended; its fields are ours to read.
        assert_eq!(
            unsafe { (*raw(&mut f)).state.load(Ordering::Acquire) },
            STATE_IDLE
        );
        assert_eq!(run(&mut f, 7), 70);
        assert_eq!(run(&mut f, 8), 80);
        assert_eq!(*LOG.lock().unwrap(), [(100, 5), (100, 7), (100, 8)]);
        assert_eq!(SWITCHES.load(Ordering::Relaxed), switches + 6);
        assert!(runtime_fiber_name_here().is_null());
        // SAFETY: a suspended fiber.
        unsafe {
            assert_eq!(fiber_finalize(raw(&mut f)), 0);
            assert_eq!(fiber_finalize(raw(&mut f)), ERROR_STATE);
            let mut ret = 0;
            assert_eq!(fiber_run(raw(&mut f), 0, &mut ret), ERROR_STATE);
        }
    }

    static PING: AtomicPtr<GuestFiber> = AtomicPtr::new(ptr::null_mut());
    static PONG: AtomicPtr<GuestFiber> = AtomicPtr::new(ptr::null_mut());
    static TRACE: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn trace(line: String) {
        TRACE.lock().unwrap().push(line);
    }

    fn here() -> String {
        // SAFETY: the name of the fiber running here, NUL-terminated.
        unsafe { CStr::from_ptr(runtime_fiber_name_here()) }
            .to_string_lossy()
            .into_owned()
    }

    /// A switches to B, B back to A, A returns to the thread.
    unsafe extern "C" fn ping(_: u64, run: u64) {
        // SAFETY: running on a fiber; both fibers are set up before the run.
        unsafe {
            trace(format!("{} {run}", here()));
            let mut self_fiber = ptr::null_mut();
            assert_eq!(fiber_get_self(&mut self_fiber), 0);
            assert_eq!(self_fiber, PING.load(Ordering::Relaxed));
            let mut back = 0;
            assert_eq!(fiber_switch(PONG.load(Ordering::Relaxed), 1, &mut back), 0);
            trace(format!("{} {back}", here()));
            fiber_return_to_thread(back + 1, ptr::null_mut());
        }
        std::process::abort();
    }

    unsafe extern "C" fn pong(_: u64, run: u64) {
        // SAFETY: as `ping`.
        unsafe {
            trace(format!("{} {run}", here()));
            assert_eq!(
                fiber_switch(PING.load(Ordering::Relaxed), 2, ptr::null_mut()),
                0
            );
        }
        std::process::abort();
    }

    /// `sceFiberSwitch` between two fibers, the argument each one gets, and the states.
    #[test]
    fn switch_between_fibers() {
        let _serial = serial();
        TRACE.lock().unwrap().clear();
        let (mut a, mut ctx_a, mut b, mut ctx_b) = (fiber(), context(), fiber(), context());
        init(&mut a, c"ping", ping, 0, &mut ctx_a);
        init(&mut b, c"pong", pong, 0, &mut ctx_b);
        PING.store(raw(&mut a), Ordering::Relaxed);
        PONG.store(raw(&mut b), Ordering::Relaxed);
        assert_eq!(run(&mut a, 9), 3);
        assert_eq!(*TRACE.lock().unwrap(), ["ping 9", "pong 1", "ping 2"]);
        // SAFETY: both suspended.
        unsafe {
            assert_eq!((*raw(&mut a)).state.load(Ordering::Acquire), STATE_IDLE);
            assert_eq!((*raw(&mut b)).state.load(Ordering::Acquire), STATE_IDLE);
            let mut ret = 0;
            assert_eq!(fiber_switch(raw(&mut a), 0, &mut ret), ERROR_PERMISSION);
            assert_eq!(fiber_return_to_thread(0, &mut ret), ERROR_PERMISSION);
            let mut out = ptr::null_mut();
            assert_eq!(fiber_get_self(&mut out), ERROR_PERMISSION);
            assert_eq!(fiber_get_self(ptr::null_mut()), ERROR_NULL);
        }
    }

    static THREADS: Mutex<Vec<libc::pthread_t>> = Mutex::new(Vec::new());

    unsafe extern "C" fn wanders(_: u64, run: u64) {
        let mut next = run;
        loop {
            // `pthread_self`, an external call: the thread is looked up again after each resume.
            // SAFETY: no preconditions.
            THREADS
                .lock()
                .unwrap()
                .push(unsafe { libc::pthread_self() });
            // SAFETY: running on a fiber.
            unsafe { fiber_return_to_thread(next + 1, &mut next) };
        }
    }

    /// A fiber suspended on one thread resumes on another (the job system's workers).
    #[test]
    fn fibers_resume_on_another_thread() {
        let _serial = serial();
        THREADS.lock().unwrap().clear();
        let (mut f, mut ctx) = (fiber(), context());
        init(&mut f, c"wanderer", wanders, 0, &mut ctx);
        assert_eq!(run(&mut f, 1), 2);
        let address = raw(&mut f) as usize;
        let other = std::thread::spawn(move || {
            let mut ret = 0;
            // SAFETY: the fiber is suspended and outlives this thread (joined below).
            let result = unsafe { fiber_run(address as *mut GuestFiber, 10, &mut ret) };
            // SAFETY: no preconditions.
            (result, ret, unsafe { libc::pthread_self() })
        })
        .join()
        .unwrap();
        assert_eq!((other.0, other.1), (0, 11));
        assert_eq!(run(&mut f, 20), 21);
        let threads = THREADS.lock().unwrap().clone();
        // SAFETY: no preconditions.
        let me = unsafe { libc::pthread_self() };
        assert_eq!(threads, [me, other.2, me]);
    }

    unsafe extern "C" fn once(init: u64, run: u64) {
        // SAFETY: running on a fiber.
        unsafe { fiber_return_to_thread(init * run, ptr::null_mut()) };
        std::process::abort();
    }

    /// A fiber without a context runs on the thread's stack and starts from its entry each time.
    #[test]
    fn fibers_without_a_context_run_on_the_thread_stack() {
        let _serial = serial();
        let mut f = fiber();
        // SAFETY: a live fiber, no context.
        let result = unsafe {
            fiber_initialize(
                raw(&mut f),
                c"bare".as_ptr(),
                Some(once),
                6,
                ptr::null_mut(),
                0,
                ptr::null(),
                0,
            )
        };
        assert_eq!(result, 0);
        assert_eq!(run(&mut f, 7), 42);
        assert_eq!(run(&mut f, 2), 12);
    }

    /// The argument checks of `_sceFiberInitializeImpl` and friends, the C's error codes.
    #[test]
    fn initialize_checks_its_arguments() {
        let _serial = serial();
        let (mut f, mut ctx) = (fiber(), context());
        let fiber = raw(&mut f);
        let context: *mut c_void = ptr::from_mut(&mut *ctx).cast();
        let name = c"x".as_ptr();
        let mut block = [0u64; 16];
        let opt: *mut u32 = block.as_mut_ptr().cast();
        // SAFETY: locals throughout; misaligned pointers are only checked, never used.
        unsafe {
            let init = |fiber, name, entry, addr, size, opt| {
                fiber_initialize(fiber, name, entry, 0, addr, size, opt, 0)
            };
            let entry = Some(once as FiberEntry);
            assert_eq!(
                init(ptr::null_mut(), name, entry, context, 4096, ptr::null()),
                ERROR_NULL
            );
            assert_eq!(
                init(fiber, ptr::null(), entry, context, 4096, ptr::null()),
                ERROR_NULL
            );
            assert_eq!(
                init(fiber, name, None, context, 4096, ptr::null()),
                ERROR_NULL
            );
            let odd = fiber.cast::<u8>().add(4).cast();
            assert_eq!(
                init(odd, name, entry, context, 4096, ptr::null()),
                ERROR_ALIGNMENT
            );
            let odd_context = context.cast::<u8>().add(8).cast();
            assert_eq!(
                init(fiber, name, entry, odd_context, 4096, ptr::null()),
                ERROR_ALIGNMENT
            );
            assert_eq!(
                init(fiber, name, entry, context, 256, ptr::null()),
                ERROR_RANGE
            );
            assert_eq!(
                init(fiber, name, entry, context, 4100, ptr::null()),
                ERROR_INVALID
            );
            assert_eq!(
                init(fiber, name, entry, context, 0, ptr::null()),
                ERROR_INVALID
            );
            assert_eq!(
                init(fiber, name, entry, ptr::null_mut(), 4096, ptr::null()),
                ERROR_INVALID
            );
            assert_eq!(init(fiber, name, entry, context, 4096, opt), ERROR_INVALID);
            assert_eq!(fiber_opt_initialize(ptr::null_mut()), ERROR_NULL);
            assert_eq!(fiber_opt_initialize(opt.add(1)), ERROR_ALIGNMENT);
            assert_eq!(fiber_opt_initialize(opt), 0);
            assert_eq!(opt.read(), OPT_SIGNATURE);
            assert_eq!(init(fiber, name, entry, context, 4096, opt), 0);
            assert_eq!(fiber_finalize(ptr::null_mut()), ERROR_NULL);
            assert_eq!(
                fiber_finalize(fiber.cast::<u8>().add(4).cast()),
                ERROR_ALIGNMENT
            );
            let mut garbage = Fiber([0x55; 256]);
            assert_eq!(fiber_finalize(raw(&mut garbage)), ERROR_INVALID);
            let mut ret = 0;
            assert_eq!(
                fiber_run_with(fiber, context, 4096, 0, &mut ret),
                ERROR_INVALID,
                "has one"
            );
            assert_eq!(
                fiber_run_with(fiber, odd_context, 4096, 0, &mut ret),
                ERROR_ALIGNMENT
            );
            assert_eq!(fiber_rename(fiber, ptr::null()), ERROR_NULL);
            assert_eq!(fiber_rename(raw(&mut garbage), name), ERROR_INVALID);
        }
    }

    /// Rename (31 bytes kept), `sceFiberGetInfo` and the context size check's margin.
    #[test]
    fn info_rename_and_size_check() {
        let _serial = serial();
        let (mut f, mut ctx) = (fiber(), context());
        assert_eq!(fiber_start_size_check(1), ERROR_INVALID);
        assert_eq!(fiber_start_size_check(0), 0);
        assert_eq!(fiber_start_size_check(0), ERROR_STATE);
        init(&mut f, c"measured", echo, 3, &mut ctx);
        assert_eq!(fiber_stop_size_check(), 0);
        assert_eq!(fiber_stop_size_check(), ERROR_STATE);
        // SAFETY: an initialized fiber and a local info block.
        unsafe {
            let mut info: GuestFiberInfo = std::mem::zeroed();
            assert_eq!(
                fiber_get_info(raw(&mut f), &mut info),
                ERROR_INVALID,
                "size unset"
            );
            info.size = 128;
            assert_eq!(fiber_get_info(raw(&mut f), &mut info), 0);
            assert_eq!(info.arg_on_initialize, 3);
            assert_eq!(info.size_context, 64 * 1024);
            assert_eq!(info.addr_context, ptr::from_mut(&mut *ctx).cast());
            assert_eq!(info.size_context_margin, 64 * 1024 - 8, "untouched");
            assert_eq!(CStr::from_ptr(info.name.as_ptr()), c"measured");
            LOG.lock().unwrap().clear();
            assert_eq!(run(&mut f, 1), 4);
            assert_eq!(fiber_get_info(raw(&mut f), &mut info), 0);
            assert!(
                info.size_context_margin < 64 * 1024 - 8 - 64,
                "the fiber used its stack"
            );
            assert_eq!(info.size_context_margin % 8, 0);
            let long = c"a fiber name longer than thirty-one bytes";
            assert_eq!(fiber_rename(raw(&mut f), long.as_ptr()), 0);
            assert_eq!(fiber_get_info(raw(&mut f), &mut info), 0);
            assert_eq!(
                CStr::from_ptr(info.name.as_ptr()),
                c"a fiber name longer than thirty"
            );
        }
        let (mut plain, mut plain_context) = (fiber(), context());
        init(&mut plain, c"plain", echo, 0, &mut plain_context);
        // SAFETY: as above.
        unsafe {
            let mut info: GuestFiberInfo = std::mem::zeroed();
            info.size = 128;
            assert_eq!(fiber_get_info(raw(&mut plain), &mut info), 0);
            assert_eq!(info.size_context_margin, u64::MAX, "no size check");
        }
    }

    static FIBER_FPU: Mutex<Option<(u32, u16, bool)>> = Mutex::new(None);

    unsafe extern "C" fn reads_fpu(_: u64, _: u64) {
        *FIBER_FPU.lock().unwrap() = Some(fpu_state());
        // SAFETY: running on a fiber.
        unsafe { fiber_return_to_thread(0, ptr::null_mut()) };
        std::process::abort();
    }

    /// `test_runtime.c` `guest_fpu_state`, the fiber half: a fiber of an SDK 3.50 game starts
    /// with MXCSR 0x9fc0 and the x87 control word 0x037f, and the thread gets its own back.
    #[test]
    fn sdk_350_fibers_start_with_the_ps4_fpu_state() {
        let _serial = serial();
        let (mut f, mut ctx) = (fiber(), context());
        init(&mut f, c"fpu", reads_fpu, 0, &mut ctx);
        let before = fpu_state();
        run(&mut f, 0);
        assert_eq!(*FIBER_FPU.lock().unwrap(), Some((0x9fc0, 0x037f, true)));
        // The thread's own state comes back (the exception flags of `before`'s test aside).
        let after = fpu_state();
        assert_eq!(
            (after.0 & !0x3f, after.1, after.2),
            (before.0 & !0x3f, before.1, before.2)
        );
    }

    /// The NIDs `src/import_names.inc` lists for libSceFiber resolve to their functions; the four
    /// exports it does not list yet are reachable once it does (the lookup goes by symbol).
    #[test]
    fn the_nids_resolve() {
        let nids = [
            ("hVYD7Ou2pCQ#libSceFiber", "_sceFiberInitializeImpl"),
            ("asjUJJ+aa8s#libSceFiber", "sceFiberOptParamInitialize"),
            ("JeNX5F-NzQU#libSceFiber", "sceFiberFinalize"),
            ("a0LLrZWac0M#libSceFiber", "sceFiberRun"),
            ("PFT2S-tJ7Uk#libSceFiber", "sceFiberSwitch"),
            ("p+zLIOg27zU#libSceFiber", "sceFiberGetSelf"),
            ("B0ZX2hx9DMw#libSceFiber", "sceFiberReturnToThread"),
            ("uq2Y5BFz0PE#libSceFiber", "sceFiberGetInfo"),
            ("JzyT91ucGDc#libSceFiber", "sceFiberRename"),
        ];
        for (nid, symbol) in nids {
            assert_eq!(crate::abi::nid_name(nid), Some(symbol));
            let expected = exports()
                .iter()
                .find(|(name, _)| *name == symbol)
                .unwrap()
                .1;
            assert_ne!(expected, 0);
            let nid = std::ffi::CString::new(nid).unwrap();
            // SAFETY: a NUL-terminated string.
            assert_eq!(unsafe { runtime_fiber_resolve(nid.as_ptr()) }, expected);
        }
        // SAFETY: NUL-terminated literals, and null.
        unsafe {
            assert_eq!(runtime_fiber_resolve(c"aI+OeCz8xrQ#p#J".as_ptr()), 0);
            assert_eq!(runtime_fiber_resolve(c"unknown".as_ptr()), 0);
            assert_eq!(runtime_fiber_resolve(ptr::null()), 0);
        }
    }
}
