//! The first run's shared steps against temp folders and synthetic files only: no dump, key or
//! game file. The Steam checks follow `launcher/bbport_steam.py` (`steam_running`, the
//! shortcut name of `uncharted_launcher.py`); the pull runs a real pairing and transfer over
//! loopback TCP against `hub::serve_game`.

use super::*;
use crate::dump::FoundGame;
use crate::vdf::{self, Value};
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh temp folder under `std::env::temp_dir()`, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new() -> Temp {
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "uncharted-setup-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Temp(path)
    }

    fn dirs(&self) -> AppDirs {
        AppDirs {
            config: self.0.join("config"),
            data: self.0.join("data"),
        }
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn fake_dump(folder: &Path) {
    write(&folder.join("sce_sys").join("param.sfo"), b"synthetic");
}

fn manifest(game: Game) -> InstallManifest {
    InstallManifest {
        game,
        title_id: "CUSA02320".into(),
        app_ver: "01.00".into(),
        files: vec!["boot.bin".into()],
    }
}

#[test]
fn first_run_until_a_game_is_installed_or_a_device_paired() {
    let temp = Temp::new();
    let dirs = temp.dirs();
    assert!(is_first_run(&dirs));
    assert!(installed_games(&dirs).is_empty());

    // An unfinished install (no install.json) does not count.
    write(&dirs.install("u2").join("boot.bin"), b"x");
    assert!(!installed(&dirs, Game::U2));
    assert!(is_first_run(&dirs));
    write(&dirs.install("u2").join("install.json"), b"{}\n");
    assert!(installed(&dirs, Game::U2));
    assert_eq!(installed_games(&dirs), [Game::U2]);
    assert!(!is_first_run(&dirs));

    // A paired device alone is enough too; an empty list is not.
    let temp = Temp::new();
    let dirs = temp.dirs();
    write(&peers_file(&dirs), b"[]");
    assert!(!has_peers(&dirs));
    assert!(is_first_run(&dirs));
    write(&peers_file(&dirs), b"[{\"device\":\"desktop\"}]");
    assert!(has_peers(&dirs));
    assert!(!is_first_run(&dirs));
}

#[test]
fn the_dump_folder_comes_from_the_variables_then_the_note() {
    let temp = Temp::new();
    let dirs = temp.dirs();
    assert_eq!(remembered_dump(&dirs), None);
    assert_eq!(dump_folder(Game::U1, |_| None, Some(&dirs)), None);

    let picked = temp.0.join("dumps").join("CUSA02320");
    remember_dump(&dirs, &picked).unwrap();
    assert_eq!(remembered_dump(&dirs), Some(picked.clone()));
    assert_eq!(dump_folder(Game::U1, |_| None, Some(&dirs)), Some(picked));

    let env = |name: &str| match name {
        "BB_GAME_DIR_U2" => Some("/games/u2".to_owned()),
        "BB_GAME_DIR_U3" => Some(String::new()),
        "BB_GAME_DIR" => Some("/games/all".to_owned()),
        _ => None,
    };
    assert_eq!(
        dump_folder(Game::U2, env, Some(&dirs)),
        Some(PathBuf::from("/games/u2"))
    );
    assert_eq!(
        dump_folder(Game::U3, env, Some(&dirs)),
        Some(PathBuf::from("/games/all"))
    );
    assert_eq!(dump_folder(Game::U1, |_| None, None), None);

    // An empty note is no note.
    fs::write(dirs.config.join(DUMP_NOTE), "\n").unwrap();
    assert_eq!(remembered_dump(&dirs), None);
}

#[test]
fn search_roots_in_order_each_once() {
    let temp = Temp::new();
    let dirs = temp.dirs();
    let home = temp.0.join("home");
    let vars = Vars {
        home: home.clone(),
        data: temp.0.join("data"),
        config: temp.0.join("config"),
    };
    remember_dump(&dirs, &home.join("Games")).unwrap();
    let env = |name: &str| (name == "BB_GAME_DIR").then(|| "/dumps".to_owned());
    assert_eq!(
        search_roots(&vars, env, Some(&dirs)),
        [
            PathBuf::from("/dumps"),
            home.join("Games"),
            home.join("Downloads"),
            home.join("Desktop"),
            home.clone(),
        ]
    );
}

#[test]
fn dump_candidates_two_levels_deep_skipping_hidden_folders() {
    let temp = Temp::new();
    let root = temp.0.join("Games");
    fake_dump(&root.join("CUSA02320"));
    fake_dump(&root.join("ps4").join("CUSA02343"));
    fake_dump(&root.join("a").join("b").join("too-deep"));
    fake_dump(&root.join(".cache").join("hidden"));
    // A folder with the file name but not a file, and a folder without sce_sys.
    fs::create_dir_all(root.join("broken").join("sce_sys").join("param.sfo")).unwrap();
    fs::create_dir_all(root.join("empty")).unwrap();

    let found = dump_candidates(&[root.clone(), root.join("CUSA02320"), temp.0.join("none")]);
    assert_eq!(
        found,
        [root.join("CUSA02320"), root.join("ps4").join("CUSA02343")]
    );
    // A root that is itself a dump is offered as it is.
    assert_eq!(
        dump_candidates(&[root.join("CUSA02320")]),
        [root.join("CUSA02320")]
    );
}

fn report(games: &[Game], decrypted: bool, folder: &Path) -> DumpReport {
    DumpReport {
        title_id: Some("CUSA02320".into()),
        games: games
            .iter()
            .map(|game| FoundGame {
                game: *game,
                folder: folder.to_path_buf(),
                app_ver: Some("01.00".into()),
            })
            .collect(),
        decrypted,
        problems: Vec::new(),
    }
}

#[test]
fn prepare_plan_takes_every_game_or_the_one_asked_for() {
    let folder = PathBuf::from("/dumps/CUSA02320");
    let all = report(&Game::ALL, true, &folder);
    assert!(can_prepare(&all));
    assert_eq!(
        prepare_plan(&all, None),
        Game::ALL.map(|g| (g, folder.clone())).to_vec()
    );
    assert_eq!(
        prepare_plan(&all, Some(Game::U3)),
        [(Game::U3, folder.clone())]
    );
    assert!(prepare_plan(&report(&[Game::U1], true, &folder), Some(Game::U2)).is_empty());
    assert!(!can_prepare(&report(&Game::ALL, false, &folder)));
    assert!(!can_prepare(&report(&[], true, &folder)));
}

#[test]
fn prepare_all_goes_on_after_a_failure_and_notes_the_dump() {
    let temp = Temp::new();
    let dirs = temp.dirs();
    let folder = temp.0.join("dump");
    let plan: Vec<(Game, PathBuf)> = Game::ALL.map(|g| (g, folder.clone())).to_vec();
    let mut seen = Vec::new();
    let done = prepare_all_with(
        &plan,
        &dirs,
        &mut |index, game| seen.push((index, game)),
        |dump, install, game| {
            assert_eq!(dump, folder);
            assert_eq!(install, dirs.install(game.key()));
            if game == Game::U2 {
                Err(Error::WrongRevision {
                    game: "u2".into(),
                    found: "01.02".into(),
                })
            } else {
                Ok(manifest(game))
            }
        },
    );
    assert_eq!(seen, [(0, Game::U1), (1, Game::U2), (2, Game::U3)]);
    assert_eq!(done.len(), 3);
    assert_eq!(done[0].result, Ok(manifest(Game::U1)));
    assert!(done[1].result.as_ref().unwrap_err().contains("01.02"));
    assert_eq!(done[2].result, Ok(manifest(Game::U3)));
    assert_eq!(remembered_dump(&dirs), Some(folder.clone()));

    // Nothing prepared: nothing noted.
    let temp = Temp::new();
    let dirs = temp.dirs();
    let done = prepare_all_with(&plan[..1], &dirs, &mut |_, _| {}, |_, _, _| {
        Err(Error::BadDump("The dump is missing eboot.bin.".into()))
    });
    assert_eq!(
        done[0].result,
        Err(Error::BadDump("The dump is missing eboot.bin.".into()).to_string())
    );
    assert_eq!(remembered_dump(&dirs), None);
}

#[test]
fn the_real_prepare_refuses_a_folder_that_is_no_dump() {
    let temp = Temp::new();
    let dirs = temp.dirs();
    let plan = [(Game::U1, temp.0.join("not-a-dump"))];
    let done = prepare_all(&plan, &dirs, &PrepareOptions::default(), |_, _| {});
    assert!(done[0].result.is_err());
    assert!(!dirs.install("u1").exists());
    assert_eq!(remembered_dump(&dirs), None);
}

#[test]
fn device_name_from_the_environment() {
    let env = |name: &str| match name {
        "COMPUTERNAME" => Some("  DESKTOP-3090 ".to_owned()),
        "HOSTNAME" => Some("ignored".to_owned()),
        _ => None,
    };
    assert_eq!(device_name(env), "DESKTOP-3090");
    let env = |name: &str| (name == "HOSTNAME").then(|| "deck\u{7}".to_owned());
    assert_eq!(device_name(env), "deck");
    let long = "x".repeat(100);
    assert_eq!(device_name(|_| Some(long.clone())).len(), 64);
    assert!(!device_name(|_| None).is_empty());
}

#[test]
fn pairing_codes() {
    assert_eq!(pairing_code("123456").unwrap(), "123456");
    assert_eq!(pairing_code(" 123 456 ").unwrap(), "123456");
    assert_eq!(pairing_code("123-456").unwrap(), "123456");
    for bad in ["12345", "1234567", "12345a", "", "١٢٣٤٥٦"] {
        assert_eq!(
            pairing_code(bad),
            Err(Error::Hub(
                "the pairing code must contain six digits".into()
            )),
            "{bad}"
        );
    }
}

#[test]
fn install_pulled_keeps_the_device_owned_folders() {
    let temp = Temp::new();
    let install = temp.0.join("installs").join("u1");
    let staged = temp.0.join("pull").join("u1");
    write(&install.join("install.json"), b"old");
    write(&install.join("boot.bin"), b"old boot");
    write(&install.join("saves").join("SAVEDATA01").join("x"), b"save");
    write(&install.join("logs").join("launcher.log"), b"log");
    write(&staged.join("install.json"), b"new");
    write(&staged.join("boot.bin"), b"new boot");

    install_pulled(&staged, &install).unwrap();
    assert!(!staged.exists());
    assert_eq!(fs::read(install.join("install.json")).unwrap(), b"new");
    assert_eq!(fs::read(install.join("boot.bin")).unwrap(), b"new boot");
    assert_eq!(
        fs::read(install.join("saves").join("SAVEDATA01").join("x")).unwrap(),
        b"save"
    );
    assert_eq!(
        fs::read(install.join("logs").join("launcher.log")).unwrap(),
        b"log"
    );
    assert!(!install.with_extension("replaced").exists());

    // A staged folder without install.json is refused and nothing moves.
    let half = temp.0.join("pull").join("u2");
    write(&half.join("boot.bin"), b"half");
    assert!(install_pulled(&half, &temp.0.join("installs").join("u2")).is_err());
    assert!(half.join("boot.bin").is_file());
}

#[test]
fn install_all_pulled_takes_each_game_the_hub_sent() {
    let temp = Temp::new();
    let dirs = temp.dirs();
    let staged = temp.0.join("pull");
    write(&staged.join("u1").join("install.json"), b"{}");
    write(&staged.join("u3").join("install.json"), b"{}");
    write(&staged.join("u2").join("boot.bin"), b"unfinished");
    assert_eq!(
        install_all_pulled(&staged, &dirs).unwrap(),
        [Game::U1, Game::U3]
    );
    assert!(installed(&dirs, Game::U1));
    assert!(!installed(&dirs, Game::U2));
    assert!(installed(&dirs, Game::U3));
}

#[test]
fn bytes_for_people() {
    let temp = Temp::new();
    write(&temp.0.join("a"), &[0; 1500]);
    write(&temp.0.join("b").join("c"), &[0; 500]);
    assert_eq!(folder_bytes(&temp.0), 2000);
    assert_eq!(folder_bytes(&temp.0.join("missing")), 0);
    assert_eq!(bytes_text(0), "0 B");
    assert_eq!(bytes_text(999), "999 B");
    assert_eq!(bytes_text(512_000), "512 KB");
    assert_eq!(bytes_text(48_500_000), "48 MB");
    assert_eq!(bytes_text(2_460_000_000), "2.4 GB");
}

#[test]
fn the_shortcut_opens_the_launcher() {
    let exe = Path::new("/opt/uncharted/uncharted-app");
    let s = launcher_shortcut(exe, None);
    assert_eq!(s.name, "Uncharted: The Nathan Drake Collection");
    assert_eq!(s.exe, "/opt/uncharted/uncharted-app");
    assert_eq!(s.start_dir, "/opt/uncharted");
    assert_eq!(s.launch_options, "");
    assert_eq!(s.icon, "");
    let s = launcher_shortcut(exe, Some(Path::new("/icons/u.png")));
    assert_eq!(s.icon, "/icons/u.png");
}

#[test]
fn without_a_dump_only_the_icon_is_written() {
    let images = launcher_images(None);
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].0, Art::Icon);
    assert!(images[0].1.starts_with(b"\x89PNG\r\n\x1a\n"));
    // Until artwork::render is built (1.22) a dump adds nothing either.
    let temp = Temp::new();
    assert_eq!(launcher_images(Some(&temp.0)).len(), 1);
}

fn map_get<'a>(map: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    map.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

#[test]
fn add_to_steam_writes_every_user_and_the_icon() {
    let temp = Temp::new();
    let userdata = temp.0.join("userdata");
    for user in ["0", "123", "456", "notes"] {
        fs::create_dir_all(userdata.join(user)).unwrap();
    }
    assert_eq!(
        steam_users(&userdata),
        [userdata.join("123"), userdata.join("456")]
    );
    let exe = temp.0.join("app").join("uncharted-app");
    let images = launcher_images(None);

    // Steam open: refused, nothing written.
    let err = add_to_steam(&userdata, &exe, &images, &|| true).unwrap_err();
    assert_eq!(err, Error::Steam("close Steam first".into()));
    assert!(!userdata.join("123").join("config").exists());

    let added = add_to_steam(&userdata, &exe, &images, &|| false).unwrap();
    let app_id = launcher_shortcut(&exe, None).app_id();
    assert_eq!(added.app_id, app_id);
    assert_eq!(
        added.shortcuts,
        ["123", "456"].map(|u| userdata.join(u).join("config").join("shortcuts.vdf"))
    );
    let icon =
        |u: &str| artwork::grid_dir(&userdata.join(u)).join(artwork::file_name(Art::Icon, app_id));
    assert_eq!(added.images, [icon("123"), icon("456")]);
    assert_eq!(fs::read(icon("456")).unwrap(), artwork::emblem_png());

    let doc = vdf::read(&fs::read(&added.shortcuts[1]).unwrap()).unwrap();
    let Some(Value::Map(list)) = map_get(&doc, "shortcuts") else {
        panic!("no shortcuts map");
    };
    let Value::Map(entry) = &list[0].1 else {
        panic!("not a map");
    };
    assert_eq!(
        map_get(entry, "AppName"),
        Some(&Value::Str(SHORTCUT_NAME.into()))
    );
    assert_eq!(
        map_get(entry, "icon"),
        Some(&Value::Str(icon("123").to_string_lossy().into_owned()))
    );
    assert_eq!(map_get(entry, "appid"), Some(&Value::Int(app_id)));

    // Again: the same entry is updated, not added twice.
    add_to_steam(&userdata, &exe, &images, &|| false).unwrap();
    let doc = vdf::read(&fs::read(&added.shortcuts[0]).unwrap()).unwrap();
    let Some(Value::Map(list)) = map_get(&doc, "shortcuts") else {
        panic!("no shortcuts map");
    };
    assert_eq!(list.len(), 1);
    assert!(added.shortcuts[0]
        .with_file_name("shortcuts.vdf.bak")
        .is_file());

    // No Steam here at all.
    let err = add_to_steam(&temp.0.join("none"), &exe, &images, &|| false).unwrap_err();
    assert!(matches!(err, Error::Steam(m) if m.starts_with("no Steam user folder")));
}

#[test]
fn userdata_is_the_first_root_that_exists() {
    let temp = Temp::new();
    let vars = Vars {
        home: temp.0.clone(),
        data: temp.0.join("data"),
        config: temp.0.join("config"),
    };
    assert_eq!(userdata(Os::Linux, &vars), None);
    fs::create_dir_all(temp.0.join(".steam/steam/userdata")).unwrap();
    assert_eq!(
        userdata(Os::Linux, &vars),
        Some(temp.0.join(".steam/steam/userdata"))
    );
    fs::create_dir_all(temp.0.join(".local/share/Steam/userdata")).unwrap();
    assert_eq!(
        userdata(Os::Linux, &vars),
        Some(temp.0.join(".local/share/Steam/userdata"))
    );
    assert_eq!(userdata(Os::Android, &vars), None);
}

#[test]
fn steam_quit_and_start_commands() {
    let linux = steam_command(Os::Linux, true).unwrap();
    assert_eq!(linux.program, PathBuf::from("steam"));
    assert_eq!(linux.args, ["-shutdown"]);
    assert!(steam_command(Os::Linux, false).unwrap().args.is_empty());
    let windows = steam_command(Os::Windows, true).unwrap();
    assert_eq!(
        windows.program,
        PathBuf::from(r"C:\Program Files (x86)\Steam\steam.exe")
    );
    assert_eq!(windows.args, ["-shutdown"]);
    assert_eq!(steam_command(Os::Mac, false).unwrap().args, ["-a", "Steam"]);
    assert_eq!(steam_command(Os::Ios, true), None);
}

#[test]
fn steam_running_from_tasklist_and_proc() {
    // `tasklist /NH` output with and without Steam (bbport_steam.py lower-cases it).
    assert!(tasklist_shows_steam(
        b"steam.exe                    10912 Console                    1    123,456 K\r\n"
    ));
    assert!(tasklist_shows_steam(b"Steam.EXE 1 Console 1 1 K"));
    assert!(!tasklist_shows_steam(
        b"INFO: No tasks are running which match the specified criteria.\r\n"
    ));

    let temp = Temp::new();
    write(&temp.0.join("1").join("comm"), b"systemd\n");
    write(&temp.0.join("42").join("comm"), b"steamwebhelper\n");
    write(&temp.0.join("self").join("comm"), b"steam\n");
    assert!(!proc_shows_steam(&temp.0));
    write(&temp.0.join("4242").join("comm"), b"steam\n");
    assert!(proc_shows_steam(&temp.0));
    assert!(!proc_shows_steam(&temp.0.join("missing")));
}

#[cfg(feature = "hub")]
mod hub_client {
    use super::*;
    use crate::hub::{self, Found, PairingLimiter, SystemClock, TcpTransport};
    use crate::setup::client::{self, HubChoice};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
    use std::time::Duration;

    #[test]
    fn typed_hub_addresses() {
        let choice = client::hub_address(" 192.168.1.20:47120 ").unwrap();
        assert_eq!(
            choice,
            HubChoice {
                label: "192.168.1.20:47120".into(),
                name: "192.168.1.20:47120".into(),
                addr: "192.168.1.20:47120".parse().unwrap(),
            }
        );
        assert!(client::hub_address("[fe80::1]:47120").is_ok());
        assert_eq!(
            client::hub_address("192.168.1.20"),
            Err(Error::Hub(
                "add the hub's port after its address, as 192.168.1.20:47120".into()
            ))
        );
        assert_eq!(
            client::hub_address("hub.local:47120"),
            Err(Error::Hub(
                "type the hub's address and port, as 192.168.1.20:47120".into()
            ))
        );
        assert!(matches!(
            client::hub_address("8.8.8.8:47120"),
            Err(Error::Hub(m)) if m.contains("not on the home network")
        ));
    }

    #[test]
    fn found_hubs_that_serve_this_device() {
        let found = [
            Found {
                hub: "desktop".into(),
                addr: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)),
                port: 47120,
                targets: vec![Os::Linux, Os::Windows],
            },
            Found {
                hub: "mac".into(),
                addr: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 30)),
                port: 47121,
                targets: vec![Os::Mac, Os::Ios],
            },
        ];
        let choices = client::choices(&found, Os::Linux);
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].label, "desktop at 192.168.1.20:47120");
        assert_eq!(choices[0].name, "desktop");
        assert!(client::choices(&found, Os::Android).is_empty());
    }

    /// A hub on loopback that pairs with `code` and serves `prepared` once.
    fn fake_hub(
        code: &'static str,
        prepared: PathBuf,
    ) -> (SocketAddr, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut t = TcpTransport::from_stream(stream).unwrap();
            let mut limiter = PairingLimiter::default();
            let Ok(peer) = hub::pair_limited(&mut t, code, "client", &mut limiter, &SystemClock)
            else {
                return;
            };
            let _ = hub::serve_game(&mut t, &peer, &prepared, Os::current());
        });
        (addr, handle)
    }

    #[test]
    fn pair_and_pull_installs_what_the_hub_prepared() {
        let temp = Temp::new();
        let prepared = temp.0.join("hub-installs");
        write(
            &prepared.join("u2").join("install.json"),
            b"{\"game\":\"u2\"}\n",
        );
        write(&prepared.join("u2").join("boot.bin"), b"BBPROBE2 synthetic");
        let (addr, hub) = fake_hub("246810", prepared);

        let dirs = temp.dirs();
        write(&dirs.install("u2").join("saves").join("keep"), b"my save");
        let mut t = TcpTransport::connect(addr, Duration::from_secs(5)).unwrap();
        let pulled = client::pair_and_pull(
            &mut t,
            "desktop",
            "246 810",
            &dirs,
            Os::current(),
            &SystemClock,
        )
        .unwrap();
        hub.join().unwrap();
        assert_eq!(pulled.games, [Game::U2]);
        assert!(pulled.bytes > 0);
        assert!(installed(&dirs, Game::U2));
        assert_eq!(
            fs::read(dirs.install("u2").join("boot.bin")).unwrap(),
            b"BBPROBE2 synthetic"
        );
        assert_eq!(
            fs::read(dirs.install("u2").join("saves").join("keep")).unwrap(),
            b"my save"
        );
        assert!(!client::staging(&dirs).exists());
        if cfg!(windows) {
            // No protected peer storage on Windows yet: paired for this pull only.
            assert!(pulled.peer_note.is_some());
            assert!(!is_first_run(&dirs));
        } else {
            assert_eq!(pulled.peer_note, None);
            assert!(has_peers(&dirs));
        }
    }

    #[test]
    fn a_wrong_code_pairs_nothing_and_installs_nothing() {
        let temp = Temp::new();
        let prepared = temp.0.join("hub-installs");
        write(&prepared.join("u1").join("install.json"), b"{}\n");
        let (addr, hub) = fake_hub("111111", prepared);
        let dirs = temp.dirs();
        let mut t = TcpTransport::connect(addr, Duration::from_secs(5)).unwrap();
        let err = client::pair_and_pull(
            &mut t,
            "desktop",
            "222222",
            &dirs,
            Os::current(),
            &SystemClock,
        )
        .unwrap_err();
        hub.join().unwrap();
        assert_eq!(err, Error::Hub("the pairing code did not match".into()));
        assert!(!installed(&dirs, Game::U1));
        assert!(!has_peers(&dirs));
    }
}
