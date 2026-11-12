//! `uncharted-universal`: the launcher without its window. The Steam shortcut in Game Mode runs
//! `uncharted-universal play`; the other commands do what the app's pages do (docs/code-map.md,
//! Faces). Replaces the command line of `run.py`. No argument-parsing crate: the commands take
//! few arguments and are parsed by hand.
//!
//! ```text
//! uncharted-universal check      [<dump folder>]
//! uncharted-universal prepare    <u1|u2|u3> [--dump <folder>]
//! uncharted-universal play       <u1|u2|u3>
//! uncharted-universal steam-add
//! uncharted-universal patches
//! uncharted-universal hub        serve|pair|pull
//! uncharted-universal fleet
//! uncharted-universal generate-art [<output folder>]
//! uncharted-universal setup      [status|hub|client|steam]
//! ```
//!
//! Most commands are still skeletons; `generate-art` writes the launcher collection art and
//! `setup` is the app's guided first run without the window (`setup.rs`).

mod setup;

use std::process::ExitCode;

/// The commands, with the one line `--help` shows for each.
const COMMANDS: &[(&str, &str)] = &[
    (
        "check",
        "check a dump folder: layout, decrypted, games and revisions",
    ),
    ("prepare", "prepare a game from the dump into an install"),
    ("play", "pull the save, play, push the save"),
    ("steam-add", "add the launcher to Steam with artwork"),
    ("patches", "list the third-party patches for this dump"),
    ("hub", "serve, pair with or pull from a build hub"),
    ("fleet", "show the paired devices and their builds"),
    ("generate-art", "write the collection artwork to a folder"),
    (
        "setup",
        "first run: prepare from the dump, or pull from the hub; add to Steam",
    ),
];

/// Exit status of a command that is not built yet.
const NOT_BUILT: u8 = 2;

fn usage() -> String {
    let mut text = String::from("usage: uncharted-universal <command> [arguments]\n\ncommands:\n");
    for (name, about) in COMMANDS {
        text.push_str(&format!("  {name:<10} {about}\n"));
    }
    text.push_str("\nCommands still in progress report that they are not built yet.\n");
    text
}

/// What running with `args` (without the program name) prints and returns.
pub(crate) struct Outcome {
    stdout: String,
    stderr: String,
    code: u8,
}

fn run(args: &[String]) -> Outcome {
    let first = args.first().map(String::as_str);
    match first {
        None | Some("-h" | "--help" | "help") => Outcome {
            stdout: usage(),
            stderr: String::new(),
            code: 0,
        },
        Some(command) if COMMANDS.iter().any(|(name, _)| *name == command) => Outcome {
            stdout: String::new(),
            stderr: format!("uncharted-universal {command}: not built yet\n"),
            code: NOT_BUILT,
        },
        Some(other) => Outcome {
            stdout: String::new(),
            stderr: format!(
                "uncharted-universal: unknown command '{other}'\n\n{}",
                usage()
            ),
            code: NOT_BUILT,
        },
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "generate-art") {
        let out = args
            .get(1)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "launcher/covers".into());
        return match uncharted_core::artwork::generate_collection_art(&out) {
            Ok(()) => {
                println!("wrote launcher artwork to {}", out.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("could not write launcher artwork: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let outcome = if args.first().is_some_and(|s| s == "setup") {
        setup::main(&args[1..])
    } else {
        run(&args)
    };
    print!("{}", outcome.stdout);
    eprint!("{}", outcome.stderr);
    ExitCode::from(outcome.code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn help_lists_every_command_and_exits_zero() {
        for flag in [&[][..], &["--help"], &["-h"], &["help"]] {
            let out = run(&args(flag));
            assert_eq!(out.code, 0);
            for (name, _) in COMMANDS {
                assert!(out.stdout.contains(name), "{name} missing from help");
            }
        }
    }

    #[test]
    fn every_command_says_not_built_and_exits_two() {
        for (name, _) in COMMANDS {
            if *name == "generate-art" || *name == "setup" {
                continue;
            }
            let out = run(&args(&[name, "u1"]));
            assert_eq!(out.code, 2);
            assert!(out.stderr.contains("not built yet"));
        }
    }

    #[test]
    fn unknown_command_exits_two() {
        let out = run(&args(&["frobnicate"]));
        assert_eq!(out.code, 2);
        assert!(out.stderr.contains("unknown command"));
    }
}
