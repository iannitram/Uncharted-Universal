//! Updating the launcher or the port (SPEC.md M19): back the saves up first, re-prepare, and
//! check the saves are still there. Replaces the update check in
//! `launcher/uncharted_launcher.py`; the shape is copied from iannitram/totk-native's
//! `update.rs` (MIT).

use crate::backup;
use crate::error::{Error, Result};
use crate::games::Game;
use crate::saves;
use std::fs;
use std::io::ErrorKind;
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

/// How many files under `dir` sync (`saves::is_synced`, by their path relative to `save_root`
/// with `/`). A missing `dir` holds none.
fn count_synced(game: Game, save_root: &Path, dir: &Path) -> Result<usize> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    let mut count = 0;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            count += count_synced(game, save_root, &path)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(save_root)
                .map_err(|e| Error::Io(e.to_string()))?;
            let rel = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            if saves::is_synced(game, &rel) {
                count += 1;
            }
        }
    }
    Ok(count)
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
    let backup = backup::backup_saves(game, save_root, backups, stamp)?;
    let files_before = count_synced(game, save_root, save_root)?;
    rebuild()?;
    let files_after = count_synced(game, save_root, save_root)?;
    if files_after < files_before {
        return Err(Error::Saves(format!(
            "saves changed during the update; restore {}",
            backup.display()
        )));
    }
    backup::prune(backups, backup::KEEP)?;
    Ok(Updated {
        backup,
        files_before,
        files_after,
    })
}
