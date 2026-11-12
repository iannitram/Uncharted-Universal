//! Per-command tests on temp-dir fixtures (no real dump, no Steam, no network). A command whose
//! core function is still a stub is tested up to that call: its arguments, folders and plans.

use super::*;
use crate::fixtures::{self, TempDir};
use crate::tests::{args, ctx};
use crate::{run, Ctx, NOT_BUILT, OK, USAGE};
use std::ffi::OsStr;
use uncharted_core::platform::Os;

fn exec(list: &[&str], ctx: &Ctx) -> Outcome {
    run(&args(list), ctx)
}

/// A machine whose home, data and config folders (on every OS) are under `dir`.
fn home(dir: &Path) -> Ctx {
    let mut ctx = ctx();
    let text = |sub: &str| dir.join(sub).display().to_string();
    for (name, value) in [
        ("HOME", text("home")),
        ("XDG_DATA_HOME", text("data")),
        ("XDG_CONFIG_HOME", text("config")),
        ("USERPROFILE", text("home")),
        ("LOCALAPPDATA", text("data")),
        ("APPDATA", text("config")),
    ] {
        ctx.env.insert(name.into(), value);
    }
    ctx
}

fn set(mut ctx: Ctx, name: &str, value: impl AsRef<OsStr>) -> Ctx {
    ctx.env
        .insert(name.into(), value.as_ref().to_string_lossy().into_owned());
    ctx
}

// ---- check ----

#[test]
fn check_reports_a_good_dump_and_exits_zero() {
    let dump = fixtures::dump("check-good", "01.00");
    let out = exec(&["check", &dump.path().display().to_string()], &ctx());
    assert_eq!(out.code, OK, "{}", out.stderr);
    assert!(out.stdout.contains("title: CUSA02320"));
    assert!(out.stdout.contains("version: 01.00"));
    assert!(out.stdout.contains("decrypted: yes"));
    for game in Game::ALL {
        assert!(out.stdout.contains(game.executable()));
    }
    assert!(out.stdout.ends_with("ready to prepare\n"));
}

#[test]
fn check_lists_problems_and_exits_one() {
    let dump = fixtures::dump("check-problems", "01.02");
    fs::remove_dir_all(dump.path().join("u3data")).unwrap();
    let out = exec(&["check", &dump.path().display().to_string()], &ctx());
    assert_eq!(out.code, FAILED);
    assert!(out.stdout.contains("2 problems:"));
    assert!(out
        .stdout
        .contains("  - The dump is missing the u3data folder."));
    assert!(out.stdout.contains("version 1.02"));
}

#[test]
fn check_takes_bb_game_dir_and_refuses_a_folder_without_param_sfo() {
    let empty = TempDir::new("check-empty");
    let out = exec(&["check"], &set(ctx(), "BB_GAME_DIR", empty.path()));
    assert_eq!(out.code, FAILED);
    assert!(out
        .stderr
        .contains("This dump is not usable: The dump is missing sce_sys/param.sfo."));
}

#[test]
fn check_without_a_folder_or_too_many_is_a_usage_error() {
    let out = exec(&["check"], &ctx());
    assert_eq!(out.code, USAGE);
    assert!(out
        .stderr
        .contains("give the dump folder or set BB_GAME_DIR"));
    assert!(out.stderr.contains("usage: uncharted-universal check"));
    assert_eq!(exec(&["check", "a", "b"], &ctx()).code, USAGE);
    assert_eq!(exec(&["check", "--deep", "a"], &ctx()).code, USAGE);
}

// ---- prepare ----

#[test]
fn prepare_needs_a_known_game() {
    assert!(exec(&["prepare"], &ctx())
        .stderr
        .contains("name the game: u1, u2 or u3"));
    assert_eq!(exec(&["prepare", "u4"], &ctx()).code, USAGE);
    assert_eq!(exec(&["prepare", "u1", "--dump"], &ctx()).code, USAGE);
    assert_eq!(exec(&["prepare", "u1", "--fast"], &ctx()).code, USAGE);
}

#[test]
fn prepare_without_user_folders_fails_before_touching_the_dump() {
    let dump = fixtures::dump("prepare-env", "01.00");
    let out = exec(
        &[
            "prepare",
            "u1",
            "--dump",
            &dump.path().display().to_string(),
        ],
        &ctx(),
    );
    assert_eq!(out.code, FAILED);
    assert!(out.stderr.contains("is not set"), "{}", out.stderr);
}

// ---- play ----

#[test]
fn play_needs_a_known_game_and_a_valid_keep() {
    assert_eq!(exec(&["play"], &ctx()).code, USAGE);
    assert_eq!(exec(&["play", "u9"], &ctx()).code, USAGE);
    let out = exec(&["play", "u1", "--keep", "both"], &ctx());
    assert_eq!(out.code, USAGE);
    assert!(out.stderr.contains("--keep is server or local, not 'both'"));
    // Arguments after `--` are the runtime's, even ones that look like ours.
    let out = exec(&["play", "u1", "--", "--help"], &ctx());
    assert_eq!(out.code, FAILED, "{}", out.stderr);
}

#[test]
fn play_plan_uses_the_selected_save_slot_the_profile_and_the_players_variables() {
    let dir = TempDir::new("play-plan");
    let ctx = set(home(dir.path()), "BB_GPU_MERGE", "1");
    let ctx = set(ctx, "BB_RUNTIME", dir.path().join("my-run"));
    let ctx = set(ctx, "PATH", "not passed on twice");
    let plan = play_plan(Game::U2, &ctx).unwrap();

    let vars = Vars::from_lookup(ctx.os, |n| ctx.var(n)).unwrap();
    let install = AppDirs::new(&vars).install("u2");
    let user_dir = saves::user_dir(ctx.os, &vars, &install).unwrap();
    assert_eq!(plan.install, install);
    assert_eq!(plan.saves_dir, user_dir.join("Uncharted 2"));
    assert_eq!(plan.program, dir.path().join("my-run"));
    let get = |name: &str| {
        plan.env
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("BB_EXE"), Some("u2"));
    assert_eq!(
        get("BB_USER_DIR"),
        Some(user_dir.display().to_string().as_str())
    );
    assert_eq!(get("BB_GPU_MERGE"), Some("1"), "the player's value wins");
    assert_eq!(get("PATH"), None, "only BB_ variables are the player's");
}

#[test]
fn play_plan_finds_the_runtime_beside_this_program() {
    let dir = TempDir::new("play-beside");
    let mut ctx = home(dir.path());
    ctx.exe = dir.path().join("bin").join("uncharted-universal");
    let plan = play_plan(Game::U1, &ctx).unwrap();
    assert_eq!(
        plan.program,
        dir.path()
            .join("bin")
            .join(format!("uncharted-run{}", std::env::consts::EXE_SUFFIX))
    );
}

#[test]
fn play_plan_on_a_phone_is_refused() {
    let dir = TempDir::new("play-phone");
    let mut ctx = home(dir.path());
    ctx.os = Os::Android;
    assert!(matches!(
        play_plan(Game::U1, &ctx),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn the_runner_clears_ld_preload_and_passes_extra_arguments() {
    let runner = RuntimeRunner {
        program: PathBuf::from("uncharted-run"),
        extra_args: vec!["--timeout".into(), "5".into()],
    };
    let command = runner.command(&[
        ("LD_PRELOAD".into(), "/steam/overlay.so".into()),
        ("BB_EXE".into(), "u3".into()),
    ]);
    let envs: Vec<_> = command.get_envs().collect();
    assert!(envs.contains(&(OsStr::new("LD_PRELOAD"), None)));
    assert!(envs.contains(&(OsStr::new("BB_EXE"), Some(OsStr::new("u3")))));
    let args: Vec<_> = command.get_args().collect();
    assert_eq!(args, [OsStr::new("--timeout"), OsStr::new("5")]);
}

#[test]
fn a_missing_runtime_is_a_launch_error() {
    let dir = TempDir::new("no-runtime");
    let runner = RuntimeRunner {
        program: dir.path().join("missing-runtime"),
        extra_args: Vec::new(),
    };
    assert!(matches!(runner.run(Game::U1, &[]), Err(Error::Launch(_))));
}

#[test]
fn the_local_relay_says_save_relay_is_not_linked() {
    let path = Path::new("saves");
    assert_eq!(
        LocalRelay.pull(path, None).unwrap(),
        Sync::Done(NO_RELAY.into())
    );
    assert_eq!(
        LocalRelay.push(path, Some(Keep::Local)).unwrap(),
        Sync::Done(NO_RELAY.into())
    );
}

#[test]
fn a_conflict_takes_keep_else_this_device_when_nobody_can_answer() {
    assert_eq!(choose_keep(Some(Keep::Server), false), Keep::Server);
    assert_eq!(choose_keep(None, false), Keep::Local);
}

#[test]
fn the_games_exit_status_is_passed_on() {
    assert_eq!(exit_status(0), 0);
    assert_eq!(exit_status(101), 101);
    assert_eq!(exit_status(-1), FAILED);
    assert_eq!(exit_status(0xC000_0005_u32 as i32), FAILED);
}

// ---- steam-add ----

#[test]
fn the_shortcut_plays_the_collection_or_the_named_game() {
    let exe = Path::new("/opt/uu/uncharted-universal");
    let collection = shortcut_for(None, exe);
    assert_eq!(collection.name, COLLECTION_TITLE);
    assert_eq!(collection.exe, exe.display().to_string());
    assert_eq!(
        collection.start_dir,
        Path::new("/opt/uu").display().to_string()
    );
    assert_eq!(collection.launch_options, "play u1");
    assert_eq!(collection.icon, collection.exe);
    let u3 = shortcut_for(Some(Game::U3), exe);
    assert_eq!(u3.name, "Uncharted 3");
    assert_eq!(u3.launch_options, "play u3");
}

#[test]
fn steam_user_folders_are_account_ids_only() {
    let userdata = TempDir::new("userdata");
    for name in ["0", "123", "anonymous", "45"] {
        fs::create_dir(userdata.path().join(name)).unwrap();
    }
    fs::write(userdata.path().join("678"), b"a file").unwrap();
    assert_eq!(
        user_dirs(userdata.path()),
        [userdata.path().join("123"), userdata.path().join("45")]
    );
    assert!(user_dirs(&userdata.path().join("missing")).is_empty());
}

#[test]
fn steam_add_checks_its_arguments() {
    assert_eq!(exec(&["steam-add", "u4"], &ctx()).code, USAGE);
    assert_eq!(exec(&["steam-add", "--userdata"], &ctx()).code, USAGE);
    assert_eq!(exec(&["steam-add", "u1", "u2"], &ctx()).code, USAGE);
}

#[test]
fn steam_add_reaches_the_shortcut_writer() {
    // Steam is "running": once `steam::write_shortcut` is built it refuses to write; while it is
    // a stub the command says which piece is missing. Either way nothing is written.
    let userdata = TempDir::new("steam-running");
    fs::create_dir(userdata.path().join("123")).unwrap();
    let mut ctx = ctx();
    ctx.steam_running = Box::new(|| true);
    let out = exec(
        &[
            "steam-add",
            "--userdata",
            &userdata.path().display().to_string(),
        ],
        &ctx,
    );
    assert!(out.code == NOT_BUILT || out.code == FAILED, "{out:?}");
    assert!(
        out.stderr.contains("not built yet: steam::") || out.stderr.contains("close Steam"),
        "{}",
        out.stderr
    );
    assert!(!userdata.path().join("123/config/shortcuts.vdf").exists());
}

// ---- patches ----

const PATCH_XML: &str = r#"<?xml version="1.0"?>
<Patches><TitleID><ID>CUSA02320</ID></TitleID>
<Metadata Name="Sixty" Author="Tester" AppVer="01.00" AppElf="eboot.bin" isEnabled="true">
<PatchList><Line Type="bytes" Address="0x00401000" Value="01 02"/></PatchList></Metadata>
<Metadata Name="Old build" AppVer="01.02" AppElf="eboot.bin" isEnabled="true">
<PatchList><Line Type="bytes" Address="0x00401000" Value="03"/></PatchList></Metadata>
<Metadata Name="Wide" AppVer="01.00" AppElf="big3-ps4_Shipping.elf" isEnabled="false">
<PatchList><Line Type="bytes" Address="0x00401000" Value="04"/></PatchList></Metadata>
<Metadata Name="Elsewhere" AppVer="01.00" AppElf="other.elf" isEnabled="true">
<PatchList><Line Type="bytes" Address="0x00401000" Value="05"/></PatchList></Metadata>
</Patches>"#;

#[test]
fn patches_lists_every_patch_per_game_with_its_state() {
    let dump = fixtures::dump("patches-dump", "01.00");
    let dir = TempDir::new("patches-dir");
    fs::write(dir.path().join("community.xml"), PATCH_XML).unwrap();
    fs::write(dir.path().join("notes.txt"), "not a patch").unwrap();
    let out = exec(
        &[
            "patches",
            &dump.path().display().to_string(),
            "--patches-dir",
            &dir.path().display().to_string(),
        ],
        &ctx(),
    );
    assert_eq!(out.code, OK, "{}", out.stderr);
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(
        lines,
        [
            "Uncharted 1 (eboot.bin):",
            "  [on ] Sixty by Tester (community.xml/Sixty)",
            "  [n/a] Old build (community.xml/Old build): This patch is for a different title, executable or revision.",
            "Uncharted 3 (big3-ps4_Shipping.elf):",
            "  [off] Wide (community.xml/Wide)",
            "other executables:",
            "  [n/a] Elsewhere (community.xml/Elsewhere): This patch is for a different title, executable or revision.",
        ]
    );
}

#[test]
fn patches_follows_the_players_selection_and_reports_a_bad_file() {
    let dump = fixtures::dump("patches-select", "01.00");
    let dir = TempDir::new("patches-select-dir");
    fs::write(dir.path().join("a.xml"), PATCH_XML).unwrap();
    fs::write(dir.path().join("b.xml"), "<Patches><Metadata></Patches>").unwrap();
    fs::write(
        dir.path().join("patches.json"),
        r#"{"enabled": ["a.xml/Wide"], "disabled": ["a.xml/Sixty"]}"#,
    )
    .unwrap();
    let ctx = set(ctx(), "BB_GAME_DIR", dump.path());
    let ctx = set(ctx, "BB_PATCHES_DIR", dir.path());
    let out = exec(&["patches"], &ctx);
    assert_eq!(out.code, FAILED);
    assert!(out.stderr.starts_with("b.xml: Patch: "), "{}", out.stderr);
    assert!(out.stdout.contains("  [off] Sixty by Tester (a.xml/Sixty)"));
    assert!(out.stdout.contains("  [on ] Wide (a.xml/Wide)"));
}

#[test]
fn patches_without_a_patch_folder_has_nothing_to_list() {
    let dump = fixtures::dump("patches-none", "01.00");
    let out = exec(&["patches", &dump.path().display().to_string()], &ctx());
    assert_eq!(out.code, OK);
    assert!(out.stdout.contains("no third-party patches"));
    assert_eq!(exec(&["patches"], &ctx()).code, USAGE);
}

#[test]
fn a_selection_file_must_be_lists_of_keys() {
    let dir = TempDir::new("selection");
    assert_eq!(
        read_selection(&dir.path().join("missing.json")).unwrap(),
        Selection::default()
    );
    let file = dir.path().join("patches.json");
    fs::write(&file, "\u{feff}{\"enabled\": [\"a/b\"]}").unwrap();
    assert_eq!(read_selection(&file).unwrap().enabled, ["a/b"]);
    for bad in [
        "[]",
        "{\"enabled\": \"a/b\"}",
        "{\"disabled\": [1]}",
        "nope",
    ] {
        fs::write(&file, bad).unwrap();
        assert!(
            matches!(read_selection(&file), Err(Error::Patch(_))),
            "{bad}"
        );
    }
}

// ---- hub ----

/// A Linux machine whose folders are under `dir`.
fn linux_home(dir: &Path) -> Ctx {
    let mut ctx = home(dir);
    ctx.os = Os::Linux;
    ctx
}

#[test]
fn hub_refuses_bad_arguments_with_status_two() {
    let dir = TempDir::new("hub-usage");
    let ctx = linux_home(dir.path());
    for (list, problem) in [
        (&["hub"][..], "name serve, pair or pull"),
        (&["hub", "join"], "unknown hub command 'join'"),
        (&["hub", "pair"], "give the pairing code the hub shows"),
        (
            &["hub", "pair", "12345"],
            "the pairing code is six digits, not '12345'",
        ),
        (&["hub", "pair", "12345a"], "six digits"),
        (
            &["hub", "pair", "123456", "--hub", "nowhere"],
            "--hub takes an address",
        ),
        (&["hub", "pair", "1", "2"], "unexpected argument '2'"),
        (
            &["hub", "serve", "--port", "http"],
            "--port takes a whole number, not 'http'",
        ),
        (
            &["hub", "serve", "--port", "70000"],
            "--port takes a whole number",
        ),
        (
            &["hub", "serve", "--role", "phone"],
            "unknown hub role 'phone'",
        ),
        (
            &["hub", "serve", "--bind", "home"],
            "--bind takes an address, not 'home'",
        ),
        (&["hub", "serve", "--stop-after", "-1"], "--stop-after"),
        (&["hub", "serve", "extra"], "unexpected argument 'extra'"),
        (&["hub", "pull", "--target", "psp"], "unknown target 'psp'"),
        (&["hub", "pull", "--code", "1"], "unknown option '--code'"),
    ] {
        let out = exec(list, &ctx);
        assert_eq!(out.code, USAGE, "{list:?}: {}", out.stderr);
        assert!(out.stderr.contains(problem), "{list:?}: {}", out.stderr);
        assert!(out.stderr.contains("usage: uncharted-universal hub"));
    }
    // Android and iOS are never hubs: there the role must be named.
    let mut phone = linux_home(dir.path());
    phone.os = Os::Android;
    assert!(exec(&["hub", "serve"], &phone)
        .stderr
        .contains("name the hub's role with --role"));
}

#[test]
fn hub_help_shows_every_subcommand() {
    let out = exec(&["hub", "--help"], &ctx());
    assert_eq!(out.code, OK);
    for line in ["hub serve [--builds", "hub pair <code>", "hub pull [--hub"] {
        assert!(out.stdout.contains(line), "{line} missing: {}", out.stdout);
    }
}

#[test]
fn serve_plans_its_folders_role_and_port() {
    let dir = TempDir::new("hub-serve-plan");
    let ctx = set(linux_home(dir.path()), "BB_DEVICE_NAME", "tablet");
    let plan = hub::serve_plan(&[], &ctx).unwrap();
    assert_eq!(
        plan,
        hub::ServePlan {
            name: "tablet".into(),
            role: uncharted_core::recipe::Hub::StrixHalo,
            targets: vec![Os::Linux, Os::Android],
            builds: dir.path().join("data/uncharted-universal/builds"),
            peers: dir.path().join("config/uncharted-universal/peers.json"),
            bind: std::net::IpAddr::from([0, 0, 0, 0]),
            port: 47_210,
            stop_after: None,
        }
    );
    let plan = hub::serve_plan(
        &args(&[
            "--role",
            "desktop",
            "--builds",
            "/b",
            "--bind",
            "127.0.0.1",
            "--port",
            "0",
            "--stop-after",
            "2",
            "--name",
            "desk",
        ]),
        &ctx,
    )
    .unwrap();
    assert_eq!(plan.role, uncharted_core::recipe::Hub::Desktop);
    assert_eq!(plan.targets, [Os::Windows]);
    assert_eq!(plan.builds, PathBuf::from("/b"));
    assert!(plan.bind.is_loopback());
    assert_eq!((plan.port, plan.stop_after), (0, Some(2)));
    assert_eq!(plan.name, "desk");
    for (os, role) in [
        (Os::Windows, "desktop"),
        (Os::Mac, "mac"),
        (Os::Linux, "strix-halo"),
    ] {
        assert_eq!(hub::default_role(os).unwrap().key(), role);
    }
}

#[test]
fn pair_and_pull_plan_their_hub_and_folders() {
    let dir = TempDir::new("hub-pair-plan");
    let ctx = linux_home(dir.path());
    for typed in ["483921", "483 921", "483-921"] {
        let plan = hub::pair_plan(&args(&[typed, "--name", "handheld"]), &ctx).unwrap();
        assert_eq!(plan.code, "483921");
        assert_eq!(plan.hub, None);
        assert_eq!(plan.name, "handheld");
        assert_eq!(
            plan.peers,
            dir.path().join("config/uncharted-universal/peers.json")
        );
    }
    let plan = hub::pair_plan(&args(&["483921", "--hub", "192.168.1.20"]), &ctx).unwrap();
    assert_eq!(plan.hub, Some("192.168.1.20:47210".parse().unwrap()));
    let plan = hub::pair_plan(&args(&["483921", "--hub", "[fd00::2]:9"]), &ctx).unwrap();
    assert_eq!(plan.hub, Some("[fd00::2]:9".parse().unwrap()));

    let plan = hub::pull_plan(&[], &ctx).unwrap();
    assert_eq!(plan.target, Os::Linux);
    assert_eq!(
        plan.into,
        dir.path().join("data/uncharted-universal/staged/linux")
    );
    assert_eq!((plan.hub, plan.from.as_deref()), (None, None));
    let plan = hub::pull_plan(
        &args(&["--target", "windows", "--from", "desk", "--into", "/s"]),
        &ctx,
    )
    .unwrap();
    assert_eq!(plan.target, Os::Windows);
    assert_eq!(plan.from.as_deref(), Some("desk"));
    assert_eq!(plan.into, PathBuf::from("/s"));
}

#[test]
fn a_device_name_comes_from_the_flag_the_variable_or_the_host() {
    let mut windows = ctx();
    windows.os = Os::Windows;
    let name = |list: &[&str], ctx: &Ctx| {
        hub::device_name(&crate::args::parse(&args(list), &["name"], 0).unwrap(), ctx)
    };
    assert_eq!(name(&[], &windows), "uncharted-universal");
    let windows = set(windows, "HOSTNAME", "host");
    assert_eq!(name(&[], &windows), "host");
    let windows = set(windows, "COMPUTERNAME", "DESK");
    assert_eq!(name(&[], &windows), "DESK");
    let windows = set(windows, "BB_DEVICE_NAME", "the desktop");
    assert_eq!(name(&[], &windows), "the desktop");
    assert_eq!(name(&["--name", "desk 2"], &windows), "desk 2");
}

#[test]
fn pull_picks_the_named_or_the_only_hub() {
    assert_eq!(hub::choose_hub(&["tablet"], None), Ok("tablet"));
    assert_eq!(
        hub::choose_hub(&["tablet", "desk"], Some("desk")),
        Ok("desk")
    );
    assert!(hub::choose_hub(&[], None)
        .unwrap_err()
        .contains("not paired with a hub yet"));
    assert_eq!(
        hub::choose_hub(&["tablet", "desk"], None).unwrap_err(),
        "this device is paired with several hubs (tablet, desk); name one with --from"
    );
    assert_eq!(
        hub::choose_hub(&["tablet"], Some("mac")).unwrap_err(),
        "this device is not paired with mac"
    );
}

#[test]
fn hub_addresses_take_an_optional_port() {
    assert_eq!(
        hub::hub_address("10.0.0.5", 47_210),
        Some("10.0.0.5:47210".parse().unwrap())
    );
    assert_eq!(
        hub::hub_address("10.0.0.5:9", 47_210),
        Some("10.0.0.5:9".parse().unwrap())
    );
    assert_eq!(
        hub::hub_address("fe80::1", 1),
        Some("[fe80::1]:1".parse().unwrap())
    );
    assert_eq!(hub::hub_address("tablet.local", 1), None);
}

#[cfg(not(feature = "hub"))]
#[test]
fn without_the_hub_feature_the_commands_say_not_built() {
    let dir = TempDir::new("hub-not-built");
    let ctx = linux_home(dir.path());
    for list in [
        &["hub", "serve"][..],
        &["hub", "pair", "483921"],
        &["hub", "pull"],
    ] {
        let out = exec(list, &ctx);
        assert_eq!(out.code, NOT_BUILT, "{list:?}");
        assert!(out.stderr.contains("(this build has no hub feature)"));
    }
}

#[cfg(feature = "hub")]
#[test]
fn hub_commands_check_the_paired_device_file_first() {
    let dir = TempDir::new("hub-peers");
    let ctx = linux_home(dir.path());
    let pull = exec(&["hub", "pull", "--hub", "127.0.0.1:9"], &ctx);
    assert_eq!(pull.code, FAILED, "{pull:?}");
    // Paired-device storage fails closed on Windows until its ACL work lands (#189).
    let expected = if cfg!(windows) {
        "not implemented on Windows"
    } else {
        "not paired with a hub yet"
    };
    assert!(pull.stderr.contains(expected), "{}", pull.stderr);
    let none = dir.path().join("none").display().to_string();
    let serve = exec(&["hub", "serve", "--builds", &none], &ctx);
    assert_eq!(serve.code, FAILED);
    let expected = if cfg!(windows) {
        "not implemented on Windows"
    } else {
        "no builds folder at"
    };
    assert!(serve.stderr.contains(expected), "{}", serve.stderr);
    // A hub that is not there is a failure, not a hang.
    #[cfg(not(windows))]
    {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = closed.local_addr().unwrap().to_string();
        drop(closed);
        let pair = exec(&["hub", "pair", "483921", "--hub", &addr], &ctx);
        assert_eq!(pair.code, FAILED, "{pair:?}");
    }
}

// ---- fleet ----

fn record(device: &str, build: &str, patch_level: u32, last_sync: u64) -> DeviceRecord {
    DeviceRecord {
        device: device.into(),
        os: Os::Linux,
        build_id: build.into(),
        patch_level,
        last_sync_unix: last_sync,
        free_bytes: 3 << 29,
    }
}

#[test]
fn fleet_without_records_says_so() {
    let dir = TempDir::new("fleet");
    let out = exec(&["fleet"], &home(dir.path()));
    assert_eq!(out.code, OK);
    assert!(out.stdout.contains("no paired devices yet"));
    assert!(out.stdout.contains(FLEET_FILE));
    let empty = dir.path().join("empty.txt");
    fs::write(&empty, "\n\n").unwrap();
    let out = exec(&["fleet", &empty.display().to_string()], &ctx());
    assert_eq!(out.stdout, "no paired devices yet\n");
    assert_eq!(exec(&["fleet", "--device"], &ctx()).code, USAGE);
}

#[test]
fn the_fleet_table_lists_each_device() {
    let mut out = Outcome::default();
    write_fleet(
        &mut out,
        &[
            record("AMD handheld", "b1", 3, 0),
            record("tablet", "b2", 4, 1_700_000_000),
        ],
        None,
    );
    assert_eq!(
        out.stdout,
        "AMD handheld  linux  build b1  patch level 3  last sync never  free 1.5 GiB\n\
         tablet  linux  build b2  patch level 4  last sync 2023-11-14  free 1.5 GiB\n"
    );
    let mut out = Outcome::default();
    write_fleet(&mut out, &[record("tablet", "b2", 4, 1)], Some("desk"));
    assert_eq!(out.code, FAILED);
    assert!(out.stderr.contains("no record for the device 'desk'"));
}

#[test]
fn dates_and_sizes_read_plainly() {
    assert_eq!(date(0), "never");
    assert_eq!(date(1), "1970-01-01");
    assert_eq!(date(951_782_400), "2000-02-29");
    assert_eq!(date(1_700_000_000), "2023-11-14");
    assert_eq!(gib(0), "0.0 GiB");
    assert_eq!(gib(10 << 30), "10.0 GiB");
}

#[test]
fn drift_reads_plainly() {
    assert_eq!(describe_drift(&Drift::None), "same build");
    assert_eq!(
        describe_drift(&Drift::Patches { theirs: 4, ours: 3 }),
        "patch level 4, this device 3"
    );
    assert_eq!(
        describe_drift(&Drift::Build {
            theirs: "a".into(),
            ours: "b".into()
        }),
        "another build at the same patch level"
    );
}

// ---- generate-art ----

#[test]
fn generate_art_writes_the_collection_art() {
    let dir = TempDir::new("art");
    let target = dir.path().join("covers");
    let out = exec(&["generate-art", &target.display().to_string()], &ctx());
    assert_eq!(out.code, OK, "{}", out.stderr);
    assert!(target.join("u1.jpg").is_file());
    assert_eq!(exec(&["generate-art", "a", "b"], &ctx()).code, USAGE);
}
