//! One function per command. Each parses its own arguments (`args`), calls `uncharted_core`
//! and turns the result into text and an exit status (`Outcome`); none has logic of its own
//! beyond choosing folders and printing.

use crate::args::{self, Parsed};
use crate::{Ctx, Outcome, FAILED};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use uncharted_core::artwork::{self, Art};
use uncharted_core::dump::{self, DumpReport};
use uncharted_core::fleet::{self, DeviceRecord, Drift};
use uncharted_core::games::Game;
use uncharted_core::launch::{self, Keep, Paths, Relay, Runner, Session, Sync};
use uncharted_core::patches::{self, Patch, Selection, Target};
use uncharted_core::platform::{AppDirs, Os, Vars};
use uncharted_core::prepare::{self, PrepareOptions};
use uncharted_core::profile::DeviceProfile;
use uncharted_core::relay::DesktopRelay;
use uncharted_core::run_env::Settings;
use uncharted_core::savecard::SideBySide;
use uncharted_core::steam::{self, Shortcut};
use uncharted_core::{host_env, profile, saves, Error, Result};

/// A command's result: `Err` is an early end (bad arguments, a core error) already in words.
type Done = std::result::Result<Outcome, Outcome>;

/// The Steam shortcut's name when it opens the collection (no game named).
pub const COLLECTION_TITLE: &str = "Uncharted: The Nathan Drake Collection";

/// The fleet records file under the launcher's config folder (one `fleet::to_line` per line).
pub const FLEET_FILE: &str = "fleet.txt";

pub fn dispatch(command: &str, args: &[String], ctx: &Ctx) -> Outcome {
    let done = match command {
        "check" => check(args, ctx),
        "prepare" => prepare(args, ctx),
        "play" => play(args, ctx),
        "steam-add" => steam_add(args, ctx),
        "patches" => patches(args, ctx),
        "hub" => hub::hub(args, ctx),
        "fleet" => fleet(args, ctx),
        "generate-art" => generate_art(args),
        "setup" => Ok(crate::setup::main(args)),
        other => unreachable!("dispatch is only called with a listed command, not {other}"),
    };
    done.unwrap_or_else(|early| early)
}

/// Every command reports bad arguments the same way: the problem, then its usage, status 2.
fn parsed(
    command: &str,
    args: &[String],
    flags: &[&str],
    positional: usize,
) -> std::result::Result<Parsed, Outcome> {
    args::parse(args, flags, positional).map_err(|problem| Outcome::usage(command, &problem))
}

fn game_arg(command: &str, p: &Parsed) -> std::result::Result<Game, Outcome> {
    let key = p
        .positional
        .first()
        .ok_or_else(|| Outcome::usage(command, "name the game: u1, u2 or u3"))?;
    args::game(key).map_err(|problem| Outcome::usage(command, &problem))
}

fn core<T>(command: &str, result: Result<T>) -> std::result::Result<T, Outcome> {
    result.map_err(|error| Outcome::error(command, &error))
}

fn vars(ctx: &Ctx) -> Result<Vars> {
    Vars::from_lookup(ctx.os, |name| ctx.var(name))
}

/// The dump folder: the first positional argument, else `BB_GAME_DIR`.
fn dump_folder(command: &str, p: &Parsed, ctx: &Ctx) -> std::result::Result<PathBuf, Outcome> {
    p.positional
        .first()
        .cloned()
        .or_else(|| ctx.var("BB_GAME_DIR"))
        .map(PathBuf::from)
        .ok_or_else(|| Outcome::usage(command, "give the dump folder or set BB_GAME_DIR"))
}

// ---- check ----

fn check(args: &[String], ctx: &Ctx) -> Done {
    let p = parsed("check", args, &[], 1)?;
    let folder = dump_folder("check", &p, ctx)?;
    let report = core("check", dump::check(&folder))?;
    let mut out = Outcome::default();
    write_report(&mut out, &folder, &report);
    if !report.problems.is_empty() {
        out.code = FAILED;
    }
    Ok(out)
}

/// The dump report as plain lines: never a byte of game data, only names, ids and versions.
fn write_report(out: &mut Outcome, folder: &Path, report: &DumpReport) {
    out.line(format!("dump: {}", folder.display()));
    out.line(format!(
        "title: {}",
        report.title_id.as_deref().unwrap_or("unknown")
    ));
    let version = report.games.first().and_then(|g| g.app_ver.as_deref());
    out.line(format!("version: {}", version.unwrap_or("unknown")));
    out.line(format!(
        "decrypted: {}",
        if report.decrypted { "yes" } else { "no" }
    ));
    if report.games.is_empty() {
        out.line("games: none found");
    } else {
        out.line("games:");
        for found in &report.games {
            out.line(format!(
                "  {}  {}  {}",
                found.game.key(),
                found.game.name(),
                found.game.executable()
            ));
        }
    }
    if report.problems.is_empty() {
        out.line("ready to prepare");
    } else {
        out.line(format!(
            "{} problem{}:",
            report.problems.len(),
            if report.problems.len() == 1 { "" } else { "s" }
        ));
        for problem in &report.problems {
            out.line(format!("  - {problem}"));
        }
    }
}

// ---- prepare ----

fn prepare(args: &[String], ctx: &Ctx) -> Done {
    let p = parsed("prepare", args, &["dump"], 1)?;
    let game = game_arg("prepare", &p)?;
    let install = AppDirs::new(&core("prepare", vars(ctx))?).install(game.key());
    let dump = match p.flag("dump") {
        Some(folder) => PathBuf::from(folder),
        None => core("prepare", prepare::locate(game, |name| ctx.var(name)))?.0,
    };
    // Third-party patches come from BB_PATCHES_DIR, as in `run.py` and `patches` above; none
    // set writes no patch file.
    let patches = match ctx.var("BB_PATCHES_DIR").map(PathBuf::from) {
        Some(dir) => {
            let config = ctx
                .var("BB_PATCHES_CONFIG")
                .map(PathBuf::from)
                .unwrap_or_else(|| dir.join("patches.json"));
            let selection = core("prepare", read_selection(&config))?;
            Some(prepare::PatchSource { dir, selection })
        }
        None => None,
    };
    let options = PrepareOptions {
        patches,
        ..PrepareOptions::default()
    };
    let manifest = core("prepare", prepare::prepare(&dump, &install, game, &options))?;
    let mut out = Outcome::default();
    out.line(format!(
        "prepared {} ({} {}) into {}: {} files",
        game.name(),
        manifest.title_id,
        manifest.app_ver,
        install.display(),
        manifest.files.len()
    ));
    Ok(out)
}

// ---- play ----

/// Where `play` takes a game's save from and what it starts the runtime with.
#[derive(Debug, PartialEq, Eq)]
pub struct PlayPlan {
    pub install: PathBuf,
    /// `run.py`'s folders: the install is its `BB_DATA_DIR` (`out/` inside it), the user
    /// folder is `saves::user_dir`.
    pub paths: Paths,
    /// This device's profile (`profile::detect_with_os_release`).
    pub profile: &'static DeviceProfile,
    /// The player's own `BB_*` variables; they win over every default.
    pub player_env: BTreeMap<String, String>,
    pub settings: Settings,
    /// `BB_RUNTIME`, else `uncharted-run` beside this program.
    pub program: PathBuf,
}

pub fn play_plan(game: Game, ctx: &Ctx) -> Result<PlayPlan> {
    let vars = vars(ctx)?;
    let install = AppDirs::new(&vars).install(game.key());
    let default_user = saves::user_dir(ctx.os, &vars, &install)?;
    let user_dir = ctx
        .var("BB_USER_DIR")
        .map(PathBuf::from)
        .unwrap_or(default_user);
    let mut player_env: BTreeMap<String, String> = ctx
        .env
        .iter()
        .filter(|(name, _)| name.starts_with("BB_"))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    // CLI Steam shortcuts bypass the GUI, so they must read its persisted prompt choice too.
    // A caller's explicit environment remains authoritative; Automatic leaves detection alone.
    if !player_env.contains_key("BB_BUTTON_PROMPTS") {
        if let Some(choice) = fs::read(AppDirs::new(&vars).settings())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| value.get("button_prompts")?.as_str().map(str::to_owned))
            .filter(|choice| {
                matches!(
                    choice.as_str(),
                    "xbox" | "playstation" | "nintendo" | "keyboard"
                )
            })
        {
            player_env.insert("BB_BUTTON_PROMPTS".into(), choice);
        }
    }
    let (cpu_model, os_release) = device_strings(ctx);
    let device = profile::detect_with_os_release(ctx.os, &cpu_model, &os_release);
    let paths = Paths {
        data_dir: install.clone(),
        out_dir: install.join("out"),
        user_dir,
    };
    let program = ctx
        .var("BB_RUNTIME")
        .map(PathBuf::from)
        .unwrap_or_else(|| resolve_runtime(&ctx.exe, ctx.os));
    let resolved =
        uncharted_core::launch_settings::load_for_launch(&AppDirs::new(&vars).settings(), device);
    Ok(PlayPlan {
        install,
        paths,
        profile: resolved.profile,
        player_env,
        settings: resolved.settings,
        program,
    })
}

impl PlayPlan {
    /// The session `launch::play` builds each start's environment from (`Session::env`:
    /// `run_env::build_env`, the path variables of `run.py`, `BB_EXE`).
    pub fn session(&self, os: Os) -> Session<'_> {
        Session {
            settings: &self.settings,
            profile: self.profile,
            player_env: &self.player_env,
            paths: &self.paths,
            os,
        }
    }
}

/// `/proc/cpuinfo` "model name" and `/etc/os-release` on Linux (the profile picks the AMD
/// handheld or the Strix Halo tablet from them); empty elsewhere, which picks the OS default.
fn device_strings(ctx: &Ctx) -> (String, String) {
    if ctx.os != uncharted_core::platform::Os::Linux || !cfg!(target_os = "linux") {
        return (String::new(), String::new());
    }
    let cpu = fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split_once(':'))
                .map(|(_, model)| model.trim().to_owned())
        })
        .unwrap_or_default();
    let release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    (cpu, release)
}

/// While `uncharted-run.exe` is still a probe until 3.17 (#443), Windows Play prefers the
/// packaged native loader (`bin/bb-probe.exe` or `bb-probe.exe` beside `exe`) when present.
fn resolve_runtime(exe: &Path, os: Os) -> PathBuf {
    if os == Os::Windows {
        if let Some(dir) = exe.parent().filter(|d| !d.as_os_str().is_empty()) {
            for candidate in [
                dir.join("bin").join("bb-probe.exe"),
                dir.join("bb-probe.exe"),
            ] {
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    beside(exe, "uncharted-run")
}

/// A program in the same folder as `exe`, with the OS's executable suffix.
fn beside(exe: &Path, name: &str) -> PathBuf {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    match exe.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(file),
        _ => PathBuf::from(file),
    }
}

/// Platforms whose release does not yet carry the helper keep local play available.
struct UnbundledRelay;

impl Relay for UnbundledRelay {
    fn requires_save_mapping(&self) -> bool {
        false
    }
    fn pull(&self, _: Game, _: u8, _: &Path, _: Option<Keep>) -> Result<Sync> {
        Ok(Sync::LocalOnly(
            "This platform build has no Save Relay; using local saves.",
        ))
    }
    fn push(&self, game: Game, slot: u8, path: &Path, keep: Option<Keep>) -> Result<Sync> {
        self.pull(game, slot, path, keep)
    }
}

/// Starts the runtime as a child process and waits for it, so Steam sees the game running
/// for as long as it runs (what the Windows `Play Uncharted.exe`, `packaging/windows/play.c`,
/// did until 2.10 replaced it): no stdin, the player's extra arguments last, and on Linux the host's
/// library injection taken out (`host_env`: Steam's `LD_PRELOAD` overlay and the rest of
/// what `packaging/bbport-entry.c` removed). `main` has already cleaned this process; `host`
/// repeats it for the child so a runner built elsewhere (the tests) is cleaned too.
pub struct RuntimeRunner {
    pub program: PathBuf,
    pub extra_args: Vec<String>,
    pub relay_prompt_stdio: bool,
    /// `host_env::changes` for this machine.
    pub host: Vec<host_env::Change>,
}

impl RuntimeRunner {
    /// `guest_argv`: the `--guest-argv` file after a LoadExec switch, before the player's
    /// arguments (`run.py` `main`).
    pub fn command(&self, env: &[(String, String)], guest_argv: Option<&Path>) -> Command {
        let mut command = Command::new(&self.program);
        if let Some(file) = guest_argv {
            command.arg("--guest-argv").arg(file);
        }
        command
            .args(&self.extra_args)
            .envs(env.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::null());
        if self.relay_prompt_stdio {
            command.stdout(Stdio::from(io::stderr()));
        }
        host_env::apply_to_command(&mut command, &self.host);
        command
    }
}

impl Runner for RuntimeRunner {
    fn run(&self, _game: Game, env: &[(String, String)], guest_argv: Option<&Path>) -> Result<i32> {
        let status = self.command(env, guest_argv).status().map_err(|error| {
            Error::Launch(format!(
                "could not start {}: {error}",
                self.program.display()
            ))
        })?;
        // A process ended by a signal has no code: count it as a failure.
        Ok(status.code().unwrap_or(1))
    }
}

/// Only data from the conflict that stopped this particular pull or push goes to the GUI.
fn conflict_event(phase: &str, pair: &SideBySide) -> serde_json::Value {
    uncharted_core::relay::conflict_event(phase, pair)
}

fn choose_keep(
    keep: Option<Keep>,
    interactive: bool,
    stdio: bool,
    phase: &str,
    cards: &SideBySide,
) -> Result<Keep> {
    if let Some(keep) = keep {
        return Ok(keep);
    }
    if stdio {
        let mut out = io::stdout().lock();
        writeln!(out, "{}", conflict_event(phase, cards))?;
        out.flush()?;
    } else if interactive {
        for (label, card) in [("Server", &cards.server), ("Local", &cards.local)] {
            eprintln!(
                "{label}: {} | {} | {} | {}",
                card.device.as_deref().unwrap_or("unknown device"),
                card.slot,
                card.saved_unix,
                card.detail.as_deref().unwrap_or("")
            );
        }
        eprintln!("Choose server or local, or cancel to leave both saves pending.");
    } else {
        println!("{}", conflict_event(phase, cards));
        return Err(Error::Launch("Save conflict: both saves are pending. Choose a side with --keep server or --keep local.".into()));
    }
    let mut answer = String::new();
    io::stdin().lock().take(16).read_line(&mut answer)?;
    if answer.ends_with('\n') {
        match answer.trim() {
            "server" => return Ok(Keep::Server),
            "local" => return Ok(Keep::Local),
            _ => (),
        }
    }
    Err(Error::Launch(
        "Save conflict left pending; neither side was selected.".into(),
    ))
}

fn describe(sync: &Sync, pushing: bool) -> String {
    match sync {
        Sync::LocalOnly(message) => (*message).into(),
        Sync::Done(message) => message.clone(),
        Sync::Offline if pushing => {
            "the save server is offline; the save is pushed next time".into()
        }
        Sync::Offline => "the save server is offline; playing on with this device's save".into(),
        Sync::Conflict(_) => "a conflict is still open".into(),
    }
}

/// The game's exit status as this program's: 0-255 as is, anything else (a Windows NTSTATUS
/// crash code, a negative value) as a failure.
pub fn exit_status(code: i32) -> u8 {
    u8::try_from(code).unwrap_or(FAILED)
}

fn play(args: &[String], ctx: &Ctx) -> Done {
    let p = parsed("play", args, &["keep", "relay-prompt"], 1)?;
    let game = game_arg("play", &p)?;
    let keep = match p.flag("keep") {
        None => None,
        Some("server") => Some(Keep::Server),
        Some("local") => Some(Keep::Local),
        Some(other) => {
            return Err(Outcome::usage(
                "play",
                &format!("--keep is server or local, not '{other}'"),
            ))
        }
    };
    let stdio = match p.flag("relay-prompt") {
        None => false,
        Some("stdio") if keep.is_none() => true,
        _ => {
            return Err(Outcome::usage(
                "play",
                "--relay-prompt takes stdio and requires an interactive choice without --keep",
            ))
        }
    };
    let plan = core("play", play_plan(game, ctx))?;
    let root = ctx.exe.parent().unwrap_or(Path::new("."));
    let relay = DesktopRelay {
        program: beside(&ctx.exe, "save-relay"),
        manifests: root.join("save-relay-manifests"),
    };
    let adapter: &dyn Relay = if ctx.os == Os::Windows || relay.program.is_file() {
        &relay
    } else {
        &UnbundledRelay
    };
    if ctx.os == Os::Windows && !plan.player_env.contains_key("BB_SAVEDATA_DIRS") {
        core("play", saves::migrate_windows_slots(&plan.paths.user_dir))?;
    }
    let runner = RuntimeRunner {
        program: plan.program.clone(),
        extra_args: p.rest,
        relay_prompt_stdio: stdio,
        host: host_env::changes(ctx.os, |name| ctx.var(name)),
    };
    let interactive = ctx.interactive;
    let chosen = std::cell::Cell::new(keep);
    let choose = |phase: &str, cards: &SideBySide| {
        choose_keep(chosen.take(), interactive, stdio, phase, cards)
    };
    let played = core(
        "play",
        launch::play(adapter, &runner, game, &plan.session(ctx.os), &choose),
    )?;
    let mut out = Outcome::default();
    out.line(format!("before play: {}", describe(&played.pulled, false)));
    out.line(format!(
        "{} quit with exit status {}",
        played.game.name(),
        played.exit_code
    ));
    out.line(format!("after play: {}", describe(&played.pushed, true)));
    if stdio {
        out.stderr = std::mem::take(&mut out.stdout);
    }
    out.code = exit_status(played.exit_code);
    Ok(out)
}

// ---- steam-add ----

/// The program a Steam shortcut starts: inside the Linux AppImage the AppImage file itself
/// (`APPIMAGE`, set by its runtime; `ctx.exe` is in a mount that is gone once it quits, and
/// the AppImage's `AppRun` hands `play` on to this program), else this program.
pub fn shortcut_target(ctx: &Ctx) -> PathBuf {
    match ctx.var("APPIMAGE") {
        Some(appimage) if ctx.os == uncharted_core::platform::Os::Linux => PathBuf::from(appimage),
        _ => ctx.exe.clone(),
    }
}

/// The shortcut Steam gets: this program with `play <game>` (`play u1` opens the collection's
/// menu, so no game named means the collection).
pub fn shortcut_for(game: Option<Game>, exe: &Path) -> Shortcut {
    let exe_text = exe.display().to_string();
    let start_dir = exe
        .parent()
        .map(|dir| dir.display().to_string())
        .unwrap_or_default();
    Shortcut {
        name: game.map_or(COLLECTION_TITLE.to_owned(), |g| g.name().to_owned()),
        exe: exe_text.clone(),
        start_dir,
        launch_options: format!("play {}", game.unwrap_or(Game::U1).key()),
        icon: exe_text,
    }
}

/// The Steam user folders under `userdata` (`userdata/<account id>`, digits, never `0`),
/// sorted: `user_configs` in `bbport_steam.py`.
pub fn user_dirs(userdata: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(userdata)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| {
                        !name.is_empty() && name != "0" && name.bytes().all(|b| b.is_ascii_digit())
                    })
        })
        .collect();
    dirs.sort();
    dirs
}

/// The library images for `game` cut from the dump folder; a picture the folder lacks is left out.
fn render_images(dump: &Path, game: Game) -> Result<Vec<(Art, Vec<u8>)>> {
    let mut images = Vec::new();
    for art in artwork::ALL {
        if let Some(bytes) = artwork::render(dump, game, art)? {
            images.push((art, bytes));
        }
    }
    Ok(images)
}

fn steam_add(args: &[String], ctx: &Ctx) -> Done {
    let p = parsed("steam-add", args, &["userdata", "dump"], 1)?;
    let game = match p.positional.first() {
        Some(key) => {
            Some(args::game(key).map_err(|problem| Outcome::usage("steam-add", &problem))?)
        }
        None => None,
    };
    let shortcut = shortcut_for(game, &shortcut_target(ctx));
    let userdata = match p.flag("userdata") {
        Some(folder) => PathBuf::from(folder),
        None => {
            let roots = steam::userdata_roots(&core("steam-add", vars(ctx))?);
            core(
                "steam-add",
                roots.into_iter().find(|root| root.is_dir()).ok_or_else(|| {
                    Error::Steam(
                        "Steam was not found on this device (or nobody has signed in to it yet)"
                            .into(),
                    )
                }),
            )?
        }
    };
    let written = core(
        "steam-add",
        steam::write_shortcut(&userdata, &shortcut, &*ctx.steam_running),
    )?;
    let mut out = Outcome::default();
    for file in &written {
        out.line(format!("wrote {}", file.display()));
    }
    // The pictures are a nicety: the shortcut works without them, so a failure is a note.
    let dump = p
        .flag("dump")
        .map(str::to_owned)
        .or_else(|| ctx.var("BB_GAME_DIR"));
    let Some(dump) = dump else {
        out.note("artwork skipped: give --dump or set BB_GAME_DIR to cut it from the dump");
        return Ok(out);
    };
    match render_images(Path::new(&dump), game.unwrap_or(Game::U1)) {
        Err(error) => out.note(format!("artwork skipped: {error}")),
        Ok(images) if images.is_empty() => out.note("artwork skipped: the dump has no pictures"),
        Ok(images) => {
            let app_id = shortcut.app_id();
            for user in user_dirs(&userdata) {
                match steam::write_artwork(&user, app_id, &images) {
                    Ok(files) => {
                        for file in files {
                            out.line(format!("wrote {}", file.display()));
                        }
                    }
                    Err(error) => {
                        out.note(format!("artwork skipped for {}: {error}", user.display()))
                    }
                }
            }
        }
    }
    out.line(format!("{} is in the Steam library", shortcut.name));
    Ok(out)
}

/// Whether Steam is running on this machine: `steam` in `/proc` on Linux, `tasklist` on
/// Windows (`steam_running` in `bbport_steam.py`), `pgrep steam_osx` on a Mac.
pub fn steam_running() -> bool {
    if cfg!(target_os = "windows") {
        Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq steam.exe", "/NH"])
            .stdin(Stdio::null())
            .output()
            .map(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .to_ascii_lowercase()
                    .contains("steam.exe")
            })
            .unwrap_or(false)
    } else if cfg!(target_os = "macos") {
        Command::new("pgrep")
            .args(["-x", "steam_osx"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    } else {
        fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .bytes()
                    .all(|b| b.is_ascii_digit())
            })
            .any(|entry| {
                fs::read_to_string(entry.path().join("comm"))
                    .is_ok_and(|comm| comm.trim() == "steam")
            })
    }
}

// ---- patches ----

/// `patches.json` (`{"enabled": [...], "disabled": [...]}`); a missing file is no selection.
pub fn read_selection(path: &Path) -> Result<Selection> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Selection::default())
        }
        Err(error) => return Err(error.into()),
    };
    let bad = || Error::Patch(format!("{} is not a patch selection", path.display()));
    let value: serde_json::Value =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| bad())?;
    let list = |key: &str| -> Result<Vec<String>> {
        match value.get(key) {
            None => Ok(Vec::new()),
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .map(|item| item.as_str().map(str::to_owned).ok_or_else(bad))
                .collect(),
            Some(_) => Err(bad()),
        }
    };
    if !value.is_object() {
        return Err(bad());
    }
    Ok(Selection {
        enabled: list("enabled")?,
        disabled: list("disabled")?,
    })
}

fn patches(args: &[String], ctx: &Ctx) -> Done {
    let p = parsed("patches", args, &["patches-dir"], 1)?;
    let folder = dump_folder("patches", &p, ctx)?;
    let mut out = Outcome::default();
    let Some(dir) = p
        .flag("patches-dir")
        .map(str::to_owned)
        .or_else(|| ctx.var("BB_PATCHES_DIR"))
        .map(PathBuf::from)
    else {
        out.line("no third-party patches: give --patches-dir or set BB_PATCHES_DIR");
        return Ok(out);
    };
    let report = core("patches", dump::check(&folder))?;
    let config = ctx
        .var("BB_PATCHES_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join("patches.json"));
    let selection = core("patches", read_selection(&config))?;

    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|error| Outcome::error("patches", &Error::from(error)))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
        })
        .collect();
    files.sort();
    let mut all: Vec<Patch> = Vec::new();
    for file in &files {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match fs::read_to_string(file)
            .map_err(Error::from)
            .and_then(|xml| patches::parse(&name, &xml))
        {
            Ok(found) => all.extend(found),
            Err(error) => {
                out.note(format!("{name}: {error}"));
                out.code = FAILED;
            }
        }
    }
    write_patches(&mut out, &report, &all, &selection);
    Ok(out)
}

/// The About page's patch list, one section per game, then patches for no game of the
/// collection (always unavailable).
fn write_patches(out: &mut Outcome, report: &DumpReport, all: &[Patch], selection: &Selection) {
    let title_id = report.title_id.clone().unwrap_or_default();
    let app_ver = report
        .games
        .first()
        .and_then(|g| g.app_ver.clone())
        .unwrap_or_default();
    if all.is_empty() {
        out.line("no third-party patches in the patch folder");
        return;
    }
    let mut sections: Vec<(String, Vec<Patch>, Target)> = Game::ALL
        .into_iter()
        .map(|game| {
            (
                format!("{} ({})", game.name(), game.executable()),
                all.iter()
                    .filter(|patch| patches::game_of(patch) == Some(game))
                    .cloned()
                    .collect(),
                Target {
                    title_id: title_id.clone(),
                    executable: game.executable().into(),
                    app_ver: app_ver.clone(),
                },
            )
        })
        .collect();
    sections.push((
        "other executables".into(),
        all.iter()
            .filter(|patch| patches::game_of(patch).is_none())
            .cloned()
            .collect(),
        Target {
            title_id: String::new(),
            executable: String::new(),
            app_ver: String::new(),
        },
    ));
    for (heading, list, target) in sections {
        if list.is_empty() {
            continue;
        }
        out.line(format!("{heading}:"));
        for entry in patches::about_list(&list, &target, selection) {
            let state = match (entry.available, entry.enabled) {
                (false, _) => "n/a",
                (true, true) => "on ",
                (true, false) => "off",
            };
            let by = if entry.author.is_empty() {
                String::new()
            } else {
                format!(" by {}", entry.author)
            };
            let why = if entry.why_not.is_empty() {
                String::new()
            } else {
                format!(": {}", entry.why_not)
            };
            out.line(format!(
                "  [{state}] {}{by} ({}){why}",
                entry.name, entry.key
            ));
        }
    }
}

// ---- hub ----

mod hub;
pub use hub::DETAILS as HUB_DETAILS;

// ---- fleet ----

fn fleet(args: &[String], ctx: &Ctx) -> Done {
    if args.first().map(String::as_str) == Some("record") {
        return fleet_record(&args[1..], ctx);
    }
    let p = parsed("fleet", args, &["device"], 1)?;
    let file = match p.positional.first() {
        Some(file) => PathBuf::from(file),
        None => AppDirs::new(&core("fleet", vars(ctx))?)
            .config
            .join(FLEET_FILE),
    };
    let mut out = Outcome::default();
    if !file.exists() {
        out.line(format!(
            "no paired devices yet ({} does not exist)",
            file.display()
        ));
        return Ok(out);
    }
    let records = core("fleet", fleet::read_records(&file))?;
    write_fleet(&mut out, &records, p.flag("device"));
    Ok(out)
}

fn fleet_record(args: &[String], ctx: &Ctx) -> Done {
    let p = parsed(
        "fleet",
        args,
        &[
            "device",
            "build",
            "os",
            "patch-level",
            "sync-unix",
            "free-bytes",
            "file",
        ],
        0,
    )?;
    let device = p
        .flag("device")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Outcome::usage("fleet", "give --device <name>"))?;
    let build_id = p
        .flag("build")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Outcome::usage("fleet", "give --build <id>"))?;
    let os = match p.flag("os") {
        Some(key) => Os::from_key(key)
            .ok_or_else(|| Outcome::usage("fleet", &format!("unknown os: {key}")))?,
        None => ctx.os,
    };
    let patch_level: u32 = parse_int_flag("patch-level", p.flag("patch-level"))?;
    let last_sync_unix: u64 = parse_int_flag("sync-unix", p.flag("sync-unix"))?;
    let free_bytes: u64 = parse_int_flag("free-bytes", p.flag("free-bytes"))?;
    let file = match p.flag("file") {
        Some(file) => PathBuf::from(file),
        None => AppDirs::new(&core("fleet", vars(ctx))?)
            .config
            .join(FLEET_FILE),
    };
    let record = DeviceRecord {
        device: device.to_string(),
        os,
        build_id: build_id.to_string(),
        patch_level,
        last_sync_unix,
        free_bytes,
    };
    let records = core("fleet", fleet::upsert_file(&file, record))?;
    let mut out = Outcome::default();
    write_fleet(&mut out, &records, Some(device));
    Ok(out)
}

fn parse_int_flag<T: Default + std::str::FromStr>(
    name: &str,
    value: Option<&str>,
) -> std::result::Result<T, Outcome> {
    match value {
        None => Ok(T::default()),
        Some(raw) => raw.parse().map_err(|_| {
            Outcome::usage("fleet", &format!("--{name} must be a non-negative integer"))
        }),
    }
}

/// The fleet table; with `here` (this device's name), each other device's drift and the
/// warning the Continue screen shows before play when the latest save came from another build.
fn write_fleet(out: &mut Outcome, records: &[DeviceRecord], here: Option<&str>) {
    if records.is_empty() {
        out.line("no paired devices yet");
        return;
    }
    let here_record = here.and_then(|name| records.iter().find(|r| r.device == name));
    if let (Some(name), None) = (here, here_record) {
        out.note(format!("no record for the device '{name}'"));
        out.code = FAILED;
    }
    for record in records {
        let mut line = format!(
            "{}  {}  build {}  patch level {}  last sync {}  free {}",
            record.device,
            record.os.key(),
            record.build_id,
            record.patch_level,
            date(record.last_sync_unix),
            gib(record.free_bytes)
        );
        if let Some(here) = here_record {
            if here.device == record.device {
                line.push_str("  (this device)");
            } else {
                line.push_str(&format!(
                    "  {}",
                    describe_drift(&fleet::drift(here, record))
                ));
            }
        }
        out.line(line);
    }
    let Some(here) = here_record else { return };
    let latest = records
        .iter()
        .filter(|r| r.last_sync_unix > 0)
        .max_by_key(|r| r.last_sync_unix);
    if let Some(latest) = latest.filter(|r| r.device != here.device) {
        match fleet::warning(&fleet::drift(here, latest), latest) {
            Some(warning) => out.line(warning),
            None => out.line(format!(
                "same build as {}, which made the latest save",
                latest.device
            )),
        }
    }
}

fn describe_drift(drift: &Drift) -> String {
    match drift {
        Drift::None => "same build".into(),
        Drift::Patches { theirs, ours } => format!("patch level {theirs}, this device {ours}"),
        Drift::Build { .. } => "another build at the same patch level".into(),
    }
}

/// Seconds since the epoch as a UTC date ("2023-11-14"), or "never" for 0.
pub fn date(unix: u64) -> String {
    if unix == 0 {
        return "never".into();
    }
    // Days to a civil date (Howard Hinnant's days_from_civil, inverted).
    let days = (unix / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Bytes as GiB with one decimal ("12.5 GiB").
pub fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / (1u64 << 30) as f64)
}

// ---- generate-art ----

fn generate_art(args: &[String]) -> Done {
    let p = parsed("generate-art", args, &[], 1)?;
    let folder = p
        .positional
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| "crates/core/data/collection-artwork".into());
    core(
        "generate-art",
        artwork::generate_collection_art(&folder)
            .map_err(|error| Error::Io(format!("could not write launcher artwork: {error}"))),
    )?;
    let mut out = Outcome::default();
    out.line(format!("wrote launcher artwork to {}", folder.display()));
    Ok(out)
}

#[cfg(test)]
mod tests;
