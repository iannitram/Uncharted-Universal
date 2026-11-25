//! `uu-recomp`: builds the AOT library of one title from the owner's own dump, on a Mac.
//!
//! ```text
//! uu-recomp --title u1|u2|u3 [--game-dir <folder>] [--out <folder>] [--jobs <n>] [--allow-no-dynamic]
//! ```
//!
//! `--game-dir` is the dump's game folder (default `~/Games/shadps4 games/CUSA02320`); the
//! executable is the title's (`eboot.bin`, `big2-ps4_Shipping.elf`, `big3-ps4_Shipping.elf`).
//! `--out` defaults to `~/Games/Uncharted-Universal/recomp-out/<title>/` and is refused when
//! it is inside a git checkout or the dump: the library holds the game's code and is never
//! committed or distributed. The run is `prepare::load_image`, `uu-scan`, `uu-bbrecomp` and
//! `clang -dynamiclib`; `summary.json` in the output folder holds counts and timings only.
//! `--allow-no-dynamic` accepts an executable without dynamic data (a plain ELF, as the tests
//! use): its setjmp callers cannot be told apart, so a real title never needs it.
//!
//! Exit status: 0 done; 1 failed; 2 bad arguments.

#[cfg(all(target_vendor = "apple", target_arch = "aarch64"))]
mod apple {
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use uncharted_core::games::Game;
    use uncharted_core::link::recomp_inputs;
    use uncharted_core::prepare::load_image;
    use uncharted_recomp::guard::{check_output_dir, expand_home};
    use uncharted_recomp::pipeline::{run, Request};

    const DEFAULT_GAME_DIR: &str = "~/Games/shadps4 games/CUSA02320";
    const USAGE: &str = "usage: uu-recomp --title u1|u2|u3 [--game-dir <folder>] [--out <folder>] [--jobs <n>] [--allow-no-dynamic]";

    struct Args {
        game: Game,
        game_dir: String,
        out: Option<String>,
        jobs: usize,
        allow_no_dynamic: bool,
    }

    fn parse(args: &[String]) -> Result<Args, String> {
        let mut game = None;
        let mut game_dir = DEFAULT_GAME_DIR.to_string();
        let mut out = None;
        let mut jobs = std::thread::available_parallelism()
            .map_or(2, |n| n.get())
            .min(4);
        let mut allow_no_dynamic = false;
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let mut value = |name: &str| it.next().cloned().ok_or(format!("{name} needs a value"));
            match arg.as_str() {
                "--title" => {
                    let key = value("--title")?;
                    game = Some(
                        Game::from_key(&key)
                            .ok_or(format!("--title {key}: expected u1, u2 or u3"))?,
                    );
                }
                "--game-dir" => game_dir = value("--game-dir")?,
                "--out" => out = Some(value("--out")?),
                "--jobs" => {
                    jobs = value("--jobs")?
                        .parse::<usize>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or("--jobs needs a positive number")?
                }
                "--allow-no-dynamic" => allow_no_dynamic = true,
                other => return Err(format!("unknown argument {other}")),
            }
        }
        Ok(Args {
            game: game.ok_or("--title is required")?,
            game_dir,
            out,
            jobs,
            allow_no_dynamic,
        })
    }

    pub fn main() -> ExitCode {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.iter().any(|a| a == "--help" || a == "-h") {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        let args = match parse(&args) {
            Ok(args) => args,
            Err(message) => {
                eprintln!("uu-recomp: {message}\n{USAGE}");
                return ExitCode::from(2);
            }
        };
        match go(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("uu-recomp: {message}");
                ExitCode::from(1)
            }
        }
    }

    fn go(args: &Args) -> Result<(), String> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let game_dir = expand_home(&args.game_dir, home.as_deref());
        let key = args.game.key();
        let out_default = format!("~/Games/Uncharted-Universal/recomp-out/{key}");
        let out = expand_home(args.out.as_deref().unwrap_or(&out_default), home.as_deref());
        // The folder is checked before anything is read, let alone written.
        let out = check_output_dir(&out, &game_dir).map_err(|e| e.to_string())?;

        let executable = game_dir.join(args.game.executable());
        let bytes =
            std::fs::read(&executable).map_err(|e| format!("{}: {e}", executable.display()))?;
        let image = load_image(&bytes).map_err(|e| format!("{}: {e}", args.game.executable()))?;
        let inputs =
            recomp_inputs(&bytes).map_err(|e| format!("{}: {e}", args.game.executable()))?;
        if !inputs.has_dynamic {
            if !args.allow_no_dynamic {
                return Err(format!(
                    "{} has no dynamic data: import names and function pointers are unknown, so setjmp callers \
                     could not be left to the interpreter (--allow-no-dynamic for a synthetic ELF)",
                    args.game.executable()
                ));
            }
            eprintln!("uu-recomp: no dynamic data: setjmp callers are not known");
        }
        let summary = run(&Request {
            title: key,
            elf: &image.elf,
            inputs: &inputs,
            out: &out,
            jobs: args.jobs,
        })?;
        report(&summary, &out);
        Ok(())
    }

    fn report(s: &uncharted_recomp::pipeline::Summary, out: &Path) {
        println!(
            "{}: {} functions found ({} from unwind tables), {} generated, {} of {} instructions in C, \
             {} C files ({} bytes), library {} bytes, {:.1}s",
            s.title,
            s.functions_found,
            s.functions_unwound,
            s.functions_generated,
            s.instructions_in_c,
            s.instructions,
            s.c_files,
            s.generated_c_bytes,
            s.library_bytes,
            s.seconds_total
        );
        println!("wrote {}", out.display());
    }
}

#[cfg(all(target_vendor = "apple", target_arch = "aarch64"))]
fn main() -> std::process::ExitCode {
    apple::main()
}

#[cfg(not(all(target_vendor = "apple", target_arch = "aarch64")))]
fn main() -> std::process::ExitCode {
    eprintln!("uu-recomp: the AOT recompiler runs on Apple arm64 hosts only");
    std::process::ExitCode::from(3)
}
