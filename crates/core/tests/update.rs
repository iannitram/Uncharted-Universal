//! `update`: back the saves up, re-prepare, recount the saves (SPEC.md M19, issue #290). One
//! test per sentence of the module's contract (the shape of iannitram/totk-native's
//! `update.rs`, MIT), with Uncharted savedata names. `tests/test_run_updates.py` covers
//! `run.py`'s title-update overlay (`merge_update`), not this flow, so it has no case to port
//! here. Everything runs in temp folders under `std::env::temp_dir()`; no game data.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use uncharted_core::backup::{list, KEEP};
use uncharted_core::games::Game;
use uncharted_core::update::{update, Updated};
use uncharted_core::Error;

/// A fresh temp folder under `std::env::temp_dir()`, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let path = std::env::temp_dir().join(format!(
            "uncharted-update-test-{name}-{}",
            std::process::id()
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

fn put(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

/// A save root for Uncharted 2 with three synced files and four that never sync.
fn saves_of_u2(root: &Path) -> PathBuf {
    let saves = root.join("Uncharted 2");
    put(&saves, "SAVEDATA020/USR-DATA", "u0");
    put(&saves, "SAVEDATA021/USR-DATA", "u1");
    put(&saves, "SAVEDATA020.sce_sys/param.bin", "param");
    put(&saves, "where.json", "{}");
    put(&saves, "save_slots.json", "{}");
    put(&saves, "SAVEDATA020/write.tmp", "temp");
    put(&saves, "SAVEDATA030/USR-DATA", "other game");
    saves
}

/// `count` older backups, named so they sort before any `2026...` stamp.
fn old_backups(backups: &Path, count: usize) {
    for n in 0..count {
        put(backups, &format!("2025{n:04}/SAVEDATA020/USR-DATA"), "old");
    }
}

#[test]
fn update_backs_up_rebuilds_and_counts_the_synced_files() {
    let t = Temp::new("ok");
    let (saves, backups) = (saves_of_u2(&t.0), t.0.join("backups"));
    let runs = Cell::new(0);

    let done = update(Game::U2, &saves, &backups, "20261112T120000Z", &mut || {
        runs.set(runs.get() + 1);
        Ok(())
    })
    .unwrap();

    assert_eq!(runs.get(), 1);
    assert_eq!(
        done,
        Updated {
            backup: backups.join("20261112T120000Z"),
            files_before: 3,
            files_after: 3,
        }
    );
    assert_eq!(read(&done.backup.join("SAVEDATA021/USR-DATA")), "u1");
    assert!(!done.backup.join("where.json").exists());
}

#[test]
fn the_backup_is_made_before_the_rebuild_runs() {
    let t = Temp::new("order");
    let (saves, backups) = (saves_of_u2(&t.0), t.0.join("backups"));
    let folder = backups.join("S1");
    let seen = Cell::new(false);

    update(Game::U2, &saves, &backups, "S1", &mut || {
        seen.set(read(&folder.join("SAVEDATA020/USR-DATA")) == "u0");
        Ok(())
    })
    .unwrap();

    assert!(seen.get());
}

#[test]
fn a_rebuild_error_is_returned_after_the_backup_and_the_backup_stays() {
    let t = Temp::new("rebuild-error");
    let (saves, backups) = (saves_of_u2(&t.0), t.0.join("backups"));
    old_backups(&backups, KEEP + 2);

    let result = update(Game::U2, &saves, &backups, "S1", &mut || {
        Err(Error::Prepare("boot image failed".into()))
    });

    assert_eq!(result, Err(Error::Prepare("boot image failed".into())));
    assert_eq!(read(&backups.join("S1/SAVEDATA020/USR-DATA")), "u0");
    // Nothing is pruned when the update fails.
    assert_eq!(list(&backups).unwrap().len(), KEEP + 3);
}

#[test]
fn fewer_synced_files_after_is_a_saves_error_and_nothing_is_deleted() {
    let t = Temp::new("lost");
    let (saves, backups) = (saves_of_u2(&t.0), t.0.join("backups"));
    old_backups(&backups, KEEP + 2);
    let gone = saves.join("SAVEDATA021");

    let result = update(Game::U2, &saves, &backups, "S1", &mut || {
        fs::remove_dir_all(&gone).unwrap();
        Ok(())
    });

    let folder = backups.join("S1");
    assert_eq!(
        result,
        Err(Error::Saves(format!(
            "saves changed during the update; restore {}",
            folder.display()
        )))
    );
    assert_eq!(read(&folder.join("SAVEDATA021/USR-DATA")), "u1");
    assert_eq!(list(&backups).unwrap().len(), KEEP + 3);
    assert_eq!(read(&saves.join("SAVEDATA020/USR-DATA")), "u0");
}

#[test]
fn a_save_root_gone_after_the_rebuild_counts_as_no_saves() {
    let t = Temp::new("root-gone");
    let (saves, backups) = (saves_of_u2(&t.0), t.0.join("backups"));
    let root = saves.clone();

    let result = update(Game::U2, &saves, &backups, "S1", &mut || {
        fs::remove_dir_all(&root).unwrap();
        Ok(())
    });

    assert_eq!(
        result,
        Err(Error::Saves(format!(
            "saves changed during the update; restore {}",
            backups.join("S1").display()
        )))
    );
    assert_eq!(read(&backups.join("S1/SAVEDATA020/USR-DATA")), "u0");
}

#[test]
fn only_synced_files_are_counted() {
    let t = Temp::new("counted");
    let (saves, backups) = (saves_of_u2(&t.0), t.0.join("backups"));
    let root = saves.clone();

    // Losing files that never sync, or another game's saves, is not a lost save; a new save is
    // fine.
    let done = update(Game::U2, &saves, &backups, "S1", &mut || {
        fs::remove_file(root.join("where.json")).unwrap();
        fs::remove_file(root.join("SAVEDATA020/write.tmp")).unwrap();
        fs::remove_dir_all(root.join("SAVEDATA030")).unwrap();
        put(&root, "SAVEDATA022/USR-DATA", "u2");
        Ok(())
    })
    .unwrap();

    assert_eq!((done.files_before, done.files_after), (3, 4));
}

#[test]
fn a_successful_update_keeps_the_newest_ten_backups() {
    let t = Temp::new("keep");
    let (saves, backups) = (saves_of_u2(&t.0), t.0.join("backups"));
    old_backups(&backups, KEEP + 2);

    update(Game::U2, &saves, &backups, "20261112T120000Z", &mut || {
        Ok(())
    })
    .unwrap();

    let kept = list(&backups).unwrap();
    assert_eq!(kept.len(), KEEP);
    assert_eq!(kept.first().unwrap(), "20250003");
    assert_eq!(kept.last().unwrap(), "20261112T120000Z");
}

#[test]
fn a_refused_backup_never_runs_the_rebuild() {
    let t = Temp::new("refused");
    let (saves, backups) = (saves_of_u2(&t.0), t.0.join("backups"));
    put(&backups, "S1/SAVEDATA020/USR-DATA", "old");
    let runs = Cell::new(0);

    let exists = update(Game::U2, &saves, &backups, "S1", &mut || {
        runs.set(runs.get() + 1);
        Ok(())
    });
    let missing = t.0.join("nope");
    let no_root = update(Game::U1, &missing, &backups, "S2", &mut || {
        runs.set(runs.get() + 1);
        Ok(())
    });

    assert_eq!(exists, Err(Error::Saves("backup S1 exists".into())));
    assert_eq!(
        no_root,
        Err(Error::Saves(format!(
            "no save folder at {}",
            missing.display()
        )))
    );
    assert_eq!(runs.get(), 0);
    assert_eq!(read(&backups.join("S1/SAVEDATA020/USR-DATA")), "old");
}
