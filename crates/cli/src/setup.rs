//! `uncharted-universal setup`: the guided first run of the app (roadmap 2.05) without the
//! window, step for step with the same `uncharted_core::setup` calls.
//!
//! ```text
//! uncharted-universal setup [status]
//! uncharted-universal setup hub    [<dump folder>] [u1|u2|u3]
//! uncharted-universal setup client <address:port> <six-digit code>
//! uncharted-universal setup steam  [--close-steam]
//! ```
//!
//! `hub` checks the dump (the folder given, else `BB_GAME_DIR*` or the noted folder, else the
//! first dump found nearby) and prepares its games, or only the one named. `client` pairs with
//! the hub and pulls the prepared games. `steam` adds the launcher to Steam; while Steam is open
//! it stops, unless `--close-steam` lets it close Steam and start it again afterwards.

use super::Outcome;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Duration;
use uncharted_core::games::Game;
use uncharted_core::hub::{SystemClock, TcpTransport};
use uncharted_core::platform::{AppDirs, Os, Vars};
use uncharted_core::prepare::PrepareOptions;
use uncharted_core::setup::{self, client};
use uncharted_core::{dump, Error};

pub const USAGE: &str = "usage: uncharted-universal setup [status]
       uncharted-universal setup hub [<dump folder>] [u1|u2|u3]
       uncharted-universal setup client <address:port> <six-digit code>
       uncharted-universal setup steam [--close-steam]
";

/// How long a connection to a hub may take to open.
const CONNECT: Duration = Duration::from_secs(10);
/// How long Steam gets to quit.
const STEAM_QUIT: Duration = Duration::from_secs(60);

/// Where setup works: this device, or temp folders in the tests.
pub struct Cx<'a> {
    pub os: Os,
    pub vars: Vars,
    pub dirs: AppDirs,
    pub env: &'a dyn Fn(&str) -> Option<String>,
    pub steam_running: &'a dyn Fn() -> bool,
    /// The launcher the Steam shortcut starts.
    pub app_exe: PathBuf,
}

/// `uncharted-app` beside this program.
pub fn app_exe() -> PathBuf {
    let file = format!("uncharted-app{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&file)))
        .unwrap_or_else(|| PathBuf::from(file))
}

/// Run `setup` on this device.
pub fn main(args: &[String]) -> Outcome {
    let os = Os::current();
    let env = |k: &str| std::env::var(k).ok();
    let vars = match Vars::from_lookup(os, env) {
        Ok(vars) => vars,
        Err(err) => return fail(format!("cannot find this device's folders: {err}")),
    };
    let cx = Cx {
        os,
        dirs: AppDirs::new(&vars),
        vars,
        env: &env,
        steam_running: &setup::steam_running,
        app_exe: app_exe(),
    };
    run(args, &cx, &mut |line| eprintln!("{line}"))
}

fn fail(message: String) -> Outcome {
    Outcome {
        stdout: String::new(),
        stderr: format!("uncharted-universal setup: {message}\n"),
        code: 1,
    }
}

fn usage(problem: &str) -> Outcome {
    Outcome {
        stdout: String::new(),
        stderr: format!("uncharted-universal setup: {problem}\n\n{USAGE}"),
        code: 2,
    }
}

fn done(stdout: String) -> Outcome {
    Outcome {
        stdout,
        stderr: String::new(),
        code: 0,
    }
}

/// Run `setup` with `args` (after `setup`); `progress` gets a line as each long step starts.
pub fn run(args: &[String], cx: &Cx, progress: &mut dyn FnMut(&str)) -> Outcome {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] | ["status"] => done(status(cx)),
        ["-h" | "--help" | "help"] => done(USAGE.to_owned()),
        ["hub", rest @ ..] => hub(rest, cx, progress),
        ["client", addr, code] => client(addr, code, cx, progress),
        ["client", ..] => usage("client takes the hub's address:port and the six-digit code"),
        ["steam"] => steam(cx, false, progress),
        ["steam", "--close-steam"] => steam(cx, true, progress),
        [other, ..] => usage(&format!("unknown step '{other}'")),
    }
}

/// What is set up on this device.
fn status(cx: &Cx) -> String {
    let installed = setup::installed_games(&cx.dirs);
    let mut out = String::new();
    let names: Vec<&str> = installed.iter().map(|g| g.key()).collect();
    let _ = writeln!(
        out,
        "installed: {}",
        if names.is_empty() {
            "none".to_owned()
        } else {
            names.join(" ")
        }
    );
    let _ = writeln!(
        out,
        "paired hub: {}",
        if setup::has_peers(&cx.dirs) {
            "yes"
        } else {
            "no"
        }
    );
    let dump = Game::ALL
        .into_iter()
        .find_map(|game| setup::dump_folder(game, cx.env, Some(&cx.dirs)));
    let _ = writeln!(
        out,
        "dump folder: {}",
        dump.map_or("not known".to_owned(), |d| d.display().to_string())
    );
    let _ = writeln!(
        out,
        "first run: {}",
        if setup::is_first_run(&cx.dirs) {
            "needed (setup hub or setup client)"
        } else {
            "done"
        }
    );
    out
}

/// The hub path: check the dump, prepare its games.
fn hub(args: &[&str], cx: &Cx, progress: &mut dyn FnMut(&str)) -> Outcome {
    let (folder, only) = match args {
        [] => (None, None),
        [one] => match Game::from_key(one) {
            Some(game) => (None, Some(game)),
            None => (Some(PathBuf::from(one)), None),
        },
        [folder, game] => match Game::from_key(game) {
            Some(game) => (Some(PathBuf::from(folder)), Some(game)),
            None => return usage(&format!("unknown game '{game}' (u1, u2 or u3)")),
        },
        _ => return usage("hub takes a dump folder and a game, both optional"),
    };
    let Some(folder) = folder.or_else(|| find_dump(cx, only)) else {
        return fail(
            "no dump found: give its folder, as setup hub /home/deck/Games/CUSA02320".into(),
        );
    };
    progress(&format!("checking {}", folder.display()));
    let report = match dump::check(&folder) {
        Ok(report) => report,
        Err(err) => return fail(message(&err)),
    };
    let mut out = String::new();
    for found in &report.games {
        let _ = writeln!(
            out,
            "found {} {}",
            found.game.key(),
            found.app_ver.as_deref().unwrap_or("(version unknown)")
        );
    }
    for problem in &report.problems {
        let _ = writeln!(out, "problem: {problem}");
    }
    let plan = if setup::can_prepare(&report) {
        setup::prepare_plan(&report, only)
    } else {
        Vec::new()
    };
    if plan.is_empty() {
        return Outcome {
            stdout: out,
            stderr: "uncharted-universal setup: nothing in this dump can be prepared\n".into(),
            code: 1,
        };
    }
    let total = plan.len();
    let results = setup::prepare_all(
        &plan,
        &cx.dirs,
        &PrepareOptions::default(),
        |index, game| {
            progress(&format!(
                "preparing {} ({} of {total})",
                game.name(),
                index + 1
            ))
        },
    );
    let mut failed = false;
    for prepared in &results {
        match &prepared.result {
            Ok(_) => {
                let _ = writeln!(out, "prepared {}", prepared.game.key());
            }
            Err(err) => {
                failed = true;
                let _ = writeln!(out, "failed {}: {err}", prepared.game.key());
            }
        }
    }
    Outcome {
        stdout: out,
        stderr: String::new(),
        code: u8::from(failed),
    }
}

/// The dump the hub path uses when no folder is given: the configured or noted one, else the
/// first found nearby.
fn find_dump(cx: &Cx, only: Option<Game>) -> Option<PathBuf> {
    let games = only.map_or(Game::ALL.to_vec(), |g| vec![g]);
    games
        .into_iter()
        .find_map(|game| setup::dump_folder(game, cx.env, Some(&cx.dirs)))
        .or_else(|| {
            let roots = setup::search_roots(&cx.vars, cx.env, Some(&cx.dirs));
            setup::dump_candidates(&roots).into_iter().next()
        })
}

/// The client path: pair with the hub and pull its prepared games.
fn client(addr: &str, code: &str, cx: &Cx, progress: &mut dyn FnMut(&str)) -> Outcome {
    let hub = match client::hub_address(addr) {
        Ok(hub) => hub,
        Err(err) => return fail(message(&err)),
    };
    progress(&format!("pairing with {} and pulling the games", hub.label));
    let pulled = TcpTransport::connect(hub.addr, CONNECT).and_then(|mut t| {
        client::pair_and_pull(&mut t, &hub.name, code, &cx.dirs, cx.os, &SystemClock)
    });
    match pulled {
        Ok(pulled) => {
            let mut out = String::new();
            for game in &pulled.games {
                let _ = writeln!(out, "installed {}", game.key());
            }
            let _ = writeln!(out, "received {}", setup::bytes_text(pulled.bytes));
            if let Some(note) = pulled.peer_note {
                let _ = writeln!(out, "hub not remembered: {note}");
            }
            done(out)
        }
        Err(err) => fail(message(&err)),
    }
}

/// Add the launcher to Steam.
fn steam(cx: &Cx, close: bool, progress: &mut dyn FnMut(&str)) -> Outcome {
    let Some(userdata) = setup::userdata(cx.os, &cx.vars) else {
        return fail("Steam is not installed for this user: no Steam userdata folder".into());
    };
    let closed = if (cx.steam_running)() {
        if !close {
            return fail("Steam is open; close it first, or run setup steam --close-steam".into());
        }
        progress("closing Steam");
        if let Err(err) = close_steam(cx) {
            return fail(err);
        }
        true
    } else {
        false
    };
    let dump = Game::ALL
        .into_iter()
        .find_map(|game| setup::dump_folder(game, cx.env, Some(&cx.dirs)));
    let images = setup::launcher_images(dump.as_deref());
    let result = setup::add_to_steam(&userdata, &cx.app_exe, &images, cx.steam_running);
    if closed {
        if let Some(command) = setup::steam_command(cx.os, false) {
            progress("starting Steam again");
            let _ = std::process::Command::new(&command.program)
                .args(&command.args)
                .spawn();
        }
    }
    match result {
        Ok(added) => {
            let mut out = String::new();
            for path in added.shortcuts.iter().chain(&added.images) {
                let _ = writeln!(out, "wrote {}", path.display());
            }
            let _ = writeln!(
                out,
                "added {} (app id {})",
                setup::SHORTCUT_NAME,
                added.app_id
            );
            done(out)
        }
        Err(err) => fail(message(&err)),
    }
}

fn close_steam(cx: &Cx) -> Result<(), String> {
    let command = setup::steam_command(cx.os, true).ok_or("this device has no Steam")?;
    std::process::Command::new(&command.program)
        .args(&command.args)
        .status()
        .map_err(|e| format!("could not ask Steam to close: {e}"))?;
    let start = std::time::Instant::now();
    while (cx.steam_running)() {
        if start.elapsed() > STEAM_QUIT {
            return Err("Steam did not close; close it and try again".into());
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}

/// A core error in plain words: a hub's own message without the "Build hub:" prefix.
fn message(err: &Error) -> String {
    match err {
        Error::Hub(m) => m.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A fresh temp folder under `std::env::temp_dir()`, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Temp {
            static N: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "uncharted-cli-setup-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Temp(path)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn cx<'a>(root: &Path, running: &'a dyn Fn() -> bool) -> Cx<'a> {
        let vars = Vars {
            home: root.join("home"),
            data: root.join("data"),
            config: root.join("config"),
        };
        Cx {
            os: Os::Linux,
            dirs: AppDirs::new(&vars),
            vars,
            env: &no_env,
            steam_running: running,
            app_exe: root.join("uncharted-app"),
        }
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn run_quiet(list: &[&str], cx: &Cx) -> Outcome {
        run(&args(list), cx, &mut |_| {})
    }

    #[test]
    fn status_of_a_new_device_says_first_run() {
        let temp = Temp::new();
        let out = run_quiet(&[], &cx(&temp.0, &|| false));
        assert_eq!(out.code, 0);
        assert_eq!(
            out.stdout,
            "installed: none\npaired hub: no\ndump folder: not known\n\
             first run: needed (setup hub or setup client)\n"
        );
        assert_eq!(
            run_quiet(&["status"], &cx(&temp.0, &|| false)).stdout,
            out.stdout
        );
    }

    #[test]
    fn status_lists_the_installs() {
        let temp = Temp::new();
        let cx = cx(&temp.0, &|| false);
        let install = cx.dirs.install("u2");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join(uncharted_core::prepare::MANIFEST_FILE), "{}").unwrap();
        let out = run_quiet(&["status"], &cx);
        assert!(out.stdout.starts_with("installed: u2\n"), "{}", out.stdout);
        assert!(out.stdout.ends_with("first run: done\n"));
    }

    #[test]
    fn hub_without_a_dump_says_where_to_point_it() {
        let temp = Temp::new();
        let out = run_quiet(&["hub"], &cx(&temp.0, &|| false));
        assert_eq!(out.code, 1);
        assert!(out.stderr.contains("no dump found"), "{}", out.stderr);
        let out = run_quiet(&["hub", "/x", "u9"], &cx(&temp.0, &|| false));
        assert_eq!(out.code, 2);
        assert!(out.stderr.contains("unknown game 'u9'"));
    }

    #[test]
    fn hub_reports_a_folder_that_is_not_a_dump() {
        let temp = Temp::new();
        let folder = temp.0.join("empty");
        std::fs::create_dir_all(&folder).unwrap();
        let mut lines = Vec::new();
        let out = run(
            &args(&["hub", folder.to_str().unwrap()]),
            &cx(&temp.0, &|| false),
            &mut |l| lines.push(l.to_owned()),
        );
        assert_eq!(out.code, 1);
        assert!(lines[0].starts_with("checking "));
    }

    #[test]
    fn client_refuses_a_bad_address_and_needs_both_arguments() {
        let temp = Temp::new();
        let cx = cx(&temp.0, &|| false);
        let out = run_quiet(&["client", "192.168.1.20", "123456"], &cx);
        assert_eq!(out.code, 1);
        assert_eq!(
            out.stderr,
            "uncharted-universal setup: add the hub's port after its address, as \
             192.168.1.20:47120\n"
        );
        let out = run_quiet(&["client", "192.168.1.20:47120"], &cx);
        assert_eq!(out.code, 2);
        assert!(out.stderr.contains("usage:"));
    }

    #[test]
    fn steam_needs_its_userdata_and_stops_while_it_is_open() {
        let temp = Temp::new();
        let out = run_quiet(&["steam"], &cx(&temp.0, &|| false));
        assert_eq!(out.code, 1);
        assert!(out.stderr.contains("no Steam userdata folder"));

        let userdata = temp.0.join("home/.local/share/Steam/userdata");
        std::fs::create_dir_all(userdata.join("1234/config")).unwrap();
        let out = run_quiet(&["steam"], &cx(&temp.0, &|| true));
        assert_eq!(out.code, 1, "{}", out.stderr);
        assert!(out.stderr.contains("--close-steam"), "{}", out.stderr);

        let out = run_quiet(&["steam"], &cx(&temp.0, &|| false));
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(out.stdout.contains("shortcuts.vdf"), "{}", out.stdout);
        assert!(out
            .stdout
            .contains("added Uncharted: The Nathan Drake Collection (app id "));
    }

    #[test]
    fn unknown_steps_show_the_usage() {
        let temp = Temp::new();
        let out = run_quiet(&["frob"], &cx(&temp.0, &|| false));
        assert_eq!(out.code, 2);
        assert!(out.stderr.contains("unknown step 'frob'"));
        assert_eq!(
            run_quiet(&["--help"], &cx(&temp.0, &|| false)).stdout,
            USAGE
        );
    }
}
