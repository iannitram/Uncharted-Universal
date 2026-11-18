//! The behaviour of `src/runtime_savedata.c`, case by case: names, mounts and their errors,
//! parameters, the `BB_SAVEDATA_DIRS` mapping, searches, picks, SaveDataMemory and the
//! `where.json` commit on close.

use super::store::*;
use std::fs;
use std::path::{Path, PathBuf};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("uu-savestore-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn store(&self, spec: Option<&str>) -> Store {
        let mut store = Store::new();
        store.initialize();
        store.vfs = false;
        store.title_id = "CUSA00000".into();
        store.user = self.0.to_str().unwrap().into();
        store.spec = spec.map(str::to_owned);
        store
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn name(text: &str) -> [u8; 32] {
    let mut raw = [0u8; 32];
    raw[..text.len()].copy_from_slice(text.as_bytes());
    raw
}

fn point(text: &str) -> Vec<u8> {
    let mut raw = text.as_bytes().to_vec();
    raw.resize(16, 0);
    raw
}

#[test]
fn names_are_checked_like_the_c_module() {
    assert_eq!(
        valid_name(&name("SAVEDATA01P"), 32).as_deref(),
        Some("SAVEDATA01P")
    );
    assert_eq!(valid_name(&name(""), 32), None);
    assert_eq!(valid_name(&[b'a'; 32], 32), None); // no NUL within the 32 bytes
    for bad in ["a/b", "a\\b", ".hidden", "a..b", ".."] {
        assert_eq!(valid_name(&name(bad), 32), None, "{bad}");
    }
    assert_eq!(valid_name(&name("a.b"), 32).as_deref(), Some("a.b"));
    assert!(like(b"SAVEDATA01P", b"SAVEDATA%"));
    assert!(like(b"SAVEDATA01P", b"SAVEDATA0_P"));
    assert!(!like(b"SAVEDATA01P", b"SAVEDATA02%"));
    assert!(like(b"", b"%"));
    assert!(!like(b"a", b""));
}

#[test]
fn mapping_keeps_each_prefix_in_its_folder() {
    let dir = TempDir::new("map");
    let store = dir.store(Some("SAVEDATA01=Uncharted 1;SAVEDATA02=Uncharted 2"));
    let user = &store.user;
    assert_eq!(
        store.mapped_folder(Some("SAVEDATA01P"), 0),
        Some(format!("{user}/Uncharted 1"))
    );
    assert_eq!(
        store.mapped_folder(Some("SAVEDATA02"), 0),
        Some(format!("{user}/Uncharted 2"))
    );
    assert_eq!(store.mapped_folder(Some("SPRJ0005"), 0), None);
    assert_eq!(
        store.mapped_folder(None, 1),
        Some(format!("{user}/Uncharted 2"))
    );
    assert_eq!(store.mapped_folder(None, 2), None);
    assert_eq!(store.root(1, None), format!("{user}/savedata/1/CUSA00000"));
    assert_eq!(
        store.root(1, Some("CUSA11111")),
        format!("{user}/savedata/1/CUSA11111")
    );
    assert_eq!(dir.store(None).mapped_folder(Some("SAVEDATA01"), 0), None);
}

#[test]
fn mounts_follow_the_c_error_order_and_modes() {
    let dir = TempDir::new("mount");
    let mut store = dir.store(None);
    let n = name("SLOT");
    assert_eq!(
        store.mount(1, None, Some(&n), 0, true).unwrap_err(),
        ERR_NOT_FOUND
    );
    assert_eq!(
        store.mount(-1, None, Some(&n), 4, true).unwrap_err(),
        ERR_INVALID_USER
    );
    assert_eq!(
        store.mount(1, None, None, 4, true).unwrap_err(),
        ERR_PARAMETER
    );
    assert_eq!(
        store.mount(1, None, Some(&n), 4, false).unwrap_err(),
        ERR_PARAMETER
    );
    assert_eq!(
        store
            .mount(1, None, Some(&name("a/b")), 4, true)
            .unwrap_err(),
        ERR_PARAMETER
    );
    let (mount, created) = store.mount(1, None, Some(&n), MODE_CREATE, true).unwrap();
    assert!(created);
    assert!(mount.starts_with("/savedata"));
    let host = dir.0.join("savedata/1/CUSA00000/SLOT");
    assert!(host.is_dir());
    assert_eq!(
        fs::metadata(dir.0.join("savedata/1/CUSA00000/SLOT.sce_sys/param.bin"))
            .unwrap()
            .len(),
        1328
    );
    assert_eq!(
        store.mount(1, None, Some(&n), 0, true).unwrap_err(),
        ERR_BAD_MOUNTED
    );
    assert_eq!(store.umount(Some(&point(&mount))).code, 0);
    assert_eq!(
        store
            .mount(1, None, Some(&n), MODE_CREATE, true)
            .unwrap_err(),
        ERR_EXISTS
    );
    let (_, created) = store.mount(1, None, Some(&n), MODE_CREATE2, true).unwrap();
    assert!(!created);
    assert_eq!(store.mounts_done, 2);
    assert_eq!(store.terminate(), 0);
    assert_eq!(store.terminate(), ERR_NOT_INITIALIZED);
    assert_eq!(
        store.mount(1, None, Some(&n), 0, true).unwrap_err(),
        ERR_NOT_INITIALIZED
    );
}

#[test]
fn the_seventeenth_mount_is_refused_and_a_slot_is_reused() {
    let dir = TempDir::new("full");
    let mut store = dir.store(None);
    let mut points = Vec::new();
    for i in 0..16 {
        let n = name(&format!("S{i}"));
        points.push(store.mount(1, None, Some(&n), MODE_CREATE, true).unwrap().0);
    }
    assert_eq!(points[3], "/savedata3");
    assert_eq!(
        store
            .mount(1, None, Some(&name("S16")), MODE_CREATE, true)
            .unwrap_err(),
        ERR_MOUNT_FULL
    );
    assert_eq!(store.writing(), 16);
    assert_eq!(store.umount(Some(&point("/savedata3"))).code, 0);
    assert_eq!(store.writing(), 15);
    let (again, _) = store
        .mount(1, None, Some(&name("S16")), MODE_CREATE, true)
        .unwrap();
    assert_eq!(again, "/savedata3");
    assert_eq!(store.umount(Some(&point("/savedata3"))).slot, Some(3));
    assert_eq!(store.umount(Some(&point("/savedata3"))).code, ERR_NOT_FOUND);
    assert_eq!(store.umount(Some(&point("/elsewhere"))).code, ERR_NOT_FOUND);
    assert_eq!(store.umount(None).code, ERR_NOT_FOUND);
}

#[test]
fn read_only_mounts_are_not_a_save_in_progress() {
    let dir = TempDir::new("readonly");
    let mut store = dir.store(None);
    let n = name("SAVEDATA01P");
    store.mount(1, None, Some(&n), MODE_CREATE, true).unwrap();
    assert_eq!(store.umount(Some(&point("/savedata0"))).code, 0);
    let (mount, _) = store.mount(1, None, Some(&n), MODE_RDONLY, true).unwrap();
    assert_eq!(store.writing(), 0);
    assert_eq!(store.set_param(&point(&mount), 1, b"x"), 0);
    let closed = store.umount(Some(&point(&mount)));
    assert_eq!(closed.code, 0);
    assert!(
        closed.commit.is_none(),
        "a read-only mount writes no where.json"
    );
}

#[test]
fn parameters_round_trip_through_param_bin() {
    let dir = TempDir::new("param");
    let mut store = dir.store(None);
    let n = name("SLOT");
    let (mount, _) = store.mount(1, None, Some(&n), MODE_CREATE, true).unwrap();
    let p = point(&mount);
    assert_eq!(store.set_param(&p, 1, b"Chapter 4"), 0);
    assert_eq!(store.set_param(&p, 2, b"The sub"), 0);
    assert_eq!(store.set_param(&p, 3, b"Longer detail"), 0);
    assert_eq!(store.set_param(&p, 4, &7u32.to_le_bytes()), 0);
    assert_eq!(store.set_param(&p, 4, b"ab"), ERR_PARAMETER);
    assert_eq!(store.set_param(&p, 9, b"x"), ERR_PARAMETER);
    assert_eq!(store.set_param(&p, 0, b"short"), ERR_PARAMETER);
    assert_eq!(
        store.set_param(&point("/savedata9"), 1, b"x"),
        ERR_NOT_MOUNTED
    );
    let meta = dir.0.join("savedata/1/CUSA00000/SLOT.sce_sys");
    let param = read_param(meta.to_str().unwrap());
    assert_eq!(
        (
            param.title(),
            param.subtitle(),
            param.detail(),
            param.user_param()
        ),
        (
            "Chapter 4".into(),
            "The sub".into(),
            "Longer detail".into(),
            7
        )
    );
    // The whole struct replaces it; a long title is cut to 127 bytes.
    let mut whole = Param::zero();
    whole.0[..3].copy_from_slice(b"all");
    assert_eq!(store.set_param(&p, 0, &whole.0), 0);
    assert_eq!(read_param(meta.to_str().unwrap()).title(), "all");
    assert_eq!(store.set_param(&p, 1, &[b'x'; 500]), 0);
    assert_eq!(read_param(meta.to_str().unwrap()).title().len(), 127);
    assert_eq!(store.save_icon(&p, b"png"), 0);
    assert_eq!(fs::read(meta.join("icon0.png")).unwrap(), b"png");
    assert_eq!(
        store.save_icon(&point("/savedata9"), b"png"),
        ERR_NOT_MOUNTED
    );
}

#[test]
fn closing_a_saved_game_directory_commits_where_json_in_its_folder() {
    let dir = TempDir::new("commit");
    let mut store = dir.store(Some("SAVEDATA01=Uncharted 1;SAVEDATA02=Uncharted 2"));
    let (mount, _) = store
        .mount(1, None, Some(&name("SAVEDATA02P")), MODE_CREATE, true)
        .unwrap();
    let p = point(&mount);
    store.set_param(&p, 1, "Chapter \"7\"".as_bytes());
    store.set_param(&p, 2, "雪".as_bytes());
    let closed = store.umount(Some(&p));
    let commit = closed.commit.unwrap();
    assert_eq!(commit.game, "u2");
    let folder = dir.0.join("Uncharted 2");
    assert_eq!(Path::new(&commit.saves), folder.as_path());
    commit.publish().unwrap();
    let got: serde_json::Value =
        serde_json::from_slice(&fs::read(folder.join("where.json")).unwrap()).unwrap();
    assert_eq!(got["game"], "u2");
    assert_eq!(got["title"], "Chapter \"7\"");
    assert_eq!(got["subtitle"], "雪");
    assert!(folder.join("SAVEDATA02P").is_dir());
    // Not a game save: no commit; a failure to write is an error the caller can show.
    let (m, _) = store
        .mount(1, None, Some(&name("SPRJ0005")), MODE_CREATE, true)
        .unwrap();
    assert!(store.umount(Some(&point(&m))).commit.is_none());
    let blocked = Commit {
        saves: dir
            .0
            .join("Uncharted 2/where.json/inner")
            .to_str()
            .unwrap()
            .into(),
        game: "u2",
        meta: commit.meta.clone(),
    };
    assert!(blocked.publish().is_err());
}

#[test]
fn a_loaded_save_is_not_a_commit_until_the_game_writes_parameters() {
    let dir = TempDir::new("loaded");
    let mut store = dir.store(None);
    let n = name("SAVEDATA03P");
    store.mount(1, None, Some(&n), MODE_CREATE, true).unwrap();
    assert!(store.umount(Some(&point("/savedata0"))).commit.is_some());
    store.mount(1, None, Some(&n), 0, true).unwrap();
    assert!(store.umount(Some(&point("/savedata0"))).commit.is_none());
    store.mount(1, None, Some(&n), 0, true).unwrap();
    store.set_param(&point("/savedata0"), 3, b"d");
    assert_eq!(
        store
            .umount(Some(&point("/savedata0")))
            .commit
            .unwrap()
            .game,
        "u3"
    );
}

#[test]
fn search_lists_every_folder_once_sorted_and_filtered() {
    let dir = TempDir::new("search");
    let mut store = dir.store(Some("SAVEDATA01=Uncharted 1"));
    for (n, title) in [
        ("SAVEDATA01B", "b"),
        ("SAVEDATA01A", "a"),
        ("OTHER", "o"),
        ("SAVEDATA02", "c"),
    ] {
        let (m, _) = store
            .mount(1, None, Some(&name(n)), MODE_CREATE, true)
            .unwrap();
        store.set_param(&point(&m), 1, title.as_bytes());
        store.set_param(&point(&m), 4, &(title.as_bytes()[0] as u32).to_le_bytes());
        store.umount(Some(&point(&m)));
    }
    // A stray file and a hidden entry are not slots.
    fs::write(dir.0.join("Uncharted 1/.hidden"), "x").unwrap();
    fs::write(dir.0.join("Uncharted 1/where.json"), "{}").unwrap();
    let all = |pattern: &[u8], key, order| {
        store
            .search(&Search {
                user: 1,
                title: None,
                pattern,
                key,
                order,
            })
            .iter()
            .map(|e| {
                String::from_utf8_lossy(&e.name)
                    .trim_end_matches('\0')
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        all(b"", 0, 0),
        ["OTHER", "SAVEDATA01A", "SAVEDATA01B", "SAVEDATA02"]
    );
    assert_eq!(
        all(b"", 0, 1),
        ["SAVEDATA02", "SAVEDATA01B", "SAVEDATA01A", "OTHER"]
    );
    assert_eq!(all(b"SAVEDATA01%", 0, 0), ["SAVEDATA01A", "SAVEDATA01B"]);
    assert_eq!(all(b"SAVEDATA01_", 1, 1), ["SAVEDATA01B", "SAVEDATA01A"]);
}

#[test]
fn pick_chooses_among_existing_slots_only() {
    let dir = TempDir::new("pick");
    let mut store = dir.store(None);
    for n in ["A", "B", "C"] {
        let (m, _) = store
            .mount(1, None, Some(&name(n)), MODE_CREATE, true)
            .unwrap();
        store.umount(Some(&point(&m)));
    }
    let names = [
        name("MISSING"),
        name("A"),
        name("B"),
        name("C"),
        name("../x"),
    ];
    let pick = |which, focus: Option<&[u8]>| {
        store
            .pick(1, None, &names, which, focus)
            .map(|(n, _)| n[0] as char)
    };
    assert_eq!(pick(0, None), Some('A'));
    assert_eq!(pick(1, None), Some('C'));
    assert_eq!(pick(4, Some(b"B\0")), Some('B'));
    assert_eq!(pick(4, Some(b"Z")), None);
    assert_eq!(store.pick(1, None, &[name("MISSING")], 0, None), None);
    assert_eq!(store.pick(1, None, &[], 0, None), None);
}

#[test]
fn delete_removes_the_directory_and_its_metadata() {
    let dir = TempDir::new("delete");
    let mut store = dir.store(None);
    let n = name("SLOT");
    let (m, _) = store.mount(1, None, Some(&n), MODE_CREATE, true).unwrap();
    store.umount(Some(&point(&m)));
    assert_eq!(store.delete(1, None, Some(&name("../x"))), ERR_PARAMETER);
    assert_eq!(store.delete(1, None, None), ERR_PARAMETER);
    assert_eq!(store.delete(1, None, Some(&n)), 0);
    assert!(!dir.0.join("savedata/1/CUSA00000/SLOT").exists());
    assert!(!dir.0.join("savedata/1/CUSA00000/SLOT.sce_sys").exists());
    assert_eq!(store.delete(1, None, Some(&n)), ERR_NOT_FOUND);
}

#[test]
fn save_data_memory_is_one_fixed_blob() {
    let dir = TempDir::new("memory");
    let mut store = dir.store(None);
    assert_eq!(store.memory_check(0, 1), ERR_MEMORY_NOT_READY);
    assert_eq!(store.memory_setup(1, 0, None), ERR_PARAMETER);
    assert_eq!(store.memory_setup(1, 64, None), 0);
    assert_eq!(store.memory_check(60, 8), ERR_PARAMETER);
    assert_eq!(store.memory_check(-1, 1), ERR_PARAMETER);
    assert_eq!(store.memory_check(60, 4), 0);
    assert_eq!(store.memory_io(8, None, b"hello"), 0);
    let mut out = [0u8; 5];
    assert_eq!(store.memory_io(8, Some(&mut out), &[]), 0);
    assert_eq!(&out, b"hello");
    assert_eq!(store.memory_writes, 1);
    // Setting up again keeps the contents and never shrinks.
    assert_eq!(store.memory_setup(1, 32, None), 0);
    let mut out = [0u8; 5];
    assert_eq!(store.memory_io(8, Some(&mut out), &[]), 0);
    assert_eq!(&out, b"hello");
    assert_eq!(
        fs::metadata(dir.0.join("savedata/1/CUSA00000.memory/memory.dat"))
            .unwrap()
            .len(),
        64
    );
}

#[test]
fn the_bloodborne_sound_flag_is_set_once_and_can_be_disabled() {
    let dir = TempDir::new("sound");
    let mut store = dir.store(None);
    store.title_id = "CUSA03173".into();
    let save = dir.0.join("savedata/1/CUSA03173/SPRJ0005");
    fs::create_dir_all(&save).unwrap();
    fs::write(save.join("userdata0010"), vec![0u8; 0x3000]).unwrap();
    assert!(bloodborne_sound_hack(&store, true).is_empty());
    assert_eq!(fs::read(save.join("userdata0010")).unwrap()[0x204E], 0);
    let lines = bloodborne_sound_hack(&store, false);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].ends_with("(was 0)"));
    assert_eq!(fs::read(save.join("userdata0010")).unwrap()[0x204E], 1);
    assert!(bloodborne_sound_hack(&store, false).is_empty());
    store.title_id = "CUSA00000".into();
    assert!(bloodborne_sound_hack(&store, false).is_empty());
}
