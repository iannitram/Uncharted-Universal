//! `uncharted-run`: the desktop runner (docs/code-map.md "Modules: crates/runtime").
//! Supports `--probe` (#171), `--force-fault` (#168), controller tools, `--list-missing`, and
//! Rust loader guest execution (`crates/runtime/src/loader/`, #457). The flow is
//! `loader::cli::run`; this binary has no GPU library (the Windows build with the GPU is linked
//! by CMake from `crates/runtime/c/uncharted_run_main.c`, see `loader::cli`).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    ExitCode::from(u8::try_from(uncharted_runtime::loader::cli::run(&args)).unwrap_or(1))
}
