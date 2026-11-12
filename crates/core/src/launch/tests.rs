//! Tests of `launch` with a fake `Relay` and a fake `Runner`. The LoadExec cases mirror the loop
//! of `run.py` `main`: the request file, exit code 101, the next executable and its arguments,
//! the collection-menu switch and the per-game defaults of each start.

use super::*;
use crate::profile;
use crate::savecard::SaveCard;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh temp folder under `std::env::temp_dir()`, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new() -> Temp {
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "uncharted-launch-test-{}-{}",
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

/// One relay call: "pull" or "push", and the answer it carried.
type Call = (&'static str, Option<Keep>);

/// save-relay with scripted answers, recording every call.
#[derive(Default)]
struct FakeRelay {
    pulls: RefCell<VecDeque<Result<Sync>>>,
    pushes: RefCell<VecDeque<Result<Sync>>>,
    calls: RefCell<Vec<Call>>,
}

impl FakeRelay {
    fn new(pulls: Vec<Result<Sync>>, pushes: Vec<Result<Sync>>) -> FakeRelay {
        FakeRelay {
            pulls: RefCell::new(pulls.into()),
            pushes: RefCell::new(pushes.into()),
            calls: RefCell::default(),
        }
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }
}

impl Relay for FakeRelay {
    fn pull(&self, _saves_dir: &Path, resolve: Option<Keep>) -> Result<Sync> {
        self.calls.borrow_mut().push(("pull", resolve));
        self.pulls
            .borrow_mut()
            .pop_front()
            .expect("unexpected pull")
    }

    fn push(&self, _saves_dir: &Path, resolve: Option<Keep>) -> Result<Sync> {
        self.calls.borrow_mut().push(("push", resolve));
        self.pushes
            .borrow_mut()
            .pop_front()
            .expect("unexpected push")
    }
}

/// One scripted run: the exit code, and the LoadExec request it leaves behind, if any.
struct Exit {
    code: i32,
    loadexec: Option<&'static str>,
}

fn exit(code: i32) -> Exit {
    Exit {
        code,
        loadexec: None,
    }
}

fn switch(request: &'static str) -> Exit {
    Exit {
        code: LOADEXEC_EXIT,
        loadexec: Some(request),
    }
}

/// What one start saw.
#[derive(Debug, Clone)]
struct Start {
    game: Game,
    env: BTreeMap<String, String>,
    guest_argv: Option<String>,
    loadexec_present: bool,
}

/// The runtime with scripted exits, recording every start.
struct FakeRunner {
    exits: RefCell<VecDeque<Exit>>,
    starts: RefCell<Vec<Start>>,
}

impl FakeRunner {
    fn new(exits: Vec<Exit>) -> FakeRunner {
        FakeRunner {
            exits: RefCell::new(exits.into()),
            starts: RefCell::default(),
        }
    }

    fn starts(&self) -> Vec<Start> {
        self.starts.borrow().clone()
    }
}

impl Runner for FakeRunner {
    fn run(&self, game: Game, env: &[(String, String)], guest_argv: Option<&Path>) -> Result<i32> {
        let env: BTreeMap<String, String> = env.iter().cloned().collect();
        let file = PathBuf::from(&env["BB_LOADEXEC_FILE"]);
        self.starts.borrow_mut().push(Start {
            game,
            guest_argv: guest_argv.map(|p| fs::read_to_string(p).unwrap()),
            loadexec_present: file.exists(),
            env,
        });
        let next = self.exits.borrow_mut().pop_front().expect("unexpected run");
        if let Some(request) = next.loadexec {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, request).unwrap();
        }
        Ok(next.code)
    }
}

fn card(game: Game, saved_unix: u64) -> SaveCard {
    SaveCard {
        game,
        slot: "SAVEDATA00".to_owned(),
        picture: None,
        title: None,
        detail: None,
        saved_unix,
        play_time_secs: None,
        device: None,
        build_id: None,
    }
}

fn conflict() -> Sync {
    Sync::Conflict(Box::new(SideBySide {
        server: card(Game::U2, 200),
        local: card(Game::U2, 100),
        server_is_newer: true,
    }))
}

fn done(message: &str) -> Result<Sync> {
    Ok(Sync::Done(message.to_owned()))
}

/// A session in `temp` on the desktop profile with `player` as the player's variables.
struct Fixture {
    _temp: Temp,
    settings: Settings,
    player: BTreeMap<String, String>,
    paths: Paths,
}

impl Fixture {
    fn new(player: &[(&str, &str)]) -> Fixture {
        let temp = Temp::new();
        let paths = Paths::in_data_dir(&temp.0);
        Fixture {
            _temp: temp,
            settings: Settings::default(),
            player: player
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            paths,
        }
    }

    fn session(&self, os: Os) -> Session<'_> {
        Session {
            settings: &self.settings,
            profile: profile::by_id("desktop").unwrap(),
            player_env: &self.player,
            paths: &self.paths,
            os,
        }
    }
}

fn never_asked(_: &Sync) -> Keep {
    panic!("no conflict was expected")
}

#[test]
fn clean_pull_play_push() {
    let fx = Fixture::new(&[]);
    let relay = FakeRelay::new(vec![done("pulled")], vec![done("pushed")]);
    let runner = FakeRunner::new(vec![exit(0)]);
    let played = play(
        &relay,
        &runner,
        Game::U2,
        Path::new("saves"),
        &fx.session(Os::Linux),
        &never_asked,
    )
    .unwrap();
    assert_eq!(
        played,
        Played {
            pulled: Sync::Done("pulled".to_owned()),
            exit_code: 0,
            pushed: Sync::Done("pushed".to_owned()),
        }
    );
    assert_eq!(relay.calls(), vec![("pull", None), ("push", None)]);
    assert_eq!(runner.starts().len(), 1);
    assert_eq!(runner.starts()[0].game, Game::U2);
}

#[test]
fn pull_conflict_keep_local_pulls_again_with_the_answer() {
    let fx = Fixture::new(&[]);
    let relay = FakeRelay::new(
        vec![Ok(conflict()), done("kept local")],
        vec![done("pushed")],
    );
    let runner = FakeRunner::new(vec![exit(0)]);
    let asked = RefCell::new(0);
    let choose = |s: &Sync| {
        assert!(matches!(s, Sync::Conflict(c) if c.server_is_newer));
        *asked.borrow_mut() += 1;
        Keep::Local
    };
    let played = play(
        &relay,
        &runner,
        Game::U2,
        Path::new("saves"),
        &fx.session(Os::Linux),
        &choose,
    )
    .unwrap();
    assert_eq!(*asked.borrow(), 1);
    assert_eq!(played.pulled, Sync::Done("kept local".to_owned()));
    assert_eq!(
        relay.calls(),
        vec![("pull", None), ("pull", Some(Keep::Local)), ("push", None)]
    );
}

#[test]
fn pull_conflict_keep_server() {
    let fx = Fixture::new(&[]);
    let relay = FakeRelay::new(
        vec![Ok(conflict()), done("took the server's")],
        vec![done("pushed")],
    );
    let runner = FakeRunner::new(vec![exit(0)]);
    let played = play(
        &relay,
        &runner,
        Game::U3,
        Path::new("saves"),
        &fx.session(Os::Linux),
        &|_| Keep::Server,
    )
    .unwrap();
    assert_eq!(played.pulled, Sync::Done("took the server's".to_owned()));
    assert_eq!(relay.calls()[1], ("pull", Some(Keep::Server)));
    assert_eq!(runner.starts().len(), 1);
}

#[test]
fn offline_pull_plays_on() {
    let fx = Fixture::new(&[]);
    let relay = FakeRelay::new(vec![Ok(Sync::Offline)], vec![Ok(Sync::Offline)]);
    let runner = FakeRunner::new(vec![exit(0)]);
    let played = play(
        &relay,
        &runner,
        Game::U1,
        Path::new("saves"),
        &fx.session(Os::Linux),
        &never_asked,
    )
    .unwrap();
    assert_eq!(played.pulled, Sync::Offline);
    assert_eq!(played.pushed, Sync::Offline);
    assert_eq!(runner.starts().len(), 1);
}

#[test]
fn pull_error_stops_before_the_game_starts() {
    let fx = Fixture::new(&[]);
    let relay = FakeRelay::new(vec![Err(Error::Launch("relay broke".to_owned()))], vec![]);
    let runner = FakeRunner::new(vec![]);
    let err = play(
        &relay,
        &runner,
        Game::U2,
        Path::new("saves"),
        &fx.session(Os::Linux),
        &never_asked,
    )
    .unwrap_err();
    assert_eq!(err, Error::Launch("relay broke".to_owned()));
    assert!(runner.starts().is_empty());
    assert_eq!(relay.calls(), vec![("pull", None)]);
}

#[test]
fn error_on_the_resolved_pull_stops_before_the_game_starts() {
    let fx = Fixture::new(&[]);
    let relay = FakeRelay::new(
        vec![Ok(conflict()), Err(Error::Launch("relay broke".to_owned()))],
        vec![],
    );
    let runner = FakeRunner::new(vec![]);
    let err = play(
        &relay,
        &runner,
        Game::U2,
        Path::new("saves"),
        &fx.session(Os::Linux),
        &|_| Keep::Local,
    )
    .unwrap_err();
    assert_eq!(err.code(), "launch");
    assert!(runner.starts().is_empty());
}

#[test]
fn a_conflict_left_after_the_answer_is_an_error() {
    let fx = Fixture::new(&[]);
    let relay = FakeRelay::new(vec![Ok(conflict()), Ok(conflict())], vec![]);
    let runner = FakeRunner::new(vec![]);
    let err = play(
        &relay,
        &runner,
        Game::U2,
        Path::new("saves"),
        &fx.session(Os::Linux),
        &|_| Keep::Server,
    )
    .unwrap_err();
    assert_eq!(err.code(), "launch");
    assert!(err.to_string().contains("still reports a conflict"));
    assert!(runner.starts().is_empty());
}

#[test]
fn push_error_is_returned_after_the_game_quits() {
    let fx = Fixture::new(&[]);
    let relay = FakeRelay::new(
        vec![done("pulled")],
        vec![Err(Error::Launch("push failed".to_owned()))],
    );
    let runner = FakeRunner::new(vec![exit(0)]);
    let err = play(
        &relay,
        &runner,
        Game::U2,
        Path::new("saves"),
        &fx.session(Os::Linux),
        &never_asked,
    )
    .unwrap_err();
    assert_eq!(err, Error::Launch("push failed".to_owned()));
    assert_eq!(runner.starts().len(), 1);
    assert_eq!(relay.calls(), vec![("pull", None), ("push", None)]);
}

#[test]
fn push_conflict_asks_and_pushes_again() {
    let fx = Fixture::new(&[]);
    let relay = FakeRelay::new(
        vec![done("pulled")],
        vec![Ok(conflict()), done("pushed local")],
    );
    let runner = FakeRunner::new(vec![exit(0)]);
    let played = play(
        &relay,
        &runner,
        Game::U2,
        Path::new("saves"),
        &fx.session(Os::Linux),
        &|_| Keep::Local,
    )
    .unwrap();
    assert_eq!(played.pushed, Sync::Done("pushed local".to_owned()));
    assert_eq!(
        relay.calls(),
        vec![("pull", None), ("push", None), ("push", Some(Keep::Local))]
    );
}

#[test]
fn exit_code_is_passed_through() {
    for code in [1, -1, 3, 255] {
        let fx = Fixture::new(&[]);
        let relay = FakeRelay::new(vec![done("pulled")], vec![done("pushed")]);
        let runner = FakeRunner::new(vec![exit(code)]);
        let played = play(
            &relay,
            &runner,
            Game::U2,
            Path::new("saves"),
            &fx.session(Os::Linux),
            &never_asked,
        )
        .unwrap();
        assert_eq!(played.exit_code, code);
    }
}

#[test]
fn runner_error_stops_before_the_push() {
    struct Broken;
    impl Runner for Broken {
        fn run(&self, _: Game, _: &[(String, String)], _: Option<&Path>) -> Result<i32> {
            Err(Error::Launch("could not start".to_owned()))
        }
    }
    let fx = Fixture::new(&[]);
    let relay = FakeRelay::new(vec![done("pulled")], vec![]);
    let err = play(
        &relay,
        &Broken,
        Game::U2,
        Path::new("saves"),
        &fx.session(Os::Linux),
        &never_asked,
    )
    .unwrap_err();
    assert_eq!(err.code(), "launch");
    assert_eq!(relay.calls(), vec![("pull", None)]);
}

#[test]
fn loadexec_switch_starts_the_named_game_with_its_arguments() {
    let fx = Fixture::new(&[]);
    let runner = FakeRunner::new(vec![
        switch("/app0/big2-ps4_Shipping.elf\nbig2-ps4_Shipping.elf\nispartofuccollection"),
        exit(0),
    ]);
    let code = run_until_quit(&runner, Game::U1, &fx.session(Os::Linux)).unwrap();
    assert_eq!(code, 0);
    let starts = runner.starts();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0].game, Game::U1);
    assert_eq!(starts[0].guest_argv, None);
    assert_eq!(starts[0].env["BB_EXE"], "eboot.bin");
    assert_eq!(starts[0].env["BB_SKIP_COLLECTION_MENU"], "1");
    assert_eq!(starts[1].game, Game::U2);
    assert_eq!(starts[1].env["BB_EXE"], "big2-ps4_Shipping.elf");
    assert_eq!(
        starts[1].guest_argv.as_deref(),
        Some("/app0/big2-ps4_Shipping.elf\nbig2-ps4_Shipping.elf\nispartofuccollection\n")
    );
    // Each start gets its own game's defaults (U1: no tiny readback, U2: on).
    assert_eq!(starts[0].env["BB_TINY_READBACK"], "0");
    assert_eq!(starts[1].env["BB_TINY_READBACK"], "1");
    // The request file is removed before every start.
    assert!(starts.iter().all(|s| !s.loadexec_present));
}

#[test]
fn back_to_the_collection_shows_its_menu() {
    let fx = Fixture::new(&[]);
    let runner = FakeRunner::new(vec![
        switch("/app0/big3-ps4_Shipping.elf"),
        switch("\\app0\\eboot.bin\r\neboot.bin"),
        exit(7),
    ]);
    let code = run_until_quit(&runner, Game::U1, &fx.session(Os::Linux)).unwrap();
    assert_eq!(code, 7);
    let starts = runner.starts();
    assert_eq!(
        starts.iter().map(|s| s.game).collect::<Vec<_>>(),
        vec![Game::U1, Game::U3, Game::U1]
    );
    assert_eq!(starts[0].env["BB_SKIP_COLLECTION_MENU"], "1");
    assert_eq!(starts[2].env["BB_SKIP_COLLECTION_MENU"], "0");
    assert_eq!(starts[1].env["BB_COPY64_EOP_READBACK"], "1");
    assert!(!starts[2].env.contains_key("BB_COPY64_EOP_READBACK"));
    assert_eq!(
        starts[2].guest_argv.as_deref(),
        Some("\\app0\\eboot.bin\neboot.bin\n")
    );
}

#[test]
fn exit_101_without_a_request_is_returned() {
    let fx = Fixture::new(&[]);
    let runner = FakeRunner::new(vec![exit(LOADEXEC_EXIT)]);
    let code = run_until_quit(&runner, Game::U2, &fx.session(Os::Linux)).unwrap();
    assert_eq!(code, LOADEXEC_EXIT);
    assert_eq!(runner.starts().len(), 1);
}

#[test]
fn a_stale_request_is_removed_before_the_start() {
    let fx = Fixture::new(&[]);
    fs::create_dir_all(&fx.paths.out_dir).unwrap();
    fs::write(fx.paths.loadexec_file(), "/app0/eboot.bin\n").unwrap();
    let runner = FakeRunner::new(vec![exit(LOADEXEC_EXIT)]);
    let code = run_until_quit(&runner, Game::U2, &fx.session(Os::Linux)).unwrap();
    assert_eq!(code, LOADEXEC_EXIT);
    assert!(!runner.starts()[0].loadexec_present);
    assert_eq!(runner.starts().len(), 1);
}

#[test]
fn a_request_without_a_name_is_an_error() {
    let fx = Fixture::new(&[]);
    let runner = FakeRunner::new(vec![switch("")]);
    let err = run_until_quit(&runner, Game::U2, &fx.session(Os::Linux)).unwrap_err();
    assert_eq!(err.code(), "launch");
    assert!(err.to_string().contains("without a name"));
}

#[test]
fn parse_loadexec_reads_the_first_line_name() {
    let req = parse_loadexec("/app0/big3-ps4_Shipping.elf\narg").unwrap();
    assert_eq!(req.game, Game::U3);
    assert_eq!(req.argv, vec!["/app0/big3-ps4_Shipping.elf", "arg"]);
    assert_eq!(
        parse_loadexec("C:\\dump\\eboot.bin").unwrap().game,
        Game::U1
    );
    assert_eq!(parse_loadexec("/app0/").unwrap_err().code(), "launch");
    let unknown = parse_loadexec("/app0/other.elf").unwrap_err();
    assert!(unknown.to_string().contains("other.elf"));
}

#[test]
fn u1_direct_starts_eboot_as_part_of_the_collection() {
    let fx = Fixture::new(&[("BB_U1_DIRECT", "1")]);
    let runner = FakeRunner::new(vec![exit(0)]);
    run_until_quit(&runner, Game::U1, &fx.session(Os::Linux)).unwrap();
    let start = &runner.starts()[0];
    assert_eq!(
        start.guest_argv.as_deref(),
        Some("/app0/eboot.bin\neboot.bin\nispartofuccollection\n")
    );
    assert!(!start.env.contains_key("BB_SKIP_COLLECTION_MENU"));
}

#[test]
fn session_env_sets_the_path_variables() {
    let fx = Fixture::new(&[]);
    let env: BTreeMap<String, String> = fx
        .session(Os::Linux)
        .env(Game::U2)
        .unwrap()
        .into_iter()
        .collect();
    let path = |p: PathBuf| p.to_string_lossy().into_owned();
    assert_eq!(env["BB_CONFIG"], path(fx.paths.data_dir.join("bbport.ini")));
    assert_eq!(
        env["BB_INPUT_CONFIG"],
        path(fx.paths.data_dir.join("input.ini"))
    );
    assert_eq!(env["BB_GPU_USER_DIR"], path(fx.paths.user_dir.clone()));
    assert_eq!(env["BB_USER_DIR"], path(fx.paths.user_dir.clone()));
    assert_eq!(env["BB_LOADEXEC_FILE"], path(fx.paths.loadexec_file()));
    assert_eq!(env["BB_EXE"], "big2-ps4_Shipping.elf");
    assert_eq!(env["BB_SAVEDATA_DIRS"], saves::mapping(&fx.paths.user_dir));
    // run_env's per-game defaults and fixed shader hashes are there too.
    assert_eq!(env["BB_GPU_MERGE"], "0");
    assert_eq!(env["BB_VELOCITY_PS"], "f4b18264");
}

#[test]
fn session_env_on_windows_has_no_savedata_mapping() {
    let fx = Fixture::new(&[]);
    let env = fx.session(Os::Windows).env(Game::U2).unwrap();
    assert!(!env.iter().any(|(k, _)| k == "BB_SAVEDATA_DIRS"));
}

#[test]
fn the_players_path_variables_win() {
    let fx = Fixture::new(&[
        ("BB_CONFIG", "/mine/bbport.ini"),
        ("BB_GPU_USER_DIR", "/mine/gpu"),
        ("BB_SAVEDATA_DIRS", "SAVEDATA01=Mine"),
    ]);
    let env: BTreeMap<String, String> = fx
        .session(Os::Linux)
        .env(Game::U1)
        .unwrap()
        .into_iter()
        .collect();
    assert_eq!(env["BB_CONFIG"], "/mine/bbport.ini");
    assert_eq!(env["BB_GPU_USER_DIR"], "/mine/gpu");
    assert_eq!(env["BB_SAVEDATA_DIRS"], "SAVEDATA01=Mine");
}

#[test]
fn session_env_includes_game_res_heaps() {
    let fx = Fixture::new(&[("BB_GAME_RES", "2560x1440")]);
    let env: BTreeMap<String, String> = fx
        .session(Os::Linux)
        .env(Game::U2)
        .unwrap()
        .into_iter()
        .collect();
    assert_eq!(env["BB_GAME_RES"], "2560x1440");
    assert!(env.contains_key("BB_GAME_RT_MB"));
}

#[test]
fn paths_follow_run_py_layout() {
    let paths = Paths::in_data_dir(Path::new("data"));
    assert_eq!(paths.out_dir, Path::new("data").join("out"));
    assert_eq!(paths.user_dir, Path::new("data").join("user"));
    assert_eq!(
        paths.guest_argv_file(),
        Path::new("data").join("out").join("guest-argv.txt")
    );
    assert_eq!(
        paths.game_out_dir(Game::U1),
        Path::new("data").join("out").join("eboot")
    );
    assert_eq!(
        paths.game_out_dir(Game::U2),
        Path::new("data").join("out").join("big2")
    );
    assert_eq!(
        paths.game_out_dir(Game::U3),
        Path::new("data").join("out").join("big3")
    );
}

fn child(program: &str) -> ChildRunner {
    ChildRunner {
        program: PathBuf::from(program),
        game_dirs: [(Game::U2, PathBuf::from("dump-u2"))].into(),
        paths: Paths::in_data_dir(Path::new("data")),
        timeout: "0".to_owned(),
        extra_args: vec!["--extra".into()],
    }
}

#[test]
fn child_runner_command_line_is_run_py_s() {
    let runner = child("uncharted-run");
    let out = Path::new("data").join("out").join("big2");
    let want: Vec<OsString> = vec![
        out.join("boot-linked.bin").into(),
        "--content-profile".into(),
        out.join("content.bin").into(),
        "--app0".into(),
        "dump-u2".into(),
        "--user".into(),
        Path::new("data").join("user").into(),
        "--timeout".into(),
        "0".into(),
        "--extra".into(),
    ];
    assert_eq!(runner.args(Game::U2, None).unwrap(), want);
    let with_argv = runner
        .args(Game::U2, Some(Path::new("guest-argv.txt")))
        .unwrap();
    assert_eq!(
        with_argv[9..],
        [
            OsString::from("--guest-argv"),
            "guest-argv.txt".into(),
            "--extra".into()
        ]
    );
}

#[test]
fn child_runner_needs_the_game_folder() {
    let err = child("uncharted-run").args(Game::U1, None).unwrap_err();
    assert_eq!(err.code(), "launch");
    assert!(err.to_string().contains("Uncharted 1"));
}

#[test]
fn child_runner_reports_a_missing_program() {
    let temp = Temp::new();
    let missing = temp.0.join("no-such-uncharted-run");
    let err = child(missing.to_str().unwrap())
        .run(Game::U2, &[], None)
        .unwrap_err();
    assert_eq!(err.code(), "launch");
    assert!(err.to_string().contains("could not start"));
}
