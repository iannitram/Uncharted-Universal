//! The files behind the Saves and Settings pages (roadmap 2.06): reading a game's slots and
//! backups, carrying out a `SavesCommand`, and the tune: the hardware probe
//! (`uncharted-run --probe`, #171), the benchmark reports (#173) and the cached probe report in
//! `<config>/tune.json`. The view models (`src/vm/`) never touch a file; this module does.

use crate::vm::saves::{self, GameSaves, SavesCommand, SlotInfo};
use serde_json::Value;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use uncharted_core::backup;
use uncharted_core::games::Game;
use uncharted_core::platform::Os;
use uncharted_core::profile::{self, DeviceProfile};
use uncharted_core::saves as core_saves;
use uncharted_core::tune::{self, Bench, HardwareReport};

/// A game's backups: `<data>/backups/<game key>/<stamp>`.
pub fn backups_dir(backups: &Path, game: Game) -> PathBuf {
    backups.join(game.key())
}

/// What the Saves page shows of `game`.
pub fn game_saves(game: Game, user_dir: &Path, backups: &Path) -> GameSaves {
    let slots = core_saves::slots(user_dir, game)
        .into_iter()
        .map(|slot| {
            let (detail, saved) = core_saves::details(user_dir, game, slot);
            SlotInfo {
                slot,
                name: core_saves::name(user_dir, game, slot),
                used: core_saves::used(user_dir, game, slot),
                detail,
                saved_unix: saved
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs()),
            }
        })
        .collect();
    GameSaves {
        game,
        slots,
        selected: core_saves::selected(user_dir, game),
        free: core_saves::first_free(user_dir, game),
        backups: backup::list(&backups_dir(backups, game)).unwrap_or_default(),
    }
}

/// Back the slot's saves up into a new stamped folder, then keep the newest `backup::KEEP`.
/// Returns the stamp.
pub fn back_up(
    game: Game,
    user_dir: &Path,
    backups: &Path,
    slot: u8,
    now: u64,
) -> Result<String, String> {
    let dir = backups_dir(backups, game);
    let stamp = saves::stamp(now, slot);
    backup::backup_saves(
        game,
        &core_saves::slot_dir(user_dir, game, slot),
        &dir,
        &stamp,
    )
    .map_err(|e| e.to_string())?;
    backup::prune(&dir, backup::KEEP).map_err(|e| e.to_string())?;
    Ok(stamp)
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map(|_| ())
    }
}

/// Copy a backup's save entries into the empty slot `to`. A slot with saves is never written:
/// a restore, like a copy, never replaces saves.
pub fn restore(
    game: Game,
    user_dir: &Path,
    backups: &Path,
    stamp: &str,
    to: u8,
) -> Result<(), String> {
    let folder = backups_dir(backups, game).join(stamp);
    if stamp.is_empty() || stamp.contains(['/', '\\']) || stamp.starts_with('.') || !folder.is_dir()
    {
        return Err(format!("no backup {stamp}"));
    }
    if core_saves::used(user_dir, game, to) {
        return Err(format!("{}: slot {to} is not empty", game.name()));
    }
    let target = core_saves::slot_dir(user_dir, game, to);
    let prefix = core_saves::save_prefix(game);
    let entries = fs::read_dir(&folder).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_name().to_string_lossy().starts_with(prefix) {
            copy_tree(&entry.path(), &target.join(entry.file_name())).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Carry out a command of the Saves page on the disk; the status line it leaves, or why not.
/// Delete backs the slot up first and deletes nothing when that fails.
pub fn run(
    command: &SavesCommand,
    user_dir: &Path,
    backups: &Path,
    now: u64,
) -> Result<String, String> {
    let err = |e: uncharted_core::error::Error| e.to_string();
    match command {
        SavesCommand::None | SavesCommand::Back => Ok(String::new()),
        SavesCommand::Select { game, slot } => {
            core_saves::select(user_dir, *game, *slot).map_err(err)?;
            Ok(format!(
                "Slot {slot} is the active one: {} loads it and save-relay syncs it.",
                game.name()
            ))
        }
        SavesCommand::Copy { game, from, to } => {
            core_saves::copy(user_dir, *game, *from, *to).map_err(err)?;
            Ok(format!("Copied slot {from} to slot {to}."))
        }
        SavesCommand::BackUp { game, slot } => {
            back_up(*game, user_dir, backups, *slot, now)?;
            Ok(format!("Backed up slot {slot}."))
        }
        SavesCommand::Delete { game, slot } => {
            back_up(*game, user_dir, backups, *slot, now)
                .map_err(|e| format!("Nothing deleted: the backup failed: {e}"))?;
            core_saves::delete(user_dir, *game, *slot).map_err(err)?;
            Ok(format!("Deleted slot {slot}. Its backup is in the list."))
        }
        SavesCommand::Restore { game, stamp, to } => {
            restore(*game, user_dir, backups, stamp, *to)?;
            Ok(format!("Restored the backup into slot {to}."))
        }
    }
}

// ---- the tune ----

/// The cached probe report in the config folder.
pub const TUNE_CACHE: &str = "tune.json";

/// `uncharted-run` beside this program.
pub fn probe_path() -> PathBuf {
    let file = format!("uncharted-run{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&file)))
        .unwrap_or_else(|| PathBuf::from(file))
}

/// Run `uncharted-run --probe` and return its report (JSON text), or why not.
pub fn run_probe() -> Result<String, String> {
    let out = std::process::Command::new(probe_path())
        .arg("--probe")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("could not start {}: {e}", probe_path().display()))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr)
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| "the hardware probe failed".to_owned());
        return Err(why);
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The benchmark reports (`benchmark.json`) in these save folders; a missing or damaged one is
/// left out.
pub fn read_reports(folders: &[PathBuf]) -> Vec<Value> {
    folders
        .iter()
        .filter_map(|folder| fs::read_to_string(folder.join("benchmark.json")).ok())
        .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .collect()
}

/// The report and benchmarks from a probe's JSON text and the reports.
pub fn tune_from(probe: &str, reports: &[Value]) -> Result<(HardwareReport, Bench), String> {
    let report = tune::parse_probe(probe).map_err(|e| e.to_string())?;
    let bench = crate::vm::settings::bench_from_reports(&report, reports);
    Ok((report, bench))
}

/// The probe text of the last tune, if any.
pub fn load_cache(config: &Path) -> Option<String> {
    let text = fs::read_to_string(config.join(TUNE_CACHE)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value["probe"].as_str().map(str::to_owned)
}

pub fn save_cache(config: &Path, probe: &str) -> io::Result<()> {
    fs::create_dir_all(config)?;
    let text = serde_json::json!({ "probe": probe }).to_string();
    let temp = config.join("tune.json.tmp");
    fs::write(&temp, text)?;
    fs::rename(&temp, config.join(TUNE_CACHE))
}

/// This device's profile: on Linux from the CPU model (`/proc/cpuinfo`) and `/etc/os-release`,
/// elsewhere from the OS.
pub fn detected_profile(os: Os) -> &'static DeviceProfile {
    if os != Os::Linux {
        return profile::detect(os, "");
    }
    let cpu = fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, model)| model.trim().to_owned())
        })
        .unwrap_or_default();
    let release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    profile::detect_with_os_release(os, &cpu, &release)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::tests::Temp;

    /// A synthetic save entry in a slot.
    fn put_save(user_dir: &Path, game: Game, slot: u8, entry: &str) {
        let dir = core_saves::slot_dir(user_dir, game, slot).join(entry);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("USR-DATA"), b"synthetic").unwrap();
    }

    #[test]
    fn delete_backs_up_then_restore_brings_it_back() {
        let temp = Temp::new();
        let (user, backups) = (temp.0.join("user"), temp.0.join("backups"));
        let game = Game::U2;
        let entry = format!("{}01", core_saves::save_prefix(game));
        put_save(&user, game, 2, &entry);
        core_saves::select(&user, game, 2).unwrap();
        // A file that never syncs stays out of the backup.
        fs::write(
            core_saves::slot_dir(&user, game, 2).join("where.json"),
            "{}",
        )
        .unwrap();

        let data = game_saves(game, &user, &backups);
        assert_eq!(data.selected, 2);
        assert_eq!(data.free, Some(3));
        assert!(data
            .slots
            .iter()
            .any(|s| s.slot == 2 && s.used && s.saved_unix > 0));

        let now = 1_794_519_000;
        let status = run(
            &SavesCommand::Delete { game, slot: 2 },
            &user,
            &backups,
            now,
        )
        .unwrap();
        assert!(status.starts_with("Deleted slot 2"));
        assert!(!core_saves::used(&user, game, 2));
        // Deleting the active slot makes slot 1 the active one.
        assert_eq!(core_saves::selected(&user, game), 1);

        let data = game_saves(game, &user, &backups);
        assert_eq!(data.backups, [saves::stamp(now, 2)]);
        let folder = backups_dir(&backups, game).join(&data.backups[0]);
        assert!(folder.join(&entry).join("USR-DATA").is_file());
        assert!(!folder.join("where.json").exists());

        let to = data.free.unwrap();
        let restore_cmd = SavesCommand::Restore {
            game,
            stamp: data.backups[0].clone(),
            to,
        };
        run(&restore_cmd, &user, &backups, now).unwrap();
        assert!(core_saves::used(&user, game, to));
        assert_eq!(
            fs::read(
                core_saves::slot_dir(&user, game, to)
                    .join(&entry)
                    .join("USR-DATA")
            )
            .unwrap(),
            b"synthetic"
        );
        // Never over saves.
        assert!(run(&restore_cmd, &user, &backups, now)
            .unwrap_err()
            .contains("not empty"));
    }

    #[test]
    fn a_failed_backup_deletes_nothing() {
        let temp = Temp::new();
        let (user, backups) = (temp.0.join("user"), temp.0.join("backups"));
        let game = Game::U1;
        put_save(
            &user,
            game,
            3,
            &format!("{}00", core_saves::save_prefix(game)),
        );
        let now = 1_794_519_000;
        // The stamp's folder is already there: the backup refuses.
        fs::create_dir_all(backups_dir(&backups, game).join(saves::stamp(now, 3))).unwrap();
        let err = run(
            &SavesCommand::Delete { game, slot: 3 },
            &user,
            &backups,
            now,
        )
        .unwrap_err();
        assert!(err.starts_with("Nothing deleted"));
        assert!(core_saves::used(&user, game, 3));
    }

    #[test]
    fn backups_keep_the_newest_ten() {
        let temp = Temp::new();
        let (user, backups) = (temp.0.join("user"), temp.0.join("backups"));
        let game = Game::U3;
        put_save(
            &user,
            game,
            1,
            &format!("{}00", core_saves::save_prefix(game)),
        );
        for i in 0..12 {
            back_up(game, &user, &backups, 1, 1_794_519_000 + i).unwrap();
        }
        let list = backup::list(&backups_dir(&backups, game)).unwrap();
        assert_eq!(list.len(), backup::KEEP);
        assert_eq!(list[0], saves::stamp(1_794_519_002, 1));
    }

    #[test]
    fn a_restore_names_a_backup_folder_only() {
        let temp = Temp::new();
        let (user, backups) = (temp.0.join("user"), temp.0.join("backups"));
        for stamp in ["", "..", "../x", "missing"] {
            assert!(
                restore(Game::U1, &user, &backups, stamp, 2).is_err(),
                "{stamp}"
            );
        }
    }

    #[test]
    fn copy_and_select_through_run() {
        let temp = Temp::new();
        let (user, backups) = (temp.0.join("user"), temp.0.join("backups"));
        let game = Game::U2;
        put_save(
            &user,
            game,
            1,
            &format!("{}00", core_saves::save_prefix(game)),
        );
        run(
            &SavesCommand::Copy {
                game,
                from: 1,
                to: 2,
            },
            &user,
            &backups,
            0,
        )
        .unwrap();
        assert!(core_saves::used(&user, game, 2));
        let status = run(&SavesCommand::Select { game, slot: 2 }, &user, &backups, 0).unwrap();
        assert!(status.starts_with("Slot 2 is the active one"));
        assert_eq!(core_saves::selected(&user, game), 2);
        // A copy onto saves is refused.
        assert!(run(
            &SavesCommand::Copy {
                game,
                from: 1,
                to: 2
            },
            &user,
            &backups,
            0
        )
        .is_err());
    }

    #[test]
    fn tune_cache_and_reports() {
        let temp = Temp::new();
        assert_eq!(load_cache(&temp.0), None);
        save_cache(&temp.0, "{\"probe\": 1}").unwrap();
        assert_eq!(load_cache(&temp.0).as_deref(), Some("{\"probe\": 1}"));

        let (a, b, c) = (temp.0.join("a"), temp.0.join("b"), temp.0.join("c"));
        for dir in [&a, &b, &c] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(
            a.join("benchmark.json"),
            r#"{"ended": "ok", "avg_fps": 61.0}"#,
        )
        .unwrap();
        fs::write(b.join("benchmark.json"), "damaged").unwrap();
        let reports = read_reports(&[a, b, c]);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0]["avg_fps"], 61.0);
        assert!(tune_from("not json", &reports).is_err());
    }
}
