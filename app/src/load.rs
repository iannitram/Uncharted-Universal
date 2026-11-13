//! What the Continue screen reads from disk: every save of a game across its slots
//! (`saves::slots`, `savecard::read_card`), whether the game is set up here, the fallback
//! picture from the player's own installed copy, and the game last started. Nothing here
//! writes a save; the only file written is the launcher's own `last_game` note.

use crate::screen::{GameData, SaveEntry};
use std::fs;
use std::path::{Path, PathBuf};
use uncharted_core::fleet::{self, DeviceRecord};
use uncharted_core::games::Game;
use uncharted_core::savecard::{self, SaveCard};
use uncharted_core::saves;

/// The note of the game last started, in the launcher's config folder.
pub const LAST_GAME: &str = "last_game";

/// Every save of `game` under `user_dir`, newest first (the lower slot, then the lower name, on
/// a tie): the `SAVEDATA0N*` folders of each slot `saves::slots` lists. A folder that cannot be
/// read is left out.
pub fn saves_of(user_dir: &Path, game: Game) -> Vec<SaveEntry> {
    let prefix = saves::save_prefix(game);
    let mut found = Vec::new();
    for slot in saves::slots(user_dir, game) {
        for dir in saves::saves_in(&saves::slot_dir(user_dir, game, slot)) {
            let name = dir.file_name().unwrap_or_default().to_string_lossy();
            if !name.starts_with(prefix) || name.ends_with(".memory") {
                continue;
            }
            if let Ok(card) = savecard::read_card(game, &dir) {
                found.push(SaveEntry { slot, card });
            }
        }
    }
    found.sort_by(|a, b| {
        b.card
            .saved_unix
            .cmp(&a.card.saved_unix)
            .then(a.slot.cmp(&b.slot))
            .then_with(|| a.card.slot.cmp(&b.card.slot))
    });
    found
}

/// The dump folder of `game`: `BB_GAME_DIR_U1` .. `_U3`, else `BB_GAME_DIR` (the order of
/// `prepare::locate`), read through `env`. An empty value counts as unset.
pub fn dump_dir(game: Game, env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let own = format!("BB_GAME_DIR_{}", game.key().to_ascii_uppercase());
    env(&own)
        .filter(|v| !v.is_empty())
        .or_else(|| env("BB_GAME_DIR").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
}

/// The collection's `sce_sys/icon0.png` from the player's dump, read at run time, when it can be
/// read. Never copied anywhere.
pub fn collection_icon(dump: Option<&Path>) -> Option<Vec<u8>> {
    fs::read(dump?.join("sce_sys").join("icon0.png")).ok()
}

/// What the screen knows of `game`: its saves, its selected slot, whether its install folder
/// exists, and the fallback picture.
pub fn game_data(game: Game, install: &Path, user_dir: &Path, dump: Option<&Path>) -> GameData {
    GameData {
        game,
        saves: saves_of(user_dir, game),
        selected_slot: saves::selected(user_dir, game),
        set_up: install.is_dir(),
        fallback_picture: collection_icon(dump),
        warning: None,
    }
}

/// The record of the device that made `card`, from the fleet records (1.23, 4.04), with the
/// card's own build: `None` when the card names no device or build, or no record matches.
pub fn saved_by(records: &[DeviceRecord], card: &SaveCard) -> Option<DeviceRecord> {
    let device = card.device.as_deref()?;
    let build_id = card.build_id.clone()?;
    records
        .iter()
        .find(|r| r.device == device)
        .map(|r| DeviceRecord {
            build_id,
            ..r.clone()
        })
}

/// The fleet warning for `card` (`fleet::drift`, `fleet::warning`): `None` when this device's
/// record `here` is not known, the card names no device or build, or the builds match.
pub fn warning_for(
    here: Option<&DeviceRecord>,
    records: &[DeviceRecord],
    card: &SaveCard,
) -> Option<String> {
    let here = here?;
    let saved_by = saved_by(records, card)?;
    fleet::warning(&fleet::drift(here, &saved_by), &saved_by)
}

/// The game last started, from `<config>/last_game`; `None` when it is missing or unknown.
pub fn last_started(config: &Path) -> Option<Game> {
    let text = fs::read_to_string(config.join(LAST_GAME)).ok()?;
    Game::from_key(text.trim())
}

/// Note `game` as the one last started.
pub fn remember_started(config: &Path, game: Game) -> std::io::Result<()> {
    fs::create_dir_all(config)?;
    fs::write(config.join(LAST_GAME), game.key())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Duration, UNIX_EPOCH};
    use uncharted_core::platform::Os;

    /// A fresh temp folder, removed on drop (also the other modules' tests).
    pub(crate) struct Temp(pub(crate) PathBuf);

    impl Temp {
        pub(crate) fn new() -> Temp {
            static N: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "uncharted-app-load-test-{}-{}",
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

    /// A synthetic save folder whose only file was modified at `secs`.
    fn save(dir: &Path, secs: u64) {
        fs::create_dir_all(dir).unwrap();
        let file = dir.join("USR-DATA");
        fs::write(&file, b"synthetic").unwrap();
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn saves_across_slots_newest_first() {
        let temp = Temp::new();
        let user = &temp.0;
        save(&user.join("Uncharted 2").join("SAVEDATA02"), 1_700_000_000);
        save(
            &saves::slot_dir(user, Game::U2, 3).join("SAVEDATA02-x"),
            1_700_000_500,
        );
        save(
            &saves::slot_dir(user, Game::U2, 3).join("SAVEDATA02-y"),
            1_700_000_100,
        );
        // Another game's save, metadata and memory folders: not this game's saves.
        save(&user.join("Uncharted 2").join("SAVEDATA03"), 1_800_000_000);
        save(
            &user.join("Uncharted 2").join("SAVEDATA02.sce_sys"),
            1_800_000_000,
        );
        save(
            &user.join("Uncharted 2").join("SAVEDATA02.memory"),
            1_800_000_000,
        );
        let found: Vec<(u8, String, u64)> = saves_of(user, Game::U2)
            .into_iter()
            .map(|e| (e.slot, e.card.slot, e.card.saved_unix))
            .collect();
        assert_eq!(
            found,
            [
                (3, "SAVEDATA02-x".into(), 1_700_000_500),
                (3, "SAVEDATA02-y".into(), 1_700_000_100),
                (1, "SAVEDATA02".into(), 1_700_000_000),
            ]
        );
        assert!(saves_of(user, Game::U1).is_empty());
    }

    #[test]
    fn a_tie_goes_to_the_lower_slot() {
        let temp = Temp::new();
        let user = &temp.0;
        save(
            &saves::slot_dir(user, Game::U1, 2).join("SAVEDATA01"),
            1_700_000_000,
        );
        save(&user.join("Uncharted 1").join("SAVEDATA01"), 1_700_000_000);
        let slots: Vec<u8> = saves_of(user, Game::U1).iter().map(|e| e.slot).collect();
        assert_eq!(slots, [1, 2]);
    }

    #[test]
    fn game_data_reads_set_up_slot_and_icon() {
        let temp = Temp::new();
        let user = temp.0.join("user");
        let install = temp.0.join("install");
        let dump = temp.0.join("dump");
        let data = game_data(Game::U3, &install, &user, Some(&dump));
        assert!(!data.set_up);
        assert_eq!(data.fallback_picture, None);
        assert_eq!(data.selected_slot, 1);
        assert!(data.saves.is_empty());

        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(dump.join("sce_sys")).unwrap();
        fs::write(dump.join("sce_sys").join("icon0.png"), b"synthetic icon").unwrap();
        saves::select(&user, Game::U3, 4).unwrap();
        let data = game_data(Game::U3, &install, &user, Some(&dump));
        assert!(data.set_up);
        assert_eq!(
            data.fallback_picture.as_deref(),
            Some(&b"synthetic icon"[..])
        );
        assert_eq!(data.selected_slot, 4);
        assert_eq!(collection_icon(None), None);
    }

    #[test]
    fn dump_dir_prefers_the_game_variable() {
        let env = |name: &str| match name {
            "BB_GAME_DIR_U2" => Some("/games/u2".to_owned()),
            "BB_GAME_DIR_U3" => Some(String::new()),
            "BB_GAME_DIR" => Some("/games/all".to_owned()),
            _ => None,
        };
        assert_eq!(dump_dir(Game::U2, env), Some(PathBuf::from("/games/u2")));
        assert_eq!(dump_dir(Game::U3, env), Some(PathBuf::from("/games/all")));
        assert_eq!(dump_dir(Game::U1, |_| None), None);
    }

    #[test]
    fn last_started_round_trips() {
        let temp = Temp::new();
        let config = temp.0.join("config");
        assert_eq!(last_started(&config), None);
        remember_started(&config, Game::U2).unwrap();
        assert_eq!(last_started(&config), Some(Game::U2));
        fs::write(config.join(LAST_GAME), "u9").unwrap();
        assert_eq!(last_started(&config), None);
    }

    #[test]
    fn saved_by_takes_the_record_with_the_card_build() {
        let record = |device: &str, build: &str| DeviceRecord {
            device: device.into(),
            os: Os::Linux,
            build_id: build.into(),
            patch_level: 3,
            last_sync_unix: 0,
            free_bytes: 0,
        };
        let records = [record("AMD handheld", "new"), record("desktop", "x")];
        let mut card = SaveCard {
            game: Game::U1,
            slot: "SAVEDATA01".into(),
            picture: None,
            title: None,
            detail: None,
            saved_unix: 1,
            play_time_secs: None,
            device: Some("AMD handheld".into()),
            build_id: Some("old".into()),
        };
        assert_eq!(
            saved_by(&records, &card),
            Some(record("AMD handheld", "old"))
        );
        card.build_id = None;
        assert_eq!(saved_by(&records, &card), None);
        card.build_id = Some("old".into());
        card.device = Some("phone".into());
        assert_eq!(saved_by(&records, &card), None);
        card.device = None;
        assert_eq!(saved_by(&records, &card), None);
        // No record of this device yet: no warning, and the fleet is not asked.
        card.device = Some("AMD handheld".into());
        assert_eq!(warning_for(None, &records, &card), None);
    }
}
