//! Signal and VEH fault handlers, speculative fault recovery, `MOVBE` emulation, and
//! watchdog thread-dump helpers for [`super`] (porting `fault`, `vectored_fault`, `unhandled_fault`,
//! `emulate_movbe`, `thread_dump`, and `watchdog` from `src/probe.c` into Rust for roadmap 3.17).

use crate::crash;
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

static IMAGE_BASE: AtomicU64 = AtomicU64::new(0);
static EXE_BASE: AtomicU64 = AtomicU64::new(0);
static GPU_ENABLED: AtomicBool = AtomicBool::new(false);
static GPU_HANDLE_FAULT_FN: AtomicUsize = AtomicUsize::new(0);
static GPU_DUMP_WRITES_FN: AtomicUsize = AtomicUsize::new(0);
static BENCHMARK_CRASH_FN: AtomicUsize = AtomicUsize::new(0);
static FAULT_RECOVER_FN: AtomicUsize = AtomicUsize::new(0);

/// Optional C/GPU hooks invoked by the fault handler when linked into a GPU-enabled runner.
#[derive(Debug, Clone, Copy, Default)]
pub struct FaultHooks {
    pub gpu_handle_fault: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32>,
    pub gpu_dump_writes: Option<unsafe extern "C" fn(*mut c_void)>,
    pub benchmark_crash: Option<unsafe extern "C" fn()>,
    /// Optional C/GPU speculative read recovery hook `(sig_or_code, context) -> handled (nonzero)`.
    pub fault_recover: Option<unsafe extern "C" fn(i32, *mut c_void) -> i32>,
}

/// Records the loaded guest image base, host executable base, and GPU fault-tracking state for the
/// fault/watchdog handlers.
pub fn configure_fault_context(image_base: u64, exe_base: u64, gpu_enabled: bool) {
    IMAGE_BASE.store(image_base, Ordering::Release);
    EXE_BASE.store(exe_base, Ordering::Release);
    GPU_ENABLED.store(gpu_enabled, Ordering::Release);
}

/// Registers optional GPU/benchmark/recovery callbacks for the fault handler.
pub fn configure_fault_hooks(hooks: FaultHooks) {
    GPU_HANDLE_FAULT_FN.store(
        hooks.gpu_handle_fault.map_or(0, |f| f as usize),
        Ordering::Release,
    );
    GPU_DUMP_WRITES_FN.store(
        hooks.gpu_dump_writes.map_or(0, |f| f as usize),
        Ordering::Release,
    );
    BENCHMARK_CRASH_FN.store(
        hooks.benchmark_crash.map_or(0, |f| f as usize),
        Ordering::Release,
    );
    FAULT_RECOVER_FN.store(
        hooks.fault_recover.map_or(0, |f| f as usize),
        Ordering::Release,
    );
}

/// Returns the configured `(image_base, exe_base, gpu_enabled)` tuple.
pub fn fault_context() -> (u64, u64, bool) {
    (
        IMAGE_BASE.load(Ordering::Acquire),
        EXE_BASE.load(Ordering::Acquire),
        GPU_ENABLED.load(Ordering::Acquire),
    )
}

/// Saved register state for speculative guest-memory reads inside [`with_fault_recovery`].
#[repr(C, align(16))]
#[derive(Default)]
struct FaultRecoveryBuf {
    words: [u64; 32],
}

thread_local! {
    static RUST_FAULT_RECOVER: Cell<*mut FaultRecoveryBuf> = const { Cell::new(std::ptr::null_mut()) };
}

#[cfg(target_os = "linux")]
core::arch::global_asm!(
    ".globl uncharted_run_with_recovery",
    "uncharted_run_with_recovery:",
    "push rbx",
    "push rbp",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "sub rsp, 8",
    "mov [rdi + 0], rbx",
    "mov [rdi + 8], rbp",
    "mov [rdi + 16], r12",
    "mov [rdi + 24], r13",
    "mov [rdi + 32], r14",
    "mov [rdi + 40], r15",
    "mov [rdi + 48], rsp",
    "lea rax, [rip + 2f]",
    "mov [rdi + 56], rax",
    "stmxcsr [rdi + 64]",
    "fnstcw [rdi + 68]",
    "mov rdi, rdx",
    "call rsi",
    "xor eax, eax",
    "jmp 3f",
    "2:",
    "mov eax, 1",
    "3:",
    "add rsp, 8",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbp",
    "pop rbx",
    "ret",
    ".globl uncharted_fault_longjmp",
    "uncharted_fault_longjmp:",
    "mov eax, esi",
    "test eax, eax",
    "jnz 1f",
    "mov eax, 1",
    "1:",
    "mov rbx, [rdi + 0]",
    "mov rbp, [rdi + 8]",
    "mov r12, [rdi + 16]",
    "mov r13, [rdi + 24]",
    "mov r14, [rdi + 32]",
    "mov r15, [rdi + 40]",
    "ldmxcsr [rdi + 64]",
    "fldcw [rdi + 68]",
    "mov rsp, [rdi + 48]",
    "jmp qword ptr [rdi + 56]",
);

#[cfg(windows)]
core::arch::global_asm!(
    ".globl uncharted_run_with_recovery",
    "uncharted_run_with_recovery:",
    "push rbx",
    "push rbp",
    "push rdi",
    "push rsi",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "sub rsp, 40",
    "mov [rcx + 0], rbx",
    "mov [rcx + 8], rbp",
    "mov [rcx + 16], rdi",
    "mov [rcx + 24], rsi",
    "mov [rcx + 32], r12",
    "mov [rcx + 40], r13",
    "mov [rcx + 48], r14",
    "mov [rcx + 56], r15",
    "mov [rcx + 64], rsp",
    "lea rax, [rip + 2f]",
    "mov [rcx + 72], rax",
    "movups [rcx + 80], xmm6",
    "movups [rcx + 96], xmm7",
    "movups [rcx + 112], xmm8",
    "movups [rcx + 128], xmm9",
    "movups [rcx + 144], xmm10",
    "movups [rcx + 160], xmm11",
    "movups [rcx + 176], xmm12",
    "movups [rcx + 192], xmm13",
    "movups [rcx + 208], xmm14",
    "movups [rcx + 224], xmm15",
    "stmxcsr [rcx + 240]",
    "fnstcw [rcx + 244]",
    "mov rcx, r8",
    "call rdx",
    "xor eax, eax",
    "jmp 3f",
    "2:",
    "mov eax, 1",
    "3:",
    "add rsp, 40",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rsi",
    "pop rdi",
    "pop rbp",
    "pop rbx",
    "ret",
    ".globl uncharted_fault_longjmp",
    "uncharted_fault_longjmp:",
    "mov eax, edx",
    "test eax, eax",
    "jnz 1f",
    "mov eax, 1",
    "1:",
    "mov rbx, [rcx + 0]",
    "mov rbp, [rcx + 8]",
    "mov rdi, [rcx + 16]",
    "mov rsi, [rcx + 24]",
    "mov r12, [rcx + 32]",
    "mov r13, [rcx + 40]",
    "mov r14, [rcx + 48]",
    "mov r15, [rcx + 56]",
    "movups xmm6, [rcx + 80]",
    "movups xmm7, [rcx + 96]",
    "movups xmm8, [rcx + 112]",
    "movups xmm9, [rcx + 128]",
    "movups xmm10, [rcx + 144]",
    "movups xmm11, [rcx + 160]",
    "movups xmm12, [rcx + 176]",
    "movups xmm13, [rcx + 192]",
    "movups xmm14, [rcx + 208]",
    "movups xmm15, [rcx + 224]",
    "ldmxcsr [rcx + 240]",
    "fldcw [rcx + 244]",
    "mov rsp, [rcx + 64]",
    "jmp qword ptr [rcx + 72]",
);

extern "C" {
    fn uncharted_run_with_recovery(
        buf: *mut FaultRecoveryBuf,
        cb: extern "C" fn(*mut c_void),
        ctx: *mut c_void,
    ) -> i32;
    fn uncharted_fault_longjmp(buf: *mut FaultRecoveryBuf, value: i32) -> !;
}

/// Runs `f` with speculative memory-fault recovery armed on the current thread.
///
/// Returns `Some(T)` when `f` completes normally, or `None` if a `SIGSEGV`/`SIGBUS` (Linux) or
/// access violation (Windows) is caught by the installed loader fault handler while `f` executes.
/// `f` must only perform plain reads/writes that do not rely on destructors running on fault.
pub fn with_fault_recovery<F: FnOnce() -> T, T>(f: F) -> Option<T> {
    struct State<F, T> {
        func: Option<F>,
        out: Option<T>,
    }
    extern "C" fn trampoline<F: FnOnce() -> T, T>(ctx: *mut c_void) {
        // SAFETY: `ctx` points to `State<F, T>` on `with_fault_recovery`'s stack and runs once.
        let state = unsafe { &mut *(ctx as *mut State<F, T>) };
        if let Some(f) = state.func.take() {
            state.out = Some(f());
        }
    }

    let mut state = State {
        func: Some(f),
        out: None,
    };
    let mut buf = FaultRecoveryBuf::default();
    let prev = RUST_FAULT_RECOVER.get();
    RUST_FAULT_RECOVER.set(&raw mut buf);
    // SAFETY: `buf` and `state` live on the stack for the duration of the synchronous call.
    let faulted = unsafe {
        uncharted_run_with_recovery(&raw mut buf, trampoline::<F, T>, (&raw mut state).cast())
    };
    RUST_FAULT_RECOVER.set(prev);
    if faulted == 0 {
        state.out
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
pub mod linux {
    use super::*;
    use crate::cpu_patch::{cpu_patch_breakpoint, cpu_patch_illegal};
    use std::ffi::CStr;

    /// Installs the Linux `SIGSEGV`, `SIGILL`, `SIGBUS`, `SIGTRAP`, `SIGUSR2`, and `SIGALRM`
    /// handlers for guest execution.
    ///
    /// # Safety
    /// Must be called from the main thread before guest code executes.
    pub unsafe fn install_handlers() {
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = fault_handler as *const () as usize;
            sa.sa_flags = libc::SA_SIGINFO;
            libc::sigemptyset(&mut sa.sa_mask);
            for sig in [libc::SIGSEGV, libc::SIGILL, libc::SIGBUS, libc::SIGTRAP] {
                libc::sigaction(sig, &sa, std::ptr::null_mut());
            }

            let mut dump: libc::sigaction = std::mem::zeroed();
            dump.sa_sigaction = thread_dump_handler as *const () as usize;
            dump.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
            libc::sigemptyset(&mut dump.sa_mask);
            libc::sigaction(libc::SIGUSR2, &dump, std::ptr::null_mut());

            let mut alarm_action: libc::sigaction = std::mem::zeroed();
            alarm_action.sa_sigaction = watchdog_handler as *const () as usize;
            alarm_action.sa_flags = libc::SA_SIGINFO;
            libc::sigemptyset(&mut alarm_action.sa_mask);
            libc::sigaction(libc::SIGALRM, &alarm_action, std::ptr::null_mut());
        }
    }

    extern "C" fn fault_handler(
        sig: libc::c_int,
        info: *mut libc::siginfo_t,
        context: *mut c_void,
    ) {
        if context.is_null() {
            let _ = crash::report_linux_fault_signal_safe(sig, 0, 0, 0, None);
            unsafe { libc::_exit(128 + sig) };
        }
        // SAFETY: `context` is a kernel-provided `ucontext_t *`.
        let uc = unsafe { &mut *(context as *mut libc::ucontext_t) };
        let rip_ref = &mut uc.uc_mcontext.gregs[libc::REG_RIP as usize];

        if sig == libc::SIGTRAP || sig == libc::SIGILL {
            let mut to = *rip_ref as u64;
            let handled = if sig == libc::SIGTRAP {
                unsafe { cpu_patch_breakpoint(&mut to, to.wrapping_sub(1)) }
            } else {
                unsafe { cpu_patch_illegal(&mut to) }
            };
            if handled != 0 {
                *rip_ref = to as libc::greg_t;
                return;
            }
        }

        let fault_addr = if info.is_null() {
            std::ptr::null_mut()
        } else {
            // SAFETY: kernel-provided `siginfo_t`.
            unsafe { (*info).si_addr() }
        };

        let (image_base, _, gpu_enabled) = fault_context();
        if gpu_enabled && sig == libc::SIGSEGV {
            let fn_ptr = GPU_HANDLE_FAULT_FN.load(Ordering::Acquire);
            if fn_ptr != 0 {
                // SAFETY: `configure_fault_hooks` received a valid `gpu_handle_fault` pointer.
                let f: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32 =
                    unsafe { std::mem::transmute(fn_ptr) };
                if unsafe { f(context, fault_addr) } != 0 {
                    return;
                }
            }
        }

        if sig == libc::SIGSEGV || sig == libc::SIGBUS {
            let recover_hook = FAULT_RECOVER_FN.load(Ordering::Acquire);
            if recover_hook != 0 {
                // SAFETY: `configure_fault_hooks` received a valid `fault_recover` pointer.
                let f: unsafe extern "C" fn(i32, *mut c_void) -> i32 =
                    unsafe { std::mem::transmute(recover_hook) };
                if unsafe { f(sig, context) } != 0 {
                    return;
                }
            }
            let recover_buf = RUST_FAULT_RECOVER.get();
            if !recover_buf.is_null() {
                RUST_FAULT_RECOVER.set(std::ptr::null_mut());
                let rsp = uc.uc_mcontext.gregs[libc::REG_RSP as usize] as u64;
                uc.uc_mcontext.gregs[libc::REG_RSP as usize] =
                    ((rsp.wrapping_sub(128)) & !15u64) as libc::greg_t;
                uc.uc_mcontext.gregs[libc::REG_RDI as usize] = recover_buf as usize as libc::greg_t;
                uc.uc_mcontext.gregs[libc::REG_RSI as usize] = 1;
                uc.uc_mcontext.gregs[libc::REG_RIP as usize] =
                    uncharted_fault_longjmp as *const () as usize as libc::greg_t;
                return;
            }
        }

        let rip = *rip_ref as u64;
        let mut dl: libc::Dl_info = unsafe { std::mem::zeroed() };
        let host_where = unsafe {
            if libc::dladdr(rip as usize as *const c_void, &mut dl) != 0 && !dl.dli_fname.is_null()
            {
                let fname = CStr::from_ptr(dl.dli_fname).to_string_lossy();
                let sname = if dl.dli_sname.is_null() {
                    None
                } else {
                    Some(CStr::from_ptr(dl.dli_sname).to_string_lossy())
                };
                Some((
                    fname.into_owned(),
                    rip.wrapping_sub(dl.dli_fbase as usize as u64),
                    sname.map(|s| s.into_owned()),
                ))
            } else {
                None
            }
        };

        let host_ref = host_where
            .as_ref()
            .map(|(f, off, s)| (f.as_str(), *off, s.as_deref()));
        let line =
            crash::format_linux_fault(sig, rip, image_base, fault_addr as usize as u64, host_ref);
        unsafe {
            libc::write(2, line.as_ptr() as *const c_void, line.len());
            crash::report_linux_fault_signal_safe(
                sig,
                rip,
                image_base,
                fault_addr as usize as u64,
                host_ref,
            );
            if gpu_enabled {
                let fn_ptr = GPU_DUMP_WRITES_FN.load(Ordering::Acquire);
                if fn_ptr != 0 {
                    let f: unsafe extern "C" fn(*mut c_void) = std::mem::transmute(fn_ptr);
                    f(context);
                }
            }
            let bench_ptr = BENCHMARK_CRASH_FN.load(Ordering::Acquire);
            if bench_ptr != 0 {
                let f: unsafe extern "C" fn() = std::mem::transmute(bench_ptr);
                f();
            }
            libc::_exit(128 + sig);
        }
    }

    extern "C" fn thread_dump_handler(
        _sig: libc::c_int,
        _info: *mut libc::siginfo_t,
        context: *mut c_void,
    ) {
        if !context.is_null() {
            // SAFETY: `context` is a kernel-provided `ucontext_t *`.
            unsafe { dump_frames(&*(context as *const libc::ucontext_t)) };
        }
    }

    extern "C" fn watchdog_handler(
        sig: libc::c_int,
        _info: *mut libc::siginfo_t,
        context: *mut c_void,
    ) {
        const HEAD: &[u8] = b"STOP: watchdog timeout; thread stacks:\n";
        unsafe {
            libc::write(2, HEAD.as_ptr() as *const c_void, HEAD.len());
            if !context.is_null() {
                dump_frames(&*(context as *const libc::ucontext_t));
            }
            signal_other_threads_dump();
            crash::report_signal_safe(crash::CrashKind::Exception, "watchdog", "watchdog timeout");
            libc::usleep(100_000);
            libc::_exit(128 + sig);
        }
    }

    unsafe fn signal_other_threads_dump() {
        #[repr(C)]
        struct LinuxDirent64 {
            d_ino: u64,
            d_off: i64,
            d_reclen: u16,
            d_type: u8,
        }
        const DIR_PATH: &[u8] = b"/proc/self/task\0";
        // SAFETY: static NUL-terminated procfs path.
        let dir =
            unsafe { libc::open(DIR_PATH.as_ptr().cast(), libc::O_RDONLY | libc::O_DIRECTORY) };
        if dir < 0 {
            return;
        }
        let pid = unsafe { libc::getpid() };
        let self_tid = unsafe { libc::syscall(libc::SYS_gettid) } as libc::pid_t;
        let mut buf = [0u8; 4096];
        loop {
            let n =
                unsafe { libc::syscall(libc::SYS_getdents64, dir, buf.as_mut_ptr(), buf.len()) };
            if n <= 0 {
                break;
            }
            let mut at = 0usize;
            while at + std::mem::size_of::<LinuxDirent64>() < n as usize {
                // SAFETY: `at` is aligned/bounded within `buf[..n]`; read unaligned header.
                let hdr = unsafe {
                    std::ptr::read_unaligned(buf.as_ptr().add(at).cast::<LinuxDirent64>())
                };
                if hdr.d_reclen == 0 {
                    break;
                }
                let name_start = at + std::mem::size_of::<LinuxDirent64>();
                let rec_end = (at + usize::from(hdr.d_reclen)).min(n as usize);
                if name_start < rec_end {
                    let name_bytes = &buf[name_start..rec_end];
                    let tid = parse_decimal_tid(name_bytes);
                    if tid > 0 && tid != self_tid {
                        unsafe {
                            libc::syscall(libc::SYS_tgkill, pid, tid, libc::SIGUSR2);
                            libc::usleep(20_000);
                        }
                    }
                }
                at += usize::from(hdr.d_reclen);
            }
        }
        unsafe { libc::close(dir) };
    }

    fn parse_decimal_tid(bytes: &[u8]) -> libc::pid_t {
        let mut val: i32 = 0;
        let mut any = false;
        for &b in bytes {
            if b == 0 {
                break;
            }
            if !b.is_ascii_digit() {
                return 0;
            }
            any = true;
            val = val.saturating_mul(10).saturating_add(i32::from(b - b'0'));
        }
        if any {
            val
        } else {
            0
        }
    }

    unsafe fn dump_frames(uc: &libc::ucontext_t) {
        let mut rip = uc.uc_mcontext.gregs[libc::REG_RIP as usize] as u64;
        let mut rbp = uc.uc_mcontext.gregs[libc::REG_RBP as usize] as u64;
        let (image_base, exe_base, _) = fault_context();
        let tid = unsafe { libc::syscall(libc::SYS_gettid) } as u64;

        let mut arg_line = *b"  tid=0000000000000000 rdi=0000000000000000\n";
        write_hex(&mut arg_line[6..22], tid);
        write_hex(
            &mut arg_line[27..43],
            uc.uc_mcontext.gregs[libc::REG_RDI as usize] as u64,
        );
        unsafe {
            libc::write(2, arg_line.as_ptr() as *const c_void, arg_line.len());
        }

        let mut line = *b"  tid=0000000000000000 rip=0000000000000000 image-relative=0000000000000000 host-relative=0000000000000000\n";
        for _ in 0..24 {
            write_hex(&mut line[6..22], tid);
            write_hex(&mut line[27..43], rip);
            write_hex(&mut line[59..75], rip.wrapping_sub(image_base));
            write_hex(&mut line[90..106], rip.wrapping_sub(exe_base));
            unsafe {
                libc::write(2, line.as_ptr() as *const c_void, line.len());
            }
            if rbp == 0 {
                break;
            }
            let mut frame = [0u64; 2];
            let local = libc::iovec {
                iov_base: frame.as_mut_ptr() as *mut c_void,
                iov_len: std::mem::size_of::<[u64; 2]>(),
            };
            let remote = libc::iovec {
                iov_base: rbp as usize as *mut c_void,
                iov_len: std::mem::size_of::<[u64; 2]>(),
            };
            let n = unsafe { libc::process_vm_readv(libc::getpid(), &local, 1, &remote, 1, 0) };
            if n != std::mem::size_of::<[u64; 2]>() as isize || frame[0] <= rbp {
                break;
            }
            rbp = frame[0];
            rip = frame[1];
        }
    }

    fn write_hex(out: &mut [u8], v: u64) {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        for (i, slot) in out.iter_mut().take(16).enumerate() {
            *slot = DIGITS[((v >> (60 - i * 4)) & 15) as usize];
        }
    }
}

#[cfg(windows)]
pub mod windows {
    use super::*;
    use crate::cpu_patch::{cpu_patch_breakpoint, cpu_patch_illegal};
    use crate::red_zone::red_zone_fault_handled;
    use std::sync::atomic::AtomicI64;
    use windows_sys::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, SetUnhandledExceptionFilter, EXCEPTION_POINTERS,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, Sleep, TerminateProcess};

    static MOVBE_EMULATED: AtomicI64 = AtomicI64::new(0);
    static FATAL_REPORTED: AtomicBool = AtomicBool::new(false);

    const EXCEPTION_ACCESS_VIOLATION: i32 = 0xC000_0005_u32 as i32;
    const EXCEPTION_BREAKPOINT: i32 = 0x8000_0003_u32 as i32;
    const EXCEPTION_ILLEGAL_INSTRUCTION: i32 = 0xC000_001D_u32 as i32;
    const EXCEPTION_PRIV_INSTRUCTION: i32 = 0xC000_0096_u32 as i32;
    const EXCEPTION_STACK_OVERFLOW: i32 = 0xC000_00FD_u32 as i32;
    const EXCEPTION_INT_DIVIDE_BY_ZERO: i32 = 0xC000_0094_u32 as i32;
    const EXCEPTION_DATATYPE_MISALIGNMENT: i32 = 0x8000_0002_u32 as i32;
    const EXCEPTION_CONTINUE_EXECUTION: i32 = -1;
    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

    /// Installs the Windows vectored and unhandled exception handlers for guest execution.
    ///
    /// # Safety
    /// Must be called from the main thread before guest code executes.
    pub unsafe fn install_handlers() {
        // SAFETY: process-wide VEH and unhandled filter registration.
        unsafe {
            AddVectoredExceptionHandler(1, Some(vectored_fault));
            SetUnhandledExceptionFilter(Some(unhandled_fault));
        }
    }

    /// Emulates a single `MOVBE` memory-operand instruction in `CONTEXT` (for AMD FX hosts).
    ///
    /// # Safety
    /// `regs` must point to the 16 contiguous 64-bit GPRs starting at `CONTEXT.Rax`, and `*rip`
    /// must point to readable executable memory.
    pub unsafe fn emulate_movbe(regs: &mut [u64; 16], rip: &mut u64) -> bool {
        // SAFETY: caller guarantees `*rip` points to readable guest code.
        let code = unsafe { std::slice::from_raw_parts(*rip as usize as *const u8, 15) };
        let Some(access) = crash::movbe_decode(code, regs, *rip) else {
            return false;
        };
        let ptr = access.address as usize as *mut u8;
        // SAFETY: guest instruction faulted on `MOVBE` itself, not on `address`.
        unsafe {
            if access.is_store {
                let v = regs[access.reg];
                match access.width {
                    8 => std::ptr::write_unaligned(ptr as *mut u64, v.swap_bytes()),
                    4 => std::ptr::write_unaligned(ptr as *mut u32, (v as u32).swap_bytes()),
                    2 => std::ptr::write_unaligned(ptr as *mut u16, (v as u16).swap_bytes()),
                    _ => return false,
                }
            } else {
                match access.width {
                    8 => {
                        regs[access.reg] = std::ptr::read_unaligned(ptr as *const u64).swap_bytes()
                    }
                    4 => {
                        regs[access.reg] =
                            u64::from(std::ptr::read_unaligned(ptr as *const u32).swap_bytes())
                    }
                    2 => {
                        let v = std::ptr::read_unaligned(ptr as *const u16).swap_bytes();
                        regs[access.reg] = (regs[access.reg] & !0xffff) | u64::from(v);
                    }
                    _ => return false,
                }
            }
        }
        *rip += access.len as u64;
        MOVBE_EMULATED.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Returns the number of `MOVBE` instructions emulated so far.
    pub fn movbe_emulated_count() -> i64 {
        MOVBE_EMULATED.load(Ordering::Relaxed)
    }

    unsafe fn dispatch_cpu_patch(is_breakpoint: bool, rip: &mut u64) -> bool {
        let mut to = *rip;
        let ok = if is_breakpoint {
            unsafe { cpu_patch_breakpoint(&mut to, to) }
        } else {
            unsafe { cpu_patch_illegal(&mut to) }
        };
        if ok != 0 {
            *rip = to;
            red_zone_fault_handled(to);
            true
        } else {
            false
        }
    }

    unsafe extern "system" fn vectored_fault(info: *mut EXCEPTION_POINTERS) -> i32 {
        if info.is_null() {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        // SAFETY: Windows passes valid `EXCEPTION_POINTERS`.
        let (record, ctx) = unsafe { (&*(*info).ExceptionRecord, &mut *(*info).ContextRecord) };
        let code = record.ExceptionCode;

        if code == EXCEPTION_BREAKPOINT || code == EXCEPTION_ILLEGAL_INSTRUCTION {
            let mut rip = ctx.Rip;
            if unsafe { dispatch_cpu_patch(code == EXCEPTION_BREAKPOINT, &mut rip) } {
                ctx.Rip = rip;
                return EXCEPTION_CONTINUE_EXECUTION;
            }
        }

        let (image_base, _, gpu_enabled) = fault_context();
        if code == EXCEPTION_ACCESS_VIOLATION && record.NumberParameters >= 2 {
            let fault_addr = record.ExceptionInformation[1] as *mut c_void;
            if gpu_enabled {
                let fn_ptr = GPU_HANDLE_FAULT_FN.load(Ordering::Acquire);
                if fn_ptr != 0 {
                    let f: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32 =
                        unsafe { std::mem::transmute(fn_ptr) };
                    if unsafe { f(info.cast(), fault_addr) } != 0 {
                        red_zone_fault_handled(ctx.Rip);
                        return EXCEPTION_CONTINUE_EXECUTION;
                    }
                }
            }
            let recover_hook = FAULT_RECOVER_FN.load(Ordering::Acquire);
            if recover_hook != 0 {
                let f: unsafe extern "C" fn(i32, *mut c_void) -> i32 =
                    unsafe { std::mem::transmute(recover_hook) };
                if unsafe { f(code, info.cast()) } != 0 {
                    return EXCEPTION_CONTINUE_EXECUTION;
                }
            }
            let recover_buf = RUST_FAULT_RECOVER.get();
            if !recover_buf.is_null() {
                RUST_FAULT_RECOVER.set(std::ptr::null_mut());
                ctx.Rsp = ((ctx.Rsp.wrapping_sub(128)) & !15u64).wrapping_sub(40);
                ctx.Rcx = recover_buf as usize as u64;
                ctx.Rdx = 1;
                ctx.Rip = uncharted_fault_longjmp as *const () as usize as u64;
                return EXCEPTION_CONTINUE_EXECUTION;
            }
        }

        if code == EXCEPTION_ILLEGAL_INSTRUCTION
            && image_base != 0
            && ctx.Rip.wrapping_sub(image_base) < 0x2000_0000
        {
            // SAFETY: `CONTEXT.Rax..R15` are 16 contiguous `u64` fields in `CONTEXT`.
            let regs = unsafe { &mut *(&raw mut ctx.Rax).cast::<[u64; 16]>() };
            let mut rip = ctx.Rip;
            if unsafe { emulate_movbe(regs, &mut rip) } {
                ctx.Rip = rip;
                return EXCEPTION_CONTINUE_EXECUTION;
            }
        }

        let is_fatal = matches!(
            code,
            EXCEPTION_ACCESS_VIOLATION
                | EXCEPTION_ILLEGAL_INSTRUCTION
                | EXCEPTION_PRIV_INSTRUCTION
                | EXCEPTION_STACK_OVERFLOW
                | EXCEPTION_INT_DIVIDE_BY_ZERO
                | EXCEPTION_DATATYPE_MISALIGNMENT
        );
        if is_fatal && image_base != 0 && ctx.Rip.wrapping_sub(image_base) < 0x2000_0000 {
            return unsafe { fatal_exception(info) };
        }
        EXCEPTION_CONTINUE_SEARCH
    }

    unsafe extern "system" fn unhandled_fault(info: *const EXCEPTION_POINTERS) -> i32 {
        unsafe { fatal_exception(info.cast_mut()) }
    }

    unsafe fn fatal_exception(info: *mut EXCEPTION_POINTERS) -> i32 {
        if FATAL_REPORTED.swap(true, Ordering::AcqRel) {
            unsafe { Sleep(u32::MAX) };
        }
        if !info.is_null() {
            // SAFETY: Windows passes valid `EXCEPTION_POINTERS`.
            let (record, ctx) = unsafe { (&*(*info).ExceptionRecord, &*(*info).ContextRecord) };
            let (image_base, _, gpu_enabled) = fault_context();
            let access = if record.ExceptionCode == EXCEPTION_ACCESS_VIOLATION
                && record.NumberParameters >= 2
            {
                let kind = match record.ExceptionInformation[0] {
                    1 => "write",
                    8 => "execute",
                    _ => "read",
                };
                Some((kind, record.ExceptionInformation[1] as u64))
            } else {
                None
            };
            crash::report_windows_fault_signal_safe(
                record.ExceptionCode as u32,
                ctx.Rip,
                image_base,
                access,
                None,
            );
            if gpu_enabled {
                let fn_ptr = GPU_DUMP_WRITES_FN.load(Ordering::Acquire);
                if fn_ptr != 0 {
                    let f: unsafe extern "C" fn(*mut c_void) =
                        unsafe { std::mem::transmute(fn_ptr) };
                    unsafe { f(info.cast()) };
                }
            }
        }
        let bench_ptr = BENCHMARK_CRASH_FN.load(Ordering::Acquire);
        if bench_ptr != 0 {
            let f: unsafe extern "C" fn() = unsafe { std::mem::transmute(bench_ptr) };
            unsafe { f() };
        }
        unsafe { TerminateProcess(GetCurrentProcess(), 3) };
        EXCEPTION_CONTINUE_SEARCH
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use std::sync::atomic::AtomicBool;

    #[test]
    fn configure_context_and_hooks_roundtrip() {
        configure_fault_context(0x4000_0000, 0x5555_0000, true);
        assert_eq!(fault_context(), (0x4000_0000, 0x5555_0000, true));

        extern "C" fn dummy_recover(_sig: i32, _ctx: *mut c_void) -> i32 {
            1
        }
        configure_fault_hooks(FaultHooks {
            fault_recover: Some(dummy_recover),
            ..Default::default()
        });
        assert_eq!(
            FAULT_RECOVER_FN.load(Ordering::Acquire),
            dummy_recover as *const () as usize
        );
        configure_fault_hooks(FaultHooks::default());
        configure_fault_context(0, 0, false);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn forked_sigsegv_recovery_and_fatal_crash_line() {
        // Run signal-handler tests in forked child processes so parallel tests (e.g. red_zone)
        // never race on the process-wide SIGSEGV handler.

        // 1. Speculative read recovery via `with_fault_recovery`.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork failed");
        if pid == 0 {
            unsafe { linux::install_handlers() };
            let page = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    4096,
                    libc::PROT_NONE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                )
            };
            if page == libc::MAP_FAILED {
                unsafe { libc::_exit(2) };
            }
            let faulted =
                with_fault_recovery(|| unsafe { std::ptr::read_volatile(page as *const u8) });
            if faulted.is_some() {
                unsafe { libc::_exit(3) };
            }
            let ok = with_fault_recovery(|| 42u32);
            unsafe {
                libc::munmap(page, 4096);
                libc::_exit(if ok == Some(42) { 0 } else { 4 });
            }
        }
        let mut status: libc::c_int = 0;
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 0);

        // 2. External `FaultHooks::fault_recover` interception.
        static RECOVER_CALLED: AtomicBool = AtomicBool::new(false);
        extern "C" fn hook_recover(sig: i32, ctx: *mut c_void) -> i32 {
            if sig == libc::SIGSEGV && !ctx.is_null() {
                RECOVER_CALLED.store(true, Ordering::SeqCst);
                let recover_buf = RUST_FAULT_RECOVER.get();
                if !recover_buf.is_null() {
                    RUST_FAULT_RECOVER.set(std::ptr::null_mut());
                    let uc = unsafe { &mut *(ctx as *mut libc::ucontext_t) };
                    uc.uc_mcontext.gregs[libc::REG_RDI as usize] =
                        recover_buf as usize as libc::greg_t;
                    uc.uc_mcontext.gregs[libc::REG_RSI as usize] = 1;
                    uc.uc_mcontext.gregs[libc::REG_RIP as usize] =
                        uncharted_fault_longjmp as *const () as usize as libc::greg_t;
                    return 1;
                }
            }
            0
        }
        let pid2 = unsafe { libc::fork() };
        assert!(pid2 >= 0);
        if pid2 == 0 {
            configure_fault_hooks(FaultHooks {
                fault_recover: Some(hook_recover),
                ..Default::default()
            });
            unsafe { linux::install_handlers() };
            let page = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    4096,
                    libc::PROT_NONE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                )
            };
            let res = with_fault_recovery(|| unsafe { std::ptr::read_volatile(page as *const u8) });
            let ok = res.is_none() && RECOVER_CALLED.load(Ordering::SeqCst);
            unsafe { libc::_exit(if ok { 0 } else { 5 }) };
        }
        let mut status2: libc::c_int = 0;
        assert_eq!(unsafe { libc::waitpid(pid2, &mut status2, 0) }, pid2);
        assert!(libc::WIFEXITED(status2));
        assert_eq!(libc::WEXITSTATUS(status2), 0);

        // 3. Fatal SIGSEGV emits exactly one `Crash: kind=fault ...` line on stderr and exits 128+11.
        let mut pipefd = [0 as libc::c_int; 2];
        assert_eq!(unsafe { libc::pipe(pipefd.as_mut_ptr()) }, 0);
        let pid3 = unsafe { libc::fork() };
        assert!(pid3 >= 0);
        if pid3 == 0 {
            unsafe {
                libc::close(pipefd[0]);
                libc::dup2(pipefd[1], 2);
                libc::close(pipefd[1]);
                configure_fault_context(0x1000_0000, 0, false);
                configure_fault_hooks(FaultHooks::default());
                linux::install_handlers();
                let null_ptr = 0x10usize as *const u8;
                let _ = std::ptr::read_volatile(null_ptr);
                libc::_exit(99);
            }
        }
        unsafe { libc::close(pipefd[1]) };
        let mut stderr_bytes = Vec::new();
        let mut chunk = [0u8; 512];
        loop {
            let n = unsafe { libc::read(pipefd[0], chunk.as_mut_ptr().cast(), chunk.len()) };
            if n <= 0 {
                break;
            }
            stderr_bytes.extend_from_slice(&chunk[..n as usize]);
        }
        unsafe { libc::close(pipefd[0]) };
        let mut status3: libc::c_int = 0;
        assert_eq!(unsafe { libc::waitpid(pid3, &mut status3, 0) }, pid3);
        assert!(libc::WIFEXITED(status3));
        assert_eq!(libc::WEXITSTATUS(status3), 128 + libc::SIGSEGV);

        let stderr = String::from_utf8_lossy(&stderr_bytes);
        let crash_lines: Vec<_> = stderr.lines().filter(|l| l.starts_with("Crash:")).collect();
        assert_eq!(crash_lines.len(), 1, "stderr was:\n{stderr}");
        assert!(
            crash_lines[0].starts_with("Crash: kind=fault where="),
            "unexpected crash line: {}",
            crash_lines[0]
        );
        assert!(uncharted_core::logs::crash_line(&stderr).is_some());
    }
}
