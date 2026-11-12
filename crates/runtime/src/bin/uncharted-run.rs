//! `uncharted-run`: the desktop runner (docs/code-map.md "Modules: crates/runtime").
//! Only `--probe` is built: it prints the hardware report for `crates/core::tune` (#171).
//! The game loader joins it in piece 3.16.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
