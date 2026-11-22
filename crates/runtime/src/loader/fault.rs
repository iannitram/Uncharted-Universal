//! Signal and VEH fault handlers, `MOVBE` emulation, and watchdog thread-dump helpers for
//! [`super`] (porting `fault`, `vectored_fault`, `unhandled_fault`, `emulate_movbe`, `thread_dump`,
//! and `watchdog` from `src/probe.c` into Rust for roadmap 3.17).

use crate::crash;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

static IMAGE_BASE: AtomicU64 = AtomicU64::new(0);
static EXE_BASE: AtomicU64 = AtomicU64::new(0);
static GPU_ENABLED: AtomicBool = AtomicBool::new(false);
static GPU_HANDLE_FAULT_FN: AtomicUsize = AtomicUsize::new(0);
static GPU_DUMP_WRITES_FN: AtomicUsize = AtomicUsize::new(0);
static BENCHMARK_CRASH_FN: AtomicUsize = AtomicUsize::new(0);

/// Optional C/GPU hooks invoked by the fault handler when linked into a GPU-enabled runner.
#[derive(Debug, Clone, Copy, Default)]
pub struct FaultHooks {
    pub gpu_handle_fault:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> i32>,
    pub gpu_dump_writes: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    pub benchmark_crash: Option<unsafe extern "C" fn()>,
}

/// Records the loaded guest image base, host executable base, and GPU fault-tracking state for the
/// fault/watchdog handlers.
pub fn configure_fault_context(image_base: u64, exe_base: u64, gpu_enabled: bool) {
    IMAGE_BASE.store(image_base, Ordering::Release);
    EXE_BASE.store(exe_base, Ordering::Release);
    GPU_ENABLED.store(gpu_enabled, Ordering::Release);
}

/// Registers optional GPU/benchmark callbacks for the fault handler.
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
}

/// Returns the configured `(image_base, exe_base, gpu_enabled)` tuple.
pub fn fault_context() -> (u64, u64, bool) {
    (
        IMAGE_BASE.load(Ordering::Acquire),
        EXE_BASE.load(Ordering::Acquire),
        GPU_ENABLED.load(Ordering::Acquire),
    )
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
        context: *mut libc::c_void,
    ) {
        if context.is_null() {
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
                let f: unsafe extern "C" fn(*mut libc::c_void, *mut libc::c_void) -> i32 =
                    unsafe { std::mem::transmute(fn_ptr) };
                if unsafe { f(context, fault_addr) } != 0 {
                    return;
                }
            }
        }

        let rip = *rip_ref as u64;
        let mut dl: libc::Dl_info = unsafe { std::mem::zeroed() };
        let host_where = unsafe {
            if libc::dladdr(rip as usize as *const libc::c_void, &mut dl) != 0
                && !dl.dli_fname.is_null()
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

        let line = crash::format_linux_fault(
            sig,
            rip,
            image_base,
            fault_addr as usize as u64,
            host_where
                .as_ref()
                .map(|(f, off, s)| (f.as_str(), *off, s.as_deref())),
        );
        unsafe {
            libc::write(2, line.as_ptr() as *const libc::c_void, line.len());
            if gpu_enabled {
                let fn_ptr = GPU_DUMP_WRITES_FN.load(Ordering::Acquire);
                if fn_ptr != 0 {
                    let f: unsafe extern "C" fn(*mut libc::c_void) = std::mem::transmute(fn_ptr);
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
        context: *mut libc::c_void,
    ) {
        if !context.is_null() {
            // SAFETY: `context` is a kernel-provided `ucontext_t *`.
            unsafe { dump_frames(&*(context as *const libc::ucontext_t)) };
        }
    }

    extern "C" fn watchdog_handler(
        sig: libc::c_int,
        _info: *mut libc::siginfo_t,
        context: *mut libc::c_void,
    ) {
        const HEAD: &[u8] = b"STOP: watchdog timeout; thread stacks:\n";
        unsafe {
            libc::write(2, HEAD.as_ptr() as *const libc::c_void, HEAD.len());
            if !context.is_null() {
                dump_frames(&*(context as *const libc::ucontext_t));
            }
            libc::usleep(100_000);
            libc::_exit(128 + sig);
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
            libc::write(2, arg_line.as_ptr() as *const libc::c_void, arg_line.len());
        }

        let mut line = *b"  tid=0000000000000000 rip=0000000000000000 image-relative=0000000000000000 host-relative=0000000000000000\n";
        for _ in 0..24 {
            write_hex(&mut line[6..22], tid);
            write_hex(&mut line[27..43], rip);
            write_hex(&mut line[59..75], rip.wrapping_sub(image_base));
            write_hex(&mut line[90..106], rip.wrapping_sub(exe_base));
            unsafe {
                libc::write(2, line.as_ptr() as *const libc::c_void, line.len());
            }
            if rbp == 0 {
                break;
            }
            let mut frame = [0u64; 2];
            let local = libc::iovec {
                iov_base: frame.as_mut_ptr() as *mut libc::c_void,
                iov_len: std::mem::size_of::<[u64; 2]>(),
            };
            let remote = libc::iovec {
                iov_base: rbp as usize as *mut libc::c_void,
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

    static MOVBE_EMULATED: AtomicI64 = AtomicI64::new(0);

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

    // Keep the CPU patcher symbols linked for Windows callers.
    #[allow(dead_code)]
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
}
