//! Play (SPEC.md M7, M10, M18): pull the save, start the game with the device profile and
//! `run_env`, wait for it to quit, push the save. save-relay does the syncing through `Relay`
//! (implemented by `relay.rs` once save-relay's library is linked; planned, not in the tree);
//! the game is started through `Runner`; both are traits so the flow is tested with fakes.
//! Replaces `main` of `run.py`: the LOADEXEC loop (a game that starts another executable), the
//! path variables it sets and the `bb-probe` command line. The shape is copied from
//! iannitram/totk-native's `launch.rs` (MIT).
//!
//! `docs/code-map.md` calls the second trait `Game`; it is `Runner` here because `games::Game`
//! is the enum of the three games.
//!
//! Not here yet: `run.py`'s preparation inside the loop (boot image, render-size patch, content
//! profile, trophies, patch files, cheats: their modules are stubs), `BB_RESTART_COMMAND`, and
//! the Windows no-console flag (`std::os` stays out of this module; the app is a window app).

use crate::error::{Error, Result};
use crate::games::Game;
use crate::platform::Os;
use crate::profile::DeviceProfile;
use crate::run_env::{self, Settings};
use crate::savecard::SideBySide;
use crate::saves;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `run.py`'s `LOADEXEC_EXIT`: the exit code of a run whose game asked to start another
/// executable (`runtime_services.c`); `play` then starts that game.
pub const LOADEXEC_EXIT: i32 = 101;

/// What one save-relay pull or push did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sync {
    /// Done (pulled, pushed, or nothing to do); the message is save-relay's.
    Done(String),
    /// The server is unreachable: play on (pull) or push next time (push).
    Offline,
    /// Both sides changed since the last sync; nothing was touched. The two cards are what the
    /// side-by-side prompt shows (boxed: a card holds a picture).
    Conflict(Box<SideBySide>),
}

/// The player's answer to a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    Server,
    Local,
}

/// save-relay, as the launcher uses it.
pub trait Relay {
    fn pull(&self, saves_dir: &Path, resolve: Option<Keep>) -> Result<Sync>;
    fn push(&self, saves_dir: &Path, resolve: Option<Keep>) -> Result<Sync>;
}

/// The runtime, started as a child process.
pub trait Runner {
    /// Start `uncharted-run` once for `game` with `env` added and `guest_argv` (the
    /// `--guest-argv` file a LoadExec switch hands the next game) when set, and wait until it
    /// quits; Ok with its exit code.
    fn run(&self, game: Game, env: &[(String, String)], guest_argv: Option<&Path>) -> Result<i32>;
}

/// What a play session did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Played {
    pub pulled: Sync,
    pub exit_code: i32,
    pub pushed: Sync,
}

/// The folders a play session uses: `run.py`'s `BB_DATA_DIR`, its `out/` and the user folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// `BB_DATA_DIR`: `bbport.ini`, `input.ini`.
    pub data_dir: PathBuf,
    /// `<data>/out`: the prepared games, `loadexec.txt`, `guest-argv.txt`.
    pub out_dir: PathBuf,
    /// `BB_USER_DIR`: saves and GPU caches.
    pub user_dir: PathBuf,
}

impl Paths {
    /// `run.py`'s layout: `out` and `user` inside `data_dir`.
    pub fn in_data_dir(data_dir: &Path) -> Paths {
        Paths {
            data_dir: data_dir.to_path_buf(),
            out_dir: data_dir.join("out"),
            user_dir: data_dir.join("user"),
        }
    }

    /// `BB_LOADEXEC_FILE`: where the runtime writes a LoadExec request.
    pub fn loadexec_file(&self) -> PathBuf {
        self.out_dir.join("loadexec.txt")
    }

    /// The `--guest-argv` file: the argument list of the game a LoadExec switch starts.
    pub fn guest_argv_file(&self) -> PathBuf {
        self.out_dir.join("guest-argv.txt")
    }

    /// The prepared folder of `game`: `out/eboot`, `out/big2`, `out/big3` (`run.py`: the
    /// executable's stem before its first `-`).
    pub fn game_out_dir(&self, game: Game) -> PathBuf {
        let exe = game.executable();
        let stem = exe.split('.').next().unwrap_or(exe);
        self.out_dir.join(stem.split('-').next().unwrap_or(stem))
    }
}

/// Everything the environment of one start is made from.
#[derive(Debug, Clone, Copy)]
pub struct Session<'a> {
    pub settings: &'a Settings,
    pub profile: &'a DeviceProfile,
    /// The variables the player set; a name here always keeps the player's value.
    pub player_env: &'a BTreeMap<String, String>,
    pub paths: &'a Paths,
    /// The OS the game runs on: off Windows the runtime gets `BB_SAVEDATA_DIRS`.
    pub os: Os,
}

impl Session<'_> {
    /// The environment to start `game` with, in name order: `run_env::build_env` (per-game
    /// defaults, the device profile, the settings, `render_size` heaps, the player's own
    /// variables), then the path variables of `run.py` `main` the player did not set
    /// (`BB_CONFIG`, `BB_INPUT_CONFIG`, `BB_GPU_USER_DIR`, `BB_USER_DIR`, and `BB_SAVEDATA_DIRS`
    /// off Windows), then `BB_LOADEXEC_FILE` and `BB_EXE`, which `run.py` always sets.
    pub fn env(&self, game: Game) -> Result<Vec<(String, String)>> {
        let built = run_env::build_env(game, self.settings, self.profile, self.player_env)?;
        let mut env: BTreeMap<String, String> = built.into_iter().collect();
        let paths = self.paths;
        let text = |p: PathBuf| p.to_string_lossy().into_owned();
        let mut defaults = vec![
            ("BB_CONFIG", text(paths.data_dir.join("bbport.ini"))),
            ("BB_INPUT_CONFIG", text(paths.data_dir.join("input.ini"))),
            ("BB_GPU_USER_DIR", text(paths.user_dir.clone())),
            ("BB_USER_DIR", text(paths.user_dir.clone())),
        ];
        if self.os != Os::Windows {
            defaults.push(("BB_SAVEDATA_DIRS", saves::mapping(&paths.user_dir)));
        }
        for (name, value) in defaults {
            env.entry(name.to_owned()).or_insert(value);
        }
        env.insert("BB_LOADEXEC_FILE".to_owned(), text(paths.loadexec_file()));
        env.insert("BB_EXE".to_owned(), game.executable().to_owned());
        Ok(env.into_iter().collect())
    }
}

/// A LoadExec request (`runtime_services.c`: the guest path, then one argument per line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadExec {
    /// The game whose executable the first line names.
    pub game: Game,
    /// Every line, the guest path first: the next start's `--guest-argv` list.
    pub argv: Vec<String>,
}

/// Read a LoadExec request. `run.py` takes the file name of the first line (`\` or `/`
/// separated); an empty name, or one that is not a collection executable, is a plain error.
pub fn parse_loadexec(text: &str) -> Result<LoadExec> {
    let argv: Vec<String> = text.lines().map(str::to_owned).collect();
    let first = argv
        .first()
        .map(|l| l.replace('\\', "/"))
        .unwrap_or_default();
    let target = first.rsplit('/').next().unwrap_or("").trim();
    if target.is_empty() {
        return Err(Error::Launch(
            "the game asked for another executable without a name".to_owned(),
        ));
    }
    let game = Game::from_executable(target).ok_or_else(|| {
        Error::Launch(format!(
            "the game asked for {target}, which is not one of the collection's games"
        ))
    })?;
    Ok(LoadExec { game, argv })
}

/// Start `game` and follow its LoadExec switches until a run quits for good (`run.py` `main`):
/// before each start the request file is removed; exit code `LOADEXEC_EXIT` with the file
/// present starts the executable it names, with its lines written to the `--guest-argv` file
/// and the environment built again for that game; `BB_SKIP_COLLECTION_MENU=1` (the first start
/// of U1 only) becomes `0`, so a game that returns to the collection shows its menu. Any other
/// exit code, or 101 without the file, is returned as it is. The player's `BB_U1_DIRECT=1`
/// starts U1 with the collection menu's arguments (`eboot.bin ispartofuccollection`).
pub fn run_until_quit(runner: &dyn Runner, game: Game, session: &Session) -> Result<i32> {
    let loadexec = session.paths.loadexec_file();
    let guest_file = session.paths.guest_argv_file();
    let mut game = game;
    let mut guest: Option<PathBuf> = None;
    let mut switched = false;
    // `BB_U1_DIRECT=1` (Linux fork experiment): start eboot.bin the way the collection menu
    // starts a game, to boot Uncharted 1 past the menu.
    let direct = session
        .player_env
        .get("BB_U1_DIRECT")
        .is_some_and(|v| v == "1");
    let mut first_argv = (game == Game::U1 && direct).then(|| {
        vec![
            "/app0/eboot.bin".to_owned(),
            "eboot.bin".to_owned(),
            "ispartofuccollection".to_owned(),
        ]
    });
    loop {
        if let Some(argv) = first_argv.take() {
            write_guest_argv(&guest_file, &argv)?;
            guest = Some(guest_file.clone());
        }
        let mut env = session.env(game)?;
        if switched {
            for (name, value) in env.iter_mut() {
                if name == "BB_SKIP_COLLECTION_MENU" && value == "1" {
                    *value = "0".to_owned();
                }
            }
        }
        remove_if_present(&loadexec)?;
        let code = runner.run(game, &env, guest.as_deref())?;
        if code != LOADEXEC_EXIT || !loadexec.is_file() {
            return Ok(code);
        }
        let bytes = fs::read(&loadexec)?;
        let next = parse_loadexec(&String::from_utf8_lossy(&bytes))?;
        write_guest_argv(&guest_file, &next.argv)?;
        game = next.game;
        guest = Some(guest_file.clone());
        switched = true;
    }
}

fn write_guest_argv(file: &Path, argv: &[String]) -> Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(file, argv.join("\n") + "\n")?;
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// The play flow: `relay.pull`; on `Conflict` ask `choose` and pull again with its answer;
/// `Offline` plays on. Then the game (`run_until_quit`: `runner.run` with the session's
/// environment, LoadExec switches followed), then `relay.push` (a conflict asks again). A
/// relay error before the game starts stops the flow (the game does not start); a push error
/// is returned after the game has quit. A conflict that save-relay still reports after the
/// player's answer is an error, so the flow never asks twice for one sync.
pub fn play(
    relay: &dyn Relay,
    runner: &dyn Runner,
    game: Game,
    saves_dir: &Path,
    session: &Session,
    choose: &dyn Fn(&Sync) -> Keep,
) -> Result<Played> {
    let pulled = sync(|resolve| relay.pull(saves_dir, resolve), choose, "pull")?;
    let exit_code = run_until_quit(runner, game, session)?;
    let pushed = sync(|resolve| relay.push(saves_dir, resolve), choose, "push")?;
    Ok(Played {
        pulled,
        exit_code,
        pushed,
    })
}

/// One pull or push: once as it is, once more with the player's answer to a conflict.
fn sync(
    step: impl Fn(Option<Keep>) -> Result<Sync>,
    choose: &dyn Fn(&Sync) -> Keep,
    what: &str,
) -> Result<Sync> {
    let first = step(None)?;
    if !matches!(first, Sync::Conflict(_)) {
        return Ok(first);
    }
    let keep = choose(&first);
    match step(Some(keep))? {
        Sync::Conflict(_) => Err(Error::Launch(format!(
            "save-relay still reports a conflict after the {what} was resolved"
        ))),
        done => Ok(done),
    }
}

/// The desktop runner: starts `uncharted-run` (today `bb-probe`, until piece 3.16) as a child
/// process with stdin closed (an inherited pipe cost the game its console output, `run.py`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildRunner {
    /// The `uncharted-run` executable.
    pub program: PathBuf,
    /// Each game's dump folder (`--app0`; `run.py` `locate`: `BB_GAME_DIR[_U1..]`).
    pub game_dirs: BTreeMap<Game, PathBuf>,
    pub paths: Paths,
    /// `BB_TIMEOUT`: seconds until the runtime stops the game, `0` for none.
    pub timeout: String,
    /// The player's extra arguments, added last (`run.py`: `*args`).
    pub extra_args: Vec<OsString>,
}

impl ChildRunner {
    /// The command line after the program for one start of `game` (`run.py` `main`): the
    /// linked boot image, `--content-profile`, `--app0`, `--user`, `--timeout`, then
    /// `--guest-argv` after a switch, then the player's arguments.
    pub fn args(&self, game: Game, guest_argv: Option<&Path>) -> Result<Vec<OsString>> {
        let app0 = self
            .game_dirs
            .get(&game)
            .ok_or_else(|| Error::Launch(format!("no game folder is set for {}", game.name())))?;
        let out = self.paths.game_out_dir(game);
        let mut args: Vec<OsString> = vec![
            out.join("boot-linked.bin").into(),
            "--content-profile".into(),
            out.join("content.bin").into(),
            "--app0".into(),
            app0.into(),
            "--user".into(),
            self.paths.user_dir.clone().into(),
            "--timeout".into(),
            self.timeout.clone().into(),
        ];
        if let Some(file) = guest_argv {
            args.push("--guest-argv".into());
            args.push(file.into());
        }
        args.extend(self.extra_args.iter().cloned());
        Ok(args)
    }
}

impl Runner for ChildRunner {
    fn run(&self, game: Game, env: &[(String, String)], guest_argv: Option<&Path>) -> Result<i32> {
        let args = self.args(game, guest_argv)?;
        let status = Command::new(&self.program)
            .args(args)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .status()
            .map_err(|e| {
                Error::Launch(format!("could not start {}: {e}", self.program.display()))
            })?;
        status.code().ok_or_else(|| {
            Error::Launch(format!(
                "{} was stopped before it returned an exit code",
                self.program.display()
            ))
        })
    }
}

#[cfg(test)]
mod tests;
