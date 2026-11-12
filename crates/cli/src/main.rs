//! `uncharted-universal`: the launcher without its window. The Steam shortcut in Game Mode runs
//! `uncharted-universal play`; the other commands do what the app's pages do (docs/code-map.md,
//! Faces). Replaces the command line of `run.py` and, since the release builds ship it (2.10),
//! the Windows `Play Uncharted.exe` (the deleted `packaging/windows/play.c`): `play` waits for
//! the game and returns its exit code, and passes on the arguments after `--`. In the AppImage
//! it runs as `<AppImage> play u1` (AppRun hands the commands to it). No argument-parsing crate:
//! the commands take few arguments and are parsed by hand (`args`).
//!
//! ```text
//! uncharted-universal check        [<dump folder>]
//! uncharted-universal prepare      <u1|u2|u3> [--dump <folder>]
//! uncharted-universal play         <u1|u2|u3> [--keep server|local] [-- <runtime arguments>]
//! uncharted-universal steam-add    [<u1|u2|u3>] [--userdata <folder>] [--dump <folder>]
//! uncharted-universal patches      [<dump folder>] [--patches-dir <folder>]
//! uncharted-universal hub          serve|pair|pull
//! uncharted-universal fleet        [<records file>] [--device <name>]
//! uncharted-universal generate-art [<output folder>]
//! uncharted-universal setup        [status|hub|client|steam]
//! ```
//!
//! Every command is a thin layer over `uncharted_core`; a core function that is still a stub
//! ends the command with "not built yet: <module>::<fn>" and exit status 3, so a script can tell
//! a missing piece from a failure.
//!
//! Exit status: 0 done; 1 failed (the message says why; `check` also returns 1 when the dump has
//! problems); 2 bad arguments; 3 a piece of the core is not built yet. `play` returns the game's
//! own exit status once the game has run.
//!
//! `setup` is the app's guided first run without the window (`setup.rs`, roadmap 2.05).

mod args;
mod commands;
mod setup;
#[cfg(test)]
mod fixtures;

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use uncharted_core::platform::Os;
use uncharted_core::Error;

/// The commands, with their arguments and the one line `--help` shows for each.
const COMMANDS: &[(&str, &str, &str)] = &[
    (
        "check",
        "[<dump folder>]",
        "check a dump folder: layout, decrypted, games and revisions",
    ),
    (
        "prepare",
        "<u1|u2|u3> [--dump <folder>]",
        "prepare a game from the dump into an install",
    ),
    (
        "play",
        "<u1|u2|u3> [--keep server|local] [-- <args>]",
        "pull the save, play, push the save",
    ),
    (
        "steam-add",
        "[<u1|u2|u3>] [--userdata <folder>] [--dump <folder>]",
        "add the launcher to Steam with artwork",
    ),
    (
        "patches",
        "[<dump folder>] [--patches-dir <folder>]",
        "list the third-party patches for this dump",
    ),
    (
        "hub",
        "serve|pair|pull",
        "serve, pair with or pull from a build hub",
    ),
    (
        "fleet",
        "[<records file>] [--device <name>]",
        "show the paired devices and their builds",
    ),
    (
        "generate-art",
        "[<output folder>]",
        "write the collection artwork to a folder",
    ),
    (
        "setup",
        "[status|hub|client|steam]",
        "first run: prepare from the dump, or pull from the hub; add to Steam",
    ),
];

/// Exit status: the command did what it was asked.
pub const OK: u8 = 0;
/// Exit status: the command failed; the message says why.
pub const FAILED: u8 = 1;
/// Exit status: the arguments were wrong (an unknown command, a missing game, a bad flag).
pub const USAGE: u8 = 2;
/// Exit status: a core function the command needs is still a stub.
pub const NOT_BUILT: u8 = 3;

fn usage() -> String {
    let mut text = String::from("usage: uncharted-universal <command> [arguments]\n\ncommands:\n");
    for (name, arguments, about) in COMMANDS {
        text.push_str(&format!(
            "  {name:<13} {about}\n  {:<13}   {name} {arguments}\n",
            ""
        ));
    }
    text.push_str(
        "\nA dump folder defaults to BB_GAME_DIR, a patch folder to BB_PATCHES_DIR.\n\
         Exit status: 0 done, 1 failed, 2 bad arguments, 3 not built yet;\n\
         play returns the game's own exit status.\n",
    );
    text
}

fn command_usage(name: &str) -> String {
    COMMANDS
        .iter()
        .find(|(command, _, _)| *command == name)
        .map(|(command, arguments, about)| {
            format!("usage: uncharted-universal {command} {arguments}\n\n{about}\n")
        })
        .unwrap_or_else(usage)
}

/// What running a command prints and returns.
#[derive(Debug, Default)]
pub struct Outcome {
    pub stdout: String,
    pub stderr: String,
    pub code: u8,
}

impl Outcome {
    /// Bad arguments for `command`: the problem, then that command's usage.
    pub fn usage(command: &str, problem: &str) -> Outcome {
        Outcome {
            stdout: String::new(),
            stderr: format!(
                "uncharted-universal {command}: {problem}\n\n{}",
                command_usage(command)
            ),
            code: USAGE,
        }
    }

    /// A core error, mapped to its exit status: `NotBuilt` is 3, anything else 1.
    pub fn error(command: &str, error: &Error) -> Outcome {
        let mut out = Outcome::default();
        out.fail(command, error);
        out
    }

    /// Ends this outcome with a core error (keeps what was printed so far).
    pub fn fail(&mut self, command: &str, error: &Error) {
        self.stderr
            .push_str(&format!("uncharted-universal {command}: {error}\n"));
        self.code = match error {
            Error::NotBuilt(_) => NOT_BUILT,
            _ => FAILED,
        };
    }

    pub fn line(&mut self, text: impl AsRef<str>) {
        self.stdout.push_str(text.as_ref());
        self.stdout.push('\n');
    }

    pub fn note(&mut self, text: impl AsRef<str>) {
        self.stderr.push_str(text.as_ref());
        self.stderr.push('\n');
    }
}

/// What a command may know about the machine it runs on; tests build their own.
pub struct Ctx {
    /// The environment (tests pass only what they set).
    pub env: BTreeMap<String, String>,
    pub os: Os,
    /// This program's own path: the Steam shortcut's target, the folder `uncharted-run` is
    /// looked up in.
    pub exe: PathBuf,
    /// Whether a person can answer a question on stdin (a sync conflict).
    pub interactive: bool,
    /// Whether Steam is running (`steam-add` writes only while it is closed).
    pub steam_running: Box<dyn Fn() -> bool>,
}

impl Ctx {
    /// A variable, with an empty value counted as unset.
    pub fn var(&self, name: &str) -> Option<String> {
        self.env
            .get(name)
            .filter(|value| !value.is_empty())
            .cloned()
    }

    /// The real machine.
    fn from_system() -> Ctx {
        Ctx {
            env: std::env::vars_os()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.to_string_lossy().into_owned(),
                    )
                })
                .collect(),
            os: Os::current(),
            exe: std::env::current_exe().unwrap_or_else(|_| "uncharted-universal".into()),
            interactive: std::io::stdin().is_terminal(),
            steam_running: Box::new(commands::steam_running),
        }
    }
}

/// The text of a "not built yet" panic (`not_built!` in core: `unimplemented!`), if it is one.
fn not_built_panic(payload: &(dyn std::any::Any + Send)) -> Option<String> {
    let text = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())?;
    let at = text.find("not built yet: ")?;
    Some(text[at + "not built yet: ".len()..].to_owned())
}

/// Runs `args` (without the program name). A core stub that panics instead of returning
/// `NotBuilt` (one whose signature cannot fail) is caught here and reported the same way.
fn run(args: &[String], ctx: &Ctx) -> Outcome {
    let Some(command) = args.first().map(String::as_str) else {
        return Outcome {
            stdout: usage(),
            ..Outcome::default()
        };
    };
    let rest = &args[1..];
    if matches!(command, "-h" | "--help" | "help") {
        return Outcome {
            stdout: rest.first().map_or_else(usage, |name| command_usage(name)),
            ..Outcome::default()
        };
    }
    if !COMMANDS.iter().any(|(name, _, _)| *name == command) {
        return Outcome {
            stdout: String::new(),
            stderr: format!(
                "uncharted-universal: unknown command '{command}'\n\n{}",
                usage()
            ),
            code: USAGE,
        };
    }
    if rest.iter().any(|a| a == "-h" || a == "--help") && !rest.iter().any(|a| a == "--") {
        return Outcome {
            stdout: command_usage(command),
            ..Outcome::default()
        };
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        commands::dispatch(command, rest, ctx)
    }));
    match result {
        Ok(outcome) => outcome,
        Err(payload) => match not_built_panic(payload.as_ref()) {
            Some(what) => Outcome {
                stdout: String::new(),
                stderr: format!("uncharted-universal {command}: not built yet: {what}\n"),
                code: NOT_BUILT,
            },
            None => std::panic::resume_unwind(payload),
        },
    }
}

fn main() -> ExitCode {
    // First, before any thread: the host's library injection (Steam's LD_PRELOAD overlay) out
    // of this process, so the runtime and anything else it starts never load it (Linux; this
    // replaces packaging/bbport-entry.c).
    uncharted_core::host_env::clean_process();
    // A stub's "not built yet" panic is reported as a plain line by `run`; keep its backtrace
    // note off the screen. Any other panic is a bug and prints as usual.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if not_built_panic(info.payload()).is_none() {
            default_hook(info);
        }
    }));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = run(&args, &Ctx::from_system());
    print!("{}", outcome.stdout);
    eprint!("{}", outcome.stderr);
    ExitCode::from(outcome.code)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// A machine with nothing set: no variables, Steam closed, nobody at the keyboard.
    pub fn ctx() -> Ctx {
        Ctx {
            env: BTreeMap::new(),
            os: Os::current(),
            exe: PathBuf::from("uncharted-universal"),
            interactive: false,
            steam_running: Box::new(|| false),
        }
    }

    #[test]
    fn help_lists_every_command_and_exits_zero() {
        for flag in [&[][..], &["--help"], &["-h"], &["help"]] {
            let out = run(&args(flag), &ctx());
            assert_eq!(out.code, OK);
            for (name, arguments, _) in COMMANDS {
                assert!(out.stdout.contains(name), "{name} missing from help");
                assert!(out.stdout.contains(arguments), "{arguments} missing");
            }
            assert!(out.stdout.contains("3 not built yet"));
        }
    }

    #[test]
    fn each_command_has_its_own_help() {
        for (name, arguments, _) in COMMANDS {
            for list in [vec!["help", name], vec![name, "--help"]] {
                let out = run(&args(&list), &ctx());
                assert_eq!(out.code, OK, "{list:?}");
                assert!(out
                    .stdout
                    .starts_with(&format!("usage: uncharted-universal {name} {arguments}")));
            }
        }
    }

    #[test]
    fn unknown_command_exits_two() {
        let out = run(&args(&["frobnicate"]), &ctx());
        assert_eq!(out.code, USAGE);
        assert!(out.stderr.contains("unknown command 'frobnicate'"));
    }

    #[test]
    fn core_errors_map_to_their_exit_status() {
        let built = Outcome::error("prepare", &Error::NotBuilt("prepare::prepare"));
        assert_eq!(built.code, NOT_BUILT);
        assert_eq!(
            built.stderr,
            "uncharted-universal prepare: not built yet: prepare::prepare\n"
        );
        let failed = Outcome::error("check", &Error::BadDump("no sfo".into()));
        assert_eq!(failed.code, FAILED);
        assert!(failed.stderr.contains("This dump is not usable: no sfo"));
    }

    #[test]
    fn a_stub_panic_is_reported_as_not_built() {
        let payload: Box<dyn std::any::Any + Send> =
            Box::new("not implemented: not built yet: steam::crc32".to_owned());
        assert_eq!(
            not_built_panic(payload.as_ref()).as_deref(),
            Some("steam::crc32")
        );
        let other: Box<dyn std::any::Any + Send> = Box::new("index out of bounds");
        assert_eq!(not_built_panic(other.as_ref()), None);
    }
}
