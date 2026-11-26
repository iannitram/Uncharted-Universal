//! The `uncharted-run` command flow and its C entry points (roadmap 3.16 and 3.17c/d, #457).
//!
//! [`run`] is what the Rust binary (`src/bin/uncharted-run.rs`) calls. The Windows CLANG64 build
//! links `uncharted-run.exe` from `crates/runtime/c/uncharted_run_main.c` instead: CMake links the
//! C `main` against `libuncharted_runtime.a` and `libbbgpu.a` (`gpu/CMakeLists.txt`), and the C
//! `main` hands the GPU library's entry points to [`uncharted_run_main`] in an
//! [`UnchartedRunHooks`] table. The Rust side never names a `bbgpu_*` symbol, so the same library
//! still links into `bb-probe` (which has its own `runtime_quit`/`runtime_restart`) and into the
//! tests, where there is no GPU library.

use super::entry::{self, BbGpuConfig, GpuHooks};
use super::{parse_args, Command};
use std::ffi::{c_char, c_int, c_void};

/// Runs one parsed command and returns the process exit code.
pub fn run_command(cmd: Command) -> i32 {
    match cmd {
        Command::ForceFault => {
            let _ = crate::crash::report(
                crate::crash::CrashKind::Fault,
                "0x1234",
                "read access to 0x5678 (forced test fault)",
            );
            // A second report must be a no-op: the process prints one Crash: line.
            let _ = crate::crash::report(
                crate::crash::CrashKind::Assertion,
                "fake.rs:1",
                "second report",
            );
            1
        }
        Command::Probe => {
            let gpu_id = std::env::var_os("BB_GPU_ID").map(|v| v.to_string_lossy().into_owned());
            match crate::probe::run(gpu_id.as_deref()) {
                Ok(report) => {
                    println!("{report}");
                    0
                }
                Err(message) => {
                    eprintln!("uncharted-run: {message}");
                    1
                }
            }
        }
        Command::Boot(opts) => entry::run_boot(&opts),
        Command::ControllerInfo => crate::pad::runtime_pad_tool(0, 100),
        Command::ControllerTest(strength) => crate::pad::runtime_pad_tool(1, strength),
        Command::ControllerWatch(secs) => crate::pad::runtime_pad_watch(secs),
    }
}

/// The whole command line (without `argv[0]`): help, parse, run. Returns the exit code.
pub fn run(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "usage: uncharted-run [--probe | --force-fault | --controller-info | --controller-test [strength] | --controller-watch [secs] | <boot.bin> [options]]"
        );
        return 0;
    }
    match parse_args(args) {
        Ok(cmd) => run_command(cmd),
        Err(msg) => {
            eprintln!("uncharted-run: {msg}");
            2
        }
    }
}

/// Version of [`UnchartedRunHooks`]; the C `main` sets it, and a table of another version is
/// ignored (the runner then runs without a GPU library, which `--probe` and `--cpu-only` allow).
pub const HOOKS_VERSION: u32 = 1;

/// The GPU library's entry points, filled in by the C `main` (`gpu/bbgpu.h`). Every field may be
/// null. The layout is the one in `crates/runtime/c/uncharted_run_main.c`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UnchartedRunHooks {
    pub version: u32,
    pub bbgpu_register_kernel: Option<unsafe extern "C" fn()>,
    pub bbnet_configure:
        Option<unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, *const c_char)>,
    pub bbgpu_init: Option<unsafe extern "C" fn(*const BbGpuConfig) -> c_int>,
    pub bbgpu_prepare_exit: Option<unsafe extern "C" fn()>,
    pub bbgpu_symbol_count: Option<unsafe extern "C" fn() -> u32>,
    pub bbgpu_trophy_notify:
        Option<unsafe extern "C" fn(c_int, *const c_char, *const c_char, *const c_void, u64)>,
    /// `bbgpu_handle_fault(context, address)`: 1 when a GPU page-tracking fault was handled.
    pub gpu_handle_fault: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int>,
    /// `bbgpu_dump_guest_writes(context)`.
    pub gpu_dump_writes: Option<unsafe extern "C" fn(*mut c_void)>,
    /// `bbgpu_fault_recover(code, context)`: 1 when a speculative guest read's recovery point
    /// took over the faulting thread (`gpu/shim/common/assert.cpp`).
    pub fault_recover: Option<unsafe extern "C" fn(c_int, *mut c_void) -> c_int>,
}

/// Installs the GPU library's entry points: the guest startup and shutdown hooks, and the hooks
/// of the fault handlers.
pub fn install_hooks(hooks: &UnchartedRunHooks) {
    if hooks.version != HOOKS_VERSION {
        return;
    }
    entry::configure_gpu_hooks(GpuHooks {
        bbgpu_register_kernel: hooks.bbgpu_register_kernel,
        bbnet_configure: hooks.bbnet_configure,
        bbgpu_init: hooks.bbgpu_init,
        bbgpu_prepare_exit: hooks.bbgpu_prepare_exit,
        bbgpu_symbol_count: hooks.bbgpu_symbol_count,
        bbgpu_trophy_notify: hooks.bbgpu_trophy_notify,
    });
    #[cfg(all(
        target_arch = "x86_64",
        any(target_os = "linux", target_os = "windows")
    ))]
    super::fault::configure_fault_hooks(super::fault::FaultHooks {
        gpu_handle_fault: hooks.gpu_handle_fault,
        gpu_dump_writes: hooks.gpu_dump_writes,
        benchmark_crash: None,
        fault_recover: hooks.fault_recover,
    });
}

/// `main` of the C-linked `uncharted-run`: installs `hooks` (null: no GPU library), then runs the
/// process's command line. Returns the exit code.
///
/// # Safety
/// `hooks` is null or points to a valid [`UnchartedRunHooks`].
#[no_mangle]
pub unsafe extern "C" fn uncharted_run_main(hooks: *const UnchartedRunHooks) -> c_int {
    // SAFETY: the caller passes null or a valid table.
    if let Some(hooks) = unsafe { hooks.as_ref() } {
        install_hooks(hooks);
    }
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    run(&args)
}

/// The C `main`'s `runtime_quit(code)`: log flushed, shader cache saved, no destructors.
#[no_mangle]
pub extern "C" fn uncharted_run_quit(code: c_int) -> ! {
    entry::runtime_quit(code)
}

/// The C `main`'s `runtime_restart()` (the settings menu's new render resolution).
#[no_mangle]
pub extern "C" fn uncharted_run_restart() -> ! {
    entry::runtime_restart()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::parse_args;

    #[test]
    fn missing_boot_file_returns_failure_code() {
        let cmd = parse_args(&[
            "/nonexistent/uncharted-boot-missing.bin".into(),
            "--list-missing".into(),
        ])
        .unwrap();
        assert_eq!(run_command(cmd), 1);
    }

    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    #[test]
    fn cpu_only_boot_executes_guest_entry_and_reports_fault_crash_line() {
        fn build_boot(code: &[u8]) -> Vec<u8> {
            let mut raw = Vec::new();
            raw.extend_from_slice(b"BBPROBE2");
            raw.extend_from_slice(&4096u64.to_le_bytes()); // size
            raw.extend_from_slice(&0u64.to_le_bytes()); // entry
            raw.extend_from_slice(&1u64.to_le_bytes()); // n_segments
            raw.extend_from_slice(&0u64.to_le_bytes()); // n_relocs
            raw.extend_from_slice(&0u64.to_le_bytes()); // n_imports
            raw.extend_from_slice(&0u64.to_le_bytes()); // capabilities
            raw.extend_from_slice(&0u64.to_le_bytes()); // seg 0 address
            raw.extend_from_slice(&4096u64.to_le_bytes()); // seg 0 size
            raw.extend_from_slice(&5u64.to_le_bytes()); // seg 0 RX
            let mut payload = vec![0x90u8; 4096];
            payload[..code.len()].copy_from_slice(code);
            raw.extend_from_slice(&payload);
            raw
        }

        // Test 1: guest entry returns cleanly via `ret` -> loader reports "entry unexpectedly returned".
        let ret_path =
            std::env::temp_dir().join(format!("uncharted-boot-ret-{}.bin", std::process::id()));
        std::fs::write(&ret_path, build_boot(&[0xc3])).unwrap();
        let pid_ret = unsafe { libc::fork() };
        assert!(pid_ret >= 0);
        if pid_ret == 0 {
            std::env::set_var("BB_SKIP_CPU_CHECK", "1");
            let cmd = parse_args(&[
                ret_path.to_string_lossy().into_owned(),
                "--cpu-only".into(),
                "--timeout".into(),
                "5".into(),
            ])
            .unwrap();
            let code = i32::from(run_command(cmd) != 0);
            unsafe { libc::_exit(code) };
        }
        let mut status_ret: libc::c_int = 0;
        assert_eq!(
            unsafe { libc::waitpid(pid_ret, &mut status_ret, 0) },
            pid_ret
        );
        let _ = std::fs::remove_file(&ret_path);
        assert!(libc::WIFEXITED(status_ret));
        assert_eq!(libc::WEXITSTATUS(status_ret), 1);

        // Test 2: guest entry executes `xor eax, eax; mov [rax], eax` (fault at guest offset 0x2)
        // -> fault handler emits exactly one `Crash: kind=fault where=0x2 ...` line and exits 139.
        let fault_path =
            std::env::temp_dir().join(format!("uncharted-boot-fault-{}.bin", std::process::id()));
        std::fs::write(&fault_path, build_boot(&[0x31, 0xc0, 0x89, 0x00, 0xc3])).unwrap();

        let mut pipefd = [0 as libc::c_int; 2];
        assert_eq!(unsafe { libc::pipe(pipefd.as_mut_ptr()) }, 0);
        let pid_fault = unsafe { libc::fork() };
        assert!(pid_fault >= 0);
        if pid_fault == 0 {
            unsafe {
                libc::close(pipefd[0]);
                libc::dup2(pipefd[1], 2);
                libc::close(pipefd[1]);
            }
            std::env::set_var("BB_SKIP_CPU_CHECK", "1");
            let cmd = parse_args(&[
                fault_path.to_string_lossy().into_owned(),
                "--cpu-only".into(),
                "--timeout".into(),
                "5".into(),
            ])
            .unwrap();
            let _ = run_command(cmd);
            unsafe { libc::_exit(99) };
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
        let mut status_fault: libc::c_int = 0;
        assert_eq!(
            unsafe { libc::waitpid(pid_fault, &mut status_fault, 0) },
            pid_fault
        );
        let _ = std::fs::remove_file(&fault_path);
        assert!(libc::WIFEXITED(status_fault));
        assert_eq!(libc::WEXITSTATUS(status_fault), 128 + libc::SIGSEGV);

        let stderr = String::from_utf8_lossy(&stderr_bytes);
        let crash_lines: Vec<_> = stderr.lines().filter(|l| l.starts_with("Crash:")).collect();
        assert_eq!(crash_lines.len(), 1, "stderr was:\n{stderr}");
        assert!(
            crash_lines[0].contains("kind=fault where=0x2 text=signal 11 at guest offset 0x2"),
            "unexpected crash line: {}",
            crash_lines[0]
        );

        // Test 3: non-executable entry segment is rejected before jumping to guest code.
        let mut non_exec = build_boot(&[0xc3]);
        // Set segment 0 flags (at offset 8 + 6*8 + 2*8 = 72) to RW (6) instead of RX (5).
        non_exec[72..80].copy_from_slice(&6u64.to_le_bytes());
        let non_exec_path =
            std::env::temp_dir().join(format!("uncharted-boot-noexec-{}.bin", std::process::id()));
        std::fs::write(&non_exec_path, &non_exec).unwrap();
        let pid_noexec = unsafe { libc::fork() };
        assert!(pid_noexec >= 0);
        if pid_noexec == 0 {
            std::env::set_var("BB_SKIP_CPU_CHECK", "1");
            let cmd_noexec = parse_args(&[
                non_exec_path.to_string_lossy().into_owned(),
                "--cpu-only".into(),
            ])
            .unwrap();
            let code = i32::from(run_command(cmd_noexec) != 0);
            unsafe { libc::_exit(code) };
        }
        let mut status_noexec: libc::c_int = 0;
        assert_eq!(
            unsafe { libc::waitpid(pid_noexec, &mut status_noexec, 0) },
            pid_noexec
        );
        let _ = std::fs::remove_file(&non_exec_path);
        assert!(libc::WIFEXITED(status_noexec));
        assert_eq!(libc::WEXITSTATUS(status_noexec), 1);
    }
}
