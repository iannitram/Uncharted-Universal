//! Where each game keeps its save on each device, which files travel, and the manifest save-relay
//! reads (SPEC.md M4, M18). The same savedata layout on every device, so a save moves unchanged.
//! Replaces the per-game save folders of `run.py` (`SAVE_FOLDERS`, `split_saves`); the manifest is
//! issue #163 reworked. Path strings inside the save tree use `/`.

use crate::error::{not_built, Result};
use crate::games::Game;
use crate::platform::{Os, Vars};
use std::path::{Path, PathBuf};

/// The prefix of a game's savedata entries (`SAVE_FOLDERS` in `run.py`): `SAVEDATA01` is
/// Uncharted 1, `SAVEDATA02` Uncharted 2, `SAVEDATA03` Uncharted 3.
pub fn save_prefix(game: Game) -> &'static str {
    match game {
        Game::U1 => "SAVEDATA01",
        Game::U2 => "SAVEDATA02",
        Game::U3 => "SAVEDATA03",
    }
}

/// Move entries of the shared `savedata/<user>/<title>/` into per-game folders
/// (`<user_dir>/Uncharted 1/` ..), once; entries already there, and `*.memory` folders, stay.
/// Returns the moved entry names.
pub fn split_saves(user_dir: &Path) -> Result<Vec<String>> {
    not_built("saves::split_saves")
}

/// The folder a game's save lives in on a desktop OS, under the install folder or the OS data
/// folder. Android and iOS keep it inside the app, where save-relay runs:
/// `Error::Unsupported("saves sync inside the app on <os key>")` from here.
pub fn save_root(game: Game, os: Os, vars: &Vars, install: &Path) -> Result<PathBuf> {
    not_built("saves::save_root")
}

/// Whether a path relative to a game's save root (with `/`) travels: its savedata entries and
/// settings, nothing else (never logs, caches or a device id).
pub fn is_synced(game: Game, rel_path: &str) -> bool {
    not_built!("saves::is_synced")
}

/// The save-relay manifest for the collection, on one line, with `saves_from` "launcher" so
/// save-relay refuses to run for these games without `--saves-dir` and can never sync the
/// wrong folder (the `totk` manifest of iannitram/totk-native is the model). The game id and
/// the include patterns are fixed by the save-relay adapter issue (#163 reworked, piece 9.01).
pub fn relay_manifest() -> String {
    not_built!("saves::relay_manifest")
}
