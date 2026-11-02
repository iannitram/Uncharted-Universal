//! Play (SPEC.md M7, M10, M18): pull the save, start the game with the device profile and
//! `run_env`, wait for it to quit, push the save. save-relay does the syncing through `Relay`
//! (implemented by `relay.rs` once save-relay's library is linked; planned, not in the tree);
//! the game is started through `Runner`; both are traits so the flow is tested with fakes.
//! Replaces `main` of `run.py`; the shape is copied from iannitram/totk-native's `launch.rs` (MIT).
//!
//! `docs/code-map.md` calls the second trait `Game`; it is `Runner` here because `games::Game`
//! is the enum of the three games.

use crate::error::{not_built, Result};
use crate::games::Game;
use crate::savecard::SideBySide;
use std::path::Path;

/// `run.py`'s `LOADEXEC_EXIT`: the exit code of a run whose game asked to start another
/// executable (`runtime_services.c`); `play` then starts that game.
pub const LOADEXEC_EXIT: i32 = 101;

/// What one save-relay pull or push did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sync {
    /// Done (pulled, pushed, or nothing to do); the message is save-relay's.
    Done(String),
    /// The server is unreachable: play on (pull) or push next time (push).
    Offline,
    /// Both sides changed since the last sync; nothing was touched. The two cards are what the
    /// side-by-side prompt shows (boxed: a card holds a picture).
    Conflict(Box<SideBySide>),
}

/// The player's answer to a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    Server,
    Local,
}

/// save-relay, as the launcher uses it.
pub trait Relay {
    fn pull(&self, saves_dir: &Path, resolve: Option<Keep>) -> Result<Sync>;
    fn push(&self, saves_dir: &Path, resolve: Option<Keep>) -> Result<Sync>;
}

/// The runtime, started as a child process.
pub trait Runner {
    /// Start `uncharted-run` for `game` with `env` added and wait until it quits; Ok with its
    /// exit code.
    fn run(&self, game: Game, env: &[(String, String)]) -> Result<i32>;
}

/// What a play session did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Played {
    pub pulled: Sync,
    pub exit_code: i32,
    pub pushed: Sync,
}

/// The play flow: `relay.pull`; on `Conflict` ask `choose` and pull again with its answer;
/// `Offline` plays on. Then `runner.run(game, env)`, then `relay.push` (a conflict asks again).
/// A relay error before the game starts stops the flow (the game does not start); a push error
/// is returned after the game has quit.
pub fn play(
    relay: &dyn Relay,
    runner: &dyn Runner,
    game: Game,
    saves_dir: &Path,
    env: &[(String, String)],
    choose: &dyn Fn(&Sync) -> Keep,
) -> Result<Played> {
    not_built("launch::play")
}
