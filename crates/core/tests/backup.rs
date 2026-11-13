//! `backup`: save backups before an update (SPEC.md M19, issue #286). One test per sentence of
//! the module's contract; the cases are ported from iannitram/totk-native's `tests/backup.rs`
//! (MIT) with Uncharted savedata names. Everything runs in temp folders under
//! `std::env::temp_dir()`; no game data.

use std::fs;
use std::path::{Path, PathBuf};
use uncharted_core::backup::{backup_saves, list, prune, KEEP};
use uncharted_core::games::Game;
use uncharted_core::Error;

/// A fresh temp folder under `std::env::temp_dir()`, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let path = std::env::temp_dir().join(format!(
            "uncharted-backup-test-{name}-{}",
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

/// Every file under a folder, relative with `/`, sorted.
fn files(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap();
                let parts: Vec<_> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push(parts.join("/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn backup_copies_exactly_the_synced_files_at_their_paths() {
    let t = Temp::new("copy");
    let (saves, backups) = (t.0.join("Uncharted 2"), t.0.join("backups"));
    put(&saves, "SAVEDATA020/USR-DATA", "u0");
    put(&saves, "SAVEDATA02P/sub/USR-DATA", "up");
    put(&saves, "SAVEDATA020.sce_sys/param.bin", "param");
    // None of these travel, so none is backed up.
    put(&saves, "where.json", "{}");
    put(&saves, "save_slots.json", "{}");
    put(&saves, "SAVEDATA020/write.tmp", "temp");
    put(&saves, "SAVEDATA020/.save-relay-state", "relay");
    put(&saves, "SAVEDATA030/USR-DATA", "other game");
    put(&saves, "logs/run.log", "log");
    fs::create_dir_all(saves.join("SAVEDATA02E")).unwrap();

    let made = backup_saves(Game::U2, &saves, &backups, "20261112T120000Z").unwrap();

    assert_eq!(made, backups.join("20261112T120000Z"));
    assert_eq!(
        files(&made),
        [
            "SAVEDATA020.sce_sys/param.bin",
            "SAVEDATA020/USR-DATA",
            "SAVEDATA02P/sub/USR-DATA",
        ]
    );
    assert_eq!(read(&made.join("SAVEDATA020/USR-DATA")), "u0");
    assert_eq!(read(&made.join("SAVEDATA02P/sub/USR-DATA")), "up");
    assert_eq!(read(&made.join("SAVEDATA020.sce_sys/param.bin")), "param");
    // The save root is left as it was.
    assert_eq!(read(&saves.join("where.json")), "{}");
    assert_eq!(files(&saves).len(), 9);
}

#[test]
fn backup_into_an_existing_stamp_is_refused_and_writes_nothing() {
    let t = Temp::new("exists");
    let (saves, backups) = (t.0.join("Uncharted 1"), t.0.join("backups"));
    put(&saves, "SAVEDATA010/USR-DATA", "new");
    put(&backups, "S1/SAVEDATA010/USR-DATA", "old");

    assert_eq!(
        backup_saves(Game::U1, &saves, &backups, "S1"),
        Err(Error::Saves("backup S1 exists".into()))
    );
    assert_eq!(files(&backups), ["S1/SAVEDATA010/USR-DATA"]);
    assert_eq!(read(&backups.join("S1/SAVEDATA010/USR-DATA")), "old");
}

#[test]
fn backup_of_a_missing_save_root_is_refused() {
    let t = Temp::new("missing");
    let (saves, backups) = (t.0.join("nope"), t.0.join("backups"));

    assert_eq!(
        backup_saves(Game::U3, &saves, &backups, "S1"),
        Err(Error::Saves(format!(
            "no save folder at {}",
            saves.display()
        )))
    );
    assert!(!backups.exists());
}

#[test]
fn list_is_oldest_first() {
    let t = Temp::new("list");
    let backups = t.0.join("backups");
    for stamp in ["20261031T120000Z", "20261030T090000Z", "20261101T000000Z"] {
        fs::create_dir_all(backups.join(stamp)).unwrap();
    }
    // A stray file is not a backup.
    put(&backups, "notes.txt", "x");

    assert_eq!(
        list(&backups).unwrap(),
        ["20261030T090000Z", "20261031T120000Z", "20261101T000000Z"]
    );
}

#[test]
fn list_of_a_missing_folder_is_empty() {
    let t = Temp::new("list-missing");
    assert_eq!(list(&t.0.join("backups")), Ok(Vec::new()));
}

#[test]
fn prune_deletes_the_oldest_so_keep_remain() {
    let t = Temp::new("prune");
    let backups = t.0.join("backups");
    for stamp in ["A3", "A1", "A4", "A2"] {
        put(&backups, &format!("{stamp}/SAVEDATA020/USR-DATA"), stamp);
    }

    assert_eq!(prune(&backups, 2).unwrap(), ["A1", "A2"]);
    assert_eq!(list(&backups).unwrap(), ["A3", "A4"]);
    assert_eq!(read(&backups.join("A4/SAVEDATA020/USR-DATA")), "A4");
    assert_eq!(prune(&backups, 5).unwrap(), Vec::<String>::new());
    assert_eq!(list(&backups).unwrap(), ["A3", "A4"]);
}

#[test]
fn prune_to_keep_leaves_the_newest_ten() {
    let t = Temp::new("prune-keep");
    let backups = t.0.join("backups");
    for n in 0..12 {
        fs::create_dir_all(backups.join(format!("2026111{n:02}"))).unwrap();
    }

    assert_eq!(KEEP, 10);
    assert_eq!(prune(&backups, KEEP).unwrap(), ["202611100", "202611101"]);
    assert_eq!(list(&backups).unwrap().len(), KEEP);
}
