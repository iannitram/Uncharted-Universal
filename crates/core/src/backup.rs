//! Save backups before an update (SPEC.md M19). A backup is a plain folder `<backups>/<stamp>/`
//! holding the synced files of a save root at their relative paths, so a person can restore one by
//! hand; the newest 10 are kept. New module; the shape and code are copied from
//! iannitram/totk-native's `backup.rs` (MIT), with the game passed to `saves::is_synced`.

use crate::error::{Error, Result};
use crate::games::Game;
use crate::saves;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// How many backups `update` keeps.
pub const KEEP: usize = 10;

/// Copy the files under `dir` that sync (`saves::is_synced`, by their path relative to
/// `save_root` with `/`) into `folder` at the same relative paths.
fn copy_synced(game: Game, save_root: &Path, folder: &Path, dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_synced(game, save_root, folder, &path)?;
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
                let destination = folder.join(relative);
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&path, &destination)?;
            }
        }
    }
    Ok(())
}

/// Copy every synced file (`saves::is_synced`) under `save_root` into `<backups>/<stamp>/`
/// and return that folder. An existing `<backups>/<stamp>` is `Error::Saves("backup <stamp> exists")`
/// and nothing is written; a missing `save_root` is `Error::Saves("no save folder at <save_root>")`.
pub fn backup_saves(game: Game, save_root: &Path, backups: &Path, stamp: &str) -> Result<PathBuf> {
    if !save_root.is_dir() {
        return Err(Error::Saves(format!(
            "no save folder at {}",
            save_root.display()
        )));
    }
    let folder = backups.join(stamp);
    if folder.exists() {
        return Err(Error::Saves(format!("backup {stamp} exists")));
    }
    fs::create_dir_all(&folder)?;
    copy_synced(game, save_root, &folder, save_root)?;
    Ok(folder)
}

/// The backups in `backups`, by folder name, oldest first (names are stamps, which sort by
/// time). A missing `backups` folder is an empty list.
pub fn list(backups: &Path) -> Result<Vec<String>> {
    let entries = match fs::read_dir(backups) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    Ok(names)
}

/// Delete the oldest backups so that `keep` remain; return the deleted names, oldest first.
pub fn prune(backups: &Path, keep: usize) -> Result<Vec<String>> {
    let mut names = list(backups)?;
    names.truncate(names.len().saturating_sub(keep));
    for name in &names {
        fs::remove_dir_all(backups.join(name))?;
    }
    Ok(names)
}
