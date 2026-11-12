//! The 13 cases of `tests/test_mods.py`, as expected results for an overlay list instead of a
//! tree of links. Fixtures are fake dumps: a few bytes per file, no game data.

use super::*;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);
impl Temp {
    fn new(label: &str) -> Self {
        let nonce = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("uu-mods-{}-{label}-{nonce}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A fake dump (`CUSA02320`) beside an empty `mods` folder, as `setUp` in `test_mods.py`.
struct Fixture {
    temp: Temp,
    game: PathBuf,
    assets: PathBuf,
    moddir: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let temp = Temp::new(label);
        let game = temp.0.join("CUSA02320");
        let assets = game.join("u1data/build");
        fs::create_dir_all(&assets).unwrap();
        fs::create_dir_all(game.join("sce_sys")).unwrap();
        fs::write(game.join("eboot.bin"), b"original executable").unwrap();
        fs::write(game.join("big2-ps4_Shipping.elf"), b"original executable").unwrap();
        fs::write(game.join("sce_sys/param.sfo"), b"fake").unwrap();
        fs::write(assets.join("a.pak"), b"original").unwrap();
        fs::write(assets.join("b.pak"), b"untouched").unwrap();
        let moddir = temp.0.join("mods");
        Fixture {
            temp,
            game,
            assets,
            moddir,
        }
    }

    fn out(&self) -> PathBuf {
        self.temp.0.join("out/overlay.json")
    }

    /// `mods/<name>/u1data/build/<file>` holding `contents`.
    fn put(&self, name: &str, contents: &[u8], file: &str) -> PathBuf {
        let root = self.moddir.join(name);
        let folder = root.join("u1data/build");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(file), contents).unwrap();
        root
    }

    fn put_a(&self, name: &str, contents: &[u8]) -> PathBuf {
        self.put(name, contents, "a.pak")
    }

    fn overlay(&self, layers: &[(&str, &Path)]) -> Result<Overlay> {
        let layers: Vec<(String, PathBuf)> = layers
            .iter()
            .map(|(name, path)| (name.to_string(), path.to_path_buf()))
            .collect();
        build_overlay(&self.game, &self.out(), &layers)
    }

    /// What the runtime's VFS serves at `path`: the overlay's source, else the dump's file.
    fn served(&self, overlay: &Overlay, path: &str) -> Vec<u8> {
        match overlay.files.iter().find(|f| f.game_path == path) {
            Some(file) => fs::read(&file.source).unwrap(),
            None => fs::read(self.game.join(path)).unwrap(),
        }
    }

    /// The names the runtime lists in `dir`: the dump's entries and the overlay's files.
    fn listed(&self, overlay: &Overlay, dir: &str) -> BTreeSet<String> {
        let mut names: BTreeSet<String> = fs::read_dir(self.game.join(dir))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        let prefix = format!("{dir}/");
        for file in &overlay.files {
            if let Some(rest) = file.game_path.strip_prefix(&prefix) {
                names.insert(rest.split('/').next().unwrap().to_string());
            }
        }
        names
    }

    fn dump_untouched(&self) {
        assert_eq!(fs::read(self.assets.join("a.pak")).unwrap(), b"original");
        assert_eq!(fs::read(self.assets.join("b.pak")).unwrap(), b"untouched");
        assert!(!self.assets.join("new.pak").exists());
        assert_eq!(
            fs::read(self.game.join("eboot.bin")).unwrap(),
            b"original executable"
        );
    }
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

#[test]
fn merge_replacement_new_file_and_directory_listing() {
    let f = Fixture::new("merge");
    let a = f.put_a("A", b"mod");
    f.put("A", b"new", "new.pak");
    let overlay = f.overlay(&[("A", &a)]).unwrap();
    assert_eq!(f.served(&overlay, "u1data/build/a.pak"), b"mod");
    assert_eq!(f.served(&overlay, "u1data/build/b.pak"), b"untouched");
    assert_eq!(f.served(&overlay, "u1data/build/new.pak"), b"new");
    assert_eq!(
        f.listed(&overlay, "u1data/build"),
        set(&["a.pak", "b.pak", "new.pak"])
    );
    assert_eq!(f.served(&overlay, "eboot.bin"), b"original executable");
    let paths: Vec<&str> = overlay.files.iter().map(|f| f.game_path.as_str()).collect();
    assert_eq!(paths, ["u1data/build/a.pak", "u1data/build/new.pak"]);
    f.dump_untouched();
    assert_eq!(
        fs::read(a.join("u1data/build/a.pak")).unwrap(),
        b"mod",
        "the mod's own file is never changed"
    );
}

#[test]
fn last_mod_wins_and_reordering_changes_winner() {
    let f = Fixture::new("order");
    let (a, b) = (f.put_a("A", b"A"), f.put_a("B", b"B"));
    for (layers, winner) in [
        ([("A", a.as_path()), ("B", b.as_path())], b"B"),
        ([("B", b.as_path()), ("A", a.as_path())], b"A"),
    ] {
        let overlay = f.overlay(&layers).unwrap();
        assert_eq!(overlay.files.len(), 1);
        assert_eq!(f.served(&overlay, "u1data/build/a.pak"), winner);
    }
}

#[test]
fn selection_disable_and_new_mod() {
    let f = Fixture::new("select");
    for name in ["C", "B", "A"] {
        f.put_a(name, b"mod");
    }
    let config =
        ModConfig::parse(r#"{"order": ["B", "A", "deleted"], "disabled": ["A"]}"#).unwrap();
    assert_eq!(selected(&f.moddir, &config).unwrap(), ["B", "C"]);
    assert_eq!(
        selected(&f.moddir, &ModConfig::default()).unwrap(),
        ["A", "B", "C"]
    );
}

#[test]
fn no_mod_returns_original_game() {
    let f = Fixture::new("none");
    assert_eq!(f.overlay(&[]).unwrap(), Overlay::default());
    assert!(!f.out().exists());
    assert!(!f.out().parent().unwrap().exists());
    // A list left from a launch with mods is removed, so the runtime mounts none.
    fs::create_dir_all(f.out().parent().unwrap()).unwrap();
    fs::write(f.out(), "stale").unwrap();
    assert_eq!(f.overlay(&[]).unwrap(), Overlay::default());
    assert!(!f.out().exists());
}

#[test]
fn directory_conflict_does_not_modify_base() {
    let f = Fixture::new("conflict");
    let a = f.put_a("A", b"mod");
    fs::remove_file(a.join("u1data/build/a.pak")).unwrap();
    let nested = a.join("u1data/build/b.pak");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("child"), b"bad").unwrap();
    let error = f.overlay(&[("A", &a)]).unwrap_err();
    assert_eq!(error.code(), "mods");
    f.dump_untouched();
    assert!(!f.out().exists());

    // The reverse: a mod file where the dump has a folder.
    let b = f.moddir.join("B");
    fs::create_dir_all(b.join("u1data")).unwrap();
    fs::write(b.join("u1data/build"), b"bad").unwrap();
    assert_eq!(f.overlay(&[("B", &b)]).unwrap_err().code(), "mods");
    // And between two mods: a file in one, a folder of the same name in a later one.
    let c = f.put("C", b"file", "extra");
    let d = f.moddir.join("D");
    fs::create_dir_all(d.join("u1data/build/extra")).unwrap();
    fs::write(d.join("u1data/build/extra/x"), b"x").unwrap();
    assert_eq!(
        f.overlay(&[("C", &c), ("D", &d)]).unwrap_err().code(),
        "mods"
    );
    assert!(!f.out().exists());
}

#[test]
fn mod_symlinks_rejected() {
    let f = Fixture::new("symlink");
    let a = f.put_a("A", b"mod");
    let link = a.join("u1data/escape");
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&f.assets, &link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(&f.assets, &link).or_else(|_| {
        // Without the symlink privilege (Developer Mode off) a junction stands in: the
        // fallback `mods.py` linked with, and a link this module must refuse just the same.
        let status = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(&link)
            .arg(&f.assets)
            .output()?
            .status;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other("mklink /J failed"))
        }
    });
    if made.is_err() {
        // `requires_symlinks`: no link of either kind can be made here.
        return;
    }
    assert!(link.join("a.pak").is_file(), "the link reaches the dump");
    let error = f.overlay(&[("A", &a)]).unwrap_err();
    assert_eq!(error.code(), "mods");
    assert!(error.to_string().contains("symlink"));
}

#[test]
fn executable_replacement_rejected() {
    let f = Fixture::new("exec");
    for name in [
        "eboot.bin",
        "big2-ps4_Shipping.elf",
        "BIG3-PS4_SHIPPING.ELF",
        "sce_discmap.plt",
    ] {
        let a = f.put_a(name, b"mod");
        fs::write(a.join(name), b"unsupported").unwrap();
        let error = f.overlay(&[(name, &a)]).unwrap_err();
        assert_eq!(error.code(), "mods", "{name}");
    }
    for folder in ["sce_sys", "SCE_MODULE"] {
        let a = f.put_a(folder, b"mod");
        fs::create_dir_all(a.join(folder)).unwrap();
        fs::write(a.join(folder).join("param.sfo"), b"unsupported").unwrap();
        assert_eq!(f.overlay(&[(folder, &a)]).unwrap_err().code(), "mods");
    }
    f.dump_untouched();
    assert!(!f.out().exists());
}

#[test]
fn shadps4_sibling_folder_and_global_disable() {
    let f = Fixture::new("sibling");
    let legacy = f.temp.0.join("CUSA02320-mods");
    fs::create_dir_all(legacy.join("u1data/build")).unwrap();
    fs::write(legacy.join("u1data/build/a.pak"), b"legacy").unwrap();
    let config = ModConfig::default();
    for (enabled, expected) in [(true, &b"legacy"[..]), (false, &b"original"[..])] {
        let layers = layers(&f.game, &f.moddir, &config, enabled).unwrap();
        let overlay = build_overlay(&f.game, &f.out(), &layers).unwrap();
        assert_eq!(f.served(&overlay, "u1data/build/a.pak"), expected);
    }
    // A mods folder that holds the game's folders itself is one mod, attached whole, after
    // the sibling folder.
    let direct = f.temp.0.join("direct");
    fs::create_dir_all(direct.join("u1data/build")).unwrap();
    fs::write(direct.join("u1data/build/a.pak"), b"direct").unwrap();
    let layers = layers(&f.game, &direct, &config, true).unwrap();
    let names: Vec<&str> = layers.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["CUSA02320-mods", "direct"]);
    let overlay = build_overlay(&f.game, &f.out(), &layers).unwrap();
    assert_eq!(f.served(&overlay, "u1data/build/a.pak"), b"direct");
}

#[test]
fn app0_wrapped_mod_discovered() {
    let f = Fixture::new("app0");
    fs::create_dir_all(f.moddir.join("Wrapped/app0/u2data")).unwrap();
    assert_eq!(discover(&f.moddir).unwrap(), ["Wrapped"]);
}

#[test]
fn other_case_replaces_the_game_file() {
    let f = Fixture::new("case");
    let root = f.moddir.join("Upper");
    fs::create_dir_all(root.join("U1DATA/Build")).unwrap();
    fs::write(root.join("U1DATA/Build/A.PAK"), b"upper").unwrap();
    let overlay = f.overlay(&[("Upper", &root)]).unwrap();
    let paths: Vec<&str> = overlay.files.iter().map(|f| f.game_path.as_str()).collect();
    assert_eq!(paths, ["u1data/build/a.pak"]);
    assert_eq!(f.served(&overlay, "u1data/build/a.pak"), b"upper");
    assert_eq!(f.listed(&overlay, "u1data/build"), set(&["a.pak", "b.pak"]));
    assert_eq!(
        f.listed(&overlay, "u1data"),
        set(&["build"]),
        "no second folder in another case"
    );
}

#[test]
fn wrapped_and_bare_layouts() {
    let f = Fixture::new("layouts");
    let layouts = [
        ("Title", "CUSA02320/u1data/build"),
        ("Archive", "Archive v1.2/u1data/build"),
        ("Bare", "u1data/build"),
        ("Nested", "Nested/app0/u1data/build"),
        ("Region", "cusa02826/u1data/build"),
    ];
    for (name, path) in layouts {
        let folder = f.moddir.join(name).join(path);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("a.pak"), name.as_bytes()).unwrap();
    }
    fs::write(f.moddir.join("Archive/readme.txt"), "notes").unwrap();
    fs::write(f.moddir.join("Archive/Preview.JPG"), "picture").unwrap();
    fs::write(f.moddir.join("Archive/.DS_Store"), "dotfile").unwrap();
    fs::create_dir_all(f.moddir.join("Junk/stuff")).unwrap();
    // A wrapper beside a file that is not a doc or picture is not a layout.
    fs::create_dir_all(f.moddir.join("Loose/inner/u1data")).unwrap();
    fs::write(f.moddir.join("Loose/setup.exe"), "x").unwrap();
    // Bloodborne's layout is not Uncharted's.
    fs::create_dir_all(f.moddir.join("Bloodborne/dvdroot_ps4/chr")).unwrap();
    assert_eq!(
        discover(&f.moddir).unwrap(),
        ["Archive", "Bare", "Nested", "Region", "Title"]
    );
    for (name, _) in layouts {
        let overlay = f.overlay(&[(name, &f.moddir.join(name))]).unwrap();
        assert_eq!(
            f.served(&overlay, "u1data/build/a.pak"),
            name.as_bytes(),
            "{name}"
        );
        assert_eq!(overlay.files.len(), 1, "{name}: readme files stay out");
    }
    assert_eq!(
        mod_files(&f.moddir.join("Junk")).unwrap_err().code(),
        "mods"
    );
}

#[test]
fn invalid_profile_fails() {
    f_invalid(r#"{"disabled":"A"}"#);
    f_invalid(r#"{"order":[1]}"#);
    f_invalid("[]");
    f_invalid("{");
    assert_eq!(
        ModConfig::parse("\u{feff}{\"order\":[\"A\"],\"other\":true}").unwrap(),
        ModConfig {
            disabled: vec![],
            order: vec!["A".into()],
        }
    );
    let f = Fixture::new("profile");
    let path = f.temp.0.join("mods.json");
    assert_eq!(ModConfig::load(&path).unwrap(), ModConfig::default());
    fs::write(&path, r#"{"disabled":"A"}"#).unwrap();
    assert_eq!(ModConfig::load(&path).unwrap_err().code(), "mods");
}

fn f_invalid(text: &str) {
    assert_eq!(ModConfig::parse(text).unwrap_err().code(), "mods", "{text}");
}

/// `test_run_uses_overlay_propagates_exit_and_cleans_view` drives `run.sh` and the C
/// runtime; its Rust half is the list the runtime's VFS (3.09) reads from `out`.
#[test]
fn overlay_list_written_for_the_runtime() {
    let f = Fixture::new("list");
    let a = f.put_a("A", b"mod");
    let overlay = f.overlay(&[("A", &a)]).unwrap();
    let text = fs::read_to_string(f.out()).unwrap();
    let list: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(list["version"], 1);
    assert_eq!(
        PathBuf::from(list["game"].as_str().unwrap()),
        std::path::absolute(&f.game).unwrap()
    );
    let files = list["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"], "u1data/build/a.pak");
    let source = PathBuf::from(files[0]["source"].as_str().unwrap());
    assert!(source.is_absolute());
    assert_eq!(fs::read(&source).unwrap(), b"mod");
    assert_eq!(
        source,
        std::path::absolute(&overlay.files[0].source).unwrap()
    );
    assert!(!f.temp.0.join("out/overlay.json.tmp").exists());
    f.dump_untouched();
}

#[test]
fn discover_missing_root_is_empty_and_sorts_case_insensitively() {
    let f = Fixture::new("discover");
    assert!(discover(&f.moddir).unwrap().is_empty());
    for name in ["beta", "Alpha", "gamma", "Delta"] {
        f.put_a(name, b"mod");
    }
    assert_eq!(
        discover(&f.moddir).unwrap(),
        ["Alpha", "beta", "Delta", "gamma"]
    );
}

#[test]
fn mod_files_skip_top_level_extras_and_keep_every_data_folder() {
    let f = Fixture::new("extras");
    let a = f.moddir.join("A");
    for path in [
        "u1data/x.pak",
        "u3data/deep/y.pak",
        "shareddata/z.bin",
        "screens/s.png",
    ] {
        fs::create_dir_all(a.join(path).parent().unwrap()).unwrap();
        fs::write(a.join(path), b"x").unwrap();
    }
    fs::write(a.join("readme.txt"), b"notes").unwrap();
    fs::write(a.join("u1data/.hidden"), b"dotfile").unwrap();
    let paths: Vec<String> = mod_files(&a)
        .unwrap()
        .into_iter()
        .map(|f| f.game_path)
        .collect();
    assert_eq!(
        paths,
        ["shareddata/z.bin", "u1data/x.pak", "u3data/deep/y.pak"]
    );
}
