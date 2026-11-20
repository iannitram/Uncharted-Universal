//! `uncharted-run`: the desktop runner (docs/code-map.md "Modules: crates/runtime").
//! Supports `--probe` (#171), `--force-fault` (#168), and `loader` (`BBPROBE*` validation and
//! `--list-missing` under #457); full guest execution remains in `src/probe.c` until 3.17.

use std::process::ExitCode;
use uncharted_runtime::loader::{self, BootImage, Command};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match loader::parse_args(&args) {
        Ok(cmd) => cmd,
        Err(msg) => {
            eprintln!("uncharted-run: {msg}");
            return ExitCode::from(2);
        }
    };

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
            let bytes = match std::fs::read(&opts.boot_file) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("uncharted-run: cannot open {}: {e}", opts.boot_file);
                    return ExitCode::FAILURE;
                }
            };
            let image = match BootImage::parse(&bytes, 4096) {
                Ok(img) => img,
                Err(e) => {
                    eprintln!("uncharted-run: {e}");
                    return ExitCode::FAILURE;
                }
            };
            if opts.list_missing {
                let missing = image.missing_imports(opts.strict_imports);
                for m in &missing {
                    let sym = m.symbol.unwrap_or("?");
                    let suffix = if m.is_data { " (data)" } else { "" };
                    println!("missing {} {sym}{suffix}", m.nid);
                }
                println!(
                    "{} of {} imports missing",
                    missing.len(),
                    image.import_names.len()
                );
                return if missing.is_empty() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(20)
                };
            }
            eprintln!("uncharted-run: boot image validated; full guest execution remains in src/probe.c until 3.17");
            ExitCode::from(3)
        }
        Command::ControllerInfo | Command::ControllerTest(_) | Command::ControllerWatch(_) => {
            eprintln!("uncharted-run: controller tool modes require the full loader (3.17)");
            ExitCode::from(3)
        }
    }
}
