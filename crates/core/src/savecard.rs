//! The save card the Continue screen shows (SPEC.md M10): one save's picture, title and detail,
//! when it was saved, the device and the build that made it. Read-only: nothing here writes a
//! save. A field the files do not give is `None`, never an error, so the card always shows
//! (with the game's own `icon0.png` as the fallback picture). New module, no Python
//! counterpart; the shape is copied from iannitram/totk-native's `savecard.rs` (MIT).
//!
//! A save folder is one savedata entry (`SAVEDATA02...`) with the console's metadata in
//! `sce_sys/`: `icon0.png` is the save's picture and `param.sfo` holds its title and detail.

use crate::error::{Error, Result};
use crate::games::Game;
use crate::saves::save_prefix;
use crate::sfo;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// `param.sfo` key of a save's detail line.
const DETAIL: &str = "DETAIL";
/// `param.sfo` key a save uses for its title when it has no `TITLE`.
const MAINTITLE: &str = "MAINTITLE";

/// One save, as the Continue screen and the conflict prompt show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveCard {
    pub game: Game,
    /// The save's entry name ("SAVEDATA01-..."), as found on disk.
    pub slot: String,
    /// The save's picture: its `sce_sys/icon0.png`, bytes as stored.
    pub picture: Option<Vec<u8>>,
    /// `TITLE` of the save's `sce_sys/param.sfo`.
    pub title: Option<String>,
    /// The save's detail line from the same file.
    pub detail: Option<String>,
    /// Seconds since the Unix epoch of the newest file in the save.
    pub saved_unix: u64,
    /// Play time in seconds, when the files give it.
    pub play_time_secs: Option<u64>,
    /// The device that made the save (filled in by the launcher from the fleet records).
    pub device: Option<String>,
    /// The build that made it (`bundle::Bundle::build_id`), same source.
    pub build_id: Option<String>,
}

/// Both sides of a sync conflict, for the side-by-side prompt (M10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideBySide {
    pub server: SaveCard,
    pub local: SaveCard,
    /// Which one is newer: true when the server's `saved_unix` is larger.
    pub server_is_newer: bool,
}

/// The card for one save folder of `game`. No `param.sfo` and no picture still gives a card;
/// an unreadable folder is `Error::Saves("no save at <path>")`.
///
/// `picture` is `sce_sys/icon0.png` as stored; `title` is `TITLE` of `sce_sys/param.sfo`
/// (`MAINTITLE` when there is no `TITLE`) and `detail` its `DETAIL`, each `None` when the file
/// is missing, damaged, or lacks the key or the text is empty. `saved_unix` is the newest
/// modified time of any file in the folder, at any depth (the folder's own time when it holds
/// no file). `play_time_secs`, `device` and `build_id` are `None`: the files do not give them.
pub fn read_card(game: Game, save_dir: &Path) -> Result<SaveCard> {
    let no_save = || Error::Saves(format!("no save at {}", save_dir.display()));
    if fs::read_dir(save_dir).is_err() {
        return Err(no_save());
    }
    let own_time = fs::metadata(save_dir)
        .and_then(|m| m.modified())
        .map_err(|_| no_save())?;
    let newest = newest_file_time(save_dir).unwrap_or(own_time);
    let saved_unix = newest
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());

    let sce_sys = save_dir.join("sce_sys");
    let picture = fs::read(sce_sys.join("icon0.png")).ok();
    let values = fs::read(sce_sys.join("param.sfo"))
        .ok()
        .and_then(|data| sfo::parse(&data).ok())
        .unwrap_or_default();
    let field = |key: &str| {
        sfo::text(&values, key)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    };
    let title = field(sfo::TITLE).or_else(|| field(MAINTITLE));
    let detail = field(DETAIL);

    Ok(SaveCard {
        game,
        slot: save_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        picture,
        title,
        detail,
        saved_unix,
        play_time_secs: None,
        device: None,
        build_id: None,
    })
}

/// The newest modified time of any file under `folder`, at any depth; `None` when it holds no
/// file whose time can be read. Folders that cannot be read are skipped.
fn newest_file_time(folder: &Path) -> Option<SystemTime> {
    let mut newest = None;
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if let Ok(time) = entry.metadata().and_then(|m| m.modified()) {
                newest = newest.max(Some(time));
            }
        }
    }
    newest
}

/// The newest card among the saves in a game's save root, the largest `saved_unix` winning, the
/// lower name on a tie. `Ok(None)` when there is no save.
///
/// A save is a folder directly in `save_root` whose name starts with the game's
/// `saves::save_prefix` and is not hidden, `*.sce_sys` or `*.memory`; other entries are
/// skipped. A missing `save_root` has no save.
pub fn latest(game: Game, save_root: &Path) -> Result<Option<SaveCard>> {
    if !save_root.is_dir() {
        return Ok(None);
    }
    let prefix = save_prefix(game);
    let mut newest: Option<SaveCard> = None;
    for entry in fs::read_dir(save_root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(prefix) || name.ends_with(".sce_sys") || name.ends_with(".memory") {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let card = read_card(game, &path)?;
        let replace = newest.as_ref().is_none_or(|current| {
            card.saved_unix > current.saved_unix
                || (card.saved_unix == current.saved_unix && card.slot < current.slot)
        });
        if replace {
            newest = Some(card);
        }
    }
    Ok(newest)
}

/// The pair the conflict prompt shows; `server_is_newer` only when the server's `saved_unix`
/// is strictly larger (a tie is not newer).
pub fn side_by_side(server: SaveCard, local: SaveCard) -> SideBySide {
    let server_is_newer = server.saved_unix > local.saved_unix;
    SideBySide {
        server,
        local,
        server_is_newer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    /// A fresh temp folder under `std::env::temp_dir()`, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Temp {
            static N: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "uncharted-savecard-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Temp(path)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A text-only `param.sfo` (format 0x0204) of (key, value); synthetic only.
    fn sfo_bytes(entries: &[(&str, &str)]) -> Vec<u8> {
        let (mut keys, mut vals, mut table): (Vec<u8>, Vec<u8>, Vec<u8>) = Default::default();
        for (key, value) in entries {
            let mut value = value.as_bytes().to_vec();
            value.push(0);
            table.extend((keys.len() as u16).to_le_bytes());
            table.extend(0x0204u16.to_le_bytes());
            table.extend((value.len() as u32).to_le_bytes());
            table.extend((value.len() as u32).to_le_bytes());
            table.extend((vals.len() as u32).to_le_bytes());
            keys.extend(key.as_bytes());
            keys.push(0);
            vals.extend(value);
        }
        let key_base = 20 + table.len();
        let value_base = key_base + keys.len();
        let mut d = b"\0PSF\x01\x01\0\0".to_vec();
        for n in [key_base, value_base, entries.len()] {
            d.extend((n as u32).to_le_bytes());
        }
        d.extend(table);
        d.extend(keys);
        d.extend(vals);
        d
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn write_at(path: &Path, data: &[u8], secs: u64) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(at(secs))
            .unwrap();
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nsynthetic picture";

    /// A save folder with a picture, a `param.sfo` and data files, its newest file at `secs`.
    fn make_save(root: &Path, name: &str, secs: u64) -> PathBuf {
        let dir = root.join(name);
        write_at(&dir.join("sce_sys").join("icon0.png"), PNG, secs - 30);
        write_at(
            &dir.join("sce_sys").join("param.sfo"),
            &sfo_bytes(&[
                ("TITLE", "Uncharted 2: Among Thieves"),
                ("DETAIL", "Chapter 4 - Desperate Times"),
            ]),
            secs - 20,
        );
        write_at(&dir.join("USR-DATA"), b"data", secs);
        dir
    }

    #[test]
    fn read_card_gives_picture_title_detail_and_newest_time() {
        let temp = Temp::new();
        let dir = make_save(&temp.0, "SAVEDATA02-0", 1_700_000_000);
        write_at(&dir.join("deep").join("older"), b"x", 1_600_000_000);
        let card = read_card(Game::U2, &dir).unwrap();
        assert_eq!(
            card,
            SaveCard {
                game: Game::U2,
                slot: "SAVEDATA02-0".into(),
                picture: Some(PNG.to_vec()),
                title: Some("Uncharted 2: Among Thieves".into()),
                detail: Some("Chapter 4 - Desperate Times".into()),
                saved_unix: 1_700_000_000,
                play_time_secs: None,
                device: None,
                build_id: None,
            }
        );
    }

    #[test]
    fn read_card_takes_maintitle_when_there_is_no_title() {
        let temp = Temp::new();
        let dir = temp.0.join("SAVEDATA01");
        write_at(
            &dir.join("sce_sys").join("param.sfo"),
            &sfo_bytes(&[("MAINTITLE", "Uncharted: Drake's Fortune")]),
            1_700_000_000,
        );
        let card = read_card(Game::U1, &dir).unwrap();
        assert_eq!(card.title.as_deref(), Some("Uncharted: Drake's Fortune"));
        assert_eq!(card.detail, None);
    }

    #[test]
    fn read_card_without_param_sfo_or_picture_has_none_fields() {
        let temp = Temp::new();
        let dir = temp.0.join("SAVEDATA03");
        write_at(&dir.join("USR-DATA"), b"data", 1_650_000_000);
        let card = read_card(Game::U3, &dir).unwrap();
        assert_eq!(card.slot, "SAVEDATA03");
        assert_eq!(card.picture, None);
        assert_eq!(card.title, None);
        assert_eq!(card.detail, None);
        assert_eq!(card.saved_unix, 1_650_000_000);

        // A damaged `param.sfo` reads the same as a missing one.
        write_at(
            &dir.join("sce_sys").join("param.sfo"),
            b"junk",
            1_650_000_000,
        );
        let card = read_card(Game::U3, &dir).unwrap();
        assert_eq!((card.title, card.detail), (None, None));
    }

    #[test]
    fn read_card_of_an_unreadable_folder_is_no_save() {
        let temp = Temp::new();
        let missing = temp.0.join("SAVEDATA02");
        assert_eq!(
            read_card(Game::U2, &missing),
            Err(Error::Saves(format!("no save at {}", missing.display())))
        );
        let file = temp.0.join("SAVEDATA02-file");
        fs::write(&file, b"not a folder").unwrap();
        assert_eq!(
            read_card(Game::U2, &file),
            Err(Error::Saves(format!("no save at {}", file.display())))
        );
    }

    #[test]
    fn latest_returns_the_newest_card() {
        let temp = Temp::new();
        make_save(&temp.0, "SAVEDATA02-a", 1_700_000_000);
        make_save(&temp.0, "SAVEDATA02-b", 1_700_000_500);
        make_save(&temp.0, "SAVEDATA02-c", 1_699_000_000);
        // Not this game's save, and not a save folder: skipped even though newer.
        make_save(&temp.0, "SAVEDATA03-z", 1_800_000_000);
        make_save(&temp.0, "SAVEDATA02-a.sce_sys", 1_800_000_000);
        write_at(&temp.0.join("SAVEDATA02-file"), b"x", 1_800_000_000);
        let card = latest(Game::U2, &temp.0).unwrap().unwrap();
        assert_eq!(card.slot, "SAVEDATA02-b");
        assert_eq!(card.saved_unix, 1_700_000_500);
    }

    #[test]
    fn latest_lower_name_wins_a_tie() {
        let temp = Temp::new();
        make_save(&temp.0, "SAVEDATA01-b", 1_700_000_000);
        make_save(&temp.0, "SAVEDATA01-a", 1_700_000_000);
        make_save(&temp.0, "SAVEDATA01-c", 1_700_000_000);
        let card = latest(Game::U1, &temp.0).unwrap().unwrap();
        assert_eq!(card.slot, "SAVEDATA01-a");
    }

    #[test]
    fn latest_with_no_save_is_none() {
        let temp = Temp::new();
        assert_eq!(latest(Game::U3, &temp.0), Ok(None));
        assert_eq!(latest(Game::U3, &temp.0.join("missing")), Ok(None));
        make_save(&temp.0, "SAVEDATA02-0", 1_700_000_000);
        assert_eq!(latest(Game::U3, &temp.0), Ok(None));
    }

    fn card(saved_unix: u64) -> SaveCard {
        SaveCard {
            game: Game::U2,
            slot: "SAVEDATA02".into(),
            picture: None,
            title: None,
            detail: None,
            saved_unix,
            play_time_secs: None,
            device: None,
            build_id: None,
        }
    }

    #[test]
    fn side_by_side_sets_server_is_newer() {
        let pair = side_by_side(card(200), card(100));
        assert!(pair.server_is_newer);
        assert_eq!((pair.server.saved_unix, pair.local.saved_unix), (200, 100));
        assert!(!side_by_side(card(100), card(200)).server_is_newer);
        assert!(!side_by_side(card(150), card(150)).server_is_newer);
    }
}
