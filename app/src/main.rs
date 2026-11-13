//! The launcher (ADR 0003). So far: the Continue screen's place held, a language button that
//! steps through the seven interface languages, About (version, patches, licences, the Slint
//! notice), and the after-crash screen (piece 2.07, `vm::after_crash`), all reachable with
//! keyboard and controller focus.
//!
//! `uncharted-app --play <u1|u2|u3> [-- <runtime arguments>]` starts that game at once (as a
//! Steam shortcut does) and shows the after-crash screen when it crashes; a run the app did not
//! see end is picked up on the next start. When that pickup shows the after-crash screen,
//! `--play` waits for it: the screen's Play again button starts the game.

mod focus;
mod input;
mod lang;
mod play;
mod settings_file;
mod strings;
mod vm;

use focus::{Effect, Launcher, Nav, Screen};
use input::{Action, Controller};
use lang::Texts;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use strings::{fill, Lang, Text};
use uncharted_core::games::Game;
use uncharted_core::language::{self, Language};
use uncharted_core::logs::{self, Redact, ReportDetails};
use uncharted_core::patches::{Patch, Selection, Target};
use uncharted_core::platform::{AppDirs, Os, Vars};
use uncharted_core::saves;
use vm::after_crash::{self, AfterCrash, Context, Host, Pressed};

slint::include_modules!();

/// `known_issues.json`, built in.
const KNOWN_ISSUES: &str = include_str!("../../crates/core/data/known_issues.json");

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

/// The launcher's state outside the window.
struct App {
    launcher: Launcher,
    /// `ui_language`: "" is Automatic.
    setting: String,
    /// Where the setting is saved; `None` when the platform folders are not set.
    settings_file: Option<PathBuf>,
    system_locale: Option<String>,
    texts: Texts,
    /// The chosen game's patches and identity, for About. The Continue screen (piece 2.04)
    /// picks the game; until then the list is empty.
    patches: Vec<Patch>,
    target: Target,
    selection: Selection,
    /// The launcher's folders, for crash help; `None` turns it off.
    folders: Option<Folders>,
    /// What `--play` passes on to the runtime.
    run_args: Vec<String>,
    /// The crashed run the after-crash screen shows.
    crash: Option<AfterCrash>,
    /// The after-crash screen's note under its buttons.
    note: String,
    /// The running screen's line.
    status: String,
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
    fn new(setting: String, settings_file: Option<PathBuf>, system_locale: Option<String>) -> App {
        let language = language::resolve(&setting, system_locale.as_deref());
        App {
            launcher: Launcher::default(),
            setting,
            settings_file,
            system_locale,
            texts: Texts::new(language),
            patches: Vec::new(),
            target: Target {
                title_id: String::new(),
                executable: String::new(),
                app_ver: String::new(),
            },
            selection: Selection::default(),
            folders: None,
            run_args: Vec::new(),
            crash: None,
            note: String::new(),
            status: String::new(),
        }
    }

    fn language(&self) -> Language {
        self.texts.language()
    }

    /// The after-crash texts' language: the interface language.
    fn crash_lang(&self) -> Lang {
        Lang::of(self.language())
    }

    /// The language button's value.
    fn language_name(&self) -> String {
        let name = self.language().native_name();
        if self.setting.is_empty() {
            self.texts.tr1("Automatic ({})", name)
        } else {
            name.to_owned()
        }
    }

    /// Step to the next choice, save it, and switch the texts.
    fn next_language(&mut self) {
        self.setting = language::next_setting(&self.setting).to_owned();
        if let Some(file) = &self.settings_file {
            if let Err(err) = language::write_setting(file, &self.setting) {
                eprintln!("could not save the interface language: {err}");
            }
        }
        let language = language::resolve(&self.setting, self.system_locale.as_deref());
        self.texts = Texts::new(language);
    }

    /// Push everything the window shows that Rust builds, in the current language.
    fn show(&self, window: &AppWindow) {
        if let Err(err) = slint::select_bundled_translation(self.language().code()) {
            eprintln!("could not select the interface language: {err}");
        }
        window.set_page(self.launcher.screen() as i32);
        window.set_focused_index(self.launcher.focused() as i32);
        window.set_language_name(self.language_name().into());
        window.set_version(env!("CARGO_PKG_VERSION").into());
        let patches: Vec<PatchRow> =
            vm::about::patch_rows(&self.patches, &self.target, &self.selection, &self.texts)
                .into_iter()
                .map(|row| PatchRow {
                    name: row.name.into(),
                    author: row.author.into(),
                    status: row.status.into(),
                    available: row.available,
                })
                .collect();
        window.set_patches(ModelRc::new(VecModel::from(patches)));
        window.set_shipped_licences(licence_model(vm::about::SHIPPED, &self.texts));
        window.set_learned_licences(licence_model(vm::about::LEARNED_FROM, &self.texts));
        window.set_status(self.status.as_str().into());
        window.set_crash_note(self.note.as_str().into());
        if let Some(crash) = &self.crash {
            let texts = crash.screen(self.crash_lang());
            window.set_crash_heading(texts.heading.into());
            window.set_crash_game(texts.game.into());
            window.set_crash_chapter(texts.chapter.into());
            window.set_crash_summary(texts.summary.into());
            window.set_crash_detail(texts.detail.into());
            let labels: Vec<SharedString> = texts.buttons.into_iter().map(Into::into).collect();
            window.set_crash_buttons(ModelRc::new(VecModel::from(labels)));
        }
    }

    /// Carry out what an input means.
    fn apply(&mut self, effect: Effect, window: &AppWindow) {
        match effect {
            Effect::None => {}
            Effect::Focus(index) => window.set_focused_index(index as i32),
            Effect::Show(screen) => {
                self.note.clear();
                window.set_page(screen as i32);
                window.set_focused_index(self.launcher.focused() as i32);
            }
            Effect::Scroll(steps) => window.invoke_scroll_about(steps),
            Effect::NextLanguage => {
                self.next_language();
                self.show(window);
            }
            Effect::Press(index) => self.press(window, index),
        }
    }

    /// The after-run step for `game`: the after-crash screen when it crashed, else Home.
    fn run_ended(&mut self, window: &AppWindow, game: Game) {
        self.crash = self.folders.as_ref().and_then(|folders| {
            after_crash::after_run(&folders.context(), game, &folders.saves(game), now())
        });
        self.note.clear();
        match &self.crash {
            Some(crash) => self.launcher.show_crash(crash.buttons().len()),
            None => self.launcher.go(Screen::Home),
        }
        self.show(window);
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
            Err(error) => eprintln!("{}", fill(Text::StartFailed.get(self.crash_lang()), &error)),
        }
    }

    fn running(&mut self, window: &AppWindow, game: Game) {
        self.status = fill(Text::Running.get(self.crash_lang()), game.name());
        self.launcher.go(Screen::Running);
        self.show(window);
    }

    /// After-crash button `index`.
    fn press(&mut self, window: &AppWindow, index: usize) {
        let (Some(crash), Some(folders)) = (self.crash.clone(), &self.folders) else {
            return;
        };
        let Some(&button) = crash.buttons().get(index) else {
            return;
        };
        let ctx = folders.context();
        let lang = self.crash_lang();
        let mut host = AppHost {
            window,
            folders,
            run_args: &self.run_args,
        };
        match crash.press(button, &ctx, &mut host, lang) {
            Pressed::Stay(note) => {
                self.note = note;
                window.set_crash_note(self.note.as_str().into());
            }
            Pressed::Playing => {
                self.note.clear();
                self.running(window, crash.game);
            }
            Pressed::Home => {
                self.note.clear();
                self.launcher.go(Screen::Home);
                self.show(window);
            }
        }
    }
}

/// The game `--play` starts at once: none when the pickup of an unfinished run put the
/// after-crash screen up, since starting would replace that screen before it is drawn. The
/// screen's Play again button starts the game instead.
fn start_now(play: Option<Game>, screen: Screen) -> Option<Game> {
    play.filter(|_| screen != Screen::AfterCrash)
}

fn licence_model(list: &[vm::about::Licence], texts: &Texts) -> ModelRc<LicenceRow> {
    let rows: Vec<LicenceRow> = vm::about::licence_rows(list, texts)
        .into_iter()
        .map(|row| LicenceRow {
            name: row.name.into(),
            licence: row.licence.into(),
            role: row.role.into(),
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

/// Wire the window's callbacks to `app`.
fn connect(window: &AppWindow, app: &Rc<RefCell<App>>) {
    let input = |app: &Rc<RefCell<App>>, window: &AppWindow, f: &dyn Fn(&mut App) -> Effect| {
        let mut app = app.borrow_mut();
        let effect = f(&mut app);
        app.apply(effect, window);
    };
    let (weak, state) = (window.as_weak(), app.clone());
    window.on_nav(move |code| {
        if let (Some(nav), Some(window)) = (Nav::from_code(code), weak.upgrade()) {
            input(&state, &window, &|app| app.launcher.nav(nav));
        }
    });
    let (weak, state) = (window.as_weak(), app.clone());
    window.on_clicked(move |index| {
        if let Some(window) = weak.upgrade() {
            let index = usize::try_from(index).unwrap_or(0);
            input(&state, &window, &|app| app.launcher.click(index));
        }
    });
    let (weak, state) = (window.as_weak(), app.clone());
    window.on_activate(move || {
        if let Some(window) = weak.upgrade() {
            input(&state, &window, &|app| app.launcher.activate());
        }
    });
    let (weak, state) = (window.as_weak(), app.clone());
    window.on_back(move || {
        if let Some(window) = weak.upgrade() {
            input(&state, &window, &|app| app.launcher.back());
        }
    });
    let (weak, state) = (window.as_weak(), app.clone());
    window.on_run_ended(move |key| {
        if let (Some(window), Some(game)) = (weak.upgrade(), Game::from_key(&key)) {
            state.borrow_mut().run_ended(&window, game);
        }
    });
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

    let folders = Folders::find();
    let settings_file = folders.as_ref().map(Folders::settings);
    let setting = settings_file
        .as_deref()
        .map(language::read_setting)
        .unwrap_or_default();
    let mut state = App::new(setting, settings_file, sys_locale::get_locale());
    state.folders = folders;
    state.run_args = args.run_args;
    let app = Rc::new(RefCell::new(state));

    let window = AppWindow::new()?;
    app.borrow().show(&window);
    connect(&window, &app);

    {
        let mut app = app.borrow_mut();
        // A run the app started but did not see end (closed, or left by Game Mode).
        let unfinished = app
            .folders
            .as_ref()
            .and_then(|folders| after_crash::unfinished(&folders.dirs.logs()));
        if let Some(game) = unfinished {
            app.run_ended(&window, game);
        }
        if let Some(game) = start_now(args.play, app.launcher.screen()) {
            app.start(&window, game);
        }
    }

    // Poll the controller on the UI thread; gilrs events are cheap to drain.
    let mut controller = Controller::new();
    let timer = slint::Timer::default();
    let weak = window.as_weak();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(16),
        move || {
            let (Some(controller), Some(window)) = (controller.as_mut(), weak.upgrade()) else {
                return;
            };
            for action in controller.poll() {
                match action {
                    Action::Move(nav) => window.invoke_nav(nav_code(nav)),
                    Action::Activate => window.invoke_activate(),
                    Action::Back => window.invoke_back(),
                }
            }
        },
    );

    window.run()
}

/// The `nav` callback's integer for `nav` (the inverse of `Nav::from_code`).
fn nav_code(nav: Nav) -> i32 {
    match nav {
        Nav::Up => 0,
        Nav::Down => 1,
        Nav::Left => 2,
        Nav::Right => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use i_slint_backend_testing::ElementHandle;
    use slint::Model;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn window_with(app: &Rc<RefCell<App>>) -> AppWindow {
        i_slint_backend_testing::init_no_event_loop();
        let window = AppWindow::new().unwrap();
        app.borrow().show(&window);
        connect(&window, app);
        window
    }

    fn shows(window: &AppWindow, label: &str) -> bool {
        !ElementHandle::find_by_accessible_label(window, label)
            .collect::<Vec<_>>()
            .is_empty()
    }

    #[test]
    fn nav_codes_round_trip() {
        for nav in [Nav::Up, Nav::Down, Nav::Left, Nav::Right] {
            assert_eq!(Nav::from_code(nav_code(nav)), Some(nav));
        }
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

    #[test]
    fn play_waits_for_the_after_crash_screen() {
        // No pickup screen: --play starts its game at once.
        assert_eq!(start_now(Some(Game::U2), Screen::Home), Some(Game::U2));
        // The pickup of an unfinished run showed the after-crash screen: keep it up.
        assert_eq!(start_now(Some(Game::U2), Screen::AfterCrash), None);
        assert_eq!(start_now(Some(Game::U1), Screen::AfterCrash), None);
        // Without --play nothing starts.
        assert_eq!(start_now(None, Screen::Home), None);
        assert_eq!(start_now(None, Screen::AfterCrash), None);
    }

    // One test builds the window: the testing backend is per thread and Slint's translation
    // choice is global to it.
    #[test]
    fn about_and_the_language_button_in_the_window() {
        let dir = std::env::temp_dir().join(format!("uncharted-app-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("settings.json");
        let app = Rc::new(RefCell::new(App::new(
            String::new(),
            Some(file.clone()),
            Some("ko_KR".into()),
        )));
        let window = window_with(&app);

        // Automatic with an unsupported locale is English.
        assert_eq!(window.get_language_name(), "Automatic (English)");
        assert!(shows(&window, "About"));
        assert!(shows(&window, "Continue screen goes here"));
        // The Slint notice is on About only, and the licences are filled in.
        assert_eq!(window.get_page(), 0);
        assert!(!shows(&window, "Built with Slint"));
        assert_eq!(
            window.get_shipped_licences().row_count(),
            vm::about::SHIPPED.len()
        );

        // Right, A: About. Down scrolls, B goes back with the focus on About.
        window.invoke_nav(nav_code(Nav::Right));
        assert_eq!(window.get_focused_index(), 1);
        window.invoke_activate();
        assert_eq!(window.get_page(), Screen::About as i32);
        // Only what is on screen is found: the top of About, not yet its end.
        assert!(shows(&window, "Patches"));
        assert!(!shows(&window, "Built with Slint"));
        // Down scrolls to the end (and stops there); the Slint notice is at the bottom.
        for _ in 0..40 {
            window.invoke_nav(nav_code(Nav::Down));
        }
        assert!(shows(&window, "Built with Slint"));
        assert!(!shows(&window, "Patches"));
        window.invoke_nav(nav_code(Nav::Up));
        assert!(shows(&window, "Back"), "the Back button stays on screen");
        window.invoke_back();
        assert_eq!(window.get_page(), Screen::Home as i32);
        assert_eq!(window.get_focused_index(), 1);

        // The language button steps English -> Spanish; Slint's own texts follow, and the
        // choice is saved.
        window.invoke_clicked(0);
        assert_eq!(window.get_language_name(), "English");
        assert_eq!(language::read_setting(&file), "en");
        window.invoke_activate();
        assert_eq!(window.get_language_name(), "Español");
        assert_eq!(language::read_setting(&file), "es");
        assert!(shows(&window, "Acerca de"), "Slint texts are in Spanish");
        assert!(!shows(&window, "About"));
        let roles = window.get_shipped_licences();
        assert_eq!(
            roles.row_data(0).unwrap().role,
            "Motor gráfico (nuestro fork, en gpu/)"
        );

        // The after-crash screen speaks the chosen language, the d-pad moves its focus, and
        // B goes back Home.
        {
            let mut state = app.borrow_mut();
            state.crash = Some(AfterCrash {
                game: Game::U2,
                chapter: None,
                crash: "boom".into(),
                known: None,
            });
            state.launcher.show_crash(3);
            state.show(&window);
        }
        let heading = Text::Heading.get(Lang::Es);
        assert_eq!(window.get_page(), Screen::AfterCrash as i32);
        assert_eq!(window.get_crash_heading(), heading);
        assert!(shows(&window, heading));
        assert!(!shows(&window, "Continue screen goes here"));
        assert_eq!(window.get_crash_buttons().row_count(), 3);
        window.invoke_nav(nav_code(Nav::Down));
        assert_eq!(window.get_focused_index(), 1);
        window.invoke_back();
        assert_eq!(window.get_page(), Screen::Home as i32);
        assert!(!shows(&window, heading));

        // Every language switches without an error.
        for _ in 0..6 {
            window.invoke_activate();
        }
        assert_eq!(language::read_setting(&file), "");
        assert_eq!(window.get_language_name(), "Automatic (English)");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
