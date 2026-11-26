//! `uncharted-run`: the desktop runner (docs/code-map.md "Modules: crates/runtime").
//! Supports `--probe` (#171), `--force-fault` (#168), controller tools, `--list-missing`, and
//! Rust loader guest execution (`crates/runtime/src/loader/`, #457). The flow is
//! `loader::cli::run`. On Linux with `uncharted_loader` (`UNCHARTED_GPU_BUILD`, `build.rs`) this
//! binary links `libbbgpu.so` and installs its hooks below; the Windows build with the GPU is
//! linked by CMake from `crates/runtime/c/uncharted_run_main.c` instead (see `loader::cli`).

use std::process::ExitCode;
use uncharted_runtime::loader::entry;

#[no_mangle]
pub extern "C" fn runtime_quit(code: std::ffi::c_int) -> ! {
    entry::runtime_quit(code)
}

#[no_mangle]
pub extern "C" fn runtime_restart() -> ! {
    entry::runtime_restart()
}

// `runtime_benchmark_active` and `runtime_benchmark_frame`, which the GPU library calls, are the
// library's (`benchmark_run.rs`, 3.17e); `build.rs` keeps them with `--undefined`.

#[cfg(all(uncharted_loader, target_arch = "x86_64", target_os = "linux"))]
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
        benchmark_crash: Some(uncharted_runtime::benchmark_run::uncharted_benchmark_crash),
        fault_recover: Some(bbgpu_fault_recover),
    });
}

#[cfg(not(all(uncharted_loader, target_arch = "x86_64", target_os = "linux")))]
fn configure_gpu_and_fault_hooks() {}

fn main() -> ExitCode {
    configure_gpu_and_fault_hooks();
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    ExitCode::from(u8::try_from(uncharted_runtime::loader::cli::run(&args)).unwrap_or(1))
}
