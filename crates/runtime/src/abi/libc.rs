//! Narrow, explicit PS4 libc contracts (the deleted `src/runtime.c`). No automatic success stubs:
//! each scoped NID below is a function or variable with a checked contract, and everything else
//! falls through to the C modules' resolvers.
//!
//! Guest code calls these with the System V ABI (`extern "sysv64"`), on every host OS. They run
//! on guest threads: on Linux the guest's thread pointer is GS, so the host's thread-locals here
//! (FS) are safe to use.

use super::host;
use std::cell::Cell;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

type GuestCallback = extern "sysv64" fn();
type GuestCallbackWithArg = extern "sysv64" fn(*mut c_void);

/// Capability bits given to [`runtime_start`]; bit 1 enables the libc contracts and resolution.
static CAPABILITIES: AtomicU64 = AtomicU64::new(0);
/// `__stack_chk_guard` (`f7uOxY9mM1U#p#J`), read by guest code through its address.
static STACK_CANARY: AtomicU64 = AtomicU64::new(0);
/// `_need_sceLibcInternal` (`ZT4ODD2Ts9o#libSceLibcInternal`): an SDK marker variable referenced
/// by Fios2.
static NEED_LIBC_INTERNAL: AtomicI32 = AtomicI32::new(1);
static CALLS_INIT: AtomicUsize = AtomicUsize::new(0);
static CALLS_ATEXIT: AtomicUsize = AtomicUsize::new(0);
static CALLS_CXA: AtomicUsize = AtomicUsize::new(0);
static GUARDS_ACQUIRED: AtomicUsize = AtomicUsize::new(0);
static GUARDS_RELEASED: AtomicUsize = AtomicUsize::new(0);
static MEMORY_CALLS: AtomicUsize = AtomicUsize::new(0);
static PROCESS_PARAM: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static APPLICATION_HEAP_API: AtomicPtr<*mut c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Dynamic TLS of linked modules: module IDs 2..7 (1 is the eboot's static TLS, in
/// `runtime_thread.c`).
const TLS_MODULES: usize = 8;

struct TlsModule {
    data: AtomicPtr<u8>,
    filesz: AtomicU64,
    memsz: AtomicU64,
}

static TLS: [TlsModule; TLS_MODULES] = [const {
    TlsModule {
        data: AtomicPtr::new(std::ptr::null_mut()),
        filesz: AtomicU64::new(0),
        memsz: AtomicU64::new(0),
    }
}; TLS_MODULES];

thread_local! {
    /// This thread's copy of each module's TLS block, allocated on first use and never freed.
    static TLS_BLOCKS: Cell<[*mut u8; TLS_MODULES]> = const { Cell::new([std::ptr::null_mut(); TLS_MODULES]) };
    /// This thread's static-guard owner tag, 0 until its first guard.
    static GUARD_OWNER: Cell<u32> = const { Cell::new(0) };
}
static NEXT_GUARD_OWNER: AtomicU32 = AtomicU32::new(1);

/// One `atexit` or `__cxa_atexit` registration.
#[derive(Clone, Copy)]
struct ExitHandler {
    callback: Callback,
    /// `__cxa_atexit`'s DSO handle (0 for `atexit`); `runtime_finalize(dso)` runs only its own.
    dso: usize,
    active: bool,
}

#[derive(Clone, Copy)]
enum Callback {
    Plain(GuestCallback),
    WithArg(GuestCallbackWithArg, usize),
}

/// The registry's capacity grows 64, 128, ... and stops at this many handlers (C: `realloc` past
/// 1 Mi entries was refused).
const MAX_HANDLERS: usize = 1024 * 1024;
static HANDLERS: Mutex<Vec<ExitHandler>> = Mutex::new(Vec::new());

fn handlers() -> MutexGuard<'static, Vec<ExitHandler>> {
    // Guest callbacks run outside the lock, so a poisoned lock still holds a whole registry.
    HANDLERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `STOP:` and the loader's exit code 21 (an unsupported guest request).
pub(crate) fn stop(message: &str) -> ! {
    host::err(message);
    host::quit(21)
}

/// The process param of the eboot (`sceKernelGetProcParam`), set by the loader.
#[no_mangle]
pub extern "C" fn runtime_set_procparam(param: *mut c_void) {
    PROCESS_PARAM.store(param, Ordering::Release);
}

extern "sysv64" fn guest_procparam() -> *mut c_void {
    PROCESS_PARAM.load(Ordering::Acquire)
}

extern "sysv64" fn guest_set_heap_api(api: *mut *mut c_void) {
    APPLICATION_HEAP_API.store(api, Ordering::Release);
    host::out_line("Runtime: application heap API registered");
}

/// The heap API table the game registered (`_sceKernelRtldSetApplicationHeapAPI`), or null.
#[no_mangle]
pub extern "C" fn runtime_application_heap_api() -> *mut *mut c_void {
    APPLICATION_HEAP_API.load(Ordering::Acquire)
}

/// Registers the TLS image of linked module `module` (2..7): `filesz` initialized bytes at
/// `data`, `memsz` bytes in all. Another module id is a `STOP` (exit code 21).
#[no_mangle]
pub extern "C" fn runtime_set_module_tls(
    module: u64,
    data: *const c_void,
    filesz: u64,
    memsz: u64,
) {
    if !(2..TLS_MODULES as u64).contains(&module) {
        stop("STOP: unsupported TLS module id\n");
    }
    let slot = &TLS[module as usize];
    slot.filesz.store(filesz, Ordering::Relaxed);
    slot.memsz.store(memsz, Ordering::Relaxed);
    slot.data.store(data.cast_mut().cast(), Ordering::Release);
}

/// The TLS image of the linked libc (module 2).
#[no_mangle]
pub extern "C" fn runtime_set_libc_tls(data: *const c_void, filesz: u64, memsz: u64) {
    runtime_set_module_tls(2, data, filesz, memsz);
}

/// `__tls_get_addr`: `index` is {module, offset}; the block is this thread's copy of the module's
/// TLS image. An unknown module or an offset past the image is a `STOP` (exit code 21).
extern "sysv64" fn guest_tls_get_addr(index: *const u64) -> *mut c_void {
    // SAFETY: the guest passes null or a {module, offset} pair.
    let pair = (!index.is_null()).then(|| unsafe { [*index, *index.add(1)] });
    let found = pair.and_then(|[module, offset]| {
        let slot = TLS.get(usize::try_from(module).ok().filter(|&m| m >= 2)?)?;
        let data = slot.data.load(Ordering::Acquire);
        let memsz = slot.memsz.load(Ordering::Relaxed);
        (!data.is_null() && offset < memsz).then_some((
            module as usize,
            offset as usize,
            data,
            slot,
        ))
    });
    let Some((module, offset, data, slot)) = found else {
        stop("STOP: unsupported TLS module/offset\n");
    };
    let mut blocks = TLS_BLOCKS.with(Cell::get);
    if blocks[module].is_null() {
        let memsz = slot.memsz.load(Ordering::Relaxed) as usize;
        let filesz = (slot.filesz.load(Ordering::Relaxed) as usize).min(memsz);
        // SAFETY: a zeroed block of `memsz` bytes from the C heap, kept for the thread's life.
        let block = unsafe { host::calloc(1, memsz) }.cast::<u8>();
        if block.is_null() {
            host::err("Cannot allocate module TLS\n");
            // SAFETY: ends the process.
            unsafe { host::exit(1) };
        }
        // SAFETY: the loader's image holds `filesz` bytes at `data`; the block holds `memsz`.
        unsafe { std::ptr::copy_nonoverlapping(data, block, filesz) };
        blocks[module] = block;
        TLS_BLOCKS.with(|cell| cell.set(blocks));
    }
    // SAFETY: `offset < memsz`, inside the block.
    unsafe { blocks[module].add(offset) }.cast()
}

/// Enables the runtime with capability bits `flags` (bit 1: the libc contracts and import
/// resolution) and draws the guest's stack canary, low byte zero. Without bit 1 nothing resolves.
#[no_mangle]
pub extern "C" fn runtime_start(flags: u64) {
    CAPABILITIES.store(flags, Ordering::Release);
    if flags & 1 == 0 {
        return;
    }
    let Some(canary) = random_u64() else {
        host::err("Cannot initialize stack canary\n");
        // SAFETY: ends the process.
        unsafe { host::exit(1) };
    };
    STACK_CANARY.store(canary & !255, Ordering::Release);
}

#[cfg(windows)]
fn random_u64() -> Option<u64> {
    extern "C" {
        fn rand_s(value: *mut u32) -> c_int;
    }
    let mut halves = [0u32; 2];
    // SAFETY: `rand_s` writes one u32 through a valid pointer.
    let ok = halves.iter_mut().all(|half| unsafe { rand_s(half) } == 0);
    ok.then(|| u64::from(halves[0]) | (u64::from(halves[1]) << 32))
}

#[cfg(not(windows))]
fn random_u64() -> Option<u64> {
    use std::io::Read;
    let mut bytes = [0u8; 8];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut bytes))
        .ok()?;
    Some(u64::from_ne_bytes(bytes))
}

/// The runtime's counters and every C module's report, on stdout (at exit and on demand).
#[no_mangle]
pub extern "C" fn runtime_report() {
    host::out_line(&format!(
        "Runtime: _init_env={}, atexit={}, __cxa_atexit={}, registered handlers={}",
        CALLS_INIT.load(Ordering::Relaxed),
        CALLS_ATEXIT.load(Ordering::Relaxed),
        CALLS_CXA.load(Ordering::Relaxed),
        handlers().len()
    ));
    host::out_line(&format!(
        "Runtime: static guards acquired={}, released={}",
        GUARDS_ACQUIRED.load(Ordering::Relaxed),
        GUARDS_RELEASED.load(Ordering::Relaxed)
    ));
    host::report_modules();
    host::out_line(&format!(
        "Runtime: memory operations={}",
        MEMORY_CALLS.load(Ordering::Relaxed)
    ));
}

extern "sysv64" fn init_env() {
    // prepare.py verified the supplied libc export consists of exactly C3.
    CALLS_INIT.fetch_add(1, Ordering::Relaxed);
    host::out_line("Runtime: _init_env returned (verified libc implementation: RET)");
}

fn register_handler(callback: Callback, dso: usize) -> c_int {
    let mut handlers = handlers();
    if handlers.len() >= MAX_HANDLERS || handlers.try_reserve(1).is_err() {
        return -1;
    }
    handlers.push(ExitHandler {
        callback,
        dso,
        active: true,
    });
    0
}

extern "sysv64" fn guest_atexit(callback: Option<GuestCallback>) -> c_int {
    let Some(callback) = callback else { return -1 };
    CALLS_ATEXIT.fetch_add(1, Ordering::Relaxed);
    register_handler(Callback::Plain(callback), 0)
}

extern "sysv64" fn guest_cxa_atexit(
    callback: Option<GuestCallbackWithArg>,
    argument: *mut c_void,
    dso: *mut c_void,
) -> c_int {
    let Some(callback) = callback else { return -1 };
    CALLS_CXA.fetch_add(1, Ordering::Relaxed);
    register_handler(Callback::WithArg(callback, argument as usize), dso as usize)
}

/// Runs the registered exit handlers of `dso` (all of them for null), newest first, each once.
/// A handler is marked done before it runs, so repeated or recursive finalization cannot run it
/// twice; the search restarts after each one to include handlers a destructor registered.
#[no_mangle]
pub extern "C" fn runtime_finalize(dso: *mut c_void) {
    let dso = dso as usize;
    loop {
        let handler = {
            let mut handlers = handlers();
            let Some(handler) = handlers
                .iter_mut()
                .rev()
                .find(|h| h.active && (dso == 0 || h.dso == dso))
            else {
                return;
            };
            handler.active = false;
            *handler
        };
        match handler.callback {
            Callback::Plain(callback) => callback(),
            Callback::WithArg(callback, argument) => callback(argument as *mut c_void),
        }
    }
}

extern "sysv64" fn guest_finalize(dso: *mut c_void) {
    runtime_finalize(dso);
}

/// `__cxa_guard_acquire`. The dump's libc uses the low byte for completion and the dword at +4 as
/// a lock; the lock word holds a per-thread owner tag so recursion is detectable (a `STOP`).
extern "sysv64" fn guard_acquire(guard: *mut u64) -> c_int {
    // SAFETY: the guest passes its 8-byte-aligned guard word, shared only through atomics.
    let state = unsafe { AtomicU64::from_ptr(guard) };
    let owner = GUARD_OWNER.with(|owner| {
        if owner.get() == 0 {
            owner.set(NEXT_GUARD_OWNER.fetch_add(1, Ordering::Relaxed));
        }
        owner.get()
    });
    loop {
        if state.load(Ordering::Acquire) & 1 != 0 {
            return 0;
        }
        match state.compare_exchange(
            0,
            u64::from(owner) << 32,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(expected) if expected & 1 != 0 => return 0,
            Err(expected) if (expected >> 32) as u32 == owner => {
                stop("STOP: recursive/concurrent static initialization is not supported yet\n");
            }
            // Another thread is running the initializer.
            Err(_) => std::thread::yield_now(),
        }
    }
    GUARDS_ACQUIRED.fetch_add(1, Ordering::Relaxed);
    1
}

extern "sysv64" fn guard_release(guard: *mut u64) {
    // SAFETY: as in `guard_acquire`.
    unsafe { AtomicU64::from_ptr(guard) }.store(1, Ordering::Release);
    GUARDS_RELEASED.fetch_add(1, Ordering::Relaxed);
}

extern "sysv64" fn guard_abort(guard: *mut u64) {
    // SAFETY: as in `guard_acquire`.
    unsafe { AtomicU64::from_ptr(guard) }.fetch_and(0xffff_ffff, Ordering::Release);
}

/// `__stack_chk_fail`: exit code 22, not through `runtime_quit` (as before).
extern "sysv64" fn stack_fail() -> ! {
    host::err("STOP: guest stack protector detected corruption\n");
    // SAFETY: ends the process.
    unsafe { host::exit(22) }
}

extern "sysv64" fn guest_memset(dst: *mut c_void, value: c_int, size: usize) -> *mut c_void {
    MEMORY_CALLS.fetch_add(1, Ordering::Relaxed);
    // SAFETY: the guest's memset contract: `size` writable bytes at `dst`.
    unsafe { host::memset(dst, value, size) }
}

extern "sysv64" fn guest_memcpy(dst: *mut c_void, src: *const c_void, size: usize) -> *mut c_void {
    MEMORY_CALLS.fetch_add(1, Ordering::Relaxed);
    // SAFETY: the guest's memcpy contract: `size` bytes each, not overlapping.
    unsafe { host::memcpy(dst, src, size) }
}

extern "sysv64" fn guest_memmove(dst: *mut c_void, src: *const c_void, size: usize) -> *mut c_void {
    MEMORY_CALLS.fetch_add(1, Ordering::Relaxed);
    // SAFETY: the guest's memmove contract: `size` bytes each.
    unsafe { host::memmove(dst, src, size) }
}

extern "sysv64" fn guest_memcmp(a: *const c_void, b: *const c_void, size: usize) -> c_int {
    MEMORY_CALLS.fetch_add(1, Ordering::Relaxed);
    // SAFETY: the guest's memcmp contract: `size` readable bytes each.
    unsafe { host::memcmp(a, b, size) }
}

extern "sysv64" fn guest_strlen(text: *const c_char) -> usize {
    // SAFETY: the guest's strlen contract: a NUL-terminated string.
    unsafe { host::strlen(text) }
}

/// libc `exit`: the exit handlers, the report, then the loader's exit with the guest's status.
extern "sysv64" fn guest_libc_exit(status: c_int) -> ! {
    host::out_line(&format!("Runtime: guest requested exit({status})"));
    runtime_finalize(std::ptr::null_mut());
    runtime_report();
    host::quit(status)
}

/// The address of this module's contract for `name`, the C modules' (loader build), or `None`.
/// Nothing resolves until [`runtime_start`] enabled capability 1.
pub(super) fn resolve(name: &CStr, is_data: bool) -> Option<usize> {
    if CAPABILITIES.load(Ordering::Acquire) & 1 == 0 {
        return None;
    }
    let scoped = name.to_bytes();
    if is_data {
        return match scoped {
            b"f7uOxY9mM1U#p#J" => Some(STACK_CANARY.as_ptr() as usize),
            b"ZT4ODD2Ts9o#libSceLibcInternal" => Some(NEED_LIBC_INTERNAL.as_ptr() as usize),
            _ => None,
        };
    }
    // Exact scoped imports; the suffix identifies library and module.
    let address = match scoped {
        b"bzQExy189ZI#q#q" => init_env as *const () as usize,
        b"8G2LB+A3rzg#q#q" => guest_atexit as *const () as usize,
        b"tsvEmnenz48#q#q" => guest_cxa_atexit as *const () as usize,
        b"uMei1W9uyNo#q#q" => guest_libc_exit as *const () as usize,
        // Not imported by the current eboot, exposed for ABI tests and future use.
        b"H2e8t5ScQGc#q#q" => guest_finalize as *const () as usize,
        b"3GPpjQdAMTw#q#q" => guard_acquire as *const () as usize,
        b"9rAeANT2tyE#q#q" => guard_release as *const () as usize,
        b"2emaaluWzUw#q#q" => guard_abort as *const () as usize,
        b"Ou3iL1abvng#p#J" => stack_fail as *const () as usize,
        b"8zTFvBIAIN8#q#q" => guest_memset as *const () as usize,
        b"Q3VBxCXhUHs#q#q" => guest_memcpy as *const () as usize,
        b"+P6FRGH4LfA#q#q" => guest_memmove as *const () as usize,
        b"DfivPArhucg#q#q" => guest_memcmp as *const () as usize,
        b"j4ViWNHEgww#q#q" => guest_strlen as *const () as usize,
        b"vNe1w4diLCs#p#J" => guest_tls_get_addr as *const () as usize,
        b"959qrazPIrg#p#J" => guest_procparam as *const () as usize,
        b"p5EcQeEeJAE#p#J" => guest_set_heap_api as *const () as usize,
        _ => return host::resolve_chain(name),
    };
    Some(address)
}

#[cfg(test)]
mod tests {
    //! `tests/test_runtime.c`'s cases for `runtime.c`. The runtime is process-wide state, so every
    //! test holds [`LOCK`]; the cases that end the process are in `tests/abi_stops.rs`.

    use super::*;
    use std::mem::transmute;

    static LOCK: Mutex<()> = Mutex::new(());

    fn enabled() -> MutexGuard<'static, ()> {
        let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        runtime_start(1);
        guard
    }

    fn get(name: &CStr) -> usize {
        resolve(name, false).unwrap_or_else(|| panic!("{name:?} does not resolve"))
    }

    #[test]
    fn resolver_scope() {
        let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        runtime_start(0);
        assert_eq!(resolve(c"bzQExy189ZI#q#q", false), None);
        runtime_start(1);
        assert_eq!(resolve(c"bzQExy189ZI#wrong#module", false), None);
        assert_eq!(resolve(c"unknown", false), None);
        assert_eq!(resolve(c"8zTFvBIAIN8#q#q", true), None);
        assert_eq!(resolve(c"f7uOxY9mM1U#p#J", false), None);
        let canary = resolve(c"f7uOxY9mM1U#p#J", true).unwrap() as *const u64;
        // SAFETY: the address of the canary static.
        let canary = unsafe { *canary };
        assert!(canary != 0 && canary & 255 == 0);
        let marker = resolve(c"ZT4ODD2Ts9o#libSceLibcInternal", true).unwrap() as *const i32;
        // SAFETY: the address of the marker static.
        assert_eq!(unsafe { *marker }, 1);
        // The C wrapper and the Rust function agree.
        // SAFETY: a NUL-terminated literal.
        assert_eq!(
            unsafe { super::super::ffi::runtime_resolve(c"bzQExy189ZI#q#q".as_ptr(), 0) },
            get(c"bzQExy189ZI#q#q")
        );
    }

    #[test]
    fn rust_modules_answer_after_the_libc_contracts() {
        let _guard = enabled();
        // `gettimeofday` (libScePosix) is `time`'s; `sceRtcGetTick` is `rtc`'s.
        assert_eq!(
            resolve(c"n88vx3C5nW8#I#J", false),
            crate::time::resolve(c"n88vx3C5nW8#I#J")
        );
        assert_eq!(
            resolve(c"8w-H19ip48I#H#I", false),
            crate::rtc::resolve(c"8w-H19ip48I#H#I")
        );
        assert!(resolve(c"8w-H19ip48I#H#I", false).is_some());
        assert_eq!(resolve(c"8w-H19ip48I#H#I", true), None);
    }

    #[test]
    fn init_env_counts_calls() {
        let _guard = enabled();
        let before = CALLS_INIT.load(Ordering::Relaxed);
        // SAFETY: `_init_env` is a guest callback without arguments.
        let init: GuestCallback = unsafe { transmute(get(c"bzQExy189ZI#q#q")) };
        init();
        assert_eq!(CALLS_INIT.load(Ordering::Relaxed), before + 1);
    }

    static ORDER: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    type Register = extern "sysv64" fn(Option<GuestCallback>) -> c_int;
    type RegisterCxa =
        extern "sysv64" fn(Option<GuestCallbackWithArg>, *mut c_void, *mut c_void) -> c_int;
    static REGISTER_PLAIN: AtomicUsize = AtomicUsize::new(0);

    fn push(value: usize) {
        ORDER.lock().unwrap().push(value);
    }
    extern "sysv64" fn first() {
        push(1);
    }
    extern "sysv64" fn second() {
        push(2);
    }
    extern "sysv64" fn with_arg(value: *mut c_void) {
        push(value as usize);
    }
    extern "sysv64" fn add_handler() {
        push(4);
        // SAFETY: the stored `atexit` contract.
        let register: Register = unsafe { transmute(REGISTER_PLAIN.load(Ordering::Relaxed)) };
        assert_eq!(register(Some(first)), 0);
    }

    #[test]
    fn exit_handlers() {
        let _guard = enabled();
        // SAFETY: the `atexit` and `__cxa_atexit` contracts.
        let (register, cxa): (Register, RegisterCxa) = unsafe {
            (
                transmute::<usize, Register>(get(c"8G2LB+A3rzg#q#q")),
                transmute::<usize, RegisterCxa>(get(c"tsvEmnenz48#q#q")),
            )
        };
        // SAFETY: the `__cxa_finalize` contract.
        let finalize: extern "sysv64" fn(*mut c_void) =
            unsafe { transmute(get(c"H2e8t5ScQGc#q#q")) };
        REGISTER_PLAIN.store(register as usize, Ordering::Relaxed);
        let order = || ORDER.lock().unwrap().clone();
        let dso = |value: usize| value as *mut c_void;
        // Handlers left by an earlier test in this process.
        runtime_finalize(std::ptr::null_mut());
        ORDER.lock().unwrap().clear();

        assert_eq!(register(None), -1);
        assert_eq!(cxa(None, dso(0), dso(11)), -1);
        assert_eq!(register(Some(first)), 0);
        assert_eq!(cxa(Some(with_arg), dso(3), dso(11)), 0);
        assert_eq!(register(Some(second)), 0);
        runtime_finalize(dso(11));
        assert_eq!(order(), [3]);
        finalize(dso(11));
        assert_eq!(order(), [3]);
        runtime_finalize(std::ptr::null_mut());
        assert_eq!(order(), [3, 2, 1]);
        runtime_finalize(std::ptr::null_mut());
        assert_eq!(order(), [3, 2, 1]);
        assert_eq!(register(Some(add_handler)), 0);
        runtime_finalize(std::ptr::null_mut());
        assert_eq!(order(), [3, 2, 1, 4, 1]);

        // The registry grows past its first 64 entries, and finalization is strictly LIFO.
        ORDER.lock().unwrap().clear();
        for i in 0..200 {
            assert_eq!(cxa(Some(with_arg), dso(i), dso(12)), 0);
        }
        runtime_finalize(dso(12));
        assert_eq!(order(), (0..200).rev().collect::<Vec<_>>());
    }

    #[test]
    fn guards() {
        let _guard = enabled();
        type Acquire = extern "sysv64" fn(*mut u64) -> c_int;
        type Guard = extern "sysv64" fn(*mut u64);
        // SAFETY: the `__cxa_guard_*` contracts.
        let (acquire, release, abort): (Acquire, Guard, Guard) = unsafe {
            (
                transmute::<usize, Acquire>(get(c"3GPpjQdAMTw#q#q")),
                transmute::<usize, Guard>(get(c"9rAeANT2tyE#q#q")),
                transmute::<usize, Guard>(get(c"2emaaluWzUw#q#q")),
            )
        };
        let (acquired, released) = (
            GUARDS_ACQUIRED.load(Ordering::Relaxed),
            GUARDS_RELEASED.load(Ordering::Relaxed),
        );
        let mut state = 0u64;
        let owner = || u64::from(GUARD_OWNER.with(Cell::get));
        assert_eq!(acquire(&mut state), 1);
        assert_eq!(state, owner() << 32);
        assert_ne!(owner(), 0);
        abort(&mut state);
        assert_eq!(state, 0);
        assert_eq!(acquire(&mut state), 1);
        release(&mut state);
        assert_eq!(state, 1);
        assert_eq!(acquire(&mut state), 0);
        assert_eq!(GUARDS_ACQUIRED.load(Ordering::Relaxed), acquired + 2);
        assert_eq!(GUARDS_RELEASED.load(Ordering::Relaxed), released + 1);
        // Another thread's initializer completing ends this thread's wait.
        let shared: &'static AtomicU64 = Box::leak(Box::new(AtomicU64::new(0)));
        assert_eq!(acquire(shared.as_ptr()), 1);
        let waiter = std::thread::spawn(move || acquire(shared.as_ptr()));
        std::thread::sleep(std::time::Duration::from_millis(20));
        release(shared.as_ptr());
        assert_eq!(waiter.join().unwrap(), 0);
        assert_eq!(shared.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn memory_primitives() {
        let _guard = enabled();
        type Set = extern "sysv64" fn(*mut c_void, c_int, usize) -> *mut c_void;
        type Copy = extern "sysv64" fn(*mut c_void, *const c_void, usize) -> *mut c_void;
        type Compare = extern "sysv64" fn(*const c_void, *const c_void, usize) -> c_int;
        type Length = extern "sysv64" fn(*const c_char) -> usize;
        // SAFETY: the memset, memcpy, memmove, memcmp and strlen contracts.
        let (set, copy, compare, moving, length): (Set, Copy, Compare, Copy, Length) = unsafe {
            (
                transmute::<usize, Set>(get(c"8zTFvBIAIN8#q#q")),
                transmute::<usize, Copy>(get(c"Q3VBxCXhUHs#q#q")),
                transmute::<usize, Compare>(get(c"DfivPArhucg#q#q")),
                transmute::<usize, Copy>(get(c"+P6FRGH4LfA#q#q")),
                transmute::<usize, Length>(get(c"j4ViWNHEgww#q#q")),
            )
        };
        let before = MEMORY_CALLS.load(Ordering::Relaxed);
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        let (pa, pb) = (
            a.as_mut_ptr().cast::<c_void>(),
            b.as_mut_ptr().cast::<c_void>(),
        );
        assert_eq!(set(pa, 0x5a, 16), pa);
        assert_eq!(a, [0x5a; 16]);
        assert_eq!(copy(pb, pa, 16), pb);
        assert_eq!(compare(pa, pb, 16), 0);
        a[..7].copy_from_slice(b"abcdef\0");
        // SAFETY: both ranges are inside `a`.
        moving(unsafe { pa.add(1) }, pa, 6);
        assert_eq!(&a[..7], b"aabcdef");
        assert!(compare(pa, pb, 16) > 0);
        assert_eq!(MEMORY_CALLS.load(Ordering::Relaxed), before + 5);
        assert_eq!(length(c"".as_ptr()), 0);
        assert_eq!(length(c"abc".as_ptr()), 3);
        assert_eq!(length(c"\xff\x80".as_ptr()), 2);
        // strlen is not a counted memory operation.
        assert_eq!(MEMORY_CALLS.load(Ordering::Relaxed), before + 5);
    }

    type TlsAddress = extern "sysv64" fn(*const u64) -> *mut u8;

    #[test]
    fn module_tls_is_per_thread_and_copied_from_the_image() {
        let _guard = enabled();
        static INITIAL: [u8; 2] = [42, 17];
        runtime_set_libc_tls(INITIAL.as_ptr().cast(), 2, 32);
        // SAFETY: the `__tls_get_addr` contract.
        let tls: TlsAddress = unsafe { transmute(get(c"vNe1w4diLCs#p#J")) };
        let p = tls([2, 0].as_ptr());
        // SAFETY: 32 bytes of this thread's block.
        let block = unsafe { std::slice::from_raw_parts_mut(p, 32) };
        assert_eq!(block[..3], [42, 17, 0]);
        assert_eq!(block[31], 0);
        assert_eq!(tls([2, 31].as_ptr()), p.wrapping_add(31));
        assert_eq!(tls([2, 0].as_ptr()), p);
        block[0] = 23;
        let tls_address = tls as usize;
        let other = std::thread::spawn(move || {
            // SAFETY: the same contract on another thread.
            let tls: TlsAddress = unsafe { transmute(tls_address) };
            let q = tls([2, 0].as_ptr());
            // SAFETY: 32 bytes of that thread's block.
            let block = unsafe { std::slice::from_raw_parts_mut(q, 32) };
            assert_eq!(block[..2], [42, 17]);
            block[0] = 99;
            q as usize
        })
        .join()
        .unwrap();
        assert_ne!(other, p as usize);
        assert_eq!(block[0], 23);
        assert_eq!(INITIAL[0], 42);
    }

    #[test]
    fn procparam_and_heap_api() {
        let _guard = enabled();
        let mut param = [64u64, 0, 0, 0, 0, 0, 0, 0];
        runtime_set_procparam(param.as_mut_ptr().cast());
        // SAFETY: `sceKernelGetProcParam` takes nothing and returns the pointer.
        let procparam: extern "sysv64" fn() -> *mut c_void =
            unsafe { transmute(get(c"959qrazPIrg#p#J")) };
        assert_eq!(procparam(), param.as_mut_ptr().cast());
        // SAFETY: the heap API registration takes the table pointer.
        let set_heap: extern "sysv64" fn(*mut *mut c_void) =
            unsafe { transmute(get(c"p5EcQeEeJAE#p#J")) };
        let mut heap = [std::ptr::null_mut::<c_void>(); 10];
        set_heap(heap.as_mut_ptr());
        assert_eq!(runtime_application_heap_api(), heap.as_mut_ptr());
        set_heap(std::ptr::null_mut());
        assert!(runtime_application_heap_api().is_null());
        runtime_set_procparam(std::ptr::null_mut());
        assert!(procparam().is_null());
    }

    #[test]
    fn report_counts_handlers() {
        let _guard = enabled();
        // The report prints through C stdio; here it must only run.
        runtime_report();
    }
}
