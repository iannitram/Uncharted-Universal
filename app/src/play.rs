//! Starting a game from the app, and the links it opens. The game runs through the command
//! line's `uncharted-universal play <game>` (piece 2.02: save-relay pull, the runtime, push);
//! its output goes to `<logs>/last_run.log`, where the after-run step finds the `Crash:` line.

use crate::vm::after_crash::mark_started;
use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use uncharted_core::games::Game;
use uncharted_core::logs::{self, KEEP, RUN_LOG};

/// The play command's arguments `args`, then `--` and `run_args` (the runtime's own arguments,
/// such as `--force-fault`) when there are any.
pub fn with_run_args(mut args: Vec<String>, run_args: &[String]) -> Vec<String> {
    if !run_args.is_empty() {
        args.push("--".into());
        args.extend(run_args.iter().cloned());
    }
    args
}

/// Start `game`: rotate the run logs (`last_run.log` to `last_run.1.log` ..), start `program`
/// with `args` (`play <game> ...`), the player's `BB_*` settings in its environment, stdin
/// closed and stdout and stderr in a new `last_run.log` (no console window on Windows), then
/// note the run in `last_run.json`.
/// A start that fails notes nothing, so the next launch does not take it for a run.
pub fn start(
    program: &Path,
    game: Game,
    args: &[String],
    env: &BTreeMap<String, String>,
    logs_dir: &Path,
) -> io::Result<Child> {
    std::fs::create_dir_all(logs_dir)?;
    if let Err(error) = logs::rotate(logs_dir, "last_run", KEEP) {
        eprintln!("could not rotate the run logs: {error}");
    }
    let log = File::create(logs_dir.join(RUN_LOG))?;
    let mut command = Command::new(program);
    command
        .args(args)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    crate::no_console_window(&mut command);
    let child = command.spawn()?;
    if let Err(error) = mark_started(logs_dir, game) {
        eprintln!("could not note the run in the logs folder: {error}");
    }
    Ok(child)
}

/// Open `url` in the browser without waiting for it.
pub fn open_url(url: &str) -> io::Result<()> {
    let mut command = if cfg!(windows) {
        // `cmd /c start` would read the `&` in a link as a second command.
        let mut command = Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(url);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_arguments_follow_a_double_dash() {
        let play = |key: &str| vec!["play".to_string(), key.to_string()];
        assert_eq!(with_run_args(play("u2"), &[]), ["play", "u2"]);
        assert_eq!(
            with_run_args(play("u1"), &["--force-fault".into()]),
            ["play", "u1", "--", "--force-fault"]
        );
    }
}
