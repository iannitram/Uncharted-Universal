//! Expected results from `launcher/bbport_steam.py` (`shortcut_id`, `entry`, `add_shortcut`,
//! `user_configs` and its `__main__` self-test); the app ids below are the Python's
//! `hex(shortcut_id(..))`. Temp folders only.

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh temp folder under `std::env::temp_dir()`, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new() -> Temp {
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "uncharted-steam-test-{}-{}",
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

fn shortcut(name: &str, exe: &str, start_dir: &str, icon: &str) -> Shortcut {
    Shortcut {
        name: name.to_string(),
        exe: exe.to_string(),
        start_dir: start_dir.to_string(),
        launch_options: String::new(),
        icon: icon.to_string(),
    }
}

fn launcher() -> Shortcut {
    Shortcut {
        launch_options: "play".to_string(),
        ..shortcut(
            "Uncharted Universal",
            "/home/deck/.local/bin/uncharted-universal",
            "/home/deck/.local/bin",
            "",
        )
    }
}

fn entries(doc: &[(String, Value)]) -> &[(String, Value)] {
    match doc {
        [(key, Value::Map(entries))] if key == "shortcuts" => entries,
        _ => panic!("not one shortcuts map: {doc:?}"),
    }
}

fn field<'a>(entry: &'a Value, name: &str) -> &'a Value {
    let Value::Map(fields) = entry else {
        panic!("entry is not a map: {entry:?}");
    };
    &fields.iter().find(|(k, _)| k == name).unwrap().1
}

fn steam(message: &str) -> Error {
    Error::Steam(message.to_string())
}

#[test]
fn crc32_check_values() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b""), 0);
    // zlib.crc32('Ünïcode ✓'.encode())
    assert_eq!(crc32("Ünïcode ✓".as_bytes()), 0x3330_96F1);
}

#[test]
fn app_ids_match_the_python_shortcut_id() {
    assert_eq!(
        shortcut_app_id(r#""C:\Games\bb\Play Bloodborne.exe""#, "Bloodborne"),
        0xD31F_3337
    );
    assert_eq!(shortcut_app_id(r#""C:\Other\o.exe""#, "Other"), 0xA876_2522);
    assert_eq!(launcher().app_id(), 0xB460_2143);
    assert_eq!(
        shortcut("Other", "/usr/bin/other", "/usr/bin", "").app_id(),
        0xE209_B254
    );
    // The top bit is always set (Steam's mark for a non-Steam game).
    assert_eq!(shortcut_app_id("", "") & 0x8000_0000, 0x8000_0000);
}

#[test]
fn the_entry_has_the_python_fields_in_order() {
    let s = Shortcut {
        icon: "x.exe".to_string(),
        launch_options: "--fast".to_string(),
        ..shortcut(
            "Bloodborne",
            r"C:\Games\bb\Play Bloodborne.exe",
            r"C:\Games\bb",
            "",
        )
    };
    let str_ = |v: &str| Value::Str(v.to_string());
    let want = Value::Map(vec![
        ("appid".to_string(), Value::Int(0xD31F_3337)),
        ("AppName".to_string(), str_("Bloodborne")),
        (
            "Exe".to_string(),
            str_(r#""C:\Games\bb\Play Bloodborne.exe""#),
        ),
        ("StartDir".to_string(), str_(r#""C:\Games\bb""#)),
        ("icon".to_string(), str_("x.exe")),
        ("ShortcutPath".to_string(), str_("")),
        ("LaunchOptions".to_string(), str_("--fast")),
        ("IsHidden".to_string(), Value::Int(0)),
        ("AllowDesktopConfig".to_string(), Value::Int(1)),
        ("AllowOverlay".to_string(), Value::Int(1)),
        ("OpenVR".to_string(), Value::Int(0)),
        ("Devkit".to_string(), Value::Int(0)),
        ("DevkitGameID".to_string(), str_("")),
        ("DevkitOverrideAppID".to_string(), Value::Int(0)),
        ("LastPlayTime".to_string(), Value::Int(0)),
        ("FlatpakAppID".to_string(), str_("")),
        ("tags".to_string(), Value::Map(vec![])),
    ]);
    assert_eq!(shortcut_entry(&s), want);
}

#[test]
fn upsert_writes_the_bytes_of_the_python_fixture() {
    // tests/data/shortcuts.vdf is bbport_steam.dump of entry(launcher, icon, 'play') and
    // entry('Other', '/usr/bin/other', '/usr/bin', ''), with a play time and a tag on the first.
    let first = Shortcut {
        icon: "/home/deck/.local/share/uncharted-universal/icon.png".to_string(),
        ..launcher()
    };
    let mut doc = Vec::new();
    upsert(&mut doc, &first).unwrap();
    upsert(
        &mut doc,
        &shortcut("Other", "/usr/bin/other", "/usr/bin", ""),
    )
    .unwrap();
    let Value::Map(list) = &mut doc[0].1 else {
        unreachable!()
    };
    let Value::Map(fields) = &mut list[0].1 else {
        unreachable!()
    };
    for (key, value) in fields.iter_mut() {
        match key.as_str() {
            "LastPlayTime" => *value = Value::Int(1_731_400_000),
            "tags" => *value = Value::Map(vec![("0".to_string(), Value::Str("favorite".into()))]),
            _ => {}
        }
    }
    assert_eq!(
        vdf::write(&doc),
        include_bytes!("../../tests/data/shortcuts.vdf").to_vec()
    );
}

#[test]
fn upsert_follows_the_python_self_test() {
    // add Bloodborne (x.exe), add Other, add Bloodborne again (y.exe): two entries, the first
    // updated in place.
    let bb = |icon: &str| {
        shortcut(
            "Bloodborne",
            r"C:\Games\bb\Play Bloodborne.exe",
            r"C:\Games\bb",
            icon,
        )
    };
    let mut doc = Vec::new();
    assert_eq!(upsert(&mut doc, &bb("x.exe")).unwrap(), 0xD31F_3337);
    let other = shortcut("Other", r"C:\Other\o.exe", r"C:\Other", "");
    assert_eq!(upsert(&mut doc, &other).unwrap(), 0xA876_2522);
    assert_eq!(upsert(&mut doc, &bb("y.exe")).unwrap(), 0xD31F_3337);

    let list = entries(&doc);
    let keys: Vec<&str> = list.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["0", "1"]);
    assert_eq!(
        field(&list[0].1, "AppName"),
        &Value::Str("Bloodborne".into())
    );
    assert_eq!(field(&list[0].1, "icon"), &Value::Str("y.exe".into()));
    assert_eq!(field(&list[1].1, "AppName"), &Value::Str("Other".into()));
    // And the bytes read back to the same document.
    assert_eq!(vdf::read(&vdf::write(&doc)).unwrap(), doc);
}

#[test]
fn upsert_keeps_play_time_and_tags_and_renumbers() {
    let bytes = include_bytes!("../../tests/data/shortcuts.vdf");
    let mut doc = vdf::read(bytes).unwrap();
    // Renumbering: a file whose keys Steam left out of order.
    if let Value::Map(list) = &mut doc[0].1 {
        list[0].0 = "7".to_string();
        list[1].0 = "3".to_string();
    }
    let changed = Shortcut {
        icon: "/new/icon.png".to_string(),
        ..launcher()
    };
    assert_eq!(upsert(&mut doc, &changed).unwrap(), 0xB460_2143);
    let list = entries(&doc);
    let keys: Vec<&str> = list.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["0", "1"]);
    let entry = &list[0].1;
    assert_eq!(field(entry, "icon"), &Value::Str("/new/icon.png".into()));
    assert_eq!(field(entry, "LastPlayTime"), &Value::Int(1_731_400_000));
    assert_eq!(
        field(entry, "tags"),
        &Value::Map(vec![("0".to_string(), Value::Str("favorite".into()))])
    );
    assert_eq!(field(&list[1].1, "AppName"), &Value::Str("Other".into()));

    // A new shortcut goes last.
    let third = shortcut("Third", "/opt/third", "/opt", "");
    upsert(&mut doc, &third).unwrap();
    let list = entries(&doc);
    assert_eq!(list.len(), 3);
    assert_eq!(list[2].0, "2");
    assert_eq!(field(&list[2].1, "AppName"), &Value::Str("Third".into()));
}

#[test]
fn upsert_refuses_a_document_without_the_list_first() {
    for mut doc in [
        vec![("other".to_string(), Value::Map(vec![]))],
        vec![("shortcuts".to_string(), Value::Str("x".into()))],
        vec![
            ("x".to_string(), Value::Int(1)),
            ("shortcuts".to_string(), Value::Map(vec![])),
        ],
    ] {
        let before = doc.clone();
        assert_eq!(
            upsert(&mut doc, &launcher()),
            Err(steam("shortcuts.vdf has no shortcuts list"))
        );
        assert_eq!(doc, before);
    }
}

#[test]
fn userdata_roots_per_os() {
    let vars = Vars {
        home: PathBuf::from("/home/deck"),
        data: PathBuf::from("/home/deck/.local/share"),
        config: PathBuf::from("/home/deck/.config"),
    };
    let home = &vars.home;
    assert_eq!(
        userdata_roots_for(Os::Linux, &vars),
        vec![
            home.join(".local/share/Steam/userdata"),
            home.join(".steam/steam/userdata"),
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam/userdata"),
        ]
    );
    assert_eq!(
        userdata_roots_for(Os::Windows, &vars),
        vec![PathBuf::from(r"C:\Program Files (x86)\Steam\userdata")]
    );
    assert_eq!(
        userdata_roots_for(Os::Mac, &vars),
        vec![home.join("Library/Application Support/Steam/userdata")]
    );
    assert!(userdata_roots_for(Os::Ios, &vars).is_empty());
    assert!(userdata_roots_for(Os::Android, &vars).is_empty());
    assert_eq!(
        userdata_roots(&vars),
        userdata_roots_for(Os::current(), &vars)
    );
}

#[test]
fn write_shortcut_writes_every_user_and_keeps_a_backup() {
    let temp = Temp::new();
    let userdata = temp.0.join("userdata");
    // Two users, one with a file; "0" and non-numeric folders are not users; a stray file.
    let fixture = include_bytes!("../../tests/data/shortcuts.vdf");
    fs::create_dir_all(userdata.join("12345/config")).unwrap();
    fs::write(userdata.join("12345/config/shortcuts.vdf"), fixture).unwrap();
    fs::create_dir_all(userdata.join("678")).unwrap();
    fs::create_dir_all(userdata.join("0/config")).unwrap();
    fs::create_dir_all(userdata.join("anonymous")).unwrap();
    fs::write(userdata.join("99"), b"not a folder").unwrap();

    let changed = Shortcut {
        icon: "/new/icon.png".to_string(),
        ..launcher()
    };
    let written = write_shortcut(&userdata, &changed, &|| false).unwrap();
    assert_eq!(
        written,
        vec![
            userdata.join("12345").join("config").join("shortcuts.vdf"),
            userdata.join("678").join("config").join("shortcuts.vdf"),
        ]
    );

    // The old file is kept as .bak, the new one has the change and both entries.
    assert_eq!(
        fs::read(userdata.join("12345/config/shortcuts.vdf.bak")).unwrap(),
        fixture.to_vec()
    );
    let doc = vdf::read(&fs::read(&written[0]).unwrap()).unwrap();
    let list = entries(&doc);
    assert_eq!(list.len(), 2);
    assert_eq!(
        field(&list[0].1, "icon"),
        &Value::Str("/new/icon.png".into())
    );
    assert_eq!(
        field(&list[0].1, "LastPlayTime"),
        &Value::Int(1_731_400_000)
    );

    // A user without the file gets a new one with only this shortcut, and no .bak.
    let doc = vdf::read(&fs::read(&written[1]).unwrap()).unwrap();
    assert_eq!(entries(&doc).len(), 1);
    assert_eq!(
        field(&entries(&doc)[0].1, "appid"),
        &Value::Int(0xB460_2143)
    );
    assert!(!userdata.join("678/config/shortcuts.vdf.bak").exists());
    assert!(!userdata.join("0/config/shortcuts.vdf").exists());
    assert!(!userdata.join("678/config/shortcuts.vdf.tmp").exists());

    // Writing again changes nothing but the .bak.
    let again = write_shortcut(&userdata, &changed, &|| false).unwrap();
    assert_eq!(again, written);
    assert_eq!(
        fs::read(userdata.join("678/config/shortcuts.vdf.bak")).unwrap(),
        fs::read(&written[1]).unwrap()
    );
}

#[test]
fn write_shortcut_refuses_while_steam_runs() {
    let temp = Temp::new();
    let userdata = temp.0.join("userdata");
    fs::create_dir_all(userdata.join("12345")).unwrap();
    assert_eq!(
        write_shortcut(&userdata, &launcher(), &|| true),
        Err(steam("close Steam first"))
    );
    assert!(!userdata.join("12345/config").exists());
}

#[test]
fn write_shortcut_writes_nothing_when_one_file_is_damaged() {
    let temp = Temp::new();
    let userdata = temp.0.join("userdata");
    fs::create_dir_all(userdata.join("1/config")).unwrap();
    fs::create_dir_all(userdata.join("2/config")).unwrap();
    fs::write(userdata.join("2/config/shortcuts.vdf"), b"\x00shortcuts\0").unwrap();
    assert_eq!(
        write_shortcut(&userdata, &launcher(), &|| false),
        Err(steam("shortcuts.vdf is cut short"))
    );
    assert!(!userdata.join("1/config/shortcuts.vdf").exists());
    assert!(!userdata.join("2/config/shortcuts.vdf.bak").exists());
}

#[test]
fn write_shortcut_without_users() {
    let temp = Temp::new();
    let userdata = temp.0.join("userdata");
    assert!(matches!(
        write_shortcut(&userdata, &launcher(), &|| false),
        Err(Error::Steam(m)) if m.starts_with("no Steam user folder at ")
    ));
    fs::create_dir_all(userdata.join("0")).unwrap();
    assert_eq!(
        write_shortcut(&userdata, &launcher(), &|| false),
        Ok(vec![])
    );
}

#[test]
fn write_artwork_creates_the_grid_folder() {
    let temp = Temp::new();
    let user = temp.0.join("userdata").join("12345");
    let id = launcher().app_id();
    let images = vec![
        (Art::Grid, b"grid png".to_vec()),
        (Art::Hero, b"hero png".to_vec()),
        (Art::Icon, b"icon png".to_vec()),
    ];
    let written = write_artwork(&user, id, &images).unwrap();
    let grid = user.join("config").join("grid");
    assert_eq!(
        written,
        vec![
            grid.join(format!("{id}p.png")),
            grid.join(format!("{id}_hero.png")),
            grid.join(format!("{id}_icon.png")),
        ]
    );
    for ((_, png), path) in images.iter().zip(&written) {
        assert_eq!(&fs::read(path).unwrap(), png);
    }
    // Writing again replaces the files; nothing given, nothing written.
    write_artwork(&user, id, &[(Art::Grid, b"new".to_vec())]).unwrap();
    assert_eq!(fs::read(&written[0]).unwrap(), b"new");
    assert_eq!(
        write_artwork(&user, id, &[]).unwrap(),
        Vec::<PathBuf>::new()
    );
}
