//! Guest threads on host pthreads (piece 3.13): the Linux x86-64 port of `src/runtime_thread.c`,
//! which Windows still builds until piece 3.15.
//!
//! Each guest thread owns a FreeBSD-style TCB (variant II: static TLS below the TCB). The loader
//! rewrites the eboot's `mov rax, fs:[0]` into `mov rax, gs:[0]`, so the GS base is the guest TCB
//! while glibc keeps FS. Priorities and affinity are recorded, not applied: only the Windows C
//! applies them (`BB_THREAD_PRIO`, `BB_AFFINITY`).
//!
//! The guest functions are not linker symbols (the C ones were `static`):
//! [`runtime_thread_resolve`] hands out their addresses by NID. The `runtime_*` exports are the
//! loader's (`src/probe.c`), the other C modules' ([`runtime_errno`]) and the GPU library's
//! ([`runtime_thread_attach_host`], [`runtime_thread_spawn`], [`runtime_thread_join_spawned`]).
//!
//! `scePthreadExit` leaves the guest frames without unwinding them (they have no registered
//! FDEs). The C used `setjmp` and `longjmp`; here a small assembly trampoline keeps the stack
//! pointer of the thread's start and jumps back to it.

use crate::abi::host;
use crate::memory::linux::runtime_low_map;
use crate::memory::say;
use std::alloc::Layout;
use std::cell::Cell;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::mem::MaybeUninit;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

const fn error(n: u32) -> i32 {
    (0x8002_0000 | n) as i32
}
/// `ESRCH`: not a guest thread (or one the host owns).
const NO_THREAD: i32 = error(3);
/// `EDEADLK`: a thread joining itself.
const DEADLOCK: i32 = error(11);
/// `EINVAL`.
const INVALID: i32 = error(22);

const STACK_MARGIN: u64 = 256 * 1024;
const MIN_STACK: u64 = 64 * 1024;
const DEFAULT_STACK: u64 = 1024 * 1024;
const DEFAULT_PRIO: i32 = 700;

/// A guest thread's entry function.
type GuestEntry = unsafe extern "C" fn(*mut c_void) -> *mut c_void;

/// The scheduling attributes of a thread or of a `ScePthreadAttr`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Attr {
    detached: c_int,
    policy: c_int,
    prio: c_int,
    inherit: c_int,
    stack: u64,
    guard: u64,
    affinity: u64,
}

impl Attr {
    /// A thread's attributes when none are given (the C `new_thread`).
    const THREAD: Attr = Attr {
        detached: 0,
        policy: 1,
        prio: DEFAULT_PRIO,
        inherit: 0,
        stack: DEFAULT_STACK,
        guard: 0,
        affinity: 0x7f,
    };
    /// What `scePthreadAttrInit` gives.
    const INIT: Attr = Attr {
        inherit: 4,
        guard: 4096,
        ..Attr::THREAD
    };
}

/// What a `ScePthreadAttr` points at. The guest only passes the pointer back; it counts as
/// valid while it is in [`Registry::attributes`].
struct ThreadAttr {
    values: Attr,
}

/// A guest thread (the `ScePthread` handle). Records are never freed, as in the C: the guest
/// may compare or query a handle after the thread ended.
struct GuestThread {
    host: AtomicU64,
    entry: Option<GuestEntry>,
    argument: usize,
    result: AtomicUsize,
    attr: Mutex<Attr>,
    name: [AtomicU8; 32],
    detached: AtomicBool,
    joined: AtomicBool,
    finished: AtomicBool,
    host_owned: bool,
    /// The stack pointer `scePthreadExit` returns to (the trampoline's frame).
    exit_sp: AtomicUsize,
}

impl GuestThread {
    fn leak(
        attr: Attr,
        name: &[u8],
        entry: Option<GuestEntry>,
        argument: usize,
        host_owned: bool,
    ) -> &'static GuestThread {
        let thread = Box::leak(Box::new(GuestThread {
            host: AtomicU64::new(0),
            entry,
            argument,
            result: AtomicUsize::new(0),
            attr: Mutex::new(attr),
            name: [const { AtomicU8::new(0) }; 32],
            detached: AtomicBool::new(attr.detached != 0),
            joined: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            host_owned,
            exit_sp: AtomicUsize::new(0),
        }));
        thread.set_name(name);
        thread
    }

    /// `snprintf(name, 32, "%s", name)`: at most 31 bytes, NUL-terminated.
    fn set_name(&self, name: &[u8]) {
        let length = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        let length = length.min(self.name.len() - 1);
        for (i, byte) in self.name.iter().enumerate() {
            byte.store(if i < length { name[i] } else { 0 }, Ordering::Relaxed);
        }
    }

    fn name_bytes(&self) -> Vec<u8> {
        self.name
            .iter()
            .map(|byte| byte.load(Ordering::Relaxed))
            .take_while(|&b| b != 0)
            .collect()
    }

    fn attr(&self) -> MutexGuard<'_, Attr> {
        self.attr.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn handle(&'static self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }
}

struct Registry {
    /// `Box<ThreadAttr>` addresses (`Box::into_raw`), live until `scePthreadAttrDestroy`.
    attributes: Vec<usize>,
    threads: Vec<&'static GuestThread>,
    created: usize,
    exited: usize,
    joined: usize,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    attributes: Vec::new(),
    threads: Vec::new(),
    created: 0,
    exited: 0,
    joined: 0,
});

fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(PoisonError::into_inner)
}

thread_local! {
    static CURRENT: Cell<Option<&'static GuestThread>> = const { Cell::new(None) };
    static GUEST_ERRNO: Cell<i32> = const { Cell::new(0) };
    static THREAD_ID: Cell<i32> = const { Cell::new(0) };
}

fn current_record() -> Option<&'static GuestThread> {
    CURRENT.with(Cell::get)
}

fn is_current(thread: &GuestThread) -> bool {
    current_record().is_some_and(|current| ptr::eq(current, thread))
}

/// The main executable's TLS image (`runtime_set_main_tls`).
static TLS_TEMPLATE: AtomicUsize = AtomicUsize::new(0);
static TLS_FILESZ: AtomicU64 = AtomicU64::new(0);
static TLS_MEMSZ: AtomicU64 = AtomicU64::new(0);
static TLS_ALIGN: AtomicU64 = AtomicU64::new(16);
/// The DTV every TCB points at (static module only).
static DTV: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

/// Registers the main executable's TLS image: `filesz` initialized bytes at `data`, `memsz` in
/// all, aligned to `align` (0 means 16). Threads attached afterwards get a copy.
///
/// # Safety
/// `data` holds `filesz` readable bytes for the rest of the process.
#[no_mangle]
pub unsafe extern "C" fn runtime_set_main_tls(
    data: *const c_void,
    filesz: u64,
    memsz: u64,
    align: u64,
) {
    TLS_TEMPLATE.store(data as usize, Ordering::Release);
    TLS_FILESZ.store(filesz, Ordering::Release);
    TLS_MEMSZ.store(memsz, Ordering::Release);
    TLS_ALIGN.store(if align == 0 { 16 } else { align }, Ordering::Release);
}

/// Where the TCB sits in the TLS block: the static TLS size rounded up to its alignment.
fn tls_offset() -> u64 {
    let align = TLS_ALIGN.load(Ordering::Acquire);
    TLS_MEMSZ.load(Ordering::Acquire).wrapping_add(align - 1) & !(align - 1)
}

fn set_gs(base: u64) {
    const ARCH_SET_GS: libc::c_long = 0x1001;
    // SAFETY: changes only this thread's GS base, which neither Rust nor glibc uses (both use FS).
    if unsafe { libc::syscall(libc::SYS_arch_prctl, ARCH_SET_GS, base) } != 0 {
        // SAFETY: a NUL-terminated message.
        unsafe { libc::perror(c"STOP: arch_prctl(ARCH_SET_GS)".as_ptr()) };
        host::quit(21);
    }
}

/// Builds the TCB and static TLS for the calling host thread and points GS at it.
fn attach(thread: &'static GuestThread) {
    let offset = tls_offset() as usize;
    let total = offset + 256;
    let Ok(layout) = Layout::from_size_align((total + 63) & !63, 64) else {
        host::err("Cannot allocate guest TLS\n");
        std::process::exit(1);
    };
    // SAFETY: the layout has a non-zero size.
    let block = unsafe { std::alloc::alloc_zeroed(layout) };
    if block.is_null() {
        host::err("Cannot allocate guest TLS\n");
        std::process::exit(1);
    }
    let filesz = TLS_FILESZ.load(Ordering::Acquire) as usize;
    let template = TLS_TEMPLATE.load(Ordering::Acquire) as *const u8;
    if filesz != 0 && !template.is_null() {
        // SAFETY: `runtime_set_main_tls`'s caller keeps `filesz` bytes at `template`, and
        // `filesz <= memsz <= offset`, inside the block.
        unsafe { ptr::copy_nonoverlapping(template, block, filesz.min(offset)) };
    }
    // SAFETY: `offset + 24 <= total`, inside the block; the block lives until the process ends.
    unsafe {
        let tcb = block.add(offset).cast::<u64>();
        tcb.write_unaligned(tcb as u64); // tcb_self
        tcb.add(1).write_unaligned(DTV.as_ptr() as u64); // tcb_dtv (static module only)
        tcb.add(2).write_unaligned(thread.handle() as u64); // tcb_thread
        set_gs(tcb as u64);
    }
    CURRENT.with(|current| current.set(Some(thread)));
}

fn publish(thread: &'static GuestThread) {
    registry().threads.push(thread);
}

fn current() -> &'static GuestThread {
    if let Some(thread) = current_record() {
        return thread;
    }
    let thread = GuestThread::leak(Attr::THREAD, b"host", None, 0, true);
    // SAFETY: no preconditions.
    thread
        .host
        .store(unsafe { libc::pthread_self() }, Ordering::Release);
    attach(thread);
    publish(thread);
    thread
}

/// The calling thread's guest thread. Host threads that call into guest code without being
/// created by the guest (the main thread, test threads) get a record on first use.
#[no_mangle]
pub extern "C" fn runtime_thread_current() -> *mut c_void {
    current().handle()
}

/// Host threads that call guest code (HLE decoders invoking guest callbacks) need a TCB and TLS
/// like guest threads; the record lives until the process ends.
///
/// # Safety
/// `name` is null or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn runtime_thread_attach_host(name: *const c_char) {
    let name = if name.is_null() {
        b"host".as_slice()
    } else {
        // SAFETY: the caller guarantees a NUL-terminated string.
        unsafe { CStr::from_ptr(name) }.to_bytes()
    };
    current().set_name(name);
}

/// For fault reports (no allocation, any thread): the guest thread running here, or null.
#[no_mangle]
pub extern "C" fn runtime_thread_name_here() -> *const c_char {
    current_record().map_or(ptr::null(), |thread| thread.name.as_ptr().cast())
}

/// The loader's thread becomes the guest's main thread.
#[no_mangle]
pub extern "C" fn runtime_thread_attach_main() {
    current().set_name(b"main");
}

fn find_thread(handle: *const c_void) -> Option<&'static GuestThread> {
    registry()
        .threads
        .iter()
        .copied()
        .find(|thread| ptr::eq(thread.handle(), handle))
}

/// Runs `f` on the attributes `*slot` names, under the registry lock; `None` when `slot` or
/// `*slot` is null or not a live attribute object.
///
/// # Safety
/// `slot` is null or readable.
unsafe fn with_attr<R>(slot: *const *mut ThreadAttr, f: impl FnOnce(&mut Attr) -> R) -> Option<R> {
    if slot.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees `slot` is readable.
    let handle = unsafe { slot.read() };
    if handle.is_null() {
        return None;
    }
    let registry = registry();
    registry.attributes.contains(&(handle as usize)).then(|| {
        // SAFETY: a live `Box<ThreadAttr>`; the registry lock serializes every access to it.
        f(unsafe { &mut (*handle).values })
    })
}

/// `with_attr`, or `EINVAL`.
///
/// # Safety
/// As [`with_attr`].
unsafe fn attr_op(slot: *const *mut ThreadAttr, f: impl FnOnce(&mut Attr) -> i32) -> i32 {
    // SAFETY: forwarded from the caller.
    unsafe { with_attr(slot, f) }.unwrap_or(INVALID)
}

/// Writes `value` through a guest out-pointer: `EINVAL` when it is null.
///
/// # Safety
/// `out` is null or writable.
unsafe fn put<T>(out: *mut T, value: T) -> i32 {
    if out.is_null() {
        return INVALID;
    }
    // SAFETY: the caller guarantees `out` is writable.
    unsafe { out.write(value) };
    0
}

extern "C" fn thread_self() -> *mut c_void {
    runtime_thread_current()
}

/// `scePthreadGetthreadid`: a small unique number per thread (Uncharted 3 tags its job workers).
extern "C" fn thread_get_id() -> i32 {
    static NEXT_ID: AtomicI32 = AtomicI32::new(0);
    THREAD_ID.with(|id| {
        if id.get() == 0 {
            id.set(NEXT_ID.fetch_add(1, Ordering::Relaxed) + 1);
        }
        id.get()
    })
}

extern "C" fn guest_error() -> *mut i32 {
    runtime_errno()
}

/// The guest's `errno` of the calling thread (`__error`); the C modules set it.
#[no_mangle]
pub extern "C" fn runtime_errno() -> *mut i32 {
    GUEST_ERRNO.with(Cell::as_ptr)
}

unsafe extern "C" fn attr_init(out: *mut *mut ThreadAttr) -> i32 {
    if out.is_null() {
        return INVALID;
    }
    let attr = Box::into_raw(Box::new(ThreadAttr { values: Attr::INIT }));
    registry().attributes.push(attr as usize);
    // SAFETY: checked non-null; the guest passes a writable slot.
    unsafe { out.write(attr) };
    0
}

unsafe extern "C" fn attr_destroy(slot: *mut *mut ThreadAttr) -> i32 {
    if slot.is_null() {
        return INVALID;
    }
    // SAFETY: checked non-null; the guest passes a readable slot.
    let handle = unsafe { slot.read() };
    {
        let mut registry = registry();
        let Some(index) = registry
            .attributes
            .iter()
            .position(|&attr| attr == handle as usize)
        else {
            return INVALID;
        };
        registry.attributes.swap_remove(index);
    }
    // SAFETY: made by `Box::into_raw` in `attr_init` and just taken out of the registry.
    drop(unsafe { Box::from_raw(handle) });
    // SAFETY: the slot was readable above and is the guest's own variable.
    unsafe { slot.write(ptr::null_mut()) };
    0
}

unsafe extern "C" fn attr_get(thread: *mut c_void, out: *mut *mut ThreadAttr) -> i32 {
    // SAFETY: the guest passes a readable slot or null.
    if unsafe { with_attr(out, |_| ()) }.is_none() || thread.is_null() {
        return INVALID;
    }
    let Some(thread) = find_thread(thread) else {
        return NO_THREAD;
    };
    let mut values = *thread.attr();
    values.detached = c_int::from(thread.detached.load(Ordering::Acquire));
    // SAFETY: as above.
    unsafe {
        attr_op(out, |attr| {
            *attr = values;
            0
        })
    }
}

unsafe extern "C" fn attr_set_stack(slot: *mut *mut ThreadAttr, size: u64) -> i32 {
    // SAFETY: the guest passes a readable slot or null.
    unsafe {
        attr_op(slot, |attr| {
            if size < 16384 {
                return INVALID;
            }
            attr.stack = size;
            0
        })
    }
}

unsafe extern "C" fn attr_get_stack(slot: *mut *mut ThreadAttr, size: *mut u64) -> i32 {
    // SAFETY: the guest passes a readable slot and a writable `size`, or nulls.
    unsafe { attr_op(slot, |attr| put(size, attr.stack)) }
}

unsafe extern "C" fn attr_set_detach(slot: *mut *mut ThreadAttr, state: c_int) -> i32 {
    // SAFETY: the guest passes a readable slot or null.
    unsafe {
        attr_op(slot, |attr| {
            if state != 0 && state != 1 {
                return INVALID;
            }
            attr.detached = state;
            0
        })
    }
}

unsafe extern "C" fn attr_get_detach(slot: *mut *mut ThreadAttr, state: *mut c_int) -> i32 {
    // SAFETY: the guest passes a readable slot and a writable `state`, or nulls.
    unsafe { attr_op(slot, |attr| put(state, attr.detached)) }
}

unsafe extern "C" fn attr_set_policy(slot: *mut *mut ThreadAttr, policy: c_int) -> i32 {
    // SAFETY: the guest passes a readable slot or null.
    unsafe {
        attr_op(slot, |attr| {
            if !(1..=3).contains(&policy) {
                return INVALID;
            }
            attr.policy = policy;
            0
        })
    }
}

unsafe extern "C" fn attr_set_inherit(slot: *mut *mut ThreadAttr, inherit: c_int) -> i32 {
    // SAFETY: the guest passes a readable slot or null.
    unsafe {
        attr_op(slot, |attr| {
            if inherit != 0 && inherit != 4 {
                return INVALID;
            }
            attr.inherit = inherit;
            0
        })
    }
}

unsafe extern "C" fn attr_set_param(slot: *mut *mut ThreadAttr, param: *const c_int) -> i32 {
    // SAFETY: the guest passes a readable slot and `param`, or nulls.
    unsafe {
        attr_op(slot, |attr| {
            if param.is_null() {
                return INVALID;
            }
            attr.prio = param.read();
            0
        })
    }
}

unsafe extern "C" fn attr_get_param(slot: *mut *mut ThreadAttr, param: *mut c_int) -> i32 {
    // SAFETY: the guest passes a readable slot and a writable `param`, or nulls.
    unsafe { attr_op(slot, |attr| put(param, attr.prio)) }
}

unsafe extern "C" fn attr_set_affinity(slot: *mut *mut ThreadAttr, mask: u64) -> i32 {
    // SAFETY: the guest passes a readable slot or null.
    unsafe {
        attr_op(slot, |attr| {
            attr.affinity = mask;
            0
        })
    }
}

unsafe extern "C" fn attr_affinity(slot: *mut *mut ThreadAttr, mask: *mut u64) -> i32 {
    // SAFETY: the guest passes a readable slot and a writable `mask`, or nulls.
    unsafe { attr_op(slot, |attr| put(mask, attr.affinity)) }
}

unsafe extern "C" fn attr_set_guard(slot: *mut *mut ThreadAttr, size: u64) -> i32 {
    // SAFETY: the guest passes a readable slot or null.
    unsafe {
        attr_op(slot, |attr| {
            attr.guard = size;
            0
        })
    }
}

/// Linux thread names hold 15 bytes; longer ones would be refused.
fn set_host_name(name: &[u8]) {
    let mut host = [0u8; 16];
    let length = name.len().min(host.len() - 1);
    host[..length].copy_from_slice(&name[..length]);
    // SAFETY: a NUL-terminated name of at most 15 bytes for the calling thread.
    unsafe { libc::pthread_setname_np(libc::pthread_self(), host.as_ptr().cast()) };
}

// The trampoline of `scePthreadExit` and the FPU setter (System V, x86-64).
//
// `uncharted_thread_enter(entry, argument, exit_sp)` saves the callee-saved registers, stores the
// stack pointer in `*exit_sp` (16-byte aligned) and calls `entry(argument)`; its result comes
// back in rax. `uncharted_thread_leave(sp, value)` drops every frame above `sp` and returns
// `value` from that `uncharted_thread_enter`, as `longjmp` did.
//
// `uncharted_thread_set_fpu(mxcsr)` loads MXCSR and the x87 control word 0x037f.
std::arch::global_asm!(
    ".text",
    ".p2align 4",
    ".globl uncharted_thread_enter",
    ".hidden uncharted_thread_enter",
    ".type uncharted_thread_enter, @function",
    "uncharted_thread_enter:",
    "push rbp",
    "push rbx",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "sub rsp, 8",
    "mov qword ptr [rdx], rsp",
    "mov rax, rdi",
    "mov rdi, rsi",
    "call rax",
    "2:",
    "add rsp, 8",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbx",
    "pop rbp",
    "ret",
    ".size uncharted_thread_enter, . - uncharted_thread_enter",
    ".p2align 4",
    ".globl uncharted_thread_leave",
    ".hidden uncharted_thread_leave",
    ".type uncharted_thread_leave, @function",
    "uncharted_thread_leave:",
    "mov rsp, rdi",
    "mov rax, rsi",
    "jmp 2b",
    ".size uncharted_thread_leave, . - uncharted_thread_leave",
    ".p2align 4",
    ".globl uncharted_thread_set_fpu",
    ".hidden uncharted_thread_set_fpu",
    ".type uncharted_thread_set_fpu, @function",
    "uncharted_thread_set_fpu:",
    "mov dword ptr [rsp - 8], edi",
    "ldmxcsr dword ptr [rsp - 8]",
    "mov word ptr [rsp - 8], 0x037f",
    "fldcw word ptr [rsp - 8]",
    "ret",
    ".size uncharted_thread_set_fpu, . - uncharted_thread_set_fpu",
);

extern "C" {
    fn uncharted_thread_enter(
        entry: GuestEntry,
        argument: *mut c_void,
        exit_sp: *mut usize,
    ) -> *mut c_void;
    fn uncharted_thread_leave(sp: usize, value: *mut c_void) -> !;
    fn uncharted_thread_set_fpu(mxcsr: u32);
}

/// `strtoul(text, NULL, 16)` cut to 32 bits, as the C read `BB_GUEST_MXCSR`.
fn parse_mxcsr(text: &str) -> u32 {
    let text = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text.strip_prefix('+').unwrap_or(text)),
    };
    let digits = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
        .filter(|rest| rest.starts_with(|c: char| c.is_ascii_hexdigit()))
        .unwrap_or(digits);
    crate::memory::strtoull(&format!("{sign}{digits}"), 16) as u32
}

/// Guest threads start with the PS4's FPU state: MXCSR 0x9fc0 (the value sceFiber uses for a
/// fresh FPU state: flush to zero, denormals are zero) and the x87 control word 0x037f. The
/// game's fibers already ran with it while its threads used the host's 0x1f80, so the same
/// physics job could compute differently depending on where it ran (U2's floating NPCs, #33).
/// `BB_GUEST_MXCSR=<hex>` picks another value (0 leaves the host's); `BB_GUEST_MXCSR=1f80` is
/// the old host default.
#[no_mangle]
pub extern "C" fn runtime_set_guest_fpu() {
    static VALUE: OnceLock<u32> = OnceLock::new();
    let value = *VALUE.get_or_init(|| {
        std::env::var_os("BB_GUEST_MXCSR")
            .map_or(0x9fc0, |text| parse_mxcsr(&text.to_string_lossy()))
    });
    if value != 0 {
        // SAFETY: loads this thread's MXCSR and x87 control word; nothing else changes.
        unsafe { uncharted_thread_set_fpu(value) };
    }
}

/// Runs the guest's thread-specific data destructors (`runtime_kernel.c`, loader build only).
#[cfg(uncharted_loader)]
fn keys_cleanup() {
    extern "C" {
        fn runtime_thread_keys_cleanup();
    }
    // SAFETY: called on the exiting guest thread, after its entry returned, as in the C.
    unsafe { runtime_thread_keys_cleanup() };
}

#[cfg(not(uncharted_loader))]
fn keys_cleanup() {}

extern "C" fn host_start(record: *mut c_void) -> *mut c_void {
    // SAFETY: `create` passes a leaked `GuestThread`.
    let thread: &'static GuestThread = unsafe { &*record.cast::<GuestThread>() };
    attach(thread);
    set_host_name(&thread.name_bytes());
    runtime_set_guest_fpu();
    let result = match thread.entry {
        // SAFETY: the guest's entry with its argument; `scePthreadExit` comes back here through
        // `uncharted_thread_leave` with this thread's `exit_sp`.
        Some(entry) => unsafe {
            uncharted_thread_enter(
                entry,
                thread.argument as *mut c_void,
                thread.exit_sp.as_ptr(),
            )
        },
        None => ptr::null_mut(),
    };
    thread.result.store(result as usize, Ordering::Release);
    keys_cleanup();
    let mut registry = registry();
    thread.finished.store(true, Ordering::Release);
    registry.exited += 1;
    result
}

/// `scePthreadCreate` and the POSIX variants.
///
/// # Safety
/// `out` is null or writable, `attr_slot` null or readable, `name` null or a NUL-terminated
/// string.
unsafe fn create(
    out: *mut *mut c_void,
    attr_slot: *const *mut ThreadAttr,
    entry: Option<GuestEntry>,
    argument: *mut c_void,
    name: *const c_char,
) -> i32 {
    let Some(entry) = entry.filter(|_| !out.is_null()) else {
        return INVALID;
    };
    let attr = if attr_slot.is_null() {
        Attr::THREAD
    } else {
        // SAFETY: the caller guarantees `attr_slot` is readable.
        match unsafe { with_attr(attr_slot, |attr| *attr) } {
            Some(attr) => attr,
            None => return INVALID,
        }
    };
    let name = if name.is_null() {
        b"guest".as_slice()
    } else {
        // SAFETY: the caller guarantees a NUL-terminated string.
        unsafe { CStr::from_ptr(name) }.to_bytes()
    };
    let thread = GuestThread::leak(attr, name, Some(entry), argument as usize, false);
    let stack = attr.stack.max(MIN_STACK);
    // Stacks below 1 TiB as on the PS4; guest code may pack stack addresses.
    let stack_bytes = (stack + STACK_MARGIN) as usize;
    let mut host = MaybeUninit::<libc::pthread_attr_t>::uninit();
    // SAFETY: `pthread_attr_init` initializes `host`; the stack is a fresh mapping of
    // `stack_bytes` that the thread owns from now on.
    unsafe {
        libc::pthread_attr_init(host.as_mut_ptr());
        let memory = runtime_low_map(stack_bytes, libc::PROT_READ | libc::PROT_WRITE);
        if memory.is_null() {
            libc::pthread_attr_setstacksize(host.as_mut_ptr(), stack_bytes);
        } else {
            libc::pthread_attr_setstack(host.as_mut_ptr(), memory, stack_bytes);
        }
    }
    publish(thread);
    // Publish the handle before the thread can run and inspect itself.
    // SAFETY: checked non-null; the guest passes a writable slot.
    unsafe { out.write(thread.handle()) };
    // SAFETY: `host` is initialized; the record outlives the thread.
    let e = unsafe {
        let e = libc::pthread_create(
            thread.host.as_ptr(),
            host.as_ptr(),
            host_start,
            thread.handle(),
        );
        libc::pthread_attr_destroy(host.as_mut_ptr());
        e
    };
    if e != 0 {
        host::err(&format!("STOP: host pthread_create failed: {e}\n"));
        host::quit(21);
    }
    if thread.detached.load(Ordering::Acquire) {
        // SAFETY: a thread just created, not joined or detached yet.
        unsafe { libc::pthread_detach(thread.host.load(Ordering::Acquire)) };
    }
    registry().created += 1;
    say(&format!(
        "Runtime: guest thread '{}' created (stack={stack}, prio={})",
        String::from_utf8_lossy(&thread.name_bytes()),
        attr.prio
    ));
    0
}

unsafe extern "C" fn thread_create(
    out: *mut *mut c_void,
    attr: *mut *mut ThreadAttr,
    entry: Option<GuestEntry>,
    argument: *mut c_void,
    name: *const c_char,
) -> i32 {
    // SAFETY: forwarded from the guest.
    unsafe { create(out, attr, entry, argument, name) }
}

unsafe extern "C" fn thread_join(thread: *mut c_void, result: *mut *mut c_void) -> i32 {
    let Some(thread) = find_thread(thread).filter(|thread| !thread.host_owned) else {
        return NO_THREAD;
    };
    if is_current(thread) {
        return DEADLOCK;
    }
    if thread.detached.load(Ordering::Acquire) || thread.joined.swap(true, Ordering::AcqRel) {
        return INVALID;
    }
    let mut value = ptr::null_mut();
    // SAFETY: a guest-created thread, neither detached nor joined before.
    let e = unsafe { libc::pthread_join(thread.host.load(Ordering::Acquire), &mut value) };
    if e != 0 {
        host::err(&format!("STOP: host pthread_join failed: {e}\n"));
        host::quit(21);
    }
    if !result.is_null() {
        // SAFETY: checked non-null; the guest passes a writable slot.
        unsafe { result.write(thread.result.load(Ordering::Acquire) as *mut c_void) };
    }
    registry().joined += 1;
    0
}

extern "C" fn thread_detach(thread: *mut c_void) -> i32 {
    let Some(thread) = find_thread(thread) else {
        return NO_THREAD;
    };
    if thread.detached.swap(true, Ordering::AcqRel) {
        return INVALID;
    }
    if !thread.host_owned {
        // SAFETY: a guest-created thread, detached once.
        unsafe { libc::pthread_detach(thread.host.load(Ordering::Acquire)) };
    }
    0
}

extern "C" fn thread_exit(value: *mut c_void) -> ! {
    let thread = current();
    if thread.host_owned {
        host::err("STOP: pthread_exit on host-owned/main thread\n");
        host::quit(21);
    }
    thread.result.store(value as usize, Ordering::Release);
    // SAFETY: a guest-created thread is running inside `uncharted_thread_enter` of its
    // `host_start`, whose frame `exit_sp` holds; the frames above it are guest frames and this
    // one, none with anything to drop.
    unsafe { uncharted_thread_leave(thread.exit_sp.load(Ordering::Acquire), value) }
}

extern "C" fn thread_yield() -> i32 {
    // SAFETY: no preconditions.
    unsafe { libc::sched_yield() };
    0
}

unsafe extern "C" fn thread_get_prio(thread: *mut c_void, prio: *mut c_int) -> i32 {
    let Some(thread) = find_thread(thread) else {
        return NO_THREAD;
    };
    let value = thread.attr().prio;
    // SAFETY: the guest passes a writable `prio` or null.
    unsafe { put(prio, value) }
}

/// PS4 priorities run from 256 (highest) to 767 (lowest), 700 being the default. Only the
/// Windows C maps them to host priorities (`BB_THREAD_PRIO=1`); here they are recorded.
extern "C" fn thread_set_prio(thread: *mut c_void, prio: c_int) -> i32 {
    let Some(thread) = find_thread(thread) else {
        return NO_THREAD;
    };
    thread.attr().prio = prio;
    0
}

/// The guest's core masks: only the Windows C applies them (`BB_AFFINITY=1`).
extern "C" fn thread_set_affinity(thread: *mut c_void, mask: u64) -> i32 {
    let Some(thread) = find_thread(thread) else {
        return NO_THREAD;
    };
    thread.attr().affinity = mask;
    0
}

unsafe extern "C" fn thread_get_affinity(thread: *mut c_void, mask: *mut u64) -> i32 {
    let Some(thread) = find_thread(thread) else {
        return NO_THREAD;
    };
    let value = thread.attr().affinity;
    // SAFETY: the guest passes a writable `mask` or null.
    unsafe { put(mask, value) }
}

unsafe extern "C" fn thread_rename(thread: *mut c_void, name: *const c_char) -> i32 {
    let Some(thread) = find_thread(thread) else {
        return NO_THREAD;
    };
    if name.is_null() {
        return INVALID;
    }
    // SAFETY: checked non-null; the guest passes a NUL-terminated string.
    thread.set_name(unsafe { CStr::from_ptr(name) }.to_bytes());
    if is_current(thread) {
        set_host_name(&thread.name_bytes());
    }
    0
}

extern "C" fn thread_equal(a: *mut c_void, b: *mut c_void) -> i32 {
    i32::from(a == b)
}

/// POSIX (libScePosix) variants return positive errno values.
fn posix(result: i32) -> i32 {
    if result == 0 {
        0
    } else {
        (result as u32 & 0xffff) as i32
    }
}

unsafe extern "C" fn posix_attr_init(attr: *mut *mut ThreadAttr) -> i32 {
    // SAFETY: forwarded from the guest.
    posix(unsafe { attr_init(attr) })
}

unsafe extern "C" fn posix_attr_destroy(attr: *mut *mut ThreadAttr) -> i32 {
    // SAFETY: forwarded from the guest.
    posix(unsafe { attr_destroy(attr) })
}

unsafe extern "C" fn posix_attr_set_detach(attr: *mut *mut ThreadAttr, state: c_int) -> i32 {
    // SAFETY: forwarded from the guest.
    posix(unsafe { attr_set_detach(attr, state) })
}

unsafe extern "C" fn posix_attr_set_stack(attr: *mut *mut ThreadAttr, size: u64) -> i32 {
    // SAFETY: forwarded from the guest.
    posix(unsafe { attr_set_stack(attr, size) })
}

unsafe extern "C" fn posix_attr_set_param(attr: *mut *mut ThreadAttr, param: *const c_int) -> i32 {
    // SAFETY: forwarded from the guest.
    posix(unsafe { attr_set_param(attr, param) })
}

unsafe extern "C" fn posix_create(
    thread: *mut *mut c_void,
    attr: *mut *mut ThreadAttr,
    entry: Option<GuestEntry>,
    argument: *mut c_void,
) -> i32 {
    // SAFETY: forwarded from the guest.
    posix(unsafe { create(thread, attr, entry, argument, c"posix".as_ptr()) })
}

unsafe extern "C" fn posix_create_name(
    thread: *mut *mut c_void,
    attr: *mut *mut ThreadAttr,
    entry: Option<GuestEntry>,
    argument: *mut c_void,
    name: *const c_char,
) -> i32 {
    // SAFETY: forwarded from the guest.
    posix(unsafe { create(thread, attr, entry, argument, name) })
}

unsafe extern "C" fn posix_join(thread: *mut c_void, result: *mut *mut c_void) -> i32 {
    // SAFETY: forwarded from the guest.
    posix(unsafe { thread_join(thread, result) })
}

/// Co-op: a guest thread started by a host library (the NP matching and signaling threads call
/// the game's callbacks), with a `stack`-byte stack (0: the default). Returns 0 or a positive
/// errno value.
///
/// # Safety
/// `out` is writable, `name` null or a NUL-terminated string, `entry` guest code taking
/// `argument`.
#[no_mangle]
pub unsafe extern "C" fn runtime_thread_spawn(
    out: *mut *mut c_void,
    entry: Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void>,
    argument: *mut c_void,
    stack: u64,
    name: *const c_char,
) -> i32 {
    let mut attr = ptr::null_mut();
    // SAFETY: a local slot; the rest is forwarded from the caller.
    unsafe {
        let mut result = attr_init(&mut attr);
        if result == 0 && stack != 0 {
            result = attr_set_stack(&mut attr, stack);
        }
        if result == 0 {
            result = create(out, &attr, entry, argument, name);
        }
        if !attr.is_null() {
            attr_destroy(&mut attr);
        }
        posix(result)
    }
}

/// Joins a thread [`runtime_thread_spawn`] started: 0 or a positive errno value.
///
/// # Safety
/// `result` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn runtime_thread_join_spawned(
    thread: *mut c_void,
    result: *mut *mut c_void,
) -> i32 {
    // SAFETY: forwarded from the caller.
    posix(unsafe { thread_join(thread, result) })
}

/// Guest import NIDs (`src/import_names.inc` where it lists them) and the functions behind
/// them, the C `runtime_thread_resolve` table in its order.
fn table() -> [(&'static str, usize); 38] {
    [
        ("aI+OeCz8xrQ#p#J", thread_self as *const () as usize),
        ("EotR8a3ASf4#I#J", thread_self as *const () as usize),
        ("EI-5-jlq2dE#p#J", thread_get_id as *const () as usize),
        ("9BcDykPmo1I#p#J", guest_error as *const () as usize),
        ("nsYoNRywwNg#p#J", attr_init as *const () as usize),
        ("62KCwEMmzcM#p#J", attr_destroy as *const () as usize),
        ("x1X76arYMxU#p#J", attr_get as *const () as usize),
        ("8+s5BzZjxSg#p#J", attr_affinity as *const () as usize),
        ("UTXzJbWhhTE#p#J", attr_set_stack as *const () as usize),
        ("-Wreprtu0Qs#p#J", attr_set_detach as *const () as usize),
        ("4+h9EzwKF4I#p#J", attr_set_policy as *const () as usize),
        ("DzES9hQF4f4#p#J", attr_set_param as *const () as usize),
        ("3qxgM4ezETA#p#J", attr_set_affinity as *const () as usize),
        ("eXbUSpEaTsA#p#J", attr_set_inherit as *const () as usize),
        ("JaRMy+QcpeU#p#J", attr_get_detach as *const () as usize),
        ("-fA+7ZlGDQs#p#J", attr_get_stack as *const () as usize),
        ("FXPWHNk8Of0#p#J", attr_get_param as *const () as usize),
        ("El+cQ20DynU#p#J", attr_set_guard as *const () as usize),
        ("6UgtwV+0zb4#p#J", thread_create as *const () as usize),
        ("onNY9Byn-W8#p#J", thread_join as *const () as usize),
        ("4qGrR6eoP9Y#p#J", thread_detach as *const () as usize),
        ("3kg7rT0NQIs#p#J", thread_exit as *const () as usize),
        ("T72hz6ffq08#p#J", thread_yield as *const () as usize),
        ("1tKyG7RlMJo#p#J", thread_get_prio as *const () as usize),
        ("W0Hpm2X0uPE#p#J", thread_set_prio as *const () as usize),
        ("bt3CTBKmGyI#p#J", thread_set_affinity as *const () as usize),
        ("rcrVFJsQWRY#p#J", thread_get_affinity as *const () as usize),
        ("GBUY7ywdULE#p#J", thread_rename as *const () as usize),
        ("3PtV6p3QNX4#p#J", thread_equal as *const () as usize),
        ("wtkt-teR1so#I#J", posix_attr_init as *const () as usize),
        ("zHchY8ft5pk#I#J", posix_attr_destroy as *const () as usize),
        (
            "E+tyo3lp5Lw#I#J",
            posix_attr_set_detach as *const () as usize,
        ),
        (
            "2Q0z6rnBrTE#I#J",
            posix_attr_set_stack as *const () as usize,
        ),
        (
            "euKRgm0Vn2M#I#J",
            posix_attr_set_param as *const () as usize,
        ),
        ("OxhIB8LB-PQ#I#J", posix_create as *const () as usize),
        ("Jmi+9w9u0E4#I#J", posix_create_name as *const () as usize),
        ("h9CcP3J0oVM#I#J", posix_join as *const () as usize),
        ("FJrT5LuUBAU#I#J", thread_exit as *const () as usize),
    ]
}

/// The guest function for an import NID, or 0 (`runtime_resolve` asks each module in turn).
///
/// # Safety
/// `name` is null or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn runtime_thread_resolve(name: *const c_char) -> usize {
    if name.is_null() {
        return 0;
    }
    // SAFETY: the caller guarantees a NUL-terminated string.
    let nid = unsafe { CStr::from_ptr(name) }.to_bytes();
    table()
        .iter()
        .find(|(entry, _)| entry.as_bytes() == nid)
        .map_or(0, |&(_, function)| function)
}

/// The counter line at exit.
#[no_mangle]
pub extern "C" fn runtime_thread_report() {
    let line = {
        let registry = registry();
        format!(
            "Runtime: guest threads created={}, exited={}, joined={}",
            registry.created, registry.exited, registry.joined
        )
    };
    say(&line);
}

/// The thread and fiber tests share the guest thread counters, the TLS image and the FPU state
/// checks: one at a time.
#[cfg(test)]
pub(crate) fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The calling thread's MXCSR and x87 control word, and whether a denormal input reads as zero.
#[cfg(test)]
pub(crate) fn fpu_state() -> (u32, u16, bool) {
    let mut mxcsr = 0u32;
    let mut control = 0u16;
    // SAFETY: both store into locals and change nothing.
    unsafe {
        std::arch::asm!("stmxcsr [{}]", in(reg) &mut mxcsr, options(nostack, preserves_flags));
        std::arch::asm!("fnstcw [{}]", in(reg) &mut control, options(nostack, preserves_flags));
    }
    // A denormal: with denormals-are-zero the product is zero.
    let tiny = std::hint::black_box(1e-40f32);
    let flushed = tiny * std::hint::black_box(1.0f32) == 0.0;
    (mxcsr, control, flushed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    fn resolve(nid: &CStr) -> usize {
        // SAFETY: a NUL-terminated literal.
        let address = unsafe { runtime_thread_resolve(nid.as_ptr()) };
        assert_ne!(address, 0, "{nid:?}");
        address
    }

    type SelfFn = extern "C" fn() -> *mut c_void;
    type AttrInit = unsafe extern "C" fn(*mut *mut c_void) -> i32;
    type AttrGet = unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> i32;
    type AttrAffinity = unsafe extern "C" fn(*mut *mut c_void, *mut u64) -> i32;
    type AttrDestroy = unsafe extern "C" fn(*mut *mut c_void) -> i32;
    type AttrDetach = unsafe extern "C" fn(*mut *mut c_void, c_int) -> i32;
    type Create = unsafe extern "C" fn(
        *mut *mut c_void,
        *mut *mut c_void,
        Option<GuestEntry>,
        *mut c_void,
        *const c_char,
    ) -> i32;
    type Join = unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> i32;
    type Exit = extern "C" fn(*mut c_void) -> !;
    type PosixJoin = Join;

    fn get<T: Copy>(nid: &CStr) -> T {
        assert_eq!(size_of::<T>(), size_of::<usize>());
        let address = resolve(nid);
        // SAFETY: each test names the signature of the function behind the NID (the C's).
        unsafe { std::mem::transmute_copy::<usize, T>(&address) }
    }

    fn create_thread(entry: GuestEntry, argument: usize, name: &CStr) -> *mut c_void {
        let create: Create = get(c"6UgtwV+0zb4#p#J");
        let mut thread = ptr::null_mut();
        // SAFETY: a local out slot, no attributes, a test entry and a literal name.
        let result = unsafe {
            create(
                &mut thread,
                ptr::null_mut(),
                Some(entry),
                argument as *mut c_void,
                name.as_ptr(),
            )
        };
        assert_eq!(result, 0);
        assert!(!thread.is_null());
        thread
    }

    fn join(thread: *mut c_void) -> (i32, usize) {
        let join: Join = get(c"onNY9Byn-W8#p#J");
        let mut value = ptr::null_mut();
        // SAFETY: a local result slot.
        let result = unsafe { join(thread, &mut value) };
        (result, value as usize)
    }

    /// `test_runtime.c` `thread_attributes`: the main thread's identity and attributes, and the
    /// error codes of the attribute calls.
    #[test]
    fn thread_attributes_as_the_c_test() {
        let _serial = serial();
        let this: SelfFn = get(c"aI+OeCz8xrQ#p#J");
        let main_identity = this() as usize;
        assert_eq!(this() as usize, main_identity);
        let other = std::thread::spawn(move || this() as usize).join().unwrap();
        assert_ne!(other, main_identity);
        let init: AttrInit = get(c"nsYoNRywwNg#p#J");
        let attr_get: AttrGet = get(c"x1X76arYMxU#p#J");
        let affinity: AttrAffinity = get(c"8+s5BzZjxSg#p#J");
        let destroy: AttrDestroy = get(c"62KCwEMmzcM#p#J");
        let mut attr = ptr::null_mut();
        let mut mask = 0u64;
        // SAFETY: locals throughout; `&mut mask` stands for a handle that is not a thread.
        unsafe {
            assert_eq!(init(ptr::null_mut()) as u32, 0x8002_0016);
            assert_eq!(init(&mut attr), 0);
            assert_eq!(attr_get(main_identity as *mut c_void, &mut attr), 0);
            assert_eq!((affinity(&mut attr, &mut mask), mask), (0, 0x7f));
            let not_a_thread = ptr::addr_of_mut!(mask).cast();
            assert_eq!(attr_get(not_a_thread, &mut attr) as u32, 0x8002_0003);
            assert_eq!(affinity(&mut attr, ptr::null_mut()) as u32, 0x8002_0016);
            assert_eq!(destroy(&mut attr), 0);
            assert!(attr.is_null());
            assert_eq!(affinity(&mut attr, &mut mask) as u32, 0x8002_0016);
        }
    }

    /// The attribute setters check their values as the C did, and a thread's attributes come
    /// back through `scePthreadAttrGet`.
    #[test]
    fn attribute_values_are_checked() {
        let _serial = serial();
        let mut attr = ptr::null_mut();
        let mut value = 0u64;
        let mut state: c_int = -1;
        // SAFETY: locals throughout.
        unsafe {
            assert_eq!(attr_init(&mut attr), 0);
            assert_eq!(attr_get_stack(&mut attr, &mut value), 0);
            assert_eq!(value, DEFAULT_STACK);
            assert_eq!(attr_set_stack(&mut attr, 16383), INVALID);
            assert_eq!(attr_set_stack(&mut attr, 16384), 0);
            assert_eq!(attr_set_detach(&mut attr, 2), INVALID);
            assert_eq!(attr_set_detach(&mut attr, 1), 0);
            assert_eq!(attr_get_detach(&mut attr, &mut state), 0);
            assert_eq!(state, 1);
            assert_eq!(attr_set_policy(&mut attr, 0), INVALID);
            assert_eq!(attr_set_policy(&mut attr, 3), 0);
            assert_eq!(attr_set_inherit(&mut attr, 1), INVALID);
            assert_eq!(attr_set_inherit(&mut attr, 0), 0);
            let prio: c_int = 767;
            assert_eq!(attr_set_param(&mut attr, ptr::null()), INVALID);
            assert_eq!(attr_set_param(&mut attr, &prio), 0);
            let mut read: c_int = 0;
            assert_eq!(attr_get_param(&mut attr, &mut read), 0);
            assert_eq!(read, 767);
            assert_eq!(attr_set_affinity(&mut attr, 0x3), 0);
            assert_eq!(attr_set_guard(&mut attr, 0), 0);
            let mut copy = attr;
            assert_eq!(attr_destroy(&mut attr), 0);
            assert_eq!(attr_destroy(&mut copy), INVALID);
            assert_eq!(attr_set_stack(&mut copy, 1 << 20), INVALID);
            assert_eq!(posix_attr_init(ptr::null_mut()), 22);
        }
    }

    unsafe extern "C" fn add_one(argument: *mut c_void) -> *mut c_void {
        (argument as usize + 1) as *mut c_void
    }

    unsafe extern "C" fn exits_early(argument: *mut c_void) -> *mut c_void {
        let exit: Exit = get(c"3kg7rT0NQIs#p#J");
        exit((argument as usize * 2) as *mut c_void)
    }

    static SEEN: StdMutex<Vec<(String, i32, usize, i32)>> = StdMutex::new(Vec::new());

    /// Records the name, the thread id, the errno slot and what joining itself answers.
    unsafe extern "C" fn records_itself(argument: *mut c_void) -> *mut c_void {
        // SAFETY: the name is NUL-terminated while the thread runs.
        let name = unsafe { CStr::from_ptr(runtime_thread_name_here()) }
            .to_string_lossy()
            .into_owned();
        let id = thread_get_id();
        assert_eq!(thread_get_id(), id);
        let errno = runtime_errno() as usize;
        // SAFETY: a null result slot.
        let self_join = unsafe { thread_join(runtime_thread_current(), ptr::null_mut()) };
        SEEN.lock().unwrap().push((name, id, errno, self_join));
        argument
    }

    /// Create, join (the entry's result, and `scePthreadExit`'s value), the join errors and the
    /// counters of the report line.
    #[test]
    fn thread_lifecycle() {
        let _serial = serial();
        let before = {
            let r = registry();
            (r.created, r.exited, r.joined)
        };
        let first = create_thread(add_one, 41, c"adder");
        assert_eq!(join(first), (0, 42));
        assert_eq!(join(first).0, INVALID, "joined twice");
        let second = create_thread(exits_early, 21, c"exiter");
        assert_eq!(join(second), (0, 42));
        // SAFETY: the thread ended; the record stays.
        let record = unsafe { &*second.cast::<GuestThread>() };
        assert!(record.finished.load(Ordering::Acquire));
        assert_eq!(record.result.load(Ordering::Acquire), 42);
        let main = runtime_thread_current();
        assert_eq!(join(main).0, NO_THREAD, "host-owned");
        assert_eq!(join(ptr::null_mut()).0, NO_THREAD);
        let posix_join: PosixJoin = get(c"h9CcP3J0oVM#I#J");
        // SAFETY: a null result slot.
        assert_eq!(unsafe { posix_join(main, ptr::null_mut()) }, 3);
        let after = {
            let r = registry();
            (r.created, r.exited, r.joined)
        };
        assert_eq!(after, (before.0 + 2, before.1 + 2, before.2 + 2));
    }

    /// Each guest thread has its own id, errno and name, and joining itself is `EDEADLK`. (The
    /// two may run one after the other and reuse the same TLS, so only the live main thread's
    /// errno slot is compared.)
    #[test]
    fn threads_know_themselves() {
        let _serial = serial();
        SEEN.lock().unwrap().clear();
        let a = create_thread(records_itself, 1, c"a long guest thread name, cut at 31");
        let b = create_thread(records_itself, 2, c"second");
        assert_eq!(join(a), (0, 1));
        assert_eq!(join(b), (0, 2));
        let mut seen = SEEN.lock().unwrap().clone();
        seen.sort();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].0, "a long guest thread name, cut a");
        assert_eq!(seen[1].0, "second");
        assert_ne!(seen[0].1, seen[1].1);
        assert!(seen[0].1 > 0 && seen[1].1 > 0);
        assert_eq!((seen[0].3, seen[1].3), (DEADLOCK, DEADLOCK));
        assert_ne!(runtime_errno() as usize, seen[0].2);
    }

    /// Detached threads cannot be joined; priorities, affinity and names are recorded.
    #[test]
    fn detach_priority_affinity_and_rename() {
        let _serial = serial();
        let set_detach: AttrDetach = get(c"-Wreprtu0Qs#p#J");
        let create: Create = get(c"6UgtwV+0zb4#p#J");
        let mut attr = ptr::null_mut();
        let mut thread = ptr::null_mut();
        // SAFETY: locals and a test entry.
        unsafe {
            assert_eq!(attr_init(attr_slot(&mut attr)), 0);
            assert_eq!(set_detach(&mut attr, 1), 0);
            assert_eq!(
                create(
                    &mut thread,
                    &mut attr,
                    Some(add_one),
                    ptr::null_mut(),
                    ptr::null()
                ),
                0
            );
            assert_eq!(attr_destroy(attr_slot(&mut attr)), 0);
        }
        assert_eq!(join(thread).0, INVALID);
        assert_eq!(thread_detach(thread), INVALID);
        let worker = create_thread(add_one, 0, c"worker");
        let mut prio: c_int = 0;
        let mut mask = 0u64;
        // SAFETY: locals.
        unsafe {
            assert_eq!(thread_get_prio(worker, &mut prio), 0);
            assert_eq!(prio, DEFAULT_PRIO);
            assert_eq!(thread_set_prio(worker, 256), 0);
            assert_eq!(thread_get_prio(worker, &mut prio), 0);
            assert_eq!(prio, 256);
            assert_eq!(thread_get_prio(worker, ptr::null_mut()), INVALID);
            assert_eq!(thread_set_affinity(worker, 0x10), 0);
            assert_eq!(thread_get_affinity(worker, &mut mask), 0);
            assert_eq!(mask, 0x10);
            assert_eq!(thread_rename(worker, c"renamed".as_ptr()), 0);
            assert_eq!(thread_rename(worker, ptr::null()), INVALID);
            assert_eq!(thread_set_prio(ptr::null_mut(), 1), NO_THREAD);
            assert_eq!(thread_get_affinity(ptr::null_mut(), &mut mask), NO_THREAD);
        }
        // SAFETY: a live record.
        assert_eq!(
            unsafe { &*worker.cast::<GuestThread>() }.name_bytes(),
            b"renamed"
        );
        assert_eq!(join(worker), (0, 1));
        assert_eq!(thread_equal(worker, worker), 1);
        assert_eq!(thread_equal(worker, thread), 0);
        assert_eq!(thread_yield(), 0);
    }

    fn attr_slot(attr: &mut *mut c_void) -> *mut *mut ThreadAttr {
        ptr::from_mut(attr).cast()
    }

    /// The co-op entry points answer positive errno values.
    #[test]
    fn spawn_and_join_spawned() {
        let _serial = serial();
        let mut thread = ptr::null_mut();
        let mut value = ptr::null_mut();
        // SAFETY: locals and a test entry.
        unsafe {
            assert_eq!(
                runtime_thread_spawn(
                    &mut thread,
                    Some(add_one),
                    9 as *mut c_void,
                    256 * 1024,
                    c"np".as_ptr()
                ),
                0
            );
            assert_eq!(runtime_thread_join_spawned(thread, &mut value), 0);
            assert_eq!(value as usize, 10);
            assert_eq!(runtime_thread_join_spawned(thread, &mut value), 22);
            assert_eq!(
                runtime_thread_spawn(
                    &mut thread,
                    Some(add_one),
                    ptr::null_mut(),
                    100,
                    ptr::null()
                ),
                22,
                "stack below 16 KiB"
            );
            assert_eq!(
                runtime_thread_spawn(&mut thread, None, ptr::null_mut(), 0, ptr::null()),
                22
            );
        }
    }

    static GUEST_FPU: StdMutex<Option<(u32, u16, bool)>> = StdMutex::new(None);

    unsafe extern "C" fn reads_fpu(_: *mut c_void) -> *mut c_void {
        *GUEST_FPU.lock().unwrap() = Some(fpu_state());
        ptr::null_mut()
    }

    /// `test_runtime.c` `guest_fpu_state`, the thread half (#29): a guest thread created from a
    /// thread with the host's MXCSR 0x1f80 starts with 0x9fc0 and the x87 control word 0x037f,
    /// so a denormal reads as zero.
    #[test]
    fn guest_threads_start_with_the_ps4_fpu_state() {
        let _serial = serial();
        assert!(std::env::var_os("BB_GUEST_MXCSR").is_none());
        let (host_mxcsr, _, host_flushed) = fpu_state();
        assert_eq!(
            host_mxcsr & 0x8040,
            0,
            "the test thread keeps the host state"
        );
        assert!(!host_flushed);
        let thread = create_thread(reads_fpu, 0, c"fpu");
        assert_eq!(join(thread).0, 0);
        assert_eq!(*GUEST_FPU.lock().unwrap(), Some((0x9fc0, 0x037f, true)));
    }

    #[test]
    fn guest_mxcsr_reads_as_strtoul_base_16() {
        assert_eq!(parse_mxcsr("1f80"), 0x1f80);
        assert_eq!(parse_mxcsr(" 0x9FC0"), 0x9fc0);
        assert_eq!(parse_mxcsr("0"), 0);
        assert_eq!(parse_mxcsr("zz"), 0);
        assert_eq!(parse_mxcsr("0x"), 0);
        assert_eq!(parse_mxcsr("1f80junk"), 0x1f80);
        assert_eq!(parse_mxcsr("123456789"), 0x2345_6789);
    }

    static GS_SEEN: StdMutex<Vec<u64>> = StdMutex::new(Vec::new());

    unsafe extern "C" fn reads_tcb(_: *mut c_void) -> *mut c_void {
        let (tcb, dtv, thread): (u64, u64, u64);
        // SAFETY: GS points at this thread's TCB, three readable words.
        unsafe {
            std::arch::asm!("mov {}, qword ptr gs:[0]", out(reg) tcb, options(nostack, readonly));
            std::arch::asm!("mov {}, qword ptr gs:[8]", out(reg) dtv, options(nostack, readonly));
            std::arch::asm!("mov {}, qword ptr gs:[16]", out(reg) thread, options(nostack, readonly));
        }
        // SAFETY: the static TLS block lies right below the TCB (32 bytes here).
        let image = unsafe { std::slice::from_raw_parts((tcb - 32) as *const u8, 32) };
        let mut seen = vec![tcb, dtv, thread];
        seen.extend(image.iter().map(|&b| u64::from(b)));
        *GS_SEEN.lock().unwrap() = seen;
        ptr::null_mut()
    }

    /// A guest thread's GS base is its TCB: self pointer, the DTV and the thread handle, with
    /// the main executable's TLS image copied below it (filesz bytes, the rest zero).
    #[test]
    fn guest_threads_have_a_tcb_and_static_tls_behind_gs() {
        let _serial = serial();
        static IMAGE: [u8; 20] = *b"guest tls image 1234";
        // SAFETY: a static image; restored below.
        unsafe { runtime_set_main_tls(IMAGE.as_ptr().cast(), 20, 32, 16) };
        let thread = create_thread(reads_tcb, 0, c"tls");
        assert_eq!(join(thread).0, 0);
        // SAFETY: back to no image.
        unsafe { runtime_set_main_tls(ptr::null(), 0, 0, 0) };
        let seen = GS_SEEN.lock().unwrap().clone();
        assert_ne!(seen[0], 0);
        assert_eq!(seen[0] % 16, 0);
        assert_eq!(seen[1], DTV.as_ptr() as u64);
        assert_eq!(seen[2], thread as u64);
        let image: Vec<u8> = seen[3..].iter().map(|&b| b as u8).collect();
        assert_eq!(&image[..20], &IMAGE);
        assert_eq!(&image[20..], &[0; 12]);
    }

    /// Every NID of the table resolves, and each one `import_names.inc` lists names the symbol
    /// the C table meant. Three are not in that file yet, as in the C.
    #[test]
    fn the_nids_name_the_c_symbols() {
        let symbols = [
            "scePthreadSelf",
            "pthread_self",
            "scePthreadGetthreadid",
            "__error",
            "scePthreadAttrInit",
            "scePthreadAttrDestroy",
            "scePthreadAttrGet",
            "scePthreadAttrGetaffinity",
            "scePthreadAttrSetstacksize",
            "scePthreadAttrSetdetachstate",
            "scePthreadAttrSetschedpolicy",
            "scePthreadAttrSetschedparam",
            "scePthreadAttrSetaffinity",
            "scePthreadAttrSetinheritsched",
            "scePthreadAttrGetdetachstate",
            "scePthreadAttrGetstacksize",
            "scePthreadAttrGetschedparam",
            "scePthreadAttrSetguardsize",
            "scePthreadCreate",
            "scePthreadJoin",
            "scePthreadDetach",
            "scePthreadExit",
            "scePthreadYield",
            "scePthreadGetprio",
            "scePthreadSetprio",
            "scePthreadSetaffinity",
            "scePthreadGetaffinity",
            "scePthreadRename",
            "scePthreadEqual",
            "pthread_attr_init",
            "pthread_attr_destroy",
            "pthread_attr_setdetachstate",
            "pthread_attr_setstacksize",
            "pthread_attr_setschedparam",
            "pthread_create",
            "pthread_create_name_np",
            "pthread_join",
            "pthread_exit",
        ];
        let mut missing = Vec::new();
        for ((nid, function), symbol) in table().iter().zip(symbols) {
            assert_ne!(*function, 0);
            let name = std::ffi::CString::new(*nid).unwrap();
            assert_eq!(resolve(&name), *function);
            match crate::abi::nid_name(nid) {
                Some(found) => assert_eq!(found, symbol, "{nid}"),
                None => missing.push(symbol),
            }
        }
        assert_eq!(
            missing,
            [
                "scePthreadAttrGetstacksize",
                "scePthreadAttrGetschedparam",
                "scePthreadAttrSetguardsize"
            ]
        );
        // SAFETY: NUL-terminated literals, and null.
        unsafe {
            assert_eq!(runtime_thread_resolve(c"aI+OeCz8xrQ#q#q".as_ptr()), 0);
            assert_eq!(runtime_thread_resolve(ptr::null()), 0);
        }
    }
}
