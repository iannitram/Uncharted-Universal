//! The launcher (ADR 0003). So far: the Continue screen's place held (piece 2.04) with two
//! placeholder buttons, and the after-crash screen (piece 2.07, `vm::after_crash`). Keyboard
//! and controller focus move between the buttons of the page shown.
//!
//! `uncharted-app --play <u1|u2|u3> [-- <runtime arguments>]` starts that game at once (as a
//! Steam shortcut does) and shows the after-crash screen when it crashes; a run the app did not
//! see end is picked up on the next start.

mod focus;
mod input;
mod play;
mod settings_file;
mod strings;
mod vm;

use focus::{FocusRing, Nav};
use input::{Action, Controller};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use strings::{fill, Lang, Text};
use uncharted_core::games::Game;
use uncharted_core::logs::{self, Redact, ReportDetails};
use uncharted_core::platform::{AppDirs, Os, Vars};
use uncharted_core::saves;
use vm::after_crash::{self, AfterCrash, Context, Host, Pressed};

slint::include_modules!();

/// The home page's placeholder buttons.
const HOME_BUTTONS: usize = 2;
/// `known_issues.json`, built in.
const KNOWN_ISSUES: &str = include_str!("../../crates/core/data/known_issues.json");

/// The page shown (`page` in `ui/app.slint`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Home = 0,
    AfterCrash = 1,
    Running = 2,
}

/// What the command line asks for.
#[derive(Debug, Default, PartialEq, Eq)]
struct Args {
    play: Option<Game>,
    run_args: Vec<String>,
}

/// `--play <game>`, then everything after `--` for the runtime. Anything else is an error.
fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--play" => {
                let key = rest.next().ok_or("--play needs a game: u1, u2 or u3")?;
                parsed.play =
                    Some(Game::from_key(key).ok_or_else(|| format!("unknown game: {key}"))?);
            }
            "--" => {
                parsed.run_args = rest.by_ref().cloned().collect();
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(parsed)
}

/// The launcher's folders, when the OS gives them.
struct Folders {
    vars: Vars,
    dirs: AppDirs,
}

impl Folders {
    fn find() -> Option<Folders> {
        match Vars::from_lookup(Os::current(), |name| std::env::var(name).ok()) {
            Ok(vars) => Some(Folders {
                dirs: AppDirs::new(&vars),
                vars,
            }),
            Err(error) => {
                eprintln!("crash help is off: {error}");
                None
            }
        }
    }

    fn settings(&self) -> PathBuf {
        self.dirs.config.join(settings_file::SETTINGS_FILE)
    }

    /// The game's saves folder, where the runtime writes `where.json`.
    fn saves(&self, game: Game) -> PathBuf {
        saves::save_root(
            game,
            Os::current(),
            &self.vars,
            &self.dirs.install(game.key()),
        )
        .unwrap_or_else(|_| self.dirs.data.join("user").join(game.name()))
    }

    fn context(&self) -> Context {
        let settings = settings_file::read(&self.settings());
        let preset = settings
            .get("preset")
            .and_then(|value| value.as_str())
            .unwrap_or("default")
            .to_string();
        let launcher = std::env::current_exe()
            .map(|exe| exe.display().to_string())
            .unwrap_or_else(|_| "?".into());
        Context {
            logs: self.dirs.logs(),
            build: env!("CARGO_PKG_VERSION").to_string(),
            preset,
            issues: logs::parse_known_issues(KNOWN_ISSUES).unwrap_or_default(),
            details: ReportDetails {
                version: env!("CARGO_PKG_VERSION").to_string(),
                at_unix: now(),
                system: vec![format!("Launcher: {launcher}")],
                settings,
                // Not measured here: no low-space line in the report.
                free_bytes: u64::MAX,
            },
            redact: Redact::for_home(&self.vars.home, Vec::new()),
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The app's state between callbacks.
struct App {
    lang: Lang,
    folders: Option<Folders>,
    run_args: Vec<String>,
    page: Page,
    ring: FocusRing,
    screen: Option<AfterCrash>,
    note: String,
}

/// The real [`Host`]: settings in `settings.json`, games through `uncharted-universal play`.
struct AppHost<'a> {
    window: &'a AppWindow,
    folders: &'a Folders,
    run_args: &'a [String],
}

impl Host for AppHost<'_> {
    fn set_setting(&mut self, key: &str, value: &str) -> Result<(), String> {
        settings_file::set_extra(&self.folders.settings(), key, value).map_err(|e| e.to_string())
    }

    fn play(&mut self, game: Game) -> Result<(), String> {
        let env = settings_file::extra(&self.folders.settings());
        let mut child = play::start(
            &play::cli_program(),
            game,
            self.run_args,
            &env,
            &self.folders.dirs.logs(),
        )
        .map_err(|e| e.to_string())?;
        let weak = self.window.as_weak();
        std::thread::spawn(move || {
            if let Err(error) = child.wait() {
                eprintln!("could not wait for the game: {error}");
            }
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak.upgrade() {
                    window.invoke_run_ended(game.key().into());
                }
            });
        });
        Ok(())
    }

    fn open_url(&mut self, url: &str) {
        if let Err(error) = play::open_url(url) {
            eprintln!("could not open {url}: {error}");
        }
    }

    fn copy_text(&mut self, text: &str) {
        self.window.invoke_copy_text(text.into());
    }
}

impl App {
    /// Show `page` with its focus on the first button.
    fn show(&mut self, window: &AppWindow, page: Page) {
        self.page = page;
        let buttons = match page {
            Page::Home => HOME_BUTTONS,
            Page::AfterCrash => self.screen.as_ref().map_or(1, |s| s.buttons().len()),
            Page::Running => 1,
        };
        self.ring = FocusRing::new(buttons.max(1));
        window.set_focused_index(0);
        window.set_page(page as i32);
        if let (Page::AfterCrash, Some(screen)) = (page, &self.screen) {
            let texts = screen.screen(self.lang);
            window.set_crash_heading(texts.heading.into());
            window.set_crash_game(texts.game.into());
            window.set_crash_chapter(texts.chapter.into());
            window.set_crash_summary(texts.summary.into());
            window.set_crash_detail(texts.detail.into());
            let labels: Vec<SharedString> = texts.buttons.into_iter().map(Into::into).collect();
            window.set_crash_buttons(ModelRc::new(VecModel::from(labels)));
        }
        window.set_crash_note(self.note.as_str().into());
    }

    /// The after-run step for `game`; the after-crash screen when it crashed, else home.
    fn run_ended(&mut self, window: &AppWindow, game: Game) {
        let Some(folders) = &self.folders else {
            self.show(window, Page::Home);
            return;
        };
        self.screen = after_crash::after_run(&folders.context(), game, &folders.saves(game), now());
        self.note.clear();
        let page = if self.screen.is_some() {
            Page::AfterCrash
        } else {
            Page::Home
        };
        self.show(window, page);
    }

    /// Start `game` and show that it runs, or say why it could not start.
    fn start(&mut self, window: &AppWindow, game: Game) {
        let Some(folders) = &self.folders else {
            return;
        };
        let mut host = AppHost {
            window,
            folders,
            run_args: &self.run_args,
        };
        match host.play(game) {
            Ok(()) => self.running(window, game),
            Err(error) => eprintln!("{}", fill(Text::StartFailed.get(self.lang), &error)),
        }
    }

    fn running(&mut self, window: &AppWindow, game: Game) {
        window.set_status(fill(Text::Running.get(self.lang), game.name()).into());
        self.show(window, Page::Running);
    }

    fn activate(&mut self, window: &AppWindow, index: usize) {
        match self.page {
            Page::Home => println!("activated placeholder button {index}"),
            Page::Running => {}
            Page::AfterCrash => {
                let (Some(screen), Some(folders)) = (self.screen.clone(), &self.folders) else {
                    return;
                };
                let Some(&button) = screen.buttons().get(index) else {
                    return;
                };
                let ctx = folders.context();
                let mut host = AppHost {
                    window,
                    folders,
                    run_args: &self.run_args,
                };
                match screen.press(button, &ctx, &mut host, self.lang) {
                    Pressed::Stay(note) => {
                        self.note = note;
                        window.set_crash_note(self.note.as_str().into());
                    }
                    Pressed::Playing => {
                        self.note.clear();
                        self.running(window, screen.game);
                    }
                    Pressed::Home => {
                        self.note.clear();
                        self.show(window, Page::Home);
                    }
                }
            }
        }
    }

    fn back(&mut self, window: &AppWindow) {
        if self.page == Page::AfterCrash {
            self.note.clear();
            self.show(window, Page::Home);
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&args) {
        Ok(args) => args,
        Err(problem) => {
            eprintln!(
                "{problem}\nusage: uncharted-app [--play <u1|u2|u3> [-- <runtime arguments>]]"
            );
            std::process::exit(2);
        }
    };

    let window = AppWindow::new()?;
    let app = Rc::new(RefCell::new(App {
        lang: Lang::detect(|name| std::env::var(name).ok()),
        folders: Folders::find(),
        run_args: args.run_args,
        page: Page::Home,
        ring: FocusRing::new(HOME_BUTTONS),
        screen: None,
        note: String::new(),
    }));

    {
        let mut app = app.borrow_mut();
        app.show(&window, Page::Home);
        // A run the app started but did not see end (closed, or left by Game Mode).
        let unfinished = app
            .folders
            .as_ref()
            .and_then(|folders| after_crash::unfinished(&folders.dirs.logs()));
        if let Some(game) = unfinished {
            app.run_ended(&window, game);
        }
        if let Some(game) = args.play {
            app.start(&window, game);
        }
    }

    let apply_nav = {
        let app = app.clone();
        let weak = window.as_weak();
        Rc::new(move |nav: Nav| {
            let mut app = app.borrow_mut();
            app.ring.step(nav);
            if let Some(window) = weak.upgrade() {
                window.set_focused_index(app.ring.index() as i32);
            }
        })
    };

    window.on_nav({
        let apply_nav = apply_nav.clone();
        move |code| {
            if let Some(nav) = Nav::from_code(code) {
                apply_nav(nav);
            }
        }
    });
    window.on_activate({
        let app = app.clone();
        let weak = window.as_weak();
        move |index| {
            if let Some(window) = weak.upgrade() {
                app.borrow_mut().activate(&window, index.max(0) as usize);
            }
        }
    });
    window.on_back({
        let app = app.clone();
        let weak = window.as_weak();
        move || {
            if let Some(window) = weak.upgrade() {
                app.borrow_mut().back(&window);
            }
        }
    });
    window.on_run_ended({
        let app = app.clone();
        let weak = window.as_weak();
        move |key| {
            if let (Some(window), Some(game)) = (weak.upgrade(), Game::from_key(&key)) {
                app.borrow_mut().run_ended(&window, game);
            }
        }
    });

    // Poll the controller on the UI thread; gilrs events are cheap to drain.
    let mut controller = Controller::new();
    let timer = slint::Timer::default();
    let weak = window.as_weak();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(16),
        move || {
            let Some(controller) = controller.as_mut() else {
                return;
            };
            for action in controller.poll() {
                let Some(window) = weak.upgrade() else {
                    return;
                };
                match action {
                    Action::Move(nav) => apply_nav(nav),
                    Action::Activate => window.invoke_activate(window.get_focused_index()),
                    Action::Back => window.invoke_back(),
                }
            }
        },
    );

    window.run()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn play_and_runtime_arguments() {
        assert_eq!(parse_args(&[]), Ok(Args::default()));
        assert_eq!(
            parse_args(&args(&["--play", "U2", "--", "--force-fault", "x"])),
            Ok(Args {
                play: Some(Game::U2),
                run_args: args(&["--force-fault", "x"]),
            })
        );
        assert!(parse_args(&args(&["--play"])).is_err());
        assert!(parse_args(&args(&["--play", "u9"])).is_err());
        assert!(parse_args(&args(&["--fast"])).is_err());
    }
}
