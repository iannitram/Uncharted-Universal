//! `uncharted-recomp`: the build step. Exit 0 done, 2 bad arguments, 3 not built yet.
//!
//! ```text
//! uncharted-recomp synthetic --work-dir <abs folder>      authored sample, full path to a library
//! uncharted-recomp build <u1|u2|u3> [--game-dir <folder>] [--config <file>] --work-dir <abs folder>
//! ```
//! `build` validates its arguments and the work folder, then stops: the ELF reader, function
//! discovery and the full decoder are not built yet (piece 8.03 is open). It never reads the dump.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use uncharted_recomp::config::GuestConfig;
use uncharted_recomp::lift::{generate, Function};
use uncharted_recomp::workdir;

fn flag(args: &[String], name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).cloned()
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(args: &[String]) -> Result<u8, String> {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let default_work = std::env::var_os("HOME")
        .map(|h| Path::new(&h).join("Games/Uncharted-Universal/recomp-out"));
    let work = flag(args, "--work-dir").map(PathBuf::from).or(default_work);
    let work = work.ok_or("no --work-dir and no HOME")?;
    match cmd {
        "synthetic" => {
            let dir =
                workdir::resolve(&work, &repo_root(), "synthetic").map_err(|e| format!("{e:?}"))?;
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let code = [0xB8, 40, 0, 0, 0, 0x05, 2, 0, 0, 0, 0xC3];
            let g = generate(&[Function {
                addr: 0x1000,
                code: &code,
            }]);
            std::fs::write(dir.join("recomp.c"), &g.c_source).map_err(|e| e.to_string())?;
            println!("{}", g.coverage.summary());
            Ok(0)
        }
        "build" => {
            let title = args.get(1).map(String::as_str).unwrap_or("");
            if !["u1", "u2", "u3"].contains(&title) {
                return Err("usage: build <u1|u2|u3> ...".into());
            }
            let cfg = flag(args, "--config")
                .ok_or("--config <file> is required (guest ranges are configuration)")?;
            let text = std::fs::read_to_string(&cfg).map_err(|e| format!("{cfg}: {e}"))?;
            GuestConfig::parse(&text).map_err(|e| e.to_string())?;
            workdir::resolve(&work, &repo_root(), title).map_err(|e| format!("{e:?}"))?;
            eprintln!("not built yet: recomp::elf and function discovery (8.03 is open)");
            Ok(3)
        }
        _ => Err("usage: uncharted-recomp synthetic|build ...".into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => ExitCode::from(code),
        Err(m) => {
            eprintln!("uncharted-recomp: {m}");
            ExitCode::from(2)
        }
    }
}
