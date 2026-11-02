//! Mod folders: which layouts are accepted, the order, and the overlay list the runtime's file
//! VFS reads. Mods replace or add files under `dvdroot_ps4`; executable and system files
//! (`eboot.bin`, `sce_module`, `sce_sys`) and symlinks are refused. Ported, not extended
//! (SPEC.md Won'ts). Replaces `scripts/mods.py`; `tests/test_mods.py` are the expected results.
//!
//! The accepted layouts are `<mod>/dvdroot_ps4`, `<mod>/app0/dvdroot_ps4` and a mod made of the
//! game's own top-level folders. `mods.py` also names a `CUSA03173` wrapper and a folder list
//! (`GAME_FOLDERS`) that are Bloodborne's: piece 1.11 decides which to keep for Uncharted.

use crate::error::{not_built, Result};
use std::path::{Path, PathBuf};

/// The player's mod choice (`mods.json`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModConfig {
    /// Folder names switched off.
    pub disabled: Vec<String>,
    /// Folder names in load order; new folders go last, alphabetically.
    pub order: Vec<String>,
}

/// One file a mod replaces or adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModFile {
    /// The path in the game, `dvdroot_ps4/...` with `/`.
    pub game_path: String,
    pub source: PathBuf,
}

/// The overlay the runtime mounts over the game's files.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Overlay {
    /// Last mod wins; sorted by `game_path`.
    pub files: Vec<ModFile>,
}

/// Names of the folders of `root` that have an accepted layout, sorted case-insensitively.
/// A missing `root` is an empty list.
pub fn discover(root: &Path) -> Result<Vec<String>> {
    not_built("mods::discover")
}

/// The enabled mods in load order: `discover` filtered and ordered by `config`.
pub fn selected(root: &Path, config: &ModConfig) -> Result<Vec<String>> {
    not_built("mods::selected")
}

/// Every file `folder` replaces or adds under `dvdroot_ps4`. A symlink, a non-regular file or
/// an executable/system replacement is `Error::Mods(..)`.
pub fn mod_files(folder: &Path) -> Result<Vec<ModFile>> {
    not_built("mods::mod_files")
}

/// The overlay for `mods` (name, folder) over the dump at `game`: later mods win; the dump and
/// the mod files are never changed. Returns the list; the runtime reads it from `out`.
pub fn build_overlay(game: &Path, out: &Path, mods: &[(String, PathBuf)]) -> Result<Overlay> {
    not_built("mods::build_overlay")
}
