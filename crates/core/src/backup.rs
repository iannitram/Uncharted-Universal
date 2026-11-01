//! Save backups before an update (SPEC.md M19). A backup is a plain folder `<backups>/<stamp>/`
//! holding the synced files of a save root at their relative paths, so a person can restore one by
//! hand; the newest 10 are kept. New module; the shape is copied from iannitram/totk-native's
//! `backup.rs` (MIT).

use crate::error::{not_built, Result};
use crate::games::Game;
use std::path::{Path, PathBuf};

/// How many backups `update` keeps.
pub const KEEP: usize = 10;

/// Copy every synced file (`saves::is_synced`) under `save_root` into `<backups>/<stamp>/`
/// and return that folder. An existing `<backups>/<stamp>` is `Error::Saves("backup <stamp> exists")`
/// and nothing is written; a missing `save_root` is `Error::Saves("no save folder at <save_root>")`.
pub fn backup_saves(game: Game, save_root: &Path, backups: &Path, stamp: &str) -> Result<PathBuf> {
    not_built("backup::backup_saves")
}

/// The backups in `backups`, by folder name, oldest first (names are stamps, which sort by
/// time). A missing `backups` folder is an empty list.
pub fn list(backups: &Path) -> Result<Vec<String>> {
    not_built("backup::list")
}

/// Delete the oldest backups so that `keep` remain; return the deleted names, oldest first.
pub fn prune(backups: &Path, keep: usize) -> Result<Vec<String>> {
    not_built("backup::prune")
}
