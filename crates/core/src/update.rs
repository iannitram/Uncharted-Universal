//! Updating the launcher or the port (SPEC.md M19): back the saves up first, re-prepare, and
//! check the saves are still there. Replaces the update check in
//! `launcher/uncharted_launcher.py`; the shape is copied from iannitram/totk-native's
//! `update.rs` (MIT).

use crate::error::{not_built, Result};
use crate::games::Game;
use std::path::{Path, PathBuf};

/// What an update did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Updated {
    /// The backup made before the rebuild (`backup::backup_saves`).
    pub backup: PathBuf,
    /// Synced files in the save root before and after; equal on success.
    pub files_before: usize,
    pub files_after: usize,
}

/// Back up `save_root` into `backups` under `stamp`, run `rebuild`, then count the synced files
/// again. A rebuild error is returned after the backup (the backup stays). Fewer synced files
/// after than before is `Error::Saves("saves changed during the update; restore <backup>")` and
/// nothing is deleted. Keeps the newest `backup::KEEP` backups.
pub fn update(
    game: Game,
    save_root: &Path,
    backups: &Path,
    stamp: &str,
    rebuild: &mut dyn FnMut() -> Result<()>,
) -> Result<Updated> {
    not_built("update::update")
}
