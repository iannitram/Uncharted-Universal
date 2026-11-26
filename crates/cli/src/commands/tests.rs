//! Per-command tests on temp-dir fixtures (no real dump, no Steam, no network). A command whose
//! core function is still a stub is tested up to that call: its arguments, folders and plans.

use super::*;
use crate::fixtures::{self, TempDir};
use crate::tests::{args, ctx};
use crate::{run, Ctx, NOT_BUILT, OK, USAGE};
use std::ffi::{OsStr, OsString};
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
fn play_without_a_prepared_install_says_to_run_prepare_and_starts_nothing() {
    let dir = TempDir::new("play-unprepared");
    let mut ctx = home(dir.path());
    // No runtime and no save-relay beside this program: nothing could start anyway.
    let bin = dir.path().join("bundle");
    fs::create_dir_all(&bin).unwrap();
    ctx.exe = bin.join("uncharted-universal");
    let install = AppDirs::new(&vars(&ctx).unwrap()).install(Game::U2.key());

    let out = exec(&["play", "u2"], &ctx);
    assert_eq!(out.code, FAILED, "{}", out.stderr);
    assert!(
        out.stderr.contains("Uncharted 2 is not prepared yet")
            && out.stderr.contains("uncharted-universal prepare u2"),
        "{}",
        out.stderr
    );
    assert!(out.stderr.contains(&install.display().to_string()));
    assert!(!out.stdout.contains("before play"), "{}", out.stdout);
    assert!(
        !out.stdout.contains("quit with exit status"),
        "{}",
        out.stdout
    );

    // A folder without install.json (prepare refused or stopped) is not an install either.
    fs::create_dir_all(&install).unwrap();
    let out = exec(&["play", "u2"], &ctx);
    assert!(out.stderr.contains("not prepared yet"), "{}", out.stderr);

    // With install.json the check passes: play goes on (here to a runtime that is missing).
    fs::write(install.join(prepare::MANIFEST_FILE), "{}").unwrap();
    let out = exec(&["play", "u2"], &ctx);
    assert!(!out.stderr.contains("not prepared yet"), "{}", out.stderr);

    // BB_RUNTIME, a runtime the player chose, skips the check.
    fs::remove_file(install.join(prepare::MANIFEST_FILE)).unwrap();
    let ctx = set(ctx, "BB_RUNTIME", bin.join("custom-runtime"));
    let out = exec(&["play", "u2"], &ctx);
    assert!(!out.stderr.contains("not prepared yet"), "{}", out.stderr);
}

/// A home under `dir` with `games` prepared (`install.json` in each install), on Windows as
/// the packaged loader sees it.
fn prepared(dir: &Path, games: &[Game]) -> (Ctx, AppDirs) {
    let mut ctx = home(dir);
    ctx.os = Os::Windows;
    let dirs = AppDirs::new(&vars(&ctx).unwrap());
    for game in games {
        let install = dirs.install(game.key());
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join(prepare::MANIFEST_FILE), "{}").unwrap();
    }
    (ctx, dirs)
}

fn runner_for(plan: &PlayPlan, extra: &[&str]) -> RuntimeRunner {
    RuntimeRunner {
        program: plan.program.clone(),
        loader: plan.loader.clone(),
        extra_args: extra.iter().map(|arg| (*arg).to_owned()).collect(),
        relay_prompt_stdio: false,
        host: Vec::new(),
    }
}

fn args_of(command: &Command) -> Vec<OsString> {
    command.get_args().map(OsStr::to_os_string).collect()
}

#[test]
fn play_of_a_prepared_install_starts_the_runtime_with_its_boot_image_and_loader_args() {
    // #666: bb-probe got only --guest-argv and the player's arguments, printed its usage line
    // and quit with 1. The boot image is argv[1] (src/probe.c), then the loader options.
    let dir = TempDir::new("play-loader-args");
    let (ctx, dirs) = prepared(dir.path(), &[Game::U2]);
    let dump = dir.path().join("dump folder");
    let user = dir.path().join("user dir");
    let ctx = set(ctx, "BB_GAME_DIR", &dump);
    let ctx = set(ctx, "BB_USER_DIR", &user);
    let plan = play_plan(Game::U2, &ctx).unwrap();
    let install = dirs.install("u2");
    let loader = plan
        .loader
        .as_ref()
        .expect("a prepared install has loader args");
    assert_eq!(loader.prepared_dirs.get(&Game::U2), Some(&install));
    assert!(!loader.prepared_dirs.contains_key(&Game::U3));

    let runner = runner_for(&plan, &["--flag"]);
    let command = runner.command_for(Game::U2, &[], None).unwrap();
    let want: Vec<OsString> = vec![
        install.join("boot-linked.bin").into(),
        "--content-profile".into(),
        install.join("content.bin").into(),
        "--app0".into(),
        dump.clone().into(),
        "--user".into(),
        user.clone().into(),
        "--timeout".into(),
        "0".into(),
        "--flag".into(),
    ];
    assert_eq!(args_of(&command), want);

    // After a LoadExec switch: --guest-argv after the loader options, before the player's.
    let guest = plan.paths.guest_argv_file();
    let command = runner.command_for(Game::U2, &[], Some(&guest)).unwrap();
    let args = args_of(&command);
    assert_eq!(args[..9], want[..9]);
    assert_eq!(
        args[9..],
        [
            OsString::from("--guest-argv"),
            guest.into(),
            "--flag".into()
        ]
    );

    // The trophies prepare wrote into the install are passed; BB_TIMEOUT sets the watchdog.
    let trophy = install.join("trophy");
    fs::create_dir_all(&trophy).unwrap();
    fs::write(
        trophy.join(uncharted_core::trophies::CACHE_FILE),
        "format\t2\n",
    )
    .unwrap();
    let plan = play_plan(Game::U2, &set(ctx, "BB_TIMEOUT", "30")).unwrap();
    let args = args_of(
        &runner_for(&plan, &[])
            .command_for(Game::U2, &[], None)
            .unwrap(),
    );
    assert_eq!(args[7..9], [OsString::from("--timeout"), "30".into()]);
    assert_eq!(args[9..], [OsString::from("--trophies"), trophy.into()]);
}

#[test]
fn play_finds_the_dump_folder_prepare_noted_and_the_games_own_folder() {
    let dir = TempDir::new("play-noted-dump");
    let (ctx, dirs) = prepared(dir.path(), &[Game::U2, Game::U3]);
    let noted = dir.path().join("noted dump");
    uncharted_core::setup::remember_dump(&dirs, &noted).unwrap();
    let plan = play_plan(Game::U2, &ctx).unwrap();
    let loader = plan.loader.as_ref().unwrap();
    assert_eq!(loader.game_dirs.get(&Game::U2), Some(&noted));
    // Without BB_USER_DIR the user folder is the one play already uses.
    assert_eq!(loader.paths.user_dir, plan.paths.user_dir);

    // BB_GAME_DIR_U3 wins for that game only.
    let own = dir.path().join("u3 alone");
    let plan = play_plan(Game::U2, &set(ctx, "BB_GAME_DIR_U3", &own)).unwrap();
    let loader = plan.loader.as_ref().unwrap();
    assert_eq!(loader.game_dirs.get(&Game::U2), Some(&noted));
    assert_eq!(loader.game_dirs.get(&Game::U3), Some(&own));
    let args = args_of(
        &runner_for(&plan, &[])
            .command_for(Game::U3, &[], None)
            .unwrap(),
    );
    assert_eq!(
        args[0],
        dirs.install("u3").join("boot-linked.bin").into_os_string()
    );
    assert_eq!(args[3..5], [OsString::from("--app0"), own.into()]);

    // A LoadExec switch to a game that was never prepared is a plain error.
    let err = runner_for(&plan, &[])
        .command_for(Game::U1, &[], None)
        .unwrap_err();
    assert!(
        err.to_string().contains("Uncharted 1 is not prepared yet"),
        "{err}"
    );
}

#[test]
fn play_of_a_prepared_install_without_a_dump_folder_stops_before_the_save_sync() {
    let dir = TempDir::new("play-no-dump");
    let (mut ctx, _) = prepared(dir.path(), &[Game::U2]);
    let bin = dir.path().join("bundle");
    fs::create_dir_all(&bin).unwrap();
    ctx.exe = bin.join("uncharted-universal.exe");
    let out = exec(&["play", "u2"], &ctx);
    assert_eq!(out.code, FAILED, "{}", out.stderr);
    assert!(
        out.stderr.contains("its dump folder is not known") && out.stderr.contains("BB_GAME_DIR"),
        "{}",
        out.stderr
    );
    assert!(!out.stdout.contains("before play"), "{}", out.stdout);
}

#[test]
fn play_without_an_install_keeps_the_plain_command_line_for_a_chosen_runtime() {
    // #644's case: nothing prepared, a BB_RUNTIME the player chose gets only its own arguments.
    let dir = TempDir::new("play-no-install");
    let (ctx, _) = prepared(dir.path(), &[]);
    let ctx = set(ctx, "BB_RUNTIME", dir.path().join("my-run"));
    let plan = play_plan(Game::U2, &ctx).unwrap();
    assert!(plan.loader.is_none());
    let args = args_of(
        &runner_for(&plan, &["--flag"])
            .command_for(Game::U2, &[], None)
            .unwrap(),
    );
    assert_eq!(args, [OsString::from("--flag")]);
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
    assert_eq!(
        saves::mapped_save_dir(Game::U2, &plan.session(ctx.os).env(Game::U2).unwrap()).unwrap(),
        user_dir.join("Uncharted 2")
    );
    assert_eq!(plan.program, dir.path().join("my-run"));
    assert_eq!(plan.paths.data_dir, install);
    assert_eq!(plan.paths.out_dir, install.join("out"));
    let env = plan.session(ctx.os).env(Game::U2).unwrap();
    let get = |name: &str| env.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
    assert_eq!(
        get("BB_EXE"),
        Some(Game::U2.executable()),
        "run.py sets the executable"
    );
    assert_eq!(
        get("BB_USER_DIR"),
        Some(user_dir.display().to_string().as_str())
    );
    assert_eq!(get("BB_GPU_MERGE"), Some("1"), "the player's value wins");
    assert_eq!(get("PATH"), None, "only BB_ variables are the player's");
}

#[test]
fn play_plan_applies_saved_display_settings() {
    let dir = TempDir::new("play-plan-saved-display");
    let ctx = home(dir.path());
    let settings_path = AppDirs::new(&vars(&ctx).unwrap()).settings();
    fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    fs::write(
        &settings_path,
        r#"{"device_profile":"desktop","frame_rate":"30","render_resolution":"1280x720","anti_aliasing":"fsr1","upscale_preset":"quality","frame_generation":"off"}"#,
    )
    .unwrap();

    let plan = play_plan(Game::U2, &ctx).unwrap();
    assert_eq!(plan.profile.id, "desktop");
    let env = plan.session(ctx.os).env(Game::U2).unwrap();
    let get = |name: &str| env.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
    assert_eq!(get("BB_VBLANK_HZ"), Some("30"));
    assert_eq!(get("BB_RESOLUTION"), Some("1280x720"));
    assert_eq!(get("BB_FSR1"), Some("1"));
    assert_eq!(get("BB_UPSCALER"), Some("off"));
    assert_eq!(get("BB_UPSCALE_QUALITY"), Some("quality"));
    assert_eq!(get("BB_FRAME_GEN"), Some("off"));
}

#[test]
fn play_plan_rereads_saved_settings_on_next_call() {
    let dir = TempDir::new("play-plan-reread");
    let ctx = home(dir.path());
    let settings_path = AppDirs::new(&vars(&ctx).unwrap()).settings();
    fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    fs::write(
        &settings_path,
        r#"{"device_profile":"desktop","frame_rate":"30"}"#,
    )
    .unwrap();

    let first = play_plan(Game::U2, &ctx).unwrap();
    fs::write(
        &settings_path,
        r#"{"device_profile":"desktop","frame_rate":"60"}"#,
    )
    .unwrap();
    let second = play_plan(Game::U2, &ctx).unwrap();

    let first_env = first.session(ctx.os).env(Game::U2).unwrap();
    let second_env = second.session(ctx.os).env(Game::U2).unwrap();
    let find = |env: &[(String, String)], key: &str| {
        env.iter()
            .find(|(n, _)| n == key)
            .map(|(_, v)| v.as_str().to_owned())
    };
    assert_eq!(find(&first_env, "BB_VBLANK_HZ").as_deref(), Some("30"));
    assert_eq!(find(&second_env, "BB_VBLANK_HZ").as_deref(), Some("60"));
}

#[test]
fn play_plan_preserves_extra_and_process_env_precedence() {
    let dir = TempDir::new("play-plan-extra-precedence");
    let ctx = home(dir.path());
    let settings_path = AppDirs::new(&vars(&ctx).unwrap()).settings();
    fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    fs::write(
        &settings_path,
        r#"{"device_profile":"desktop","frame_rate":"30","extra":{"BB_VBLANK_HZ":"60","BB_RESOLUTION_U2":"960x540"}}"#,
    )
    .unwrap();

    let plan = play_plan(Game::U2, &ctx).unwrap();
    let env = plan.session(ctx.os).env(Game::U2).unwrap();
    let get = |name: &str| env.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
    assert_eq!(get("BB_VBLANK_HZ"), Some("60"));
    assert_eq!(get("BB_RESOLUTION_U2"), Some("960x540"));

    let ctx_override = set(ctx, "BB_VBLANK_HZ", "120");
    let override_plan = play_plan(Game::U2, &ctx_override).unwrap();
    let override_env = override_plan
        .session(ctx_override.os)
        .env(Game::U2)
        .unwrap();
    let get_override = |name: &str| {
        override_env
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get_override("BB_VBLANK_HZ"), Some("120"));
}

#[test]
fn play_plan_handles_missing_malformed_and_unknown_profile() {
    let dir = TempDir::new("play-plan-malformed");
    let ctx = home(dir.path());
    let settings_path = AppDirs::new(&vars(&ctx).unwrap()).settings();
    let baseline = play_plan(Game::U2, &ctx).unwrap();
    let baseline_env = baseline.session(ctx.os).env(Game::U2).unwrap();
    assert_eq!(baseline.settings, Settings::default());

    fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    for bad in ["null", "[]", "{broken"] {
        fs::write(&settings_path, bad).unwrap();
        let plan = play_plan(Game::U2, &ctx).unwrap();
        assert_eq!(plan.profile.id, baseline.profile.id, "{bad}");
        assert_eq!(plan.settings, Settings::default(), "{bad}");
        assert_eq!(
            plan.session(ctx.os).env(Game::U2).unwrap(),
            baseline_env,
            "{bad}"
        );
    }

    fs::write(&settings_path, r#"{"device_profile":"unknown"}"#).unwrap();
    let unknown_plan = play_plan(Game::U2, &ctx).unwrap();
    assert_eq!(unknown_plan.profile.id, baseline.profile.id);
    assert_eq!(unknown_plan.settings, Settings::default());
    assert_eq!(
        unknown_plan.session(ctx.os).env(Game::U2).unwrap(),
        baseline_env
    );
}

#[test]
fn play_plan_windows_prefers_packaged_bb_probe_over_uncharted_run() {
    let dir = TempDir::new("play-win-pkg-スペース");
    let pkg = dir.path().join("Uncharted Universal");
    fs::create_dir_all(pkg.join("bin")).unwrap();
    fs::write(pkg.join("uncharted-run.exe"), b"probe").unwrap();
    fs::write(pkg.join("bin").join("bb-probe.exe"), b"native").unwrap();

    let mut ctx = home(dir.path());
    ctx.os = Os::Windows;
    ctx.exe = pkg.join("uncharted-universal.exe");

    let plan = play_plan(Game::U1, &ctx).unwrap();
    assert_eq!(plan.program, pkg.join("bin").join("bb-probe.exe"));

    // BB_RUNTIME override still wins over packaged bb-probe.exe.
    let custom = pkg.join("custom loader.exe");
    let ctx_override = set(ctx, "BB_RUNTIME", &custom);
    let override_plan = play_plan(Game::U1, &ctx_override).unwrap();
    assert_eq!(override_plan.program, custom);
}

#[test]
fn runtime_runner_missing_loader_fails_actionably_and_preserves_guest_argv() {
    let dir = TempDir::new("runner-missing");
    let missing = dir.path().join("bin").join("bb-probe.exe");
    let guest_argv = dir.path().join("guest argv.txt");
    let runner = RuntimeRunner {
        program: missing.clone(),
        loader: None,
        extra_args: vec!["--flag".into()],
        relay_prompt_stdio: false,
        host: Vec::new(),
    };
    let cmd = runner.command(&[], Some(&guest_argv));
    let cmd_args: Vec<_> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        cmd_args,
        vec!["--guest-argv", &guest_argv.display().to_string(), "--flag"]
    );
    let err = runner.run(Game::U1, &[], None).unwrap_err();
    assert!(
        err.to_string().contains("could not start"),
        "unexpected error: {err}"
    );
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
    let mut steam = ctx();
    steam.os = Os::Linux;
    steam.env.insert(
        "LD_PRELOAD".into(),
        "/home/deck/.local/share/Steam/ubuntu12_64/gameoverlayrenderer.so".into(),
    );
    steam
        .env
        .insert("LD_LIBRARY_PATH".into(), "/steam/runtime/lib".into());
    let runner = RuntimeRunner {
        program: PathBuf::from("uncharted-run"),
        loader: None,
        extra_args: vec!["--timeout".into(), "5".into()],
        relay_prompt_stdio: false,
        host: host_env::changes(steam.os, |name| steam.var(name)),
    };
    let command = runner.command(
        &[
            ("LD_PRELOAD".into(), "/steam/overlay.so".into()),
            ("BB_EXE".into(), "u3".into()),
        ],
        None,
    );
    let envs: Vec<_> = command.get_envs().collect();
    // Even a preload in the runtime's own environment list is taken out.
    assert!(envs.contains(&(OsStr::new("LD_PRELOAD"), None)));
    assert!(envs.contains(&(OsStr::new("LD_LIBRARY_PATH"), None)));
    assert!(envs.contains(&(
        OsStr::new("BB_HOST_LD_PRELOAD"),
        Some(OsStr::new(
            "/home/deck/.local/share/Steam/ubuntu12_64/gameoverlayrenderer.so"
        ))
    )));
    assert!(envs.contains(&(
        OsStr::new("DISABLE_VK_LAYER_VALVE_steam_overlay_1"),
        Some(OsStr::new("1"))
    )));
    assert!(envs.contains(&(OsStr::new("BB_EXE"), Some(OsStr::new("u3")))));
    let args: Vec<_> = command.get_args().collect();
    assert_eq!(args, [OsStr::new("--timeout"), OsStr::new("5")]);
}

#[test]
fn off_linux_the_runner_leaves_the_environment_alone() {
    let mut windows = ctx();
    windows.os = Os::Windows;
    windows
        .env
        .insert("LD_PRELOAD".into(), "/steam/overlay.so".into());
    let runner = RuntimeRunner {
        program: PathBuf::from("uncharted-run"),
        loader: None,
        extra_args: Vec::new(),
        relay_prompt_stdio: false,
        host: host_env::changes(windows.os, |name| windows.var(name)),
    };
    assert!(runner.host.is_empty());
    let command = runner.command(&[("BB_EXE".into(), "u1".into())], None);
    let envs: Vec<_> = command.get_envs().collect();
    assert_eq!(envs, [(OsStr::new("BB_EXE"), Some(OsStr::new("u1")))]);
}

#[test]
fn the_runner_passes_the_guest_argv_file_before_the_extra_arguments() {
    let runner = RuntimeRunner {
        program: PathBuf::from("uncharted-run"),
        loader: None,
        extra_args: vec!["--timeout".into(), "5".into()],
        relay_prompt_stdio: false,
        host: Vec::new(),
    };
    let file = Path::new("out").join("guest-argv.txt");
    let command = runner.command(&[], Some(&file));
    let args: Vec<_> = command.get_args().collect();
    assert_eq!(
        args,
        [
            OsStr::new("--guest-argv"),
            file.as_os_str(),
            OsStr::new("--timeout"),
            OsStr::new("5")
        ]
    );
}

#[test]
fn a_missing_runtime_is_a_launch_error() {
    let dir = TempDir::new("no-runtime");
    let runner = RuntimeRunner {
        program: dir.path().join("missing-runtime"),
        loader: None,
        extra_args: Vec::new(),
        relay_prompt_stdio: false,
        host: Vec::new(),
    };
    assert!(matches!(
        runner.run(Game::U1, &[], None),
        Err(Error::Launch(_))
    ));
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
fn inside_the_appimage_the_shortcut_starts_the_appimage() {
    let mut linux = ctx();
    linux.os = Os::Linux;
    linux.exe = PathBuf::from("/tmp/.mount_UnchaXYZ/uncharted-universal");
    assert_eq!(shortcut_target(&linux), linux.exe);
    linux.env.insert(
        "APPIMAGE".into(),
        "/home/deck/Applications/Uncharted-x86_64.AppImage".into(),
    );
    assert_eq!(
        shortcut_target(&linux),
        PathBuf::from("/home/deck/Applications/Uncharted-x86_64.AppImage")
    );
    let shortcut = shortcut_for(Some(Game::U2), &shortcut_target(&linux));
    assert_eq!(
        shortcut.exe,
        Path::new("/home/deck/Applications/Uncharted-x86_64.AppImage")
            .display()
            .to_string()
    );
    assert_eq!(shortcut.launch_options, "play u2");
    // APPIMAGE means nothing off Linux.
    let mut windows = linux;
    windows.os = Os::Windows;
    windows.exe = PathBuf::from(r"C:\Games\uncharted-windows\uncharted-universal.exe");
    assert_eq!(shortcut_target(&windows), windows.exe);
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
        (&["hub"][..], "name serve, pair, pull or pull-game"),
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
            game_data: dir.path().join("data/uncharted-universal/installs"),
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
        &["hub", "pull-game"],
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
    assert!(
        pull.stderr.contains("not paired with a hub yet"),
        "{}",
        pull.stderr
    );
    let none = dir.path().join("none").display().to_string();
    let serve = exec(&["hub", "serve", "--builds", &none], &ctx);
    assert_eq!(serve.code, FAILED);
    assert!(
        serve.stderr.contains("no builds folder at"),
        "{}",
        serve.stderr
    );
    // A hub that is not there is a failure, not a hang.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = closed.local_addr().unwrap().to_string();
    drop(closed);
    let pair = exec(&["hub", "pair", "483921", "--hub", &addr], &ctx);
    assert_eq!(pair.code, FAILED, "{pair:?}");
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
fn fleet_record_persists_updates_and_reports_drift() {
    let dir = TempDir::new("fleet-record");
    let first = exec(
        &[
            "fleet",
            "record",
            "--device",
            "Desktop",
            "--build",
            "b1",
            "--patch-level",
            "3",
            "--sync-unix",
            "1700000000",
            "--free-bytes",
            "3221225472",
        ],
        &home(dir.path()),
    );
    assert_eq!(first.code, OK, "{}", first.stderr);
    assert!(first.stdout.contains(&format!(
        "Desktop  {}  build b1  patch level 3",
        Os::current().key()
    )));
    assert!(first.stdout.contains("(this device)"));

    let second = exec(
        &[
            "fleet",
            "record",
            "--device",
            "Handheld",
            "--build",
            "b2",
            "--patch-level",
            "5",
            "--sync-unix",
            "1700000100",
        ],
        &home(dir.path()),
    );
    assert_eq!(second.code, OK, "{}", second.stderr);

    let view = exec(&["fleet", "--device", "Desktop"], &home(dir.path()));
    assert_eq!(view.code, OK, "{}", view.stderr);
    assert!(view.stdout.contains("patch level 5, this device 3"));
    assert!(view
        .stdout
        .contains("Handheld made this save with 5 of our fixes; this device has 3."));

    assert_eq!(
        exec(&["fleet", "record", "--build", "b1"], &ctx()).code,
        USAGE
    );
    assert_eq!(
        exec(&["fleet", "record", "--device", "Desktop"], &ctx()).code,
        USAGE
    );
    assert_eq!(
        exec(
            &["fleet", "record", "--device", "Desktop", "--build", "b1", "--os", "beos",],
            &ctx(),
        )
        .code,
        USAGE
    );
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

#[test]
fn an_unbundled_local_run_preserves_the_legacy_empty_save_mapping() {
    struct LegacyRunner;
    impl Runner for LegacyRunner {
        fn run(&self, _: Game, env: &[(String, String)], _: Option<&Path>) -> Result<i32> {
            assert!(env
                .iter()
                .any(|(key, value)| key == "BB_SAVEDATA_DIRS" && value.is_empty()));
            Ok(0)
        }
    }
    let dir = TempDir::new("local-legacy-mapping");
    let mut ctx = home(dir.path());
    ctx.env.insert("BB_SAVEDATA_DIRS".into(), String::new());
    let plan = play_plan(Game::U2, &ctx).unwrap();
    let played = launch::play(
        &UnbundledRelay,
        &LegacyRunner,
        Game::U2,
        &plan.session(Os::Linux),
        &|_, _| panic!("local play cannot conflict"),
    )
    .unwrap();
    assert_eq!(played.exit_code, 0);
    assert!(matches!(played.pulled, Sync::LocalOnly(_)));
    assert!(matches!(played.pushed, Sync::LocalOnly(_)));
}

#[test]
fn direct_play_passes_saved_prompt_choices_to_the_runtime_child() {
    let dir = TempDir::new("direct-prompt-choice");
    let mut ctx = home(dir.path());
    let settings = AppDirs::new(&vars(&ctx).unwrap()).settings();
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    for (json, expected) in [
        (r#"{"button_prompts":"nintendo"}"#, Some("nintendo")),
        (r#"{"button_prompts":"xbox"}"#, Some("xbox")),
        (r#"{"button_prompts":"playstation"}"#, Some("playstation")),
        (r#"{"button_prompts":"keyboard"}"#, Some("keyboard")),
        (r#"{"button_prompts":"automatic"}"#, None),
        (r#"{"button_prompts":"invalid"}"#, None),
        (r#"{"button_prompts":false}"#, None),
        ("broken JSON", None),
    ] {
        fs::write(&settings, json).unwrap();
        let plan = play_plan(Game::U2, &ctx).unwrap();
        let env = plan.session(Os::Windows).env(Game::U2).unwrap();
        let runner = RuntimeRunner {
            program: "synthetic-runtime".into(),
            loader: None,
            extra_args: vec![],
            relay_prompt_stdio: false,
            host: vec![],
        };
        let cmd = runner.command(&env, None);
        let actual = cmd
            .get_envs()
            .find(|(key, _)| *key == "BB_BUTTON_PROMPTS")
            .and_then(|(_, value)| value)
            .and_then(|value| value.to_str());
        assert_eq!(actual, expected, "{json}");
    }
    fs::write(settings, r#"{"button_prompts":"nintendo"}"#).unwrap();
    ctx.env
        .insert("BB_BUTTON_PROMPTS".into(), "playstation".into());
    assert_eq!(
        play_plan(Game::U2, &ctx).unwrap().player_env["BB_BUTTON_PROMPTS"],
        "playstation"
    );
}

#[test]
fn play_uses_bundled_save_relay_beside_executable() {
    let dir = TempDir::new("play-bundled-relay");
    let bin = dir.path().join("bundle");
    let manifests = bin.join("save-relay-manifests");
    fs::create_dir_all(&manifests).unwrap();
    fs::write(
        manifests.join("uncharted-2.json"),
        saves::relay_manifest(Game::U2),
    )
    .unwrap();

    #[cfg(unix)]
    let (relay_name, run_name, relay_body, run_body) = (
        "save-relay",
        "uncharted-run",
        format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$1\" in\n  pull) echo '{{\"ok\":true,\"command\":\"pull\",\"data\":{{\"game\":\"uncharted-2-slot-1\",\"action\":\"pulled\"}}}}' ;;\n  push) echo '{{\"ok\":true,\"command\":\"push\",\"data\":{{\"game\":\"uncharted-2-slot-1\",\"action\":\"pushed\"}}}}' ;;\nesac\n",
            dir.path().join("relay.calls").display()
        ),
        "#!/bin/sh\nexit 0\n".to_owned(),
    );
    #[cfg(windows)]
    let (relay_name, run_name, relay_body, run_body) = (
        "save-relay.exe",
        "uncharted-run.exe",
        // Not executed on Windows without a PE binary; covered by tests/windows_relay.py.
        String::new(),
        String::new(),
    );

    if cfg!(unix) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let relay_path = bin.join(relay_name);
            let run_path = bin.join(run_name);
            fs::write(&relay_path, relay_body).unwrap();
            fs::write(&run_path, run_body).unwrap();
            fs::set_permissions(&relay_path, fs::Permissions::from_mode(0o755)).unwrap();
            fs::set_permissions(&run_path, fs::Permissions::from_mode(0o755)).unwrap();

            let mut ctx = home(dir.path());
            ctx.os = Os::Linux;
            ctx.exe = bin.join("uncharted-universal");
            let install = AppDirs::new(&vars(&ctx).unwrap()).install(Game::U2.key());
            fs::create_dir_all(&install).unwrap();
            fs::write(install.join(prepare::MANIFEST_FILE), "{}").unwrap();
            // A prepared game starts with its dump folder as --app0 (#666).
            let ctx = set(ctx, "BB_GAME_DIR", dir.path().join("dump"));
            let out = exec(&["play", "u2"], &ctx);
            assert_eq!(out.code, OK, "{}", out.stderr);
            assert!(out.stdout.contains("before play: downloaded saves"));
            assert!(out.stdout.contains("after play: uploaded saves"));
            assert!(!out.stdout.contains("no Save Relay"));
            let calls = fs::read_to_string(dir.path().join("relay.calls")).unwrap();
            let lines: Vec<_> = calls.lines().collect();
            assert_eq!(lines.len(), 2);
            assert!(lines[0].starts_with("pull uncharted-2 "));
            assert!(lines[1].starts_with("push uncharted-2 "));
        }
    } else {
        let _ = (relay_name, run_name, relay_body, run_body);
    }
}

// ---- playtest ----

#[test]
fn playtest_validate_and_report_commands() {
    use uncharted_core::playtest::{
        BuildReport, FailureKind, Route, ScriptStep, Section, SectionFailure, SectionResult,
        SectionStatus,
    };

    let dir = TempDir::new("playtest-cli");
    let route_path = dir.path().join("route.json");
    let mut s1 = Section::new("intro", "Train Intro");
    s1.steps = vec![ScriptStep::new("cross", 150)];
    let route = Route {
        game: Game::U2,
        name: "u2-opening".into(),
        sections: vec![s1],
    };
    fs::write(&route_path, route.to_json()).unwrap();

    let out = exec(
        &["playtest", "validate", &route_path.display().to_string()],
        &ctx(),
    );
    assert_eq!(out.code, OK, "{}", out.stderr);
    assert!(out.stdout.contains("u2 'u2-opening': 1 sections"));

    let report_path = dir.path().join("report.json");
    let report = BuildReport {
        build: "v0.3.15".into(),
        game: Game::U2,
        route_name: "u2-opening".into(),
        at_unix: 1_700_000_000,
        sections: vec![SectionResult {
            id: "intro".into(),
            title: "Train Intro".into(),
            status: SectionStatus::Fail,
            attempts: 1,
            recoveries: Vec::new(),
            duration_ms: 500,
            frames: 15,
            avg_fps: Some(30.0),
            min_fps: Some(25.0),
            max_frame_ms: Some(40.0),
            stall_count: 0,
            frozen_frames: 0,
            failures: vec![SectionFailure {
                kind: FailureKind::Crash,
                message: "Crash: test".into(),
            }],
            log_excerpts: vec!["Crash: test".into()],
            screenshot_paths: Vec::new(),
            trophies_unlocked: Vec::new(),
            checkpoint_diff: None,
        }],
    };
    fs::write(&report_path, report.to_json()).unwrap();

    let out = exec(
        &["playtest", "report", &report_path.display().to_string()],
        &ctx(),
    );
    assert_eq!(out.code, FAILED);
    assert!(out
        .stdout
        .contains("# Playtest Report: Uncharted 2 (u2) — FAIL"));
    assert!(out.stdout.contains("## Broken Sections"));

    assert_eq!(exec(&["playtest"], &ctx()).code, USAGE);
    assert_eq!(exec(&["playtest", "unknown", "x"], &ctx()).code, USAGE);
}
