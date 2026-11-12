//! Tests of `saves`. The slot cases mirror `tests/test_save_slots.py`; the folder cases run
//! every OS through `platform::Vars`, so they hold on any host.

use super::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh temp folder under `std::env::temp_dir()`, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new() -> Temp {
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "uncharted-saves-test-{}-{}",
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

fn vars(os: Os) -> Vars {
    let env: HashMap<&str, &str> = match os {
        Os::Linux => [("HOME", "/home/p")].into(),
        Os::Windows => [
            ("USERPROFILE", "C:\\Users\\p"),
            ("LOCALAPPDATA", "C:\\Users\\p\\AppData\\Local"),
            ("APPDATA", "C:\\Users\\p\\AppData\\Roaming"),
        ]
        .into(),
        _ => [("HOME", "/Users/p")].into(),
    };
    Vars::from_lookup(os, |k| env.get(k).map(|v| (*v).to_owned())).unwrap()
}

fn make_save(folder: &Path, prefix: &str, detail: &str) {
    for name in [format!("{prefix}0"), format!("{prefix}P")] {
        fs::create_dir_all(folder.join(&name)).unwrap();
        fs::write(folder.join(&name).join("USR-DATA"), format!("data {name}")).unwrap();
        fs::create_dir_all(folder.join(format!("{name}.sce_sys"))).unwrap();
    }
    let mut param = vec![0u8; 1328];
    param[256..256 + detail.len()].copy_from_slice(detail.as_bytes());
    fs::write(
        folder.join(format!("{prefix}0.sce_sys")).join("param.bin"),
        param,
    )
    .unwrap();
}

const DETAIL: &str = "Campaign 75% Complete - The Monastery\nPlayed for 6 hours";

// ---- folders per OS ----

#[test]
fn save_root_per_os_follows_the_os_data_folder() {
    let install = Path::new("/no/such/install");
    let cases = [
        (Os::Linux, "/home/p/.local/share"),
        (Os::Windows, "C:\\Users\\p\\AppData\\Local"),
        (Os::Mac, "/Users/p/Library/Application Support"),
    ];
    for (os, data) in cases {
        for game in Game::ALL {
            let root = save_root(game, os, &vars(os), install).unwrap();
            let expected = PathBuf::from(data)
                .join("uncharted-universal")
                .join("user")
                .join(game.name());
            assert_eq!(root, expected, "{os:?} {game:?}");
        }
    }
}

#[test]
fn an_existing_install_user_folder_keeps_its_saves() {
    let temp = Temp::new();
    fs::create_dir_all(temp.0.join("user")).unwrap();
    for os in [Os::Linux, Os::Windows, Os::Mac] {
        let root = save_root(Game::U2, os, &vars(os), &temp.0).unwrap();
        assert_eq!(root, temp.0.join("user").join("Uncharted 2"), "{os:?}");
    }
}

#[test]
fn the_apps_keep_saves_inside_the_app() {
    for os in [Os::Ios, Os::Android] {
        let err = save_root(Game::U1, os, &vars(Os::Linux), Path::new("/x")).unwrap_err();
        assert_eq!(
            err,
            Error::Unsupported(format!("saves sync inside the app on {}", os.key()))
        );
    }
}

// ---- split_saves ----

#[test]
fn split_moves_each_game_to_its_folder_once() {
    let temp = Temp::new();
    let title = temp.0.join("savedata/1/CUSA02320");
    for prefix in ["SAVEDATA01", "SAVEDATA02", "SAVEDATA03"] {
        make_save(&title, prefix, "x");
    }
    fs::write(title.join("other.bin"), "keep").unwrap();
    fs::create_dir_all(temp.0.join("savedata/1/CUSA02320.memory/SAVEDATA02X")).unwrap();

    let moved = split_saves(&temp.0).unwrap();
    assert_eq!(moved.len(), 12);
    assert!(moved.contains(&"SAVEDATA020".to_owned()));
    for (game, prefix) in [("Uncharted 1", "SAVEDATA01"), ("Uncharted 2", "SAVEDATA02")] {
        assert_eq!(
            fs::read(temp.0.join(game).join(format!("{prefix}0/USR-DATA"))).unwrap(),
            format!("data {prefix}0").into_bytes()
        );
    }
    assert!(!title.join("SAVEDATA020").exists());
    assert!(title.join("other.bin").is_file());
    assert!(temp
        .0
        .join("savedata/1/CUSA02320.memory/SAVEDATA02X")
        .is_dir());
    assert_eq!(split_saves(&temp.0).unwrap(), Vec::<String>::new());
}

#[test]
fn split_leaves_an_entry_whose_target_exists() {
    let temp = Temp::new();
    let title = temp.0.join("savedata/1/CUSA02320");
    make_save(&title, "SAVEDATA02", "new");
    fs::create_dir_all(temp.0.join("Uncharted 2/SAVEDATA020")).unwrap();
    fs::write(temp.0.join("Uncharted 2/SAVEDATA020/USR-DATA"), "mine").unwrap();
    let moved = split_saves(&temp.0).unwrap();
    assert!(!moved.contains(&"SAVEDATA020".to_owned()));
    assert_eq!(
        fs::read(temp.0.join("Uncharted 2/SAVEDATA020/USR-DATA")).unwrap(),
        b"mine"
    );
    assert!(title.join("SAVEDATA020").is_dir());
}

#[test]
fn split_without_a_savedata_folder_moves_nothing() {
    let temp = Temp::new();
    assert!(split_saves(&temp.0).unwrap().is_empty());
}

// ---- is_synced ----

#[test]
fn only_a_games_own_savedata_travels() {
    assert!(is_synced(Game::U2, "SAVEDATA020/USR-DATA"));
    assert!(is_synced(Game::U2, "SAVEDATA02P/a/b"));
    assert!(is_synced(Game::U2, "SAVEDATA020.sce_sys/param.bin"));
    assert!(is_synced(Game::U3, "SAVEDATA030/USR-DATA"));
    assert!(!is_synced(Game::U1, "SAVEDATA020/USR-DATA"));
    for rel in [
        "where.json",
        "save_slots.json",
        "SAVEDATA020",
        "log.txt",
        "cache/SAVEDATA020/x",
        "SAVEDATA020/x.tmp",
        "SAVEDATA020/.save-relay-meta.json",
        "../SAVEDATA020/x",
        "/SAVEDATA020/x",
        "SAVEDATA020\\x",
        "SAVEDATA020//x",
        "",
    ] {
        assert!(!is_synced(Game::U2, rel), "{rel:?}");
    }
}

// ---- relay_manifest ----

#[test]
fn manifest_is_one_line_save_relay_format_1_from_the_launcher() {
    assert_eq!(
        relay_manifest(Game::U2),
        "{\"format\":1,\"id\":\"uncharted-2\",\"name\":\"Uncharted 2: Among Thieves\",\
         \"saves\":{},\"saves_from\":\"launcher\",\"include\":[\"SAVEDATA02*/**\"],\
         \"exclude\":[\"**/*.tmp\"]}"
    );
    for game in Game::ALL {
        let text = relay_manifest(game);
        assert!(!text.contains('\n'));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["format"], 1);
        assert_eq!(value["saves"], serde_json::json!({}));
        assert_eq!(value["saves_from"], "launcher");
        let id = value["id"].as_str().unwrap();
        assert!(!id.is_empty() && id.len() <= 64);
        assert!(id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'));
        assert!(!id.starts_with('-'));
        assert!(!value["name"].as_str().unwrap().is_empty());
        let include = value["include"].as_array().unwrap();
        assert_eq!(include.len(), 1);
        for pattern in include.iter().chain(value["exclude"].as_array().unwrap()) {
            let p = pattern.as_str().unwrap();
            assert!(!p.starts_with('/') && !p.contains('\\'));
            assert!(!p.split('/').any(|s| s == ".."));
        }
        assert!(text.contains(&format!("\"{}*/**\"", save_prefix(game))));
        assert!(is_synced(game, &format!("{}0/USR-DATA", save_prefix(game))));
    }
    let ids: Vec<String> = Game::ALL.iter().map(|g| relay_manifest(*g)).collect();
    assert_ne!(ids[0], ids[2]);
}

#[test]
fn the_manifest_follows_the_selected_slot() {
    let temp = Temp::new();
    let user = &temp.0;
    assert_eq!(relay_saves_dir(user, Game::U2), user.join("Uncharted 2"));
    select(user, Game::U2, 3).unwrap();
    assert_eq!(
        relay_saves_dir(user, Game::U2),
        user.join("Save slots").join("Uncharted 2").join("Slot 3")
    );
    // Another game keeps its own slot.
    assert_eq!(relay_saves_dir(user, Game::U1), user.join("Uncharted 1"));
    // A save made in slot 3 is the same shape relative to the folder save-relay is given.
    make_save(&relay_saves_dir(user, Game::U2), "SAVEDATA02", "x");
    assert!(is_synced(Game::U2, "SAVEDATA020/USR-DATA"));
    select(user, Game::U2, 1).unwrap();
    assert_eq!(relay_saves_dir(user, Game::U2), user.join("Uncharted 2"));
}

// ---- save slots (tests/test_save_slots.py) ----

#[test]
fn without_slots_every_game_uses_its_folder_as_before() {
    let temp = Temp::new();
    assert_eq!(
        mapping(&temp.0),
        "SAVEDATA01=Uncharted 1;SAVEDATA02=Uncharted 2;SAVEDATA03=Uncharted 3"
    );
    assert_eq!(slots(&temp.0, Game::U2), vec![1]);
}

#[test]
fn existing_saves_are_slot_one_with_their_description() {
    let temp = Temp::new();
    make_save(&temp.0.join("Uncharted 2"), "SAVEDATA02", DETAIL);
    let (text, when) = details(&temp.0, Game::U2, 1);
    assert_eq!(
        text,
        "Campaign 75% Complete - The Monastery · Played for 6 hours"
    );
    assert!(when.is_some());
    assert_eq!(details(&temp.0, Game::U2, 2), (String::new(), None));
}

#[test]
fn a_selected_slot_is_what_the_game_sees() {
    let temp = Temp::new();
    select(&temp.0, Game::U2, 3).unwrap();
    let map = mapping(&temp.0);
    assert!(map.contains("SAVEDATA02=Save slots/Uncharted 2/Slot 3"));
    assert!(map.contains("SAVEDATA01=Uncharted 1"));
    assert_eq!(slots(&temp.0, Game::U2), vec![1, 3]);
}

#[test]
fn copy_makes_an_independent_slot() {
    let temp = Temp::new();
    make_save(&temp.0.join("Uncharted 2"), "SAVEDATA02", DETAIL);
    assert_eq!(first_free(&temp.0, Game::U2), Some(2));
    copy(&temp.0, Game::U2, 1, 2).unwrap();
    let copied = temp.0.join("Save slots/Uncharted 2/Slot 2");
    assert_eq!(
        fs::read(copied.join("SAVEDATA020/USR-DATA")).unwrap(),
        b"data SAVEDATA020"
    );
    assert!(copied.join("SAVEDATA020.sce_sys/param.bin").is_file());
    fs::write(copied.join("SAVEDATA020/USR-DATA"), "changed").unwrap();
    assert_eq!(
        fs::read(temp.0.join("Uncharted 2/SAVEDATA020/USR-DATA")).unwrap(),
        b"data SAVEDATA020"
    );
    assert_eq!(slots(&temp.0, Game::U2), vec![1, 2]);
}

#[test]
fn copy_never_replaces_saves_in_the_target() {
    let temp = Temp::new();
    make_save(&temp.0.join("Uncharted 2"), "SAVEDATA02", "first");
    make_save(
        &temp.0.join("Save slots/Uncharted 2/Slot 2"),
        "SAVEDATA02",
        "second",
    );
    assert!(matches!(
        copy(&temp.0, Game::U2, 1, 2),
        Err(Error::Saves(_))
    ));
    assert!(matches!(
        copy(&temp.0, Game::U2, 1, 1),
        Err(Error::Saves(_))
    ));
    assert_eq!(details(&temp.0, Game::U2, 2).0, "second");
    assert_eq!(details(&temp.0, Game::U2, 1).0, "first");
    assert!(!temp
        .0
        .join("Save slots/Uncharted 2/Slot 2.copying")
        .exists());
}

fn snapshot(folder: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(base: &Path, folder: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path.strip_prefix(base).unwrap().to_owned();
                out.push((rel, fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(folder, folder, &mut out);
    out.sort();
    out
}

#[test]
fn switching_slots_keeps_every_save() {
    let temp = Temp::new();
    let game_folder = temp.0.join("Uncharted 2");
    make_save(&game_folder, "SAVEDATA02", "first");
    let before = snapshot(&game_folder);
    let free = first_free(&temp.0, Game::U2).unwrap();
    select(&temp.0, Game::U2, free).unwrap(); // New slot
    assert!(mapping(&temp.0).contains(&format!("SAVEDATA02=Save slots/Uncharted 2/Slot {free}")));
    let next = first_free(&temp.0, Game::U2).unwrap();
    copy(&temp.0, Game::U2, 1, next).unwrap();
    select(&temp.0, Game::U2, 1).unwrap(); // Use slot 1 again
    assert_eq!(snapshot(&game_folder), before);
    assert_eq!(
        mapping(&temp.0).split(';').nth(1),
        Some("SAVEDATA02=Uncharted 2")
    );
}

#[test]
fn deleting_slot_one_keeps_the_game_folder_and_other_files() {
    let temp = Temp::new();
    make_save(&temp.0.join("Uncharted 2"), "SAVEDATA02", DETAIL);
    fs::write(temp.0.join("Uncharted 2/notes.txt"), "mine").unwrap();
    delete(&temp.0, Game::U2, 1).unwrap();
    assert!(saves_in(&temp.0.join("Uncharted 2")).is_empty());
    assert!(temp.0.join("Uncharted 2/notes.txt").is_file());
}

#[test]
fn deleting_the_selected_slot_goes_back_to_slot_one() {
    let temp = Temp::new();
    make_save(
        &temp.0.join("Save slots/Uncharted 3/Slot 4"),
        "SAVEDATA03",
        "x",
    );
    select(&temp.0, Game::U3, 4).unwrap();
    rename(&temp.0, Game::U3, 4, "Crushing").unwrap();
    assert_eq!(name(&temp.0, Game::U3, 4), "Crushing");
    delete(&temp.0, Game::U3, 4).unwrap();
    assert_eq!(selected(&temp.0, Game::U3), 1);
    assert_eq!(name(&temp.0, Game::U3, 4), "");
    assert!(!temp.0.join("Save slots/Uncharted 3/Slot 4").exists());
}

#[test]
fn names_and_a_broken_state_file() {
    let temp = Temp::new();
    rename(&temp.0, Game::U1, 2, "  Main run  ").unwrap();
    assert_eq!(name(&temp.0, Game::U1, 2), "Main run");
    rename(&temp.0, Game::U1, 3, &"x".repeat(60)).unwrap();
    assert_eq!(name(&temp.0, Game::U1, 3).len(), 40);
    fs::write(temp.0.join(STATE), "{not json").unwrap();
    assert_eq!(selected(&temp.0, Game::U1), 1);
    assert_eq!(name(&temp.0, Game::U1, 2), "");
    // Wrong shapes in a valid file read as defaults too.
    fs::write(
        temp.0.join(STATE),
        r#"{"Uncharted 1": {"selected": 99}, "Uncharted 2": 5}"#,
    )
    .unwrap();
    assert_eq!(selected(&temp.0, Game::U1), 1);
    assert_eq!(selected(&temp.0, Game::U2), 1);
    select(&temp.0, Game::U2, 2).unwrap();
    assert_eq!(selected(&temp.0, Game::U2), 2);
}

#[test]
fn no_free_slot_once_all_are_used() {
    let temp = Temp::new();
    for n in 2..=MOST {
        make_save(
            &temp.0.join(format!("Save slots/Uncharted 1/Slot {n}")),
            "SAVEDATA01",
            "x",
        );
    }
    assert_eq!(first_free(&temp.0, Game::U1), None);
}

#[test]
fn slot_numbers_outside_one_to_ten_are_refused() {
    let temp = Temp::new();
    assert!(matches!(select(&temp.0, Game::U1, 0), Err(Error::Saves(_))));
    assert!(matches!(
        select(&temp.0, Game::U1, 11),
        Err(Error::Saves(_))
    ));
}
