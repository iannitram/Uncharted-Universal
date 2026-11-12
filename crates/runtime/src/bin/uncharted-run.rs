//! `uncharted-run`: the desktop runner (docs/code-map.md "Modules: crates/runtime").
//! Only `--probe` is built: it prints the hardware report for `crates/core::tune` (#171).
//! The game loader joins it in piece 3.16.
//!
//! `--force-fault` is a test hook, not a loader: it reports one made-up fault through
//! `crash::report` (the path the loader's fault and assertion handlers will call) and exits 1.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--force-fault"] {
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
        return ExitCode::FAILURE;
    }
    if args != ["--probe"] {
        eprintln!("usage: uncharted-run --probe (the game loader is not built yet)");
        return ExitCode::from(2);
    }
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
