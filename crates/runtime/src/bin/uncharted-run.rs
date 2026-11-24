//! `uncharted-run`: the desktop runner (docs/code-map.md "Modules: crates/runtime").
//! Supports `--probe` (#171), `--force-fault` (#168), controller tools, `--list-missing`, and
//! Rust loader guest execution (`crates/runtime/src/loader/`, #457).

use std::process::ExitCode;
use uncharted_runtime::loader::{self, entry, Command};

#[no_mangle]
pub extern "C" fn runtime_quit(code: std::ffi::c_int) -> ! {
    entry::runtime_quit(code)
}

#[no_mangle]
pub extern "C" fn runtime_restart() -> ! {
    entry::runtime_restart()
}

#[cfg(all(uncharted_gpu, target_arch = "x86_64", target_os = "linux"))]
fn configure_gpu_and_fault_hooks() {
    use std::ffi::{c_char, c_int, c_void};
    use uncharted_runtime::loader::{entry::BbGpuConfig, fault};

    extern "C" {
        fn bbgpu_register_kernel();
        fn bbnet_configure(
            app0: *const c_char,
            serial: *const c_char,
            title: *const c_char,
            app_ver: *const c_char,
        );
        fn bbgpu_init(config: *const BbGpuConfig) -> c_int;
        fn bbgpu_prepare_exit();
        fn bbgpu_symbol_count() -> u32;
        fn bbgpu_trophy_notify(
            grade: c_int,
            game: *const c_char,
            name: *const c_char,
            png: *const c_void,
            png_size: u64,
        );
        fn bbgpu_handle_fault(ucontext: *mut c_void, address: *mut c_void) -> i32;
        fn bbgpu_dump_guest_writes(ucontext: *mut c_void);
        fn bbgpu_fault_recover(sig_or_code: i32, context: *mut c_void) -> i32;
    }

    entry::configure_gpu_hooks(entry::GpuHooks {
        bbgpu_register_kernel: Some(bbgpu_register_kernel),
        bbnet_configure: Some(bbnet_configure),
        bbgpu_init: Some(bbgpu_init),
        bbgpu_prepare_exit: Some(bbgpu_prepare_exit),
        bbgpu_symbol_count: Some(bbgpu_symbol_count),
        bbgpu_trophy_notify: Some(bbgpu_trophy_notify),
    });
    fault::configure_fault_hooks(fault::FaultHooks {
        gpu_handle_fault: Some(bbgpu_handle_fault),
        gpu_dump_writes: Some(bbgpu_dump_guest_writes),
        benchmark_crash: None,
        fault_recover: Some(bbgpu_fault_recover),
    });
}

#[cfg(not(all(uncharted_gpu, target_arch = "x86_64", target_os = "linux")))]
fn configure_gpu_and_fault_hooks() {}

fn run_command(cmd: Command) -> ExitCode {
    configure_gpu_and_fault_hooks();
    match cmd {
        Command::ForceFault => {
            let _ = uncharted_runtime::crash::report(
                uncharted_runtime::crash::CrashKind::Fault,
                "0x1234",
                "read access to 0x5678 (forced test fault)",
            );
            // A second report must be a no-op: the process prints one Crash: line.
            let _ = uncharted_runtime::crash::report(
                uncharted_runtime::crash::CrashKind::Assertion,
                "fake.rs:1",
                "second report",
            );
            ExitCode::FAILURE
        }
        Command::Probe => {
            let gpu_id = std::env::var_os("BB_GPU_ID").map(|v| v.to_string_lossy().into_owned());
            match uncharted_runtime::probe::run(gpu_id.as_deref()) {
                Ok(report) => {
                    println!("{report}");
                    ExitCode::SUCCESS
                }
                Err(message) => {
                    eprintln!("uncharted-run: {message}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Boot(opts) => {
            let code = entry::run_boot(&opts);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Command::ControllerInfo => {
            let code = uncharted_runtime::pad::runtime_pad_tool(0, 100);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Command::ControllerTest(strength) => {
            let code = uncharted_runtime::pad::runtime_pad_tool(1, strength);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Command::ControllerWatch(secs) => {
            let code = uncharted_runtime::pad::runtime_pad_watch(secs);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "usage: uncharted-run [--probe | --force-fault | --controller-info | --controller-test [strength] | --controller-watch [secs] | <boot.bin> [options]]"
        );
        return ExitCode::SUCCESS;
    }
    let cmd = match loader::parse_args(&args) {
        Ok(cmd) => cmd,
        Err(msg) => {
            eprintln!("uncharted-run: {msg}");
            return ExitCode::from(2);
        }
    };
    run_command(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_boot_file_returns_failure_code() {
        let cmd = loader::parse_args(&[
            "/nonexistent/uncharted-boot-missing.bin".into(),
            "--list-missing".into(),
        ])
        .unwrap();
        assert_eq!(run_command(cmd), ExitCode::from(1));
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
            let cmd = loader::parse_args(&[
                ret_path.to_string_lossy().into_owned(),
                "--cpu-only".into(),
                "--timeout".into(),
                "5".into(),
            ])
            .unwrap();
            let code = match run_command(cmd) {
                ExitCode::SUCCESS => 0,
                _ => 1,
            };
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
            let cmd = loader::parse_args(&[
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
            let cmd_noexec = loader::parse_args(&[
                non_exec_path.to_string_lossy().into_owned(),
                "--cpu-only".into(),
            ])
            .unwrap();
            let code = match run_command(cmd_noexec) {
                ExitCode::SUCCESS => 0,
                _ => 1,
            };
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
