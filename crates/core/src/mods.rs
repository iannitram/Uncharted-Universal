//! Mod folders: which layouts are accepted, the order, and the overlay list the runtime's file
//! VFS reads. Mods replace or add files under the game's data folders (`u1data`, `u2data`,
//! `u3data`, `shareddata`); executable and system files (the three executables, `sce_module`,
//! `sce_sys`, `sce_discmap.plt`) and symlinks are refused. Ported, not extended (SPEC.md
//! Won'ts). Replaces `scripts/mods.py`; `tests/test_mods.py` are the expected results.
//!
//! Uncharted rulings (piece 1.11). `mods.py` was written for Bloodborne, whose `/app0` holds
//! one `dvdroot_ps4` folder; Uncharted's dump has no `dvdroot_ps4`: its `/app0` is the dump
//! folder itself (`eboot.bin`, `big2-ps4_Shipping.elf`, `big3-ps4_Shipping.elf`,
//! `sce_discmap.plt`, `sce_module`, `sce_sys`, `shareddata`, `u1data`, `u2data`, `u3data`).
//! So a mod's content root is a folder holding at least one of [`GAME_FOLDERS`], and a game
//! path is relative to `/app0` (`u1data/...`), which is also shadPS4's `<game>-mods` layout.
//! The accepted layouts are `<mod>/<game folders>`, `<mod>/app0/<game folders>`,
//! `<mod>/<title id>/<game folders>` (any of the collection's four ids, `games::TITLE_IDS`,
//! for `CUSA03173`), and one wrapper folder around any of these (an archive extracted into a
//! folder of its name; doc and image files may sit beside it). Bloodborne's `GAME_FOLDERS`
//! list and its `dvdroot_ps4` root are dropped.
//!
//! The overlay is a list, not a tree of links: the runtime's file VFS (piece 3.09) reads it
//! from `out` and serves each listed file in place of, or beside, the dump's. Nothing in the
//! dump or the mods is written, linked or moved.

use crate::error::{Error, Result};
use crate::games::{Game, TITLE_IDS};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The game's data folders at the top of `/app0`: a folder holding one of these is a mod's
/// content root. Matched case-insensitively.
pub const GAME_FOLDERS: [&str; 4] = ["shareddata", "u1data", "u2data", "u3data"];

/// Top-level entries of `/app0` a mod may not replace, besides the executables
/// ([`Game::executable`]). Executable patches go through `patches`, with address checks.
const SYSTEM_ENTRIES: [&str; 3] = ["sce_discmap.plt", "sce_module", "sce_sys"];

/// Files that may sit beside a wrapper folder (readme files, pictures, mod-manager notes).
const DOC_SUFFIXES: [&str; 5] = [".txt", ".md", ".jpg", ".png", ".ini"];

/// The version line of the overlay list written to `out`.
const OVERLAY_VERSION: u64 = 1;

/// The player's mod choice (`mods.json`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModConfig {
    /// Folder names switched off.
    pub disabled: Vec<String>,
    /// Folder names in load order; new folders go last, alphabetically.
    pub order: Vec<String>,
}

impl ModConfig {
    /// `mods.json` as text (a UTF-8 BOM is allowed): an object whose `disabled` and `order`,
    /// when present, are lists of folder names; other keys are ignored. Anything else is
    /// `Error::Mods`.
    pub fn parse(text: &str) -> Result<ModConfig> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let value: Value = serde_json::from_str(text)
            .map_err(|_| Error::Mods("mods.json is not valid JSON".into()))?;
        let settings = value
            .as_object()
            .ok_or_else(|| Error::Mods("mods.json must be a JSON object".into()))?;
        Ok(ModConfig {
            disabled: names(settings.get("disabled"), "Disabled mods")?,
            order: names(settings.get("order"), "Mod order")?,
        })
    }

    /// `mods.json` at `path`; a missing file is the default (every mod on, alphabetical).
    pub fn load(path: &Path) -> Result<ModConfig> {
        if !path.is_file() {
            return Ok(ModConfig::default());
        }
        ModConfig::parse(&fs::read_to_string(path)?)
    }
}

fn names(value: Option<&Value>, what: &str) -> Result<Vec<String>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let bad = || Error::Mods(format!("{what} must be a list of folder names"));
    value
        .as_array()
        .ok_or_else(bad)?
        .iter()
        .map(|name| name.as_str().map(str::to_owned).ok_or_else(bad))
        .collect()
}

/// One file a mod replaces or adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModFile {
    /// The path in the game, relative to `/app0` with `/`: `u1data/...`. From [`mod_files`]
    /// it has the mod's spelling; in an [`Overlay`] each component that exists in the dump in
    /// another case takes the dump's spelling.
    pub game_path: String,
    pub source: PathBuf,
}

/// The overlay the runtime mounts over the game's files.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Overlay {
    /// Last mod wins; sorted by `game_path`.
    pub files: Vec<ModFile>,
}

fn fold(name: &str) -> String {
    name.to_lowercase()
}

fn is_game_folder(name: &str) -> bool {
    GAME_FOLDERS.contains(&fold(name).as_str())
}

fn is_system_entry(name: &str) -> bool {
    let name = fold(name);
    SYSTEM_ENTRIES.contains(&name.as_str())
        || Game::ALL.iter().any(|game| fold(game.executable()) == name)
}

fn is_doc(name: &str) -> bool {
    let name = fold(name);
    DOC_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Dir,
    File,
    /// A symlink, a junction, a device, a pipe: never followed.
    Other,
}

struct Entry {
    name: String,
    path: PathBuf,
    kind: Kind,
}

/// The entries of `folder` without following links, dotfiles left out, sorted by name. A name
/// that is not UTF-8 is `Error::Mods`.
fn entries(folder: &Path) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| Error::Mods("A mod file name is not valid Unicode".into()))?;
        if name.starts_with('.') {
            continue;
        }
        let file_type = entry.file_type()?;
        let kind = if file_type.is_symlink() {
            Kind::Other
        } else if file_type.is_dir() {
            Kind::Dir
        } else if file_type.is_file() {
            Kind::File
        } else {
            Kind::Other
        };
        out.push(Entry {
            name,
            path: entry.path(),
            kind,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn holds_game_folder(list: &[Entry]) -> bool {
    list.iter()
        .any(|e| e.kind == Kind::Dir && is_game_folder(&e.name))
}

/// Where a mod's files are (the folder that stands for `/app0`), or `None` when `folder` has
/// no accepted layout. `folder` itself may be a link; nothing below it is followed.
fn content_root(folder: &Path) -> Result<Option<PathBuf>> {
    if !folder.is_dir() {
        return Ok(None);
    }
    layout(folder)
}

fn layout(folder: &Path) -> Result<Option<PathBuf>> {
    let list = entries(folder)?;
    if holds_game_folder(&list) {
        return Ok(Some(folder.to_path_buf()));
    }
    for wrapper in list.iter().filter(|e| e.kind == Kind::Dir) {
        let name = fold(&wrapper.name);
        let is_wrapper = name == "app0" || TITLE_IDS.iter().any(|id| fold(id) == name);
        if is_wrapper && holds_game_folder(&entries(&wrapper.path)?) {
            return Ok(Some(wrapper.path.clone()));
        }
    }
    let mut folders = list.iter().filter(|e| e.kind == Kind::Dir);
    if let (Some(only), None) = (folders.next(), folders.next()) {
        if list
            .iter()
            .all(|e| e.kind == Kind::Dir || (e.kind == Kind::File && is_doc(&e.name)))
        {
            return layout(&only.path);
        }
    }
    Ok(None)
}

fn mod_name(folder: &Path) -> String {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Names of the folders of `root` that have an accepted layout, sorted case-insensitively.
/// A missing `root` is an empty list.
pub fn discover(root: &Path) -> Result<Vec<String>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if !path.is_dir() || content_root(&path)?.is_none() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            found.push(name.to_owned());
        }
    }
    found.sort_by_key(|name| (fold(name), name.clone()));
    Ok(found)
}

/// The enabled mods in load order: `discover` filtered and ordered by `config`.
pub fn selected(root: &Path, config: &ModConfig) -> Result<Vec<String>> {
    let available = discover(root)?;
    let mut out: Vec<String> = Vec::new();
    // New folders are enabled automatically and come after `order`, alphabetically.
    for name in config.order.iter().chain(available.iter()) {
        if available.contains(name) && !config.disabled.contains(name) && !out.contains(name) {
            out.push(name.clone());
        }
    }
    Ok(out)
}

/// The layers to mount over the dump at `game`, in load order (the CLI of `mods.py`). With
/// `enabled` false (the master toggle) there are none. Otherwise shadPS4's sibling folder
/// `<game>-mods` comes first when it exists; then `mods_dir` as one mod when it holds the
/// game's folders itself, else the [`selected`] mods of `mods_dir`.
pub fn layers(
    game: &Path,
    mods_dir: &Path,
    config: &ModConfig,
    enabled: bool,
) -> Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    if !enabled {
        return Ok(out);
    }
    if let (Some(parent), Some(name)) = (game.parent(), game.file_name()) {
        let mut sibling = name.to_os_string();
        sibling.push("-mods");
        let legacy = parent.join(sibling);
        if legacy.is_dir() {
            out.push((mod_name(&legacy), legacy));
        }
    }
    if mods_dir.is_dir() && holds_game_folder(&entries(mods_dir)?) {
        out.push((mod_name(mods_dir), mods_dir.to_path_buf()));
    } else {
        for name in selected(mods_dir, config)? {
            let folder = mods_dir.join(&name);
            out.push((name, folder));
        }
    }
    Ok(out)
}

/// Every file `folder` replaces or adds under the game's data folders, sorted by
/// `game_path`. A symlink, a non-regular file or an executable/system replacement is
/// `Error::Mods(..)`. Other top-level entries (readme files, pictures) are left out.
pub fn mod_files(folder: &Path) -> Result<Vec<ModFile>> {
    let name = mod_name(folder);
    let root = content_root(folder)?.ok_or_else(|| {
        Error::Mods(format!(
            "{name}: expected u1data, u2data, u3data or shareddata inside the mod folder"
        ))
    })?;
    let mut out = Vec::new();
    walk(&name, &root, &mut Vec::new(), true, &mut out)?;
    out.sort_by(|a, b| a.game_path.cmp(&b.game_path));
    Ok(out)
}

fn walk(
    name: &str,
    dir: &Path,
    prefix: &mut Vec<String>,
    collect: bool,
    out: &mut Vec<ModFile>,
) -> Result<()> {
    let list = entries(dir)?;
    let at = |prefix: &[String], entry: &Entry| {
        let mut parts = prefix.to_vec();
        parts.push(entry.name.clone());
        parts.join("/")
    };
    if let Some(link) = list.iter().find(|e| is_link(&e.path)) {
        return Err(Error::Mods(format!(
            "{name}: mod symlinks are unsupported: {}",
            at(prefix, link)
        )));
    }
    for entry in &list {
        let top = prefix.is_empty();
        if top && is_system_entry(&entry.name) {
            return Err(Error::Mods(format!(
                "{name}: executable/system replacement is unsupported: {}",
                entry.name
            )));
        }
        // Readme and metadata at the top stay outside the mounted game.
        let keep = collect && (!top || (entry.kind == Kind::Dir && is_game_folder(&entry.name)));
        match entry.kind {
            Kind::Dir => {
                prefix.push(entry.name.clone());
                walk(name, &entry.path, prefix, keep, out)?;
                prefix.pop();
            }
            Kind::File if keep => out.push(ModFile {
                game_path: at(prefix, entry),
                source: entry.path.clone(),
            }),
            Kind::Other if keep => {
                return Err(Error::Mods(format!(
                    "{name}: not a regular mod file: {}",
                    at(prefix, entry)
                )))
            }
            _ => {}
        }
    }
    Ok(())
}

/// A symlink or a Windows junction (any name-surrogate reparse point), not followed.
fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// The dump's spelling of one directory's entries: folded name to (name, is a directory).
type Listing = HashMap<String, (String, bool)>;

fn listing<'a>(cache: &'a mut HashMap<PathBuf, Listing>, dir: &Path) -> Result<&'a Listing> {
    if !cache.contains_key(dir) {
        let mut names = Listing::new();
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                if let Ok(name) = entry.file_name().into_string() {
                    names.insert(fold(&name), (name, entry.path().is_dir()));
                }
            }
        }
        cache.insert(dir.to_path_buf(), names);
    }
    Ok(&cache[dir])
}

fn conflict(path: &str) -> Error {
    Error::Mods(format!("file/directory conflict: {path}"))
}

/// The overlay for `mods` (name, folder) over the dump at `game`: later mods win; the dump and
/// the mod files are never changed. Returns the list and writes it to the file `out` (JSON:
/// `version`, `game`, and `files` of `path` and `source`, absolute paths) for the runtime's
/// file VFS. With no mod file at all, `out` is removed if it is there and the list is empty.
/// A file where the dump or an earlier mod has a folder, or the reverse, is `Error::Mods`,
/// and `out` is left as it was.
pub fn build_overlay(game: &Path, out: &Path, mods: &[(String, PathBuf)]) -> Result<Overlay> {
    if !game.is_dir() {
        return Err(Error::Mods("the game folder does not exist".into()));
    }
    let mut cache: HashMap<PathBuf, Listing> = HashMap::new();
    // Folded game path to its spelling and whether it is a directory, for every path a mod
    // has touched so far.
    let mut spelled: HashMap<String, (String, bool)> = HashMap::new();
    let mut files: Vec<ModFile> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for (_, folder) in mods {
        for file in mod_files(folder)? {
            let parts: Vec<&str> = file.game_path.split('/').collect();
            let mut path = String::new();
            for (i, part) in parts.iter().enumerate() {
                let last = i + 1 == parts.len();
                let parent = path.clone();
                if !path.is_empty() {
                    path.push('/');
                }
                let key = fold(&format!("{parent}/{part}"));
                if let Some((spelling, is_dir)) = spelled.get(&key) {
                    if *is_dir == last {
                        return Err(conflict(&file.game_path));
                    }
                    path = spelling.clone();
                    continue;
                }
                let host = game.join(&parent);
                let (name, is_dir) = listing(&mut cache, &host)?
                    .get(&fold(part))
                    .cloned()
                    .unwrap_or_else(|| (part.to_string(), !last));
                if is_dir == last {
                    return Err(conflict(&file.game_path));
                }
                path.push_str(&name);
                spelled.insert(key, (path.clone(), is_dir));
            }
            match index.get(&fold(&path)) {
                Some(&at) => files[at].source = file.source,
                None => {
                    index.insert(fold(&path), files.len());
                    files.push(ModFile {
                        game_path: path,
                        source: file.source,
                    });
                }
            }
        }
    }
    files.sort_by(|a, b| a.game_path.cmp(&b.game_path));
    let overlay = Overlay { files };
    write_list(game, out, &overlay)?;
    Ok(overlay)
}

fn text_of(path: &Path) -> Result<String> {
    std::path::absolute(path)?
        .into_os_string()
        .into_string()
        .map_err(|_| Error::Mods("a mod or game path is not valid Unicode".into()))
}

fn write_list(game: &Path, out: &Path, overlay: &Overlay) -> Result<()> {
    if overlay.files.is_empty() {
        return match fs::remove_file(out) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        };
    }
    let files = overlay
        .files
        .iter()
        .map(|f| Ok(json!({ "path": f.game_path, "source": text_of(&f.source)? })))
        .collect::<Result<Vec<_>>>()?;
    let list = json!({ "version": OVERLAY_VERSION, "game": text_of(game)?, "files": files });
    let mut text = serde_json::to_string_pretty(&list)
        .map_err(|_| Error::Mods("the overlay list could not be written".into()))?;
    text.push('\n');
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut temp = out.as_os_str().to_os_string();
    temp.push(".tmp");
    fs::write(&temp, text)?;
    fs::rename(&temp, out)?;
    Ok(())
}

#[cfg(test)]
mod tests;
