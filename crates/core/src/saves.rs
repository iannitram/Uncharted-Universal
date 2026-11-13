//! Where each game keeps its save on each device, which files travel, and the manifest save-relay
//! reads (SPEC.md M4, M18). The same savedata layout on every device, so a save moves unchanged.
//! Replaces the per-game save folders of `run.py` (`SAVE_FOLDERS`, `split_saves`) and the slot
//! logic of `scripts/save_slots.py`; the manifest is issue #163 reworked. Path strings inside
//! the save tree use `/`.
//!
//! Layout under the saves folder (the "user folder"), as in the Python:
//!
//! - `<user>/Uncharted N/SAVEDATA0N*`: the game's saves, which is slot 1;
//! - `<user>/Save slots/Uncharted N/Slot n/SAVEDATA0N*`: slot n, for n in 2..=10;
//! - `<user>/save_slots.json`: the selected slot and the slot names (per device, never synced).
//!
//! Save-relay and slots: save-relay takes one manifest per game. Every manifest here says
//! `saves_from: "launcher"`, so the folder it syncs is whatever `--saves-dir` the launcher
//! passes, and the launcher passes [`relay_saves_dir`], the folder of the game's selected slot.
//! The include patterns are relative to that folder, so a save has the same shape in every slot
//! and moves between devices whichever slot each device has selected.

use crate::error::{Error, Result};
use crate::games::Game;
use crate::platform::{AppDirs, Os, Vars};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The prefix of a game's savedata entries (`SAVE_FOLDERS` in `run.py`): `SAVEDATA01` is
/// Uncharted 1, `SAVEDATA02` Uncharted 2, `SAVEDATA03` Uncharted 3.
pub fn save_prefix(game: Game) -> &'static str {
    match game {
        Game::U1 => "SAVEDATA01",
        Game::U2 => "SAVEDATA02",
        Game::U3 => "SAVEDATA03",
    }
}

/// The game a savedata entry name belongs to, by its first ten characters.
fn game_of_entry(name: &str) -> Option<Game> {
    let prefix: String = name.chars().take(10).collect();
    Game::ALL.into_iter().find(|g| save_prefix(*g) == prefix)
}

/// Move entries of the shared `savedata/<user>/<title>/` into per-game folders
/// (`<user_dir>/Uncharted 1/` ..), once; entries already there, and `*.memory` folders, stay.
/// Returns the moved entry names. No `savedata` folder moves nothing.
pub fn split_saves(user_dir: &Path) -> Result<Vec<String>> {
    let mut moved = Vec::new();
    for title in title_folders(&user_dir.join("savedata"))? {
        for path in sorted_all(&title)? {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let Some(game) = game_of_entry(&name) else {
                continue;
            };
            let target = user_dir.join(game.name()).join(&name);
            if target.exists() {
                continue;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            move_entry(&path, &target)?;
            moved.push(name);
        }
    }
    Ok(moved)
}

/// `savedata/*/*` that are folders, not hidden and not `*.memory`, in name order.
fn title_folders(savedata: &Path) -> Result<Vec<PathBuf>> {
    let mut titles = Vec::new();
    for user in visible_children(savedata)? {
        for title in visible_children(&user)? {
            let name = title
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if !name.ends_with(".memory") && title.is_dir() {
                titles.push(title);
            }
        }
    }
    Ok(titles)
}

/// The non-hidden children of a folder, in name order; a missing or non-folder path has none.
fn visible_children(folder: &Path) -> Result<Vec<PathBuf>> {
    if !folder.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = sorted_all(folder)?;
    paths.retain(|p| {
        !p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .starts_with('.')
    });
    Ok(paths)
}

fn sorted_all(folder: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(folder)? {
        paths.push(entry?.path());
    }
    paths.sort();
    Ok(paths)
}

/// `shutil.move`: a rename, or a copy and delete when the rename cannot cross devices.
fn move_entry(from: &Path, to: &Path) -> Result<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_tree(from, to)?;
    if from.is_dir() {
        fs::remove_dir_all(from)?;
    } else {
        fs::remove_file(from)?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        fs::copy(from, to)?;
    }
    Ok(())
}

/// The folder all games' saves live under on a desktop OS: the legacy `<install>/user` when it
/// exists (what `run.py` used, `BB_DATA_DIR/user`), otherwise `<data>/uncharted-universal/user`
/// in the OS data folder, where an update or a re-prepare of the install cannot reach it.
/// Android and iOS keep it inside the app, where save-relay runs:
/// `Error::Unsupported("saves sync inside the app on <os key>")`.
pub fn user_dir(os: Os, vars: &Vars, install: &Path) -> Result<PathBuf> {
    if matches!(os, Os::Ios | Os::Android) {
        return Err(Error::Unsupported(format!(
            "saves sync inside the app on {}",
            os.key()
        )));
    }
    let legacy = install.join("user");
    if legacy.is_dir() {
        return Ok(legacy);
    }
    Ok(AppDirs::new(vars).data.join("user"))
}

/// The folder a game's save lives in on a desktop OS (`<user dir>/Uncharted N`, slot 1; see
/// [`user_dir`]). Android and iOS keep it inside the app, where save-relay runs:
/// `Error::Unsupported("saves sync inside the app on <os key>")` from here.
pub fn save_root(game: Game, os: Os, vars: &Vars, install: &Path) -> Result<PathBuf> {
    Ok(user_dir(os, vars, install)?.join(game.name()))
}

/// Whether a path relative to a game's save root (with `/`) travels: its savedata entries
/// (`SAVEDATA0N*/..`, with their `.sce_sys` metadata) and nothing else: never `where.json`,
/// `save_slots.json`, logs, caches, a device id, temp files (`*.tmp`) or `.save-relay-*` files.
/// The same rule as the include and exclude patterns of [`relay_manifest`].
pub fn is_synced(game: Game, rel_path: &str) -> bool {
    if rel_path.is_empty() || rel_path.starts_with('/') || rel_path.contains('\\') {
        return false;
    }
    let segments: Vec<&str> = rel_path.split('/').collect();
    if segments.len() < 2 || segments.iter().any(|s| s.is_empty() || *s == "..") {
        return false;
    }
    let last = segments[segments.len() - 1];
    segments[0].starts_with(save_prefix(game))
        && !last.ends_with(".tmp")
        && !last.starts_with(".save-relay-")
}

/// The save-relay manifest of one game, on one line, with `saves_from` "launcher" so save-relay
/// refuses to run for these games without `--saves-dir` and can never sync the wrong folder (the
/// `totk` manifest of iannitram/totk-native is the model; format 1, docs/manifest.md of
/// save-relay). The ids are `uncharted-1`, `uncharted-2` and `uncharted-3`. The include
/// patterns are relative to the folder of the selected save slot ([`relay_saves_dir`]).
pub fn relay_manifest(game: Game) -> String {
    let (id, name) = match game {
        Game::U1 => ("uncharted-1", "Uncharted: Drake's Fortune"),
        Game::U2 => ("uncharted-2", "Uncharted 2: Among Thieves"),
        Game::U3 => ("uncharted-3", "Uncharted 3: Drake's Deception"),
    };
    format!(
        "{{\"format\":1,\"id\":\"{id}\",\"name\":{name},\"saves\":{{}},\"saves_from\":\"launcher\",\
         \"include\":[\"{prefix}*/**\"],\"exclude\":[\"**/*.tmp\"]}}",
        name = Value::from(name),
        prefix = save_prefix(game),
    )
}

/// The `--saves-dir` the launcher passes save-relay for a game: the folder of its selected
/// slot under `user_dir`. Changing the selected slot changes what syncs.
pub fn relay_saves_dir(user_dir: &Path, game: Game) -> PathBuf {
    slot_dir(user_dir, game, selected(user_dir, game))
}

// ---- save slots (scripts/save_slots.py, issue #71) ----

/// The most slots per game.
pub const MOST: u8 = 10;
/// The state file in the user folder.
pub const STATE: &str = "save_slots.json";
/// The folder of slots 2.. in the user folder.
pub const SLOTS_FOLDER: &str = "Save slots";

fn check_slot(slot: u8) -> Result<()> {
    if (1..=MOST).contains(&slot) {
        Ok(())
    } else {
        Err(Error::Saves(format!("slot {slot} is not 1 to {MOST}")))
    }
}

/// The slot's folder, relative to the user folder (with `/`).
pub fn relative(game: Game, slot: u8) -> String {
    if slot == 1 {
        game.name().to_owned()
    } else {
        format!("{SLOTS_FOLDER}/{}/Slot {slot}", game.name())
    }
}

pub fn slot_dir(user_dir: &Path, game: Game, slot: u8) -> PathBuf {
    user_dir.join(relative(game, slot))
}

/// The state file as a JSON object; missing, unreadable or not an object is empty.
fn load(user_dir: &Path) -> Map<String, Value> {
    fs::read_to_string(user_dir.join(STATE))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

fn store(user_dir: &Path, state: &Map<String, Value>) -> Result<()> {
    fs::create_dir_all(user_dir)?;
    let text = serde_json::to_string_pretty(state).map_err(|e| Error::Saves(e.to_string()))?;
    let temp = user_dir.join("save_slots.tmp");
    fs::write(&temp, text)?;
    fs::rename(&temp, user_dir.join(STATE))?;
    Ok(())
}

/// The object at `key`, made (or replaced, when it was not an object) if needed.
fn object_at<'a>(map: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    let entry = map.entry(key).or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    match entry {
        Value::Object(inner) => inner,
        _ => unreachable!("made an object above"),
    }
}

/// The game's selected slot; 1 when none is set or the state is unusable.
pub fn selected(user_dir: &Path, game: Game) -> u8 {
    load(user_dir)
        .get(game.name())
        .and_then(|entry| entry.get("selected"))
        .and_then(Value::as_u64)
        .filter(|slot| (1..=u64::from(MOST)).contains(slot))
        .map_or(1, |slot| slot as u8)
}

pub fn select(user_dir: &Path, game: Game, slot: u8) -> Result<()> {
    check_slot(slot)?;
    let mut state = load(user_dir);
    object_at(&mut state, game.name()).insert("selected".into(), slot.into());
    store(user_dir, &state)
}

/// The slot's name, or "".
pub fn name(user_dir: &Path, game: Game, slot: u8) -> String {
    load(user_dir)
        .get(game.name())
        .and_then(|entry| entry.get("names"))
        .and_then(|names| names.get(slot.to_string()))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

/// Name a slot (trimmed, 40 characters at most); blank text removes the name.
pub fn rename(user_dir: &Path, game: Game, slot: u8, text: &str) -> Result<()> {
    check_slot(slot)?;
    let mut state = load(user_dir);
    let names = object_at(object_at(&mut state, game.name()), "names");
    let text = text.trim();
    if text.is_empty() {
        names.remove(&slot.to_string());
    } else {
        let short: String = text.chars().take(40).collect();
        names.insert(slot.to_string(), short.into());
    }
    store(user_dir, &state)
}

/// The save directories in a slot folder (not their `.sce_sys` metadata), in name order.
pub fn saves_in(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = save_entries(folder)
        .into_iter()
        .filter(|p| {
            p.is_dir()
                && !p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .ends_with(".sce_sys")
        })
        .collect();
    found.sort();
    found
}

/// Everything named `SAVEDATA*` in a folder, in name order.
fn save_entries(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .starts_with("SAVEDATA")
        })
        .collect();
    found.sort();
    found
}

pub fn used(user_dir: &Path, game: Game, slot: u8) -> bool {
    !saves_in(&slot_dir(user_dir, game, slot)).is_empty()
}

/// The slots to list: 1, every slot with saves, and the selected one.
pub fn slots(user_dir: &Path, game: Game) -> Vec<u8> {
    let mut shown = vec![1, selected(user_dir, game)];
    shown.extend((2..=MOST).filter(|n| used(user_dir, game, *n)));
    shown.sort_unstable();
    shown.dedup();
    shown
}

/// The game's own description of the save (the detail of the campaign save's `param.bin`,
/// OrbisSaveDataParam: title 128, subtitle 128, detail 1024 bytes; lines joined with " · ")
/// and the newest save time; ("", None) for an empty slot.
pub fn details(user_dir: &Path, game: Game, slot: u8) -> (String, Option<SystemTime>) {
    let folder = slot_dir(user_dir, game, slot);
    let found = saves_in(&folder);
    if found.is_empty() {
        return (String::new(), None);
    }
    let newest = found
        .iter()
        .filter_map(|d| fs::metadata(d).and_then(|m| m.modified()).ok())
        .max();
    let prefix = save_prefix(game);
    let mut text = String::new();
    for meta in save_entries(&folder) {
        let name = meta
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let is_meta = name
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(".sce_sys"))
            .is_some_and(|digit| digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit());
        if !is_meta {
            continue;
        }
        let Ok(raw) = fs::read(meta.join("param.bin")) else {
            continue;
        };
        let field = raw.get(256..raw.len().min(256 + 1024)).unwrap_or(&[]);
        let field = field.split(|b| *b == 0).next().unwrap_or(&[]);
        text = String::from_utf8_lossy(field).trim().to_owned();
        if !text.is_empty() {
            break;
        }
    }
    let joined = text
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    (joined, newest)
}

/// The first slot from 2 that is empty and not listed, if any.
pub fn first_free(user_dir: &Path, game: Game) -> Option<u8> {
    let listed = slots(user_dir, game);
    (2..=MOST).find(|n| !used(user_dir, game, *n) && !listed.contains(n))
}

/// The target slot becomes a copy of the source slot. The target must be empty: a copy never
/// deletes or replaces saves (ours, #246; the peer's copy replaced what the target held), so the
/// player's saves in any slot stay until they delete them themselves.
pub fn copy(user_dir: &Path, game: Game, source: u8, target: u8) -> Result<()> {
    check_slot(source)?;
    check_slot(target)?;
    let not_empty = || Error::Saves(format!("{}: slot {target} is not empty", game.name()));
    let src = slot_dir(user_dir, game, source);
    let dst = slot_dir(user_dir, game, target);
    if source == target || !save_entries(&dst).is_empty() {
        return Err(not_empty());
    }
    let mut temp_name = dst.file_name().unwrap_or_default().to_owned();
    temp_name.push(".copying");
    let temp = dst.with_file_name(temp_name);
    let _ = fs::remove_dir_all(&temp);
    if let Some(parent) = temp.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(&temp)?;
    for entry in save_entries(&src) {
        copy_tree(&entry, &temp.join(entry.file_name().unwrap_or_default()))?;
    }
    fs::create_dir_all(&dst)?;
    for entry in sorted_all(&temp)? {
        let to = dst.join(entry.file_name().unwrap_or_default());
        if to.exists() {
            // Appeared during the copy: keep it, leave ours in temp.
            return Err(not_empty());
        }
        fs::rename(&entry, &to)?;
    }
    fs::remove_dir(&temp)?;
    Ok(())
}

/// Deletes the slot's saves (only the save entries: slot 1 is the game's folder).
pub fn clear(user_dir: &Path, game: Game, slot: u8) -> Result<()> {
    check_slot(slot)?;
    for entry in save_entries(&slot_dir(user_dir, game, slot)) {
        if entry.is_dir() {
            fs::remove_dir_all(&entry)?;
        } else {
            fs::remove_file(&entry)?;
        }
    }
    Ok(())
}

/// Deletes the slot's saves and its name; slot 1 stays as an empty slot, and deleting the
/// selected slot selects slot 1.
pub fn delete(user_dir: &Path, game: Game, slot: u8) -> Result<()> {
    clear(user_dir, game, slot)?;
    if slot != 1 {
        let _ = fs::remove_dir_all(slot_dir(user_dir, game, slot));
    }
    rename(user_dir, game, slot, "")?;
    if slot != 1 && selected(user_dir, game) == slot {
        select(user_dir, game, 1)?;
    }
    Ok(())
}

/// `BB_SAVEDATA_DIRS` for the runtime (`crates/runtime` `savedata` `mapped_folder`): each game's save
/// directories in its selected slot, `SAVEDATA01=Uncharted 1;..`.
pub fn mapping(user_dir: &Path) -> String {
    Game::ALL
        .iter()
        .map(|g| {
            format!(
                "{}={}",
                save_prefix(*g),
                relative(*g, selected(user_dir, *g))
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

#[cfg(test)]
mod tests;
