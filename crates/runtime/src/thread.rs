//! Guest threads on host pthreads (piece 3.13, replacing `src/runtime_thread.c` on Linux).
//!
//! Each guest thread owns a FreeBSD-style TCB (variant II: static TLS below the TCB). The loader
//! rewrites the eboot's `mov rax, fs:[0]` into `mov rax, gs:[0]`, so GS base = guest TCB while
//! glibc keeps FS. Priorities and affinity are recorded, not enforced by a PS4 scheduler.

use std::cell::Cell;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::ptr::{self, null_mut};
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

const fn err(n: i32) -> i32 {
    (0x8002_0000u32 | (n as u32)) as i32
}

const ATTR_MAGIC: u32 = 0x4154_5452;
const STACK_MARGIN: usize = 256 * 1024;
const MIN_STACK: u64 = 64 * 1024;
const DEFAULT_STACK: u64 = 1024 * 1024;
const DEFAULT_PRIO: i32 = 700;
#[cfg(unix)]
const ARCH_SET_GS: c_int = 0x1001;

type GuestEntry = unsafe extern "sysv64" fn(*mut c_void) -> *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct ThreadAttr {
    magic: u32,
    detached: i32,
    policy: i32,
    prio: i32,
    inherit: i32,
    stack: u64,
    guard: u64,
    affinity: u64,
    next: *mut ThreadAttr,
}

#[repr(C)]
pub struct GuestThread {
    tcb: *mut u64,
    tls_block: *mut u8,
    #[cfg(unix)]
    host: libc::pthread_t,
    #[cfg(windows)]
    host: usize,
    entry: Option<GuestEntry>,
    argument: *mut c_void,
    result: *mut c_void,
    attr: ThreadAttr,
    name: [u8; 32],
    detached: i32,
    finished: i32,
    joined: i32,
    host_owned: i32,
    exit_jump: [u64; 8],
    next: *mut GuestThread,
}

struct Registry {
    attributes: usize,
    threads: usize,
    created: usize,
    joined_count: usize,
    exited: usize,
}

static LOCK: Mutex<Registry> = Mutex::new(Registry {
    attributes: 0,
    threads: 0,
    created: 0,
    joined_count: 0,
    exited: 0,
});

static TLS_TEMPLATE: AtomicUsize = AtomicUsize::new(0);
static TLS_FILESZ: AtomicU64 = AtomicU64::new(0);
static TLS_MEMSZ: AtomicU64 = AtomicU64::new(0);
static TLS_ALIGN: AtomicU64 = AtomicU64::new(16);
static DTV: [u64; 3] = [0; 3];

thread_local! {
    static CURRENT: Cell<*mut GuestThread> = const { Cell::new(null_mut()) };
    static GUEST_ERRNO: std::cell::UnsafeCell<i32> = const { std::cell::UnsafeCell::new(0) };
    static THREAD_ID: Cell<i32> = const { Cell::new(0) };
}

extern "sysv64" {
    fn thread_run_with_exit_jump(
        buf: *mut [u64; 8],
        entry: GuestEntry,
        arg: *mut c_void,
    ) -> *mut c_void;
    fn thread_exit_longjmp(buf: *const [u64; 8], value: *mut c_void) -> !;
}

core::arch::global_asm!(
    ".text",
    ".globl thread_run_with_exit_jump",
    "thread_run_with_exit_jump:",
    "    mov [rdi], rbx",
    "    mov [rdi + 8], rbp",
    "    mov [rdi + 16], r12",
    "    mov [rdi + 24], r13",
    "    mov [rdi + 32], r14",
    "    mov [rdi + 40], r15",
    "    lea rax, [rsp + 8]",
    "    mov [rdi + 48], rax",
    "    mov rax, [rsp]",
    "    mov [rdi + 56], rax",
    "    mov rdi, rdx",
    "    jmp rsi",
    ".globl thread_exit_longjmp",
    "thread_exit_longjmp:",
    "    mov rbx, [rdi]",
    "    mov rbp, [rdi + 8]",
    "    mov r12, [rdi + 16]",
    "    mov r13, [rdi + 24]",
    "    mov r14, [rdi + 32]",
    "    mov r15, [rdi + 40]",
    "    mov rsp, [rdi + 48]",
    "    mov rax, rsi",
    "    jmp qword ptr [rdi + 56]",
);

#[no_mangle]
pub extern "C" fn runtime_set_main_tls(data: *const c_void, filesz: u64, memsz: u64, align: u64) {
    TLS_TEMPLATE.store(data as usize, Ordering::Release);
    TLS_FILESZ.store(filesz, Ordering::Release);
    TLS_MEMSZ.store(memsz, Ordering::Release);
    TLS_ALIGN.store(if align != 0 { align } else { 16 }, Ordering::Release);
}

fn tls_offset() -> u64 {
    let memsz = TLS_MEMSZ.load(Ordering::Acquire);
    let align = TLS_ALIGN.load(Ordering::Acquire);
    (memsz + align - 1) & !(align - 1)
}

#[cfg(windows)]
static WIN_TLS_SLOT: AtomicU32 =
    AtomicU32::new(windows_sys::Win32::System::Threading::TLS_OUT_OF_INDEXES);

/// Windows: returns the TEB `TlsSlots` byte offset (`gs:[offset]`) holding the guest TCB.
#[cfg(windows)]
#[no_mangle]
pub extern "C" fn runtime_thread_tcb_offset() -> u32 {
    use windows_sys::Win32::System::Threading::{TlsAlloc, TLS_OUT_OF_INDEXES};
    let slot = WIN_TLS_SLOT.load(Ordering::Acquire);
    let slot = if slot == TLS_OUT_OF_INDEXES {
        // SAFETY: allocates a process-wide TEB TLS slot.
        let allocated = unsafe { TlsAlloc() };
        if allocated >= 64 {
            crate::abi::host::err(&format!(
                "STOP: no TEB TLS slot below 64 (got {allocated})\n"
            ));
            quit(21);
        }
        match WIN_TLS_SLOT.compare_exchange(
            TLS_OUT_OF_INDEXES,
            allocated,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => allocated,
            Err(existing) => existing,
        }
    } else {
        slot
    };
    0x1480 + 8 * slot
}

unsafe fn set_gs(base: *mut c_void) {
    #[cfg(unix)]
    {
        // SAFETY: `SYS_arch_prctl(ARCH_SET_GS, base)` sets the calling thread's GS base on Linux x86-64.
        let rc = unsafe { libc::syscall(libc::SYS_arch_prctl, ARCH_SET_GS, base as libc::c_ulong) };
        if rc != 0 {
            crate::abi::host::err("STOP: arch_prctl(ARCH_SET_GS) failed\n");
            quit(21);
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::GetLastError;
        use windows_sys::Win32::System::Threading::TlsSetValue;
        runtime_thread_tcb_offset();
        let slot = WIN_TLS_SLOT.load(Ordering::Acquire);
        // SAFETY: `slot` was allocated by `TlsAlloc` and `base` is this thread's guest TCB.
        if unsafe { TlsSetValue(slot, base) } == 0 {
            let err = unsafe { GetLastError() };
            crate::abi::host::err(&format!("STOP: TlsSetValue failed ({err})\n"));
            quit(21);
        }
    }
}

fn quit(code: c_int) -> ! {
    #[cfg(uncharted_loader)]
    {
        extern "C" {
            fn runtime_quit(code: c_int) -> !;
        }
        // SAFETY: `runtime_quit` terminates the loader process cleanly.
        unsafe { runtime_quit(code) }
    }
    #[cfg(all(not(uncharted_loader), unix))]
    // SAFETY: `_exit` terminates immediately in test builds without a C loader.
    unsafe {
        libc::_exit(code)
    }
    #[cfg(all(not(uncharted_loader), windows))]
    std::process::exit(code)
}

unsafe fn attach(t: *mut GuestThread) {
    let offset = tls_offset() as usize;
    let total = offset + 256;
    let aligned_total = (total + 63) & !63usize;
    let layout = std::alloc::Layout::from_size_align(aligned_total, 64).expect("valid TLS layout");
    // SAFETY: allocates a 64-byte aligned zeroed TLS + TCB block.
    let block = unsafe { std::alloc::alloc_zeroed(layout) };
    if block.is_null() {
        crate::abi::host::err("Cannot allocate guest TLS\n");
        std::process::exit(1);
    }
    unsafe {
        ptr::write_bytes(block, 0, total);
        let filesz = TLS_FILESZ.load(Ordering::Acquire) as usize;
        let template = TLS_TEMPLATE.load(Ordering::Acquire) as *const u8;
        if filesz != 0 && !template.is_null() {
            ptr::copy_nonoverlapping(template, block, filesz);
        }
        let tcb = block.add(offset).cast::<u64>();
        *tcb.add(0) = tcb as u64; // tcb_self
        *tcb.add(1) = DTV.as_ptr() as u64; // tcb_dtv
        *tcb.add(2) = t as u64; // tcb_thread
        (*t).tls_block = block;
        (*t).tcb = tcb;
        set_gs(tcb.cast());
    }
    CURRENT.with(|cell| cell.set(t));
}

unsafe fn new_thread() -> *mut GuestThread {
    let layout = std::alloc::Layout::new::<GuestThread>();
    // SAFETY: allocates zeroed `GuestThread`.
    let t = unsafe { std::alloc::alloc_zeroed(layout) }.cast::<GuestThread>();
    if t.is_null() {
        return null_mut();
    }
    unsafe {
        (*t).attr = ThreadAttr {
            magic: ATTR_MAGIC,
            detached: 0,
            policy: 1,
            prio: DEFAULT_PRIO,
            inherit: 0,
            stack: DEFAULT_STACK,
            guard: 0,
            affinity: 0x7f,
            next: null_mut(),
        };
    }
    t
}

fn publish(t: *mut GuestThread) {
    let mut reg = LOCK.lock().unwrap();
    // SAFETY: protected by `LOCK`.
    unsafe { (*t).next = reg.threads as *mut GuestThread };
    reg.threads = t as usize;
}

fn copy_name(dst: &mut [u8; 32], name: &str) {
    dst.fill(0);
    let bytes = name.as_bytes();
    let len = bytes.len().min(dst.len() - 1);
    dst[..len].copy_from_slice(&bytes[..len]);
}

unsafe fn copy_c_name(dst: &mut [u8; 32], name: *const c_char, fallback: &str) {
    if name.is_null() {
        copy_name(dst, fallback);
    } else {
        // SAFETY: `name` is non-null NUL-terminated string.
        let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
        dst.fill(0);
        let len = bytes.len().min(dst.len() - 1);
        dst[..len].copy_from_slice(&bytes[..len]);
    }
}

pub(crate) fn current_thread() -> *mut GuestThread {
    let existing = CURRENT.with(Cell::get);
    if !existing.is_null() {
        return existing;
    }
    // SAFETY: allocates and attaches a `GuestThread` record for this host thread.
    unsafe {
        let t = new_thread();
        if t.is_null() {
            crate::abi::host::err("Cannot allocate guest thread\n");
            std::process::exit(1);
        }
        #[cfg(unix)]
        {
            (*t).host = libc::pthread_self();
        }
        #[cfg(windows)]
        {
            (*t).host = windows_sys::Win32::System::Threading::GetCurrentThread() as usize;
        }
        (*t).host_owned = 1;
        copy_name(&mut (*t).name, "host");
        attach(t);
        publish(t);
        t
    }
}

/// # Safety
/// `name` must be null or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn runtime_thread_attach_host(name: *const c_char) {
    let t = current_thread();
    // SAFETY: `t` is valid and owned by this thread; `name` is null or a NUL-terminated C string.
    unsafe { copy_c_name(&mut (*t).name, name, "host") };
}

#[no_mangle]
pub extern "C" fn runtime_thread_name_here() -> *const c_char {
    let t = CURRENT.with(Cell::get);
    if t.is_null() {
        ptr::null()
    } else {
        // SAFETY: `t` is valid and `name` is NUL-terminated.
        unsafe { (*t).name.as_ptr().cast::<c_char>() }
    }
}

#[no_mangle]
pub extern "C" fn runtime_thread_attach_main() {
    let t = current_thread();
    // SAFETY: `t` is valid and owned by the main thread.
    unsafe { copy_name(&mut (*t).name, "main") };
}

fn find_thread(handle: *mut c_void) -> Option<*mut GuestThread> {
    if handle.is_null() {
        return None;
    }
    let reg = LOCK.lock().unwrap();
    let mut cur = reg.threads as *mut GuestThread;
    while !cur.is_null() {
        if cur.cast::<c_void>() == handle {
            return Some(cur);
        }
        // SAFETY: protected by `LOCK`.
        cur = unsafe { (*cur).next };
    }
    None
}

fn find_attr(slot: *mut *mut ThreadAttr) -> Option<*mut ThreadAttr> {
    if slot.is_null() {
        return None;
    }
    // SAFETY: `slot` is non-null.
    let target = unsafe { *slot };
    if target.is_null() {
        return None;
    }
    let reg = LOCK.lock().unwrap();
    let mut cur = reg.attributes as *mut ThreadAttr;
    while !cur.is_null() {
        // SAFETY: protected by `LOCK`.
        if cur == target && unsafe { (*cur).magic } == ATTR_MAGIC {
            return Some(cur);
        }
        cur = unsafe { (*cur).next };
    }
    None
}

unsafe extern "sysv64" fn thread_self() -> *mut c_void {
    current_thread().cast()
}

extern "sysv64" fn thread_get_id() -> i32 {
    static NEXT_ID: AtomicI32 = AtomicI32::new(0);
    THREAD_ID.with(|cell| {
        let mut id = cell.get();
        if id == 0 {
            id = NEXT_ID.fetch_add(1, Ordering::Relaxed) + 1;
            cell.set(id);
        }
        id
    })
}

extern "sysv64" fn guest_error() -> *mut i32 {
    GUEST_ERRNO.with(|cell| cell.get())
}

#[no_mangle]
pub extern "C" fn runtime_errno() -> *mut i32 {
    GUEST_ERRNO.with(|cell| cell.get())
}

unsafe extern "sysv64" fn attr_init(out: *mut *mut ThreadAttr) -> i32 {
    if out.is_null() {
        return err(22);
    }
    let a = Box::into_raw(Box::new(ThreadAttr {
        magic: ATTR_MAGIC,
        detached: 0,
        policy: 1,
        prio: DEFAULT_PRIO,
        inherit: 4,
        stack: DEFAULT_STACK,
        guard: 4096,
        affinity: 0x7f,
        next: null_mut(),
    }));
    let mut reg = LOCK.lock().unwrap();
    unsafe {
        (*a).next = reg.attributes as *mut ThreadAttr;
        reg.attributes = a as usize;
        *out = a;
    }
    0
}

unsafe extern "sysv64" fn attr_destroy(slot: *mut *mut ThreadAttr) -> i32 {
    let Some(a) = find_attr(slot) else {
        return err(22);
    };
    {
        let mut reg = LOCK.lock().unwrap();
        let mut cur = reg.attributes as *mut ThreadAttr;
        let mut prev: *mut ThreadAttr = null_mut();
        while !cur.is_null() && cur != a {
            prev = cur;
            // SAFETY: protected by `LOCK`.
            cur = unsafe { (*cur).next };
        }
        if cur == a {
            unsafe {
                if prev.is_null() {
                    reg.attributes = (*a).next as usize;
                } else {
                    (*prev).next = (*a).next;
                }
                (*a).magic = 0;
            }
        }
    }
    unsafe {
        drop(Box::from_raw(a));
        *slot = null_mut();
    }
    0
}

unsafe extern "sysv64" fn attr_get(thread: *mut c_void, out: *mut *mut ThreadAttr) -> i32 {
    let Some(a) = find_attr(out) else {
        return err(22);
    };
    if thread.is_null() {
        return err(22);
    }
    let Some(t) = find_thread(thread) else {
        return err(3);
    };
    unsafe {
        let next = (*a).next;
        *a = (*t).attr;
        (*a).magic = ATTR_MAGIC;
        (*a).next = next;
        (*a).detached = (*t).detached;
    }
    0
}

unsafe extern "sysv64" fn attr_set_stack(slot: *mut *mut ThreadAttr, size: u64) -> i32 {
    let Some(a) = find_attr(slot) else {
        return err(22);
    };
    if size < 16384 {
        return err(22);
    }
    unsafe { (*a).stack = size };
    0
}

unsafe extern "sysv64" fn attr_get_stack(slot: *mut *mut ThreadAttr, size: *mut u64) -> i32 {
    let Some(a) = find_attr(slot) else {
        return err(22);
    };
    if size.is_null() {
        return err(22);
    }
    unsafe { *size = (*a).stack };
    0
}

unsafe extern "sysv64" fn attr_set_detach(slot: *mut *mut ThreadAttr, state: i32) -> i32 {
    let Some(a) = find_attr(slot) else {
        return err(22);
    };
    if state != 0 && state != 1 {
        return err(22);
    }
    unsafe { (*a).detached = state };
    0
}

unsafe extern "sysv64" fn attr_get_detach(slot: *mut *mut ThreadAttr, state: *mut i32) -> i32 {
    let Some(a) = find_attr(slot) else {
        return err(22);
    };
    if state.is_null() {
        return err(22);
    }
    unsafe { *state = (*a).detached };
    0
}

unsafe extern "sysv64" fn attr_set_policy(slot: *mut *mut ThreadAttr, policy: i32) -> i32 {
    let Some(a) = find_attr(slot) else {
        return err(22);
    };
    if !(1..=3).contains(&policy) {
        return err(22);
    }
    unsafe { (*a).policy = policy };
    0
}

unsafe extern "sysv64" fn attr_set_inherit(slot: *mut *mut ThreadAttr, inherit: i32) -> i32 {
    let Some(a) = find_attr(slot) else {
        return err(22);
    };
    if inherit != 0 && inherit != 4 {
        return err(22);
    }
    unsafe { (*a).inherit = inherit };
    0
}

unsafe extern "sysv64" fn attr_set_param(slot: *mut *mut ThreadAttr, param: *const i32) -> i32 {
    let Some(a) = find_attr(slot) else {
        return err(22);
    };
    if param.is_null() {
        return err(22);
    }
    unsafe { (*a).prio = *param };
    0
}

unsafe extern "sysv64" fn attr_get_param(slot: *mut *mut ThreadAttr, param: *mut i32) -> i32 {
    let Some(a) = find_attr(slot) else {
        return err(22);
    };
    if param.is_null() {
        return err(22);
    }
    unsafe { *param = (*a).prio };
    0
}

unsafe extern "sysv64" fn attr_set_affinity(slot: *mut *mut ThreadAttr, mask: u64) -> i32 {
    let Some(a) = find_attr(slot) else {
        return err(22);
    };
    unsafe { (*a).affinity = mask };
    0
}

unsafe extern "sysv64" fn attr_affinity(slot: *mut *mut ThreadAttr, mask: *mut u64) -> i32 {
    let Some(a) = find_attr(slot) else {
        return err(22);
    };
    if mask.is_null() {
        return err(22);
    }
    unsafe { *mask = (*a).affinity };
    0
}

unsafe extern "sysv64" fn attr_set_guard(slot: *mut *mut ThreadAttr, size: u64) -> i32 {
    let Some(a) = find_attr(slot) else {
        return err(22);
    };
    unsafe { (*a).guard = size };
    0
}

/// Linux thread names hold 15 characters; longer ones would be rejected.
#[cfg(unix)]
fn set_host_name(name: &[u8; 32]) {
    let len = name
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(name.len())
        .min(15);
    let mut host = [0u8; 16];
    host[..len].copy_from_slice(&name[..len]);
    // SAFETY: `host` is NUL-terminated and <= 15 bytes.
    unsafe { libc::pthread_setname_np(libc::pthread_self(), host.as_ptr().cast()) };
}

#[cfg(windows)]
fn set_host_name(_name: &[u8; 32]) {}

pub(crate) fn parse_guest_mxcsr(raw: &str) -> u32 {
    let trimmed = raw.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if hex.is_empty() {
        return 0;
    }
    u32::from_str_radix(hex, 16).unwrap_or(0)
}

pub(crate) fn guest_mxcsr_value() -> u32 {
    static VALUE: OnceLock<u32> = OnceLock::new();
    *VALUE.get_or_init(|| match std::env::var("BB_GUEST_MXCSR") {
        Ok(env) => parse_guest_mxcsr(&env),
        Err(_) => 0x9fc0,
    })
}

/// The PS4's floating-point state for guest code on this thread (MXCSR 0x9fc0, x87 control word 0x037f).
#[no_mangle]
pub extern "C" fn runtime_set_guest_fpu() {
    let value = guest_mxcsr_value();
    if value == 0 {
        return;
    }
    let mxcsr = AtomicU32::new(value);
    let fpucw: u16 = 0x037f;
    // SAFETY: loads MXCSR and x87 control word for guest FPU state.
    unsafe {
        core::arch::asm!(
            "ldmxcsr [{mxcsr}]",
            "fldcw [{fpucw}]",
            mxcsr = in(reg) mxcsr.as_ptr(),
            fpucw = in(reg) &fpucw,
            options(nostack),
        );
    }
}

extern "C" fn host_start(p: *mut c_void) -> *mut c_void {
    let t = p.cast::<GuestThread>();
    // SAFETY: `t` was allocated by `create` and published before `pthread_create`.
    unsafe {
        attach(t);
        set_host_name(&(*t).name);
        runtime_set_guest_fpu();
        if let Some(entry) = (*t).entry {
            (*t).result = thread_run_with_exit_jump(&mut (*t).exit_jump, entry, (*t).argument);
        }
        crate::kernel::ffi::runtime_thread_keys_cleanup();
        {
            let mut reg = LOCK.lock().unwrap();
            (*t).finished = 1;
            reg.exited += 1;
        }
        (*t).result
    }
}

#[cfg(windows)]
unsafe extern "system" fn win_host_start(p: *mut c_void) -> u32 {
    host_start(p);
    0
}

unsafe fn create(
    out: *mut *mut GuestThread,
    attr_slot: *mut *mut ThreadAttr,
    entry: Option<GuestEntry>,
    argument: *mut c_void,
    name: *const c_char,
) -> i32 {
    if out.is_null() || entry.is_none() {
        return err(22);
    }
    let mut a: *mut ThreadAttr = null_mut();
    if !attr_slot.is_null() {
        let Some(found) = find_attr(attr_slot) else {
            return err(22);
        };
        a = found;
    }
    let t = unsafe { new_thread() };
    if t.is_null() {
        return err(12);
    }
    unsafe {
        if !a.is_null() {
            (*t).attr = *a;
            (*t).attr.next = null_mut();
        }
        (*t).entry = entry;
        (*t).argument = argument;
        (*t).detached = (*t).attr.detached;
        copy_c_name(&mut (*t).name, name, "guest");

        let stack = (*t).attr.stack.max(MIN_STACK);
        let stack_bytes = stack as usize + STACK_MARGIN;
        publish(t);
        *out = t;

        #[cfg(unix)]
        {
            let mut host = std::mem::MaybeUninit::<libc::pthread_attr_t>::uninit();
            libc::pthread_attr_init(host.as_mut_ptr());
            let stack_memory = crate::memory::abi::runtime_low_map(
                stack_bytes,
                libc::PROT_READ | libc::PROT_WRITE,
            );
            if !stack_memory.is_null() {
                libc::pthread_attr_setstack(host.as_mut_ptr(), stack_memory, stack_bytes);
            } else {
                libc::pthread_attr_setstacksize(host.as_mut_ptr(), stack_bytes);
            }
            let e = libc::pthread_create(&mut (*t).host, host.as_ptr(), host_start, t.cast());
            libc::pthread_attr_destroy(host.as_mut_ptr());
            if e != 0 {
                crate::abi::host::err(&format!("STOP: host pthread_create failed: {e}\n"));
                quit(21);
            }
            if (*t).detached != 0 {
                libc::pthread_detach((*t).host);
            }
        }

        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
            use windows_sys::Win32::System::Threading::CreateThread;
            let handle = CreateThread(
                null_mut(),
                stack_bytes,
                Some(win_host_start),
                t.cast(),
                0,
                null_mut(),
            );
            if handle.is_null() {
                let e = GetLastError();
                crate::abi::host::err(&format!("STOP: host CreateThread failed: {e}\n"));
                quit(21);
            }
            (*t).host = handle as usize;
            if (*t).detached != 0 {
                CloseHandle(handle);
                (*t).host = 0;
            }
        }
        {
            let mut reg = LOCK.lock().unwrap();
            reg.created += 1;
        }
        let thread_name = CStr::from_ptr((*t).name.as_ptr().cast())
            .to_str()
            .unwrap_or("guest");
        println!(
            "Runtime: guest thread '{thread_name}' created (stack={stack}, prio={})",
            (*t).attr.prio
        );
    }
    0
}

unsafe extern "sysv64" fn thread_create(
    out: *mut *mut GuestThread,
    attr: *mut *mut ThreadAttr,
    entry: Option<GuestEntry>,
    argument: *mut c_void,
    name: *const c_char,
) -> i32 {
    unsafe { create(out, attr, entry, argument, name) }
}

unsafe extern "sysv64" fn thread_join(thread: *mut c_void, result: *mut *mut c_void) -> i32 {
    let Some(t) = find_thread(thread) else {
        return err(3);
    };
    unsafe {
        if (*t).host_owned != 0 {
            return err(3);
        }
        if t == CURRENT.with(Cell::get) {
            return err(11);
        }
        if (*t).detached != 0 || (*t).joined != 0 {
            return err(22);
        }
        (*t).joined = 1;
        #[cfg(unix)]
        {
            let mut value: *mut c_void = null_mut();
            let e = libc::pthread_join((*t).host, &mut value);
            if e != 0 {
                crate::abi::host::err(&format!("STOP: host pthread_join failed: {e}\n"));
                quit(21);
            }
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
            use windows_sys::Win32::System::Threading::{WaitForSingleObject, INFINITE};
            let handle = (*t).host as HANDLE;
            WaitForSingleObject(handle, INFINITE);
            CloseHandle(handle);
            (*t).host = 0;
        }
        if !result.is_null() {
            *result = (*t).result;
        }
        let mut reg = LOCK.lock().unwrap();
        reg.joined_count += 1;
    }
    0
}

unsafe extern "sysv64" fn thread_detach(thread: *mut c_void) -> i32 {
    let Some(t) = find_thread(thread) else {
        return err(3);
    };
    unsafe {
        if (*t).detached != 0 {
            return err(22);
        }
        (*t).detached = 1;
        if (*t).host_owned == 0 {
            #[cfg(unix)]
            libc::pthread_detach((*t).host);
            #[cfg(windows)]
            {
                use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
                let handle = (*t).host as HANDLE;
                if !handle.is_null() {
                    CloseHandle(handle);
                    (*t).host = 0;
                }
            }
        }
    }
    0
}

unsafe extern "sysv64" fn thread_exit(value: *mut c_void) -> ! {
    let t = current_thread();
    unsafe {
        if (*t).host_owned != 0 {
            crate::abi::host::err("STOP: pthread_exit on host-owned/main thread\n");
            quit(21);
        }
        (*t).result = value;
        thread_exit_longjmp(&(*t).exit_jump, value);
    }
}

extern "sysv64" fn thread_yield() -> i32 {
    std::thread::yield_now();
    0
}

unsafe extern "sysv64" fn thread_get_prio(thread: *mut c_void, prio: *mut i32) -> i32 {
    let Some(t) = find_thread(thread) else {
        return err(3);
    };
    if prio.is_null() {
        return err(22);
    }
    unsafe { *prio = (*t).attr.prio };
    0
}

unsafe extern "sysv64" fn thread_set_prio(thread: *mut c_void, prio: i32) -> i32 {
    let Some(t) = find_thread(thread) else {
        return err(3);
    };
    unsafe { (*t).attr.prio = prio };
    0
}

unsafe extern "sysv64" fn thread_set_affinity(thread: *mut c_void, mask: u64) -> i32 {
    let Some(t) = find_thread(thread) else {
        return err(3);
    };
    unsafe { (*t).attr.affinity = mask };
    0
}

unsafe extern "sysv64" fn thread_get_affinity(thread: *mut c_void, mask: *mut u64) -> i32 {
    let Some(t) = find_thread(thread) else {
        return err(3);
    };
    if mask.is_null() {
        return err(22);
    }
    unsafe { *mask = (*t).attr.affinity };
    0
}

unsafe extern "sysv64" fn thread_rename(thread: *mut c_void, name: *const c_char) -> i32 {
    let Some(t) = find_thread(thread) else {
        return err(3);
    };
    if name.is_null() {
        return err(22);
    }
    unsafe {
        copy_c_name(&mut (*t).name, name, "guest");
        if t == CURRENT.with(Cell::get) {
            set_host_name(&(*t).name);
        }
    }
    0
}

extern "sysv64" fn thread_equal(a: *mut c_void, b: *mut c_void) -> i32 {
    i32::from(a == b)
}

fn posix(r: i32) -> i32 {
    if r != 0 {
        (r as u32 & 0xffff) as i32
    } else {
        0
    }
}

unsafe extern "sysv64" fn posix_attr_init(a: *mut *mut ThreadAttr) -> i32 {
    posix(unsafe { attr_init(a) })
}
unsafe extern "sysv64" fn posix_attr_destroy(a: *mut *mut ThreadAttr) -> i32 {
    posix(unsafe { attr_destroy(a) })
}
unsafe extern "sysv64" fn posix_attr_set_detach(a: *mut *mut ThreadAttr, s: i32) -> i32 {
    posix(unsafe { attr_set_detach(a, s) })
}
unsafe extern "sysv64" fn posix_attr_set_stack(a: *mut *mut ThreadAttr, s: u64) -> i32 {
    posix(unsafe { attr_set_stack(a, s) })
}
unsafe extern "sysv64" fn posix_attr_set_param(a: *mut *mut ThreadAttr, p: *const i32) -> i32 {
    posix(unsafe { attr_set_param(a, p) })
}
unsafe extern "sysv64" fn posix_create(
    t: *mut *mut GuestThread,
    a: *mut *mut ThreadAttr,
    e: Option<GuestEntry>,
    arg: *mut c_void,
) -> i32 {
    posix(unsafe { create(t, a, e, arg, c"posix".as_ptr()) })
}
unsafe extern "sysv64" fn posix_create_name(
    t: *mut *mut GuestThread,
    a: *mut *mut ThreadAttr,
    e: Option<GuestEntry>,
    arg: *mut c_void,
    name: *const c_char,
) -> i32 {
    posix(unsafe { create(t, a, e, arg, name) })
}
unsafe extern "sysv64" fn posix_join(t: *mut c_void, r: *mut *mut c_void) -> i32 {
    posix(unsafe { thread_join(t, r) })
}

/// Co-op: guest threads started by host libraries (NP matching/signaling threads).
///
/// # Safety
/// `out` must be a valid writable pointer; `name` must be null or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn runtime_thread_spawn(
    out: *mut *mut c_void,
    // Spelled out, not `GuestEntry`: cbindgen copies an alias name into the
    // header as raw Rust. The C prototype stays `void *(*entry)(void*)`.
    entry: Option<unsafe extern "sysv64" fn(*mut c_void) -> *mut c_void>,
    argument: *mut c_void,
    stack: u64,
    name: *const c_char,
) -> i32 {
    let mut attr: *mut ThreadAttr = null_mut();
    let mut r = unsafe { attr_init(&mut attr) };
    if r == 0 && stack != 0 {
        r = unsafe { attr_set_stack(&mut attr, stack) };
    }
    if r == 0 {
        r = unsafe {
            create(
                out.cast::<*mut GuestThread>(),
                &mut attr,
                entry,
                argument,
                name,
            )
        };
    }
    if !attr.is_null() {
        unsafe { attr_destroy(&mut attr) };
    }
    posix(r)
}

/// # Safety
/// `thread` must be a handle returned by `runtime_thread_spawn`; `result` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn runtime_thread_join_spawned(
    thread: *mut c_void,
    result: *mut *mut c_void,
) -> i32 {
    posix(unsafe { thread_join(thread, result) })
}

pub(crate) fn resolve(name: &CStr) -> Option<usize> {
    let addr = match name.to_str().ok()? {
        "aI+OeCz8xrQ#p#J" | "EotR8a3ASf4#I#J" => thread_self as *const () as usize,
        "EI-5-jlq2dE#p#J" => thread_get_id as *const () as usize,
        "9BcDykPmo1I#p#J" => guest_error as *const () as usize,
        "nsYoNRywwNg#p#J" => attr_init as *const () as usize,
        "62KCwEMmzcM#p#J" => attr_destroy as *const () as usize,
        "x1X76arYMxU#p#J" => attr_get as *const () as usize,
        "8+s5BzZjxSg#p#J" => attr_affinity as *const () as usize,
        "UTXzJbWhhTE#p#J" => attr_set_stack as *const () as usize,
        "-Wreprtu0Qs#p#J" => attr_set_detach as *const () as usize,
        "4+h9EzwKF4I#p#J" => attr_set_policy as *const () as usize,
        "DzES9hQF4f4#p#J" => attr_set_param as *const () as usize,
        "3qxgM4ezETA#p#J" => attr_set_affinity as *const () as usize,
        "eXbUSpEaTsA#p#J" => attr_set_inherit as *const () as usize,
        "JaRMy+QcpeU#p#J" => attr_get_detach as *const () as usize,
        "-fA+7ZlGDQs#p#J" => attr_get_stack as *const () as usize,
        "FXPWHNk8Of0#p#J" => attr_get_param as *const () as usize,
        "El+cQ20DynU#p#J" => attr_set_guard as *const () as usize,
        "6UgtwV+0zb4#p#J" => thread_create as *const () as usize,
        "onNY9Byn-W8#p#J" => thread_join as *const () as usize,
        "4qGrR6eoP9Y#p#J" => thread_detach as *const () as usize,
        "3kg7rT0NQIs#p#J" | "FJrT5LuUBAU#I#J" => thread_exit as *const () as usize,
        "T72hz6ffq08#p#J" => thread_yield as *const () as usize,
        "1tKyG7RlMJo#p#J" => thread_get_prio as *const () as usize,
        "W0Hpm2X0uPE#p#J" => thread_set_prio as *const () as usize,
        "bt3CTBKmGyI#p#J" => thread_set_affinity as *const () as usize,
        "rcrVFJsQWRY#p#J" => thread_get_affinity as *const () as usize,
        "GBUY7ywdULE#p#J" => thread_rename as *const () as usize,
        "3PtV6p3QNX4#p#J" => thread_equal as *const () as usize,
        "wtkt-teR1so#I#J" => posix_attr_init as *const () as usize,
        "zHchY8ft5pk#I#J" => posix_attr_destroy as *const () as usize,
        "E+tyo3lp5Lw#I#J" => posix_attr_set_detach as *const () as usize,
        "2Q0z6rnBrTE#I#J" => posix_attr_set_stack as *const () as usize,
        "euKRgm0Vn2M#I#J" => posix_attr_set_param as *const () as usize,
        "OxhIB8LB-PQ#I#J" => posix_create as *const () as usize,
        "Jmi+9w9u0E4#I#J" => posix_create_name as *const () as usize,
        "h9CcP3J0oVM#I#J" => posix_join as *const () as usize,
        _ => return None,
    };
    Some(addr)
}

/// # Safety
/// `name` must be null or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn runtime_thread_resolve(name: *const c_char) -> usize {
    if name.is_null() {
        return 0;
    }
    // SAFETY: caller guarantees `name` is a NUL-terminated C string.
    resolve(unsafe { CStr::from_ptr(name) }).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn runtime_thread_report() {
    let reg = LOCK.lock().unwrap();
    println!(
        "Runtime: guest threads created={}, exited={}, joined={}",
        reg.created, reg.exited, reg.joined_count
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "sysv64" fn worker_fpu_and_exit(arg: *mut c_void) -> *mut c_void {
        let mut mxcsr: u32 = 0;
        let mut fpucw: u16 = 0;
        core::arch::asm!(
            "stmxcsr [{mxcsr}]",
            "fnstcw [{fpucw}]",
            mxcsr = in(reg) &mut mxcsr,
            fpucw = in(reg) &mut fpucw,
            options(nostack),
        );
        assert_eq!(mxcsr, 0x9fc0);
        assert_eq!(fpucw, 0x037f);
        assert!(!thread_self().is_null());
        assert!(thread_get_id() > 0);
        *guest_error() = 42;
        assert_eq!(*runtime_errno(), 42);
        thread_exit(arg);
    }

    unsafe extern "sysv64" fn worker_return(arg: *mut c_void) -> *mut c_void {
        (arg as usize + 7) as *mut c_void
    }

    #[test]
    fn thread_lifecycle_exit_fpu_and_attributes() {
        let mut attr: *mut ThreadAttr = null_mut();
        unsafe {
            assert_eq!(attr_init(&mut attr), 0);
            assert_eq!(attr_set_stack(&mut attr, 128 * 1024), 0);
            let mut stack = 0u64;
            assert_eq!(attr_get_stack(&mut attr, &mut stack), 0);
            assert_eq!(stack, 128 * 1024);

            assert_eq!(attr_set_param(&mut attr, &650), 0);
            let mut prio = 0;
            assert_eq!(attr_get_param(&mut attr, &mut prio), 0);
            assert_eq!(prio, 650);

            assert_eq!(attr_set_affinity(&mut attr, 0x3), 0);
            let mut mask = 0u64;
            assert_eq!(attr_affinity(&mut attr, &mut mask), 0);
            assert_eq!(mask, 0x3);

            let mut t: *mut GuestThread = null_mut();
            assert_eq!(
                thread_create(
                    &mut t,
                    &mut attr,
                    Some(worker_fpu_and_exit),
                    99usize as *mut c_void,
                    c"test-worker".as_ptr(),
                ),
                0
            );
            assert_eq!(thread_equal(t.cast(), t.cast()), 1);
            assert_eq!(thread_rename(t.cast(), c"renamed-w".as_ptr()), 0);
            let mut res: *mut c_void = null_mut();
            assert_eq!(thread_join(t.cast(), &mut res), 0);
            assert_eq!(res as usize, 99);
            assert_eq!(thread_join(t.cast(), null_mut()), err(22));
            assert_eq!(attr_destroy(&mut attr), 0);
        }
    }

    unsafe extern "sysv64" fn worker_return_c(arg: *mut c_void) -> *mut c_void {
        let mut mxcsr: u32 = 0;
        let mut fpucw: u16 = 0;
        core::arch::asm!(
            "stmxcsr [{mxcsr}]",
            "fnstcw [{fpucw}]",
            mxcsr = in(reg) &mut mxcsr,
            fpucw = in(reg) &mut fpucw,
            options(nostack),
        );
        assert_eq!(mxcsr, 0x9fc0);
        assert_eq!(fpucw, 0x037f);
        (arg as usize + 7) as *mut c_void
    }

    #[test]
    fn spawn_join_and_detach_and_resolve() {
        let mut spawned: *mut c_void = null_mut();
        unsafe {
            assert_eq!(
                runtime_thread_spawn(
                    &mut spawned,
                    Some(worker_return_c),
                    10usize as *mut c_void,
                    0,
                    c"coop".as_ptr(),
                ),
                0
            );
            let mut res: *mut c_void = null_mut();
            assert_eq!(runtime_thread_join_spawned(spawned, &mut res), 0);
            assert_eq!(res as usize, 17);

            let mut attr: *mut ThreadAttr = null_mut();
            assert_eq!(posix_attr_init(&mut attr), 0);
            assert_eq!(posix_attr_set_detach(&mut attr, 1), 0);
            let mut detached_t: *mut GuestThread = null_mut();
            assert_eq!(
                posix_create(&mut detached_t, &mut attr, Some(worker_return), null_mut()),
                0
            );
            assert_eq!(posix_join(detached_t.cast(), null_mut()), 22);
            assert_eq!(posix_attr_destroy(&mut attr), 0);
        }

        unsafe {
            assert_ne!(runtime_thread_resolve(c"6UgtwV+0zb4#p#J".as_ptr()), 0);
            assert_eq!(runtime_thread_resolve(c"missing".as_ptr()), 0);
        }
    }

    #[test]
    fn guest_mxcsr_parser_matches_c_hex_rules() {
        assert_eq!(parse_guest_mxcsr("0x9fc0"), 0x9fc0);
        assert_eq!(parse_guest_mxcsr("0X1f80"), 0x1f80);
        assert_eq!(parse_guest_mxcsr(" 1f80 "), 0x1f80);
        assert_eq!(parse_guest_mxcsr("0"), 0);
        assert_eq!(parse_guest_mxcsr(""), 0);
        assert_eq!(parse_guest_mxcsr("not-hex"), 0);
    }
}
