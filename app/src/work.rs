//! The first run's jobs, carried out on their own thread with `uncharted_core::setup`; what each
//! comes to goes back to the UI thread as a `firstrun::Event` through a channel.

use crate::firstrun::{message, Event, Job};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;
use uncharted_core::games::Game;
use uncharted_core::hub::{self, SystemClock, TcpTransport};
use uncharted_core::platform::{AppDirs, Os, Vars};
use uncharted_core::prepare::PrepareOptions;
use uncharted_core::setup::{self, client};

/// How long discovery listens for hubs.
const DISCOVER_MS: u64 = 1_500;
/// How long a connection to a hub may take to open.
const CONNECT: Duration = Duration::from_secs(10);
/// How often the bytes of a pull are counted.
const PULL_POLL: Duration = Duration::from_millis(500);
/// How long Steam gets to quit.
const STEAM_QUIT: Duration = Duration::from_secs(60);

/// Where the jobs work.
#[derive(Clone)]
pub struct Context {
    pub os: Os,
    pub vars: Vars,
    pub dirs: AppDirs,
    pub env: fn(&str) -> Option<String>,
}

impl Context {
    /// This device: its OS, its folders and the process environment.
    pub fn here(os: Os, vars: Vars) -> Context {
        Context {
            os,
            dirs: AppDirs::new(&vars),
            vars,
            env: |k| std::env::var(k).ok(),
        }
    }
}

/// Run `job` on a new thread; its events go to `tx`. Jobs the UI does itself (`EditField`,
/// `Finish`) and `None` start nothing.
pub fn start(job: Job, cx: &Context, tx: Sender<Event>) {
    if matches!(job, Job::None | Job::EditField | Job::Finish { .. }) {
        return;
    }
    let cx = cx.clone();
    std::thread::spawn(move || run(job, &cx, &tx));
}

/// Run `job` here, sending its events to `tx`. A closed channel (the page went away) drops them.
pub fn run(job: Job, cx: &Context, tx: &Sender<Event>) {
    let send = |event| {
        let _ = tx.send(event);
    };
    match job {
        Job::None | Job::EditField | Job::Finish { .. } => {}
        Job::Scan => {
            let roots = setup::search_roots(&cx.vars, cx.env, Some(&cx.dirs));
            send(Event::Candidates(setup::dump_candidates(&roots)));
        }
        Job::Check(folder) => {
            let result = uncharted_core::dump::check(&folder).map_err(|e| message(&e));
            send(Event::Checked(folder, result));
        }
        Job::Prepare(plan) => {
            let done = setup::prepare_all(
                &plan,
                &cx.dirs,
                &PrepareOptions::default(),
                |index, game| send(Event::Preparing { index, game }),
            );
            send(Event::Prepared(done));
        }
        Job::Discover => send(hubs(hub::discover(DISCOVER_MS), cx.os)),
        Job::PairPull { hub, code } => {
            let staged = client::staging(&cx.dirs);
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    let result = TcpTransport::connect(hub.addr, CONNECT).and_then(|mut t| {
                        client::pair_and_pull(
                            &mut t,
                            &hub.name,
                            &code,
                            &cx.dirs,
                            cx.os,
                            &SystemClock,
                        )
                    });
                    let _ = done_tx.send(result.map_err(|e| message(&e)));
                });
                loop {
                    match done_rx.recv_timeout(PULL_POLL) {
                        Ok(result) => {
                            send(Event::Pulled(result));
                            break;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            send(Event::PullBytes(setup::folder_bytes(&staged)));
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            });
        }
        Job::AddToSteam => {
            if setup::steam_running() {
                send(Event::SteamOpen);
            } else {
                send(Event::Added(add_to_steam(cx)));
            }
        }
        Job::CloseSteamAndAdd => {
            let result = close_steam(cx.os).and_then(|()| add_to_steam(cx));
            send(Event::Added(result));
        }
    }
}

/// What a look for hubs (`hub::discover`, mDNS on the home network) came to: the hubs that
/// serve `os` as choices, or the error as a sentence.
fn hubs(found: uncharted_core::Result<Vec<hub::Found>>, os: Os) -> Event {
    Event::Hubs(
        found
            .map(|found| client::choices(&found, os))
            .map_err(|e| message(&e)),
    )
}

/// Ask Steam to quit and wait until it has.
fn close_steam(os: Os) -> Result<(), String> {
    let command = setup::steam_command(os, true).ok_or("This device has no Steam.")?;
    std::process::Command::new(&command.program)
        .args(&command.args)
        .status()
        .map_err(|e| format!("Could not ask Steam to close: {e}"))?;
    let start = std::time::Instant::now();
    while setup::steam_running() {
        if start.elapsed() > STEAM_QUIT {
            return Err("Steam did not close; close it and try again.".into());
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}

/// Start Steam again after it was closed for the shortcut. Best effort.
pub fn start_steam(os: Os) {
    if let Some(command) = setup::steam_command(os, false) {
        let _ = std::process::Command::new(&command.program)
            .args(&command.args)
            .spawn();
    }
}

/// The dump folder for the launcher's artwork: the first game's that has one.
fn dump(cx: &Context) -> Option<PathBuf> {
    Game::ALL
        .into_iter()
        .find_map(|game| setup::dump_folder(game, cx.env, Some(&cx.dirs)))
}

/// Add this program to Steam, in a sentence.
fn add_to_steam(cx: &Context) -> Result<String, String> {
    let userdata = setup::userdata(cx.os, &cx.vars)
        .ok_or("Steam is not installed for this user: there is no Steam userdata folder.")?;
    let exe = std::env::current_exe().map_err(|e| format!("Cannot find this program: {e}"))?;
    let images = setup::launcher_images(dump(cx).as_deref());
    let added = setup::add_to_steam(&userdata, &exe, &images, &setup::steam_running)
        .map_err(|e| message(&e))?;
    Ok(added_text(added.shortcuts.len(), added.images.len()))
}

/// "Added to Steam for 2 users, with 1 picture."
pub fn added_text(users: usize, images: usize) -> String {
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    format!(
        "Added to Steam for {}, with {}. It shows in the library as {}.",
        plural(users, "user", "users"),
        plural(images, "picture", "pictures"),
        setup::SHORTCUT_NAME
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc;
    use uncharted_core::recipe::Hub;

    /// A fresh temp folder under `std::env::temp_dir()`, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Temp {
            static N: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "uncharted-app-work-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Temp(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn context(root: &std::path::Path) -> Context {
        let vars = Vars {
            home: root.join("home"),
            data: root.join("data"),
            config: root.join("config"),
        };
        Context {
            os: Os::Linux,
            dirs: AppDirs::new(&vars),
            vars,
            env: |_| None,
        }
    }

    fn events(job: Job, cx: &Context) -> Vec<Event> {
        let (tx, rx) = mpsc::channel();
        run(job, cx, &tx);
        drop(tx);
        rx.into_iter().collect()
    }

    #[test]
    fn scan_finds_a_dump_under_games() {
        let root = Temp::new();
        let cx = context(root.path());
        let dump = cx.vars.home.join("Games").join("CUSA02320");
        std::fs::create_dir_all(dump.join("sce_sys")).unwrap();
        std::fs::write(dump.join("sce_sys").join("param.sfo"), b"x").unwrap();
        assert_eq!(events(Job::Scan, &cx), [Event::Candidates(vec![dump])]);
    }

    #[test]
    fn check_of_a_missing_folder_is_an_error_event() {
        let root = Temp::new();
        let cx = context(root.path());
        let folder = root.path().join("nothing");
        let got = events(Job::Check(folder.clone()), &cx);
        assert!(
            matches!(&got[..], [Event::Checked(f, Err(_))] if *f == folder),
            "{got:?}"
        );
    }

    #[test]
    fn prepare_reports_each_game_then_the_results() {
        let root = Temp::new();
        let cx = context(root.path());
        let plan = vec![(Game::U1, root.path().join("empty"))];
        let got = events(Job::Prepare(plan), &cx);
        assert_eq!(got.len(), 2);
        assert_eq!(
            got[0],
            Event::Preparing {
                index: 0,
                game: Game::U1
            }
        );
        assert!(matches!(&got[1], Event::Prepared(done) if done[0].result.is_err()));
    }

    #[test]
    fn discovery_lists_the_hubs_that_serve_this_device() {
        // The in-process mDNS fake: a hub for linux and one that only serves windows.
        let net = hub::MemoryMdns::new();
        let announce = |hub_kind, target, last| {
            let a = hub::Announcement {
                hub: hub_kind,
                targets: vec![target],
                port: 47_210,
                addrs: vec![std::net::IpAddr::from([192, 168, 1, last])],
            };
            hub::announce(&net, &a).unwrap();
        };
        announce(Hub::StrixHalo, Os::Linux, 20);
        announce(Hub::Desktop, Os::Windows, 21);
        let got = hubs(hub::discover_on(&net, 1), Os::Linux);
        let Event::Hubs(Ok(choices)) = got else {
            panic!("{got:?}");
        };
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].addr.to_string(), "192.168.1.20:47210");
        assert_eq!(choices[0].name, Hub::StrixHalo.key());
    }

    #[test]
    fn a_discovery_error_is_a_message() {
        let got = hubs(
            Err(uncharted_core::Error::Hub("the mDNS daemon stopped".into())),
            Os::Linux,
        );
        assert!(
            matches!(&got, Event::Hubs(Err(m)) if m.contains("mDNS daemon stopped")),
            "{got:?}"
        );
    }

    #[test]
    fn a_hub_that_does_not_answer_is_an_error() {
        let root = Temp::new();
        // A port nothing listens on: bind one, then free it.
        let addr = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let hub = client::HubChoice {
            label: addr.to_string(),
            name: addr.to_string(),
            addr,
        };
        let got = events(
            Job::PairPull {
                hub,
                code: "123456".into(),
            },
            &context(root.path()),
        );
        assert!(matches!(got.last(), Some(Event::Pulled(Err(_)))), "{got:?}");
    }

    #[test]
    fn ui_jobs_send_nothing() {
        let root = Temp::new();
        let cx = context(root.path());
        assert!(events(Job::EditField, &cx).is_empty());
        assert!(events(Job::Finish { start_steam: true }, &cx).is_empty());
    }

    #[test]
    fn the_added_sentence() {
        assert_eq!(
            added_text(1, 2),
            "Added to Steam for 1 user, with 2 pictures. It shows in the library as \
             Uncharted: The Nathan Drake Collection."
        );
        assert!(added_text(2, 1).starts_with("Added to Steam for 2 users, with 1 picture."));
    }
}
