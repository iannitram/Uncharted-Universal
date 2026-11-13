//! The launcher (ADR 0003): the Continue screen (docs/continue-screen.md, roadmap 2.04). One
//! card per game from `core::savecard`, Continue and Play, Other saves, the sync-conflict
//! prompt and the fleet warning; keyboard, mouse and controller move the same focus. Its Saves
//! and Settings buttons open those pages (roadmap 2.06, `src/vm/`).
//!
//! The view models are `screen.rs` and `vm/` (tested without a window); this file draws their
//! views into the Slint window and carries out their commands, the files through `pages_io.rs`
//! and `settings_store.rs`. Continue and Play run
//! `uncharted-universal play <game>` (the command line, roadmap 2.02) beside this program and
//! wait for it: that command pulls the save, starts the game and pushes the save.
//!
//! Release builds (roadmap 2.10) ship this program as the AppImage's entry, in the Windows zip
//! and in the macOS app bundle, with `uncharted-universal` and `uncharted-run` beside it. On
//! Linux it first takes Steam's `LD_PRELOAD` overlay and the other host library variables out
//! of its own environment (`core::host_env`, which replaced `packaging/bbport-entry.c`), so
//! the programs it starts never load them. On Windows a release build is a window program (no
//! console window of its own) and starts the command line without one either.
//!
//! The guided first run (roadmap 2.05) opens over the screen when nothing is installed and no
//! hub is paired (`setup::is_first_run`), or from a card's Set up button: its view model is
//! `firstrun.rs`, its jobs run on threads in `work.rs`.
//!
//! When a game run ends, the after-run step (roadmap 2.07, `vm::after_crash`) reads its log and
//! shows the after-crash screen for a crash; a run the app did not see end is picked up on the
//! next start. `uncharted-app --play <u1|u2|u3> [-- <runtime arguments>]` starts that game at
//! once (unless that pickup put the after-crash screen up; its Play again starts the game).

// A release build on Windows opens no console window; debug builds and tests keep theirs.
#![cfg_attr(all(not(debug_assertions), not(test)), windows_subsystem = "windows")]

mod card;
mod firstrun;
mod focus;
mod input;
mod load;
mod pages_io;
mod play;
mod screen;
mod settings_file;
mod settings_store;
mod strings;
mod vm;
mod work;

use card::Clock;
use firstrun::{FirstRun, FirstRunView, Job, Key};
use focus::Nav;
use input::Controller;
use screen::{Command, GameData, Input, OverlayView, Page, Screen, Start, View};
use slint::{ComponentHandle, Image, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use strings::Lang;
use uncharted_core::games::Game;
use uncharted_core::launch::Keep;
use uncharted_core::logs::{self, Redact, ReportDetails};
use uncharted_core::platform::{AppDirs, Os, Vars};
use uncharted_core::saves;
use uncharted_core::setup;
use vm::after_crash::{self, AfterCrash, Host, Pressed};
use vm::saves::{GameSaves, SavesCommand, SavesView};
use vm::settings::{SettingsCommand, SettingsView};
use vm::{SavesVm, SettingsVm};

slint::include_modules!();

/// The status line until save-relay is linked (roadmap 9.01).
const NO_RELAY: &str = "Saves stay on this device: save-relay is not in this build yet";
/// `known_issues.json`, built in.
const KNOWN_ISSUES: &str = include_str!("../../crates/core/data/known_issues.json");

/// Decoded pictures, by the hash of their PNG bytes, so a key press does not decode again.
#[derive(Default)]
struct Pictures(HashMap<u64, Option<Image>>);

impl Pictures {
    fn get(&mut self, png: Option<&[u8]>) -> Option<Image> {
        let png = png?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        png.hash(&mut hasher);
        self.0
            .entry(hasher.finish())
            .or_insert_with(|| Image::load_from_data(png, Some("png")).ok())
            .clone()
    }
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

fn strings(items: &[String]) -> ModelRc<SharedString> {
    model(items.iter().map(SharedString::from).collect())
}

/// Draw `view` into the window.
fn apply(window: &AppWindow, view: &View, pictures: &mut Pictures) {
    window.set_games(model(view.games.iter().map(|g| (*g).into()).collect()));
    window.set_current_game(view.current as i32);
    let picture = pictures.get(view.card.picture.as_deref());
    window.set_card(CardData {
        title: view.card.title.as_str().into(),
        lines: strings(&view.card.lines),
        has_picture: picture.is_some(),
        picture: picture.unwrap_or_default(),
        picture_label: view.card.picture_label.as_str().into(),
    });
    window.set_actions(model(
        view.actions
            .iter()
            .zip(&view.action_hints)
            .map(|(label, hint)| ButtonData {
                label: label.as_str().into(),
                hint: (*hint).into(),
            })
            .collect(),
    ));
    window.set_pages(model(view.pages.iter().map(|p| (*p).into()).collect()));
    window.set_focus_row(view.focus.row.code());
    window.set_focus_index(view.focus.index as i32);
    window.set_status(view.status.as_str().into());
    window.set_overlay(view.overlay.code());
    match &view.overlay {
        OverlayView::None | OverlayView::Quit => {}
        OverlayView::Warning(text) | OverlayView::Running(text) => {
            window.set_overlay_text(text.as_str().into());
        }
        OverlayView::OtherSaves { title, rows, index } => {
            window.set_overlay_title(title.as_str().into());
            window.set_row_index(*index as i32);
            window.set_rows(model(
                rows.iter()
                    .map(|row| {
                        let picture = pictures.get(row.picture.as_deref());
                        RowData {
                            has_picture: picture.is_some(),
                            picture: picture.unwrap_or_default(),
                            title: row.title.as_str().into(),
                            detail: row.detail.as_str().into(),
                            when: row.when.as_str().into(),
                            device: row.device.as_str().into(),
                            slot: row.slot.as_str().into(),
                            note: row.note.as_str().into(),
                        }
                    })
                    .collect(),
            ));
        }
        OverlayView::Conflict {
            title,
            sides,
            server,
        } => {
            window.set_overlay_title(title.as_str().into());
            window.set_side_index(i32::from(*server));
            window.set_sides(model(
                sides
                    .iter()
                    .map(|side| {
                        let picture = pictures.get(side.picture.as_deref());
                        SideData {
                            heading: side.heading.as_str().into(),
                            newer: side.newer,
                            has_picture: picture.is_some(),
                            picture: picture.unwrap_or_default(),
                            title: side.title.as_str().into(),
                            lines: strings(&side.lines),
                        }
                    })
                    .collect(),
            ));
        }
    }
}

/// Draw the first-run page into the window.
fn apply_first(window: &AppWindow, view: &FirstRunView) {
    window.set_fr_step(view.step.as_str().into());
    window.set_fr_title(view.title.as_str().into());
    window.set_fr_lines(strings(&view.lines));
    window.set_fr_options(strings(&view.options));
    window.set_fr_focus(view.focus as i32);
    window.set_fr_field(view.field.as_str().into());
    window.set_fr_code(strings(&view.code));
    window.set_fr_code_cursor(view.code_cursor as i32);
    window.set_fr_has_progress(view.progress.is_some());
    window.set_fr_progress(view.progress.unwrap_or(0.0));
    window.set_fr_error(view.error.as_str().into());
    window.set_fr_hint(view.hint.as_str().into());
}

/// This device's clock and time zone.
fn local_clock_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn local_offset(unix: u64) -> i64 {
    use chrono::{Local, Offset, TimeZone};
    i64::try_from(unix)
        .ok()
        .and_then(|secs| Local.timestamp_opt(secs, 0).single())
        .map_or(0, |at| i64::from(at.offset().fix().local_minus_utc()))
}

/// Draw the Saves page.
fn apply_saves(window: &AppWindow, view: &SavesView) {
    window.set_page_games(model(view.games.iter().map(|g| (*g).into()).collect()));
    window.set_page_game(view.current as i32);
    let rows = |rows: &[vm::saves::SavesRow]| {
        model(
            rows.iter()
                .map(|row| SlotRowData {
                    title: row.title.as_str().into(),
                    detail: row.detail.as_str().into(),
                    mark: row.mark.as_str().into(),
                    buttons: strings(&row.buttons),
                })
                .collect(),
        )
    };
    window.set_save_slots(rows(&view.slots));
    window.set_save_backups(rows(&view.backups));
    window.set_saves_relay(view.relay.as_str().into());
    window.set_saves_confirm(view.confirm.as_deref().unwrap_or("").into());
    window.set_page_row(view.focus.row as i32);
    window.set_page_col(view.focus.col as i32);
    window.set_page_status(view.status.as_str().into());
}

/// Draw the Settings page.
fn apply_settings(window: &AppWindow, view: &SettingsView) {
    window.set_tune_cards(model(
        view.cards
            .iter()
            .map(|card| TuneCardData {
                title: card.title.as_str().into(),
                lines: strings(&card.lines),
                in_use: card.in_use,
            })
            .collect(),
    ));
    window.set_choices(model(
        view.choices
            .iter()
            .map(|choice| ChoiceData {
                label: choice.label.as_str().into(),
                value: choice.value.as_str().into(),
                can_prev: choice.can_prev,
                can_next: choice.can_next,
            })
            .collect(),
    ));
    window.set_setting_notes(strings(&view.notes));
    window.set_tuning(view.tuning);
    window.set_page_row(view.focus.row as i32);
    window.set_page_col(view.focus.col as i32);
    window.set_page_status(view.status.as_str().into());
}

/// Draw the after-crash screen.
fn apply_crash(window: &AppWindow, page: &CrashPage, lang: Lang) {
    let texts = page.screen.screen(lang);
    window.set_crash_heading(texts.heading.into());
    window.set_crash_game(texts.game.into());
    window.set_crash_chapter(texts.chapter.into());
    window.set_crash_summary(texts.summary.into());
    window.set_crash_detail(texts.detail.into());
    window.set_crash_buttons(strings(&texts.buttons));
    window.set_crash_note(page.note.as_str().into());
    window.set_page_row(0);
    window.set_page_col(page.focus as i32);
}

/// The after-crash screen over the Continue screen: its buttons in one row.
struct CrashPage {
    screen: AfterCrash,
    focus: usize,
    /// What the last button said (the log was copied, a setting could not be saved).
    note: String,
}

impl CrashPage {
    fn new(screen: AfterCrash) -> CrashPage {
        CrashPage {
            screen,
            focus: 0,
            note: String::new(),
        }
    }

    /// Up / Left and Down / Right (and Tab) move along the buttons and stop at the ends; A
    /// presses the focused one, B goes back to the Continue screen.
    fn handle(&mut self, input: Input) -> Cmd {
        let last = self.screen.buttons().len().saturating_sub(1);
        match input {
            Input::Move(Nav::Up | Nav::Left) | Input::Tab { back: true } => {
                self.focus = self.focus.saturating_sub(1);
            }
            Input::Move(Nav::Down | Nav::Right) | Input::Tab { back: false } => {
                self.focus = (self.focus + 1).min(last);
            }
            Input::Activate => return Cmd::Crash(Some(self.focus)),
            Input::Back => return Cmd::Crash(None),
            Input::PrevGame | Input::NextGame | Input::OtherSaves => {}
        }
        Cmd::Screen(Command::None)
    }
}

/// What the window shows over the Continue screen.
enum Shown {
    Continue,
    Saves(SavesVm),
    Settings(SettingsVm),
    AfterCrash(CrashPage),
}

impl Shown {
    /// `page` of the window: 0 the Continue screen, 1 Saves, 2 Settings, 3 after a crash.
    fn code(&self) -> i32 {
        match self {
            Shown::Continue => 0,
            Shown::Saves(_) => 1,
            Shown::Settings(_) => 2,
            Shown::AfterCrash(_) => 3,
        }
    }
}

/// A command of the screen or of a page, for `main` to carry out.
#[derive(Debug, Clone, PartialEq)]
enum Cmd {
    Screen(Command),
    Saves(SavesCommand),
    Settings(SettingsCommand),
    /// A button of the after-crash screen; `None` is B (back to the Continue screen).
    Crash(Option<usize>),
}

/// The screen, the page over it and the window, redrawn after every change.
struct Ui {
    screen: RefCell<Screen>,
    shown: RefCell<Shown>,
    window: slint::Weak<AppWindow>,
    pictures: RefCell<Pictures>,
    now: Box<dyn Fn() -> u64>,
    offset: Box<dyn Fn(u64) -> i64>,
    /// The first-run page while it is open.
    first: RefCell<Option<FirstRun>>,
    /// The language of the after-crash screen.
    lang: Lang,
}

impl Ui {
    fn redraw(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let clock = Clock {
            now: (self.now)(),
            offset: &*self.offset,
        };
        let view = self.screen.borrow().view(&clock);
        apply(&window, &view, &mut self.pictures.borrow_mut());
        match self.first.borrow().as_ref() {
            Some(first) => {
                apply_first(&window, &first.view());
                window.set_mode(1);
            }
            None => window.set_mode(0),
        }
        let shown = self.shown.borrow();
        match &*shown {
            Shown::Continue => {}
            Shown::Saves(vm) => apply_saves(&window, &vm.view(&clock)),
            Shown::Settings(vm) => apply_settings(&window, &vm.view()),
            Shown::AfterCrash(page) => apply_crash(&window, page, self.lang),
        }
        window.set_page(shown.code());
    }

    /// Open the first-run page; `only` is the game whose Set up button opened it.
    fn open_first(&self, only: Option<Game>, steam: bool) {
        *self.first.borrow_mut() = Some(FirstRun::new(only, steam));
        if let Some(window) = self.window.upgrade() {
            window.set_fr_field_text(SharedString::new());
        }
        self.redraw();
    }

    /// Do `f` to the first-run page, if open, and redraw.
    fn first_do(&self, f: impl FnOnce(&mut FirstRun) -> Job) -> Job {
        let job = match self.first.borrow_mut().as_mut() {
            Some(first) => f(first),
            None => Job::None,
        };
        self.redraw();
        job
    }

    /// Open a page over the Continue screen.
    fn show(&self, shown: Shown) {
        *self.shown.borrow_mut() = shown;
        self.redraw();
    }

    /// Back to the Continue screen, focus on the button of the page left.
    fn close(&self, page: Page) {
        *self.shown.borrow_mut() = Shown::Continue;
        self.screen.borrow_mut().focus_page(page);
    }

    fn input(&self, input: Input) -> Cmd {
        let command = match &mut *self.shown.borrow_mut() {
            Shown::Continue => Cmd::Screen(self.screen.borrow_mut().handle(input)),
            Shown::Saves(vm) => Cmd::Saves(vm.handle(input)),
            Shown::Settings(vm) => Cmd::Settings(vm.handle(input)),
            Shown::AfterCrash(page) => page.handle(input),
        };
        self.after(&command);
        command
    }

    fn click(&self, area: i32, index: i32) -> Cmd {
        let Ok(index) = usize::try_from(index) else {
            return Cmd::Screen(Command::None);
        };
        let command = Cmd::Screen(self.screen.borrow_mut().click(area, index));
        self.redraw();
        command
    }

    /// A click on a page: `row` and `col` as the page's view model takes them; `row` -3 is the
    /// page's Back button.
    fn page_click(&self, row: i32, col: i32) -> Cmd {
        let Ok(col) = usize::try_from(col) else {
            return Cmd::Screen(Command::None);
        };
        if row == -3 {
            return self.input(Input::Back);
        }
        let command = match &mut *self.shown.borrow_mut() {
            Shown::Continue => Cmd::Screen(Command::None),
            Shown::Saves(vm) => Cmd::Saves(vm.click(row, col)),
            Shown::Settings(vm) => match usize::try_from(row) {
                Ok(row) => Cmd::Settings(vm.click(row, col)),
                Err(_) => Cmd::Settings(SettingsCommand::None),
            },
            Shown::AfterCrash(_) => Cmd::Crash(Some(col)),
        };
        self.after(&command);
        command
    }

    /// Back closes the page; then redraw.
    fn after(&self, command: &Cmd) {
        match command {
            Cmd::Saves(SavesCommand::Back) => self.close(Page::Saves),
            Cmd::Settings(SettingsCommand::Back) => self.close(Page::Settings),
            Cmd::Crash(None) => *self.shown.borrow_mut() = Shown::Continue,
            _ => {}
        }
        self.redraw();
    }
}

/// Connect the window's callbacks to the screen, the pages and the first-run page; every
/// command goes to `run`, every first-run job to `jobs`.
fn wire(window: &AppWindow, ui: &Rc<Ui>, run: Rc<dyn Fn(Cmd)>, jobs: Rc<dyn Fn(Job)>) {
    window.on_input({
        let (ui, run, jobs) = (ui.clone(), run.clone(), jobs.clone());
        move |code| {
            if ui.first.borrow().is_some() {
                if let Some(key) = Key::from_code(code) {
                    jobs(ui.first_do(|first| first.handle(key)));
                }
            } else if let Some(input) = Input::from_code(code) {
                run(ui.input(input));
            }
        }
    });
    window.on_clicked({
        let (ui, run) = (ui.clone(), run.clone());
        move |area, index| run(ui.click(area, index))
    });
    window.on_fr_clicked({
        let (ui, jobs) = (ui.clone(), jobs.clone());
        move |index| {
            if let Ok(index) = usize::try_from(index) {
                jobs(ui.first_do(|first| first.click(index)));
            }
        }
    });
    window.on_field_accepted({
        let ui = ui.clone();
        move |text| jobs(ui.first_do(|first| first.field(&text)))
    });
    window.on_page_clicked({
        let ui = ui.clone();
        move |row, col| run(ui.page_click(row, col))
    });
    ui.redraw();
}

/// Where one game's files are on this device.
struct Place {
    game: Game,
    install: PathBuf,
    user_dir: PathBuf,
    dump: Option<PathBuf>,
}

fn places(os: Os, vars: &Vars) -> Vec<Place> {
    let dirs = AppDirs::new(vars);
    Game::ALL
        .into_iter()
        .filter_map(|game| {
            let install = dirs.install(game.key());
            let user_dir = saves::user_dir(os, vars, &install).ok()?;
            Some(Place {
                game,
                dump: load::dump_dir(game, |k| std::env::var(k).ok(), Some(&dirs)),
                install,
                user_dir,
            })
        })
        .collect()
}

fn load_all(places: &[Place]) -> Vec<GameData> {
    if places.is_empty() {
        return Game::ALL
            .into_iter()
            .map(|game| GameData {
                set_up: false,
                ..GameData::empty(game)
            })
            .collect();
    }
    places
        .iter()
        .map(|p| {
            let mut data = load::game_data(p.game, &p.install, &p.user_dir, p.dump.as_deref());
            // This device's own record and the fleet's records come with the fleet (1.23,
            // 4.04); until then no card names a build, so there is nothing to compare.
            data.warning = data
                .saves
                .first()
                .and_then(|s| load::warning_for(None, &[], &s.card));
            data
        })
        .collect()
}

/// `uncharted-universal` beside this program.
fn cli_path() -> PathBuf {
    let file = format!("uncharted-universal{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&file)))
        .unwrap_or_else(|| PathBuf::from(file))
}

/// Windows: start the command line (a console program) without opening a console window over
/// the launcher (`CREATE_NO_WINDOW`). Nothing to do elsewhere.
fn no_console_window(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// The arguments of `uncharted-universal play` for `start`, then `--` and the runtime's own
/// arguments (`--play ... -- <args>`) when there are any.
fn play_args(start: &Start, run_args: &[String]) -> Vec<String> {
    let mut args = vec!["play".to_owned(), start.game.key().to_owned()];
    if let Some(keep) = start.keep {
        args.push("--keep".into());
        args.push(match keep {
            Keep::Server => "server".into(),
            Keep::Local => "local".into(),
        });
    }
    if !run_args.is_empty() {
        args.push("--".into());
        args.extend(run_args.iter().cloned());
    }
    args
}

/// The status line after a run of the play command; `log` is what it printed.
fn run_status(game: Game, result: std::io::Result<std::process::ExitStatus>, log: &str) -> String {
    match result {
        Err(err) => format!("Could not start {}: {err}", cli_path().display()),
        Ok(status) if status.success() => format!("{} closed.", game.name()),
        Ok(status) => log
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "{} stopped with exit status {}.",
                    game.name(),
                    status.code().unwrap_or(-1)
                )
            }),
    }
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

/// The game `--play` starts at once: none when the pickup of an unfinished run put the
/// after-crash screen up, since starting would replace that screen before it is seen. The
/// screen's Play again button starts the game instead.
fn start_now(play: Option<Game>, crash_shown: bool) -> Option<Game> {
    play.filter(|_| !crash_shown)
}

/// What the after-run step reads on this device: the logs, the settings, the known issues.
fn crash_context(cx: &work::Context) -> after_crash::Context {
    let settings = settings_file::read(&cx.dirs.config.join(settings_file::SETTINGS_FILE));
    let preset = settings
        .get("preset")
        .and_then(|value| value.as_str())
        .unwrap_or("default")
        .to_string();
    let launcher = std::env::current_exe()
        .map(|exe| exe.display().to_string())
        .unwrap_or_else(|_| "?".into());
    after_crash::Context {
        logs: cx.dirs.logs(),
        build: env!("CARGO_PKG_VERSION").to_string(),
        preset,
        issues: logs::parse_known_issues(KNOWN_ISSUES).unwrap_or_default(),
        details: ReportDetails {
            version: env!("CARGO_PKG_VERSION").to_string(),
            at_unix: local_clock_now(),
            system: vec![format!("Launcher: {launcher}")],
            settings,
            // Not measured here: no low-space line in the report.
            free_bytes: u64::MAX,
        },
        redact: Redact::for_home(&cx.vars.home, Vec::new()),
    }
}

/// The game's saves folder, where the runtime writes `where.json`.
fn save_root(cx: &work::Context, game: Game) -> PathBuf {
    saves::save_root(game, cx.os, &cx.vars, &cx.dirs.install(game.key()))
        .unwrap_or_else(|_| cx.dirs.data.join("user").join(game.name()))
}

/// The after-run step for `game`: the after-crash screen when the run crashed.
fn after_run(cx: &work::Context, game: Game) -> Option<CrashPage> {
    after_crash::after_run(
        &crash_context(cx),
        game,
        &save_root(cx, game),
        local_clock_now(),
    )
    .map(CrashPage::new)
}

/// The after-crash screen's buttons on this device. Play again is noted here and started by
/// `main` once the press returns `Pressed::Playing`, through the same path as Continue.
struct AppHost {
    window: slint::Weak<AppWindow>,
    settings: PathBuf,
}

impl Host for AppHost {
    fn set_setting(&mut self, key: &str, value: &str) -> Result<(), String> {
        settings_file::set_extra(&self.settings, key, value).map_err(|e| e.to_string())
    }

    fn play(&mut self, _game: Game) -> Result<(), String> {
        Ok(())
    }

    fn open_url(&mut self, url: &str) {
        if let Err(error) = play::open_url(url) {
            eprintln!("could not open {url}: {error}");
        }
    }

    fn copy_text(&mut self, text: &str) {
        if let Some(window) = self.window.upgrade() {
            window.invoke_copy_text(text.into());
        }
    }
}

/// The status of a page not in this build yet: Fleet and About (Saves and Settings open their
/// pages, roadmap 2.06).
fn page_status(page: Page) -> String {
    let piece = match page {
        Page::Saves | Page::Settings => "2.06",
        Page::Fleet => "2.08",
        Page::About => "2.09",
    };
    format!("{} comes in a later build (roadmap {piece}).", page.label())
}

/// What the Saves page shows of every game: from disk where the game has a place, else empty.
fn saves_data(places: &[Place], backups: &std::path::Path) -> Vec<GameSaves> {
    Game::ALL
        .into_iter()
        .map(|game| match places.iter().find(|p| p.game == game) {
            Some(place) => pages_io::game_saves(game, &place.user_dir, backups),
            None => GameSaves::empty(game),
        })
        .collect()
}

/// The save folders save-relay syncs, where the runtime's benchmark reports are.
fn relay_dirs(places: &[Place]) -> Vec<PathBuf> {
    places
        .iter()
        .map(|p| saves::relay_saves_dir(&p.user_dir, p.game))
        .collect()
}

fn keep_status(game: Game, keep: Keep) -> String {
    let side = match keep {
        Keep::Server => "the synced",
        Keep::Local => "this device's",
    };
    format!("Keeping {side} save of {}.", game.name())
}

fn main() -> Result<(), slint::PlatformError> {
    // First, before any thread (Slint's and gilrs's included): the host's library injection
    // out of this process, so `uncharted-universal play` and the game never inherit it.
    uncharted_core::host_env::clean_process();
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
    let os = Os::current();
    let vars = Vars::from_lookup(os, |k| std::env::var(k).ok());
    let (places, status, dirs) = match &vars {
        Ok(vars) => (
            places(os, vars),
            NO_RELAY.to_owned(),
            Some(AppDirs::new(vars)),
        ),
        Err(err) => (Vec::new(), format!("Cannot find the saves: {err}"), None),
    };
    // Where the first run works; without the folders there is nowhere to set up.
    let cx = vars.ok().map(|vars| work::Context::here(os, vars));
    let steam = setup::steam_command(os, false).is_some();
    let places = Rc::new(RefCell::new(places));
    let config = dirs.as_ref().map(|d| d.config.clone());
    let backups = dirs.as_ref().map(AppDirs::backups);
    let detected = pages_io::detected_profile(os);
    let last = config.as_deref().and_then(load::last_started);
    let screen = Screen::new(load_all(&places.borrow()), last, &status);
    let ui = Rc::new(Ui {
        screen: RefCell::new(screen),
        shown: RefCell::new(Shown::Continue),
        window: window.as_weak(),
        pictures: RefCell::default(),
        now: Box::new(local_clock_now),
        offset: Box::new(local_offset),
        first: RefCell::new(None),
        lang: Lang::detect(|name| std::env::var(name).ok()),
    });

    // A finished game run comes back from its thread through this channel; the hardware probe
    // through the other.
    let (done_tx, done_rx) = mpsc::channel::<(Game, String)>();
    let (probe_tx, probe_rx) = mpsc::channel::<Result<String, String>>();
    // What the first run's jobs come to.
    let (event_tx, event_rx) = mpsc::channel::<firstrun::Event>();

    // Start a game through `uncharted-universal play` on a thread: Continue, Play, the
    // after-crash screen's Play again and `--play`. Its output goes to the run log, which the
    // after-run step reads.
    let start_game: Rc<dyn Fn(Start)> = Rc::new({
        let (ui, places, config, cx) = (ui.clone(), places.clone(), config.clone(), cx.clone());
        let run_args = args.run_args.clone();
        move |start: Start| {
            let places = places.borrow();
            let place = places.iter().find(|p| p.game == start.game);
            if let (Some(slot), Some(place)) = (start.slot, place) {
                if let Err(err) = saves::select(&place.user_dir, start.game, slot) {
                    ui.screen
                        .borrow_mut()
                        .set_status(&format!("Could not choose slot {slot}: {err}"));
                    ui.redraw();
                    return;
                }
            }
            if let Some(config) = &config {
                let _ = load::remember_started(config, start.game);
            }
            let env = config
                .as_deref()
                .map(|c| settings_file::extra(&c.join(settings_file::SETTINGS_FILE)))
                .unwrap_or_default();
            let logs = cx.as_ref().map(|cx| cx.dirs.logs());
            let game = start.game;
            let child = play::start(
                &cli_path(),
                &play_args(&start, &run_args),
                game,
                &env,
                logs.as_deref(),
            );
            let mut child = match child {
                Ok(child) => child,
                Err(err) => {
                    ui.screen
                        .borrow_mut()
                        .set_status(&run_status(game, Err(err), ""));
                    ui.redraw();
                    return;
                }
            };
            ui.screen
                .borrow_mut()
                .running(&format!("Starting {}...", game.name()));
            ui.redraw();
            let done = done_tx.clone();
            std::thread::spawn(move || {
                let result = child.wait();
                let log = logs
                    .map(|logs| std::fs::read_to_string(logs.join(logs::RUN_LOG)).unwrap_or_default())
                    .unwrap_or_default();
                let _ = done.send((game, run_status(game, result, &log)));
            });
        }
    });

    let run: Rc<dyn Fn(Cmd)> = Rc::new({
        let (ui, places, config, cx) = (ui.clone(), places.clone(), config.clone(), cx.clone());
        let start_game = start_game.clone();
        move |command| match command {
            Cmd::Screen(Command::None)
            | Cmd::Saves(SavesCommand::None | SavesCommand::Back)
            | Cmd::Settings(SettingsCommand::None | SettingsCommand::Back) => {}
            Cmd::Screen(Command::Quit) => {
                let _ = slint::quit_event_loop();
            }
            Cmd::Screen(Command::Open(Page::Saves)) => match &backups {
                Some(backups) if !places.borrow().is_empty() => {
                    let game = ui.screen.borrow().current_game();
                    let vm = SavesVm::new(saves_data(&places.borrow(), backups), game, NO_RELAY);
                    ui.show(Shown::Saves(vm));
                }
                _ => {
                    ui.screen
                        .borrow_mut()
                        .set_status("Cannot find the saves on this device.");
                    ui.redraw();
                }
            },
            Cmd::Screen(Command::Open(Page::Settings)) => {
                let stored = config
                    .as_deref()
                    .map(settings_store::load)
                    .unwrap_or_default();
                let tune = config
                    .as_deref()
                    .and_then(pages_io::load_cache)
                    .and_then(|probe| {
                        let dirs = relay_dirs(&places.borrow());
                        pages_io::tune_from(&probe, &pages_io::read_reports(&dirs)).ok()
                    });
                ui.show(Shown::Settings(SettingsVm::new(stored, detected, tune)));
            }
            Cmd::Screen(Command::Open(page)) => {
                ui.screen.borrow_mut().set_status(&page_status(page));
                ui.redraw();
            }
            Cmd::Screen(Command::SetUp(game)) => {
                if cx.is_some() {
                    ui.open_first(Some(game), steam);
                } else {
                    ui.screen
                        .borrow_mut()
                        .set_status("Cannot set up: this device's folders are unknown.");
                    ui.redraw();
                }
            }
            Cmd::Screen(Command::Keep { game, keep }) => {
                ui.screen.borrow_mut().set_status(&keep_status(game, keep));
                ui.redraw();
            }
            Cmd::Screen(Command::Start(start)) => start_game(start),
            Cmd::Crash(None) => {}
            Cmd::Crash(Some(index)) => {
                let (Some(cx), Some(screen)) = (&cx, crash_screen(&ui)) else {
                    return;
                };
                let Some(&button) = screen.buttons().get(index) else {
                    return;
                };
                let mut host = AppHost {
                    window: ui.window.clone(),
                    settings: cx.dirs.config.join(settings_file::SETTINGS_FILE),
                };
                match screen.press(button, &crash_context(cx), &mut host, ui.lang) {
                    Pressed::Stay(note) => {
                        if let Shown::AfterCrash(page) = &mut *ui.shown.borrow_mut() {
                            page.note = note;
                        }
                        ui.redraw();
                    }
                    Pressed::Playing => {
                        *ui.shown.borrow_mut() = Shown::Continue;
                        start_game(Start {
                            game: screen.game,
                            slot: None,
                            keep: None,
                        });
                    }
                    Pressed::Home => {
                        *ui.shown.borrow_mut() = Shown::Continue;
                        ui.redraw();
                    }
                }
            }
            Cmd::Saves(command) => {
                let Some(backups) = &backups else {
                    return;
                };
                {
                    let mut shown = ui.shown.borrow_mut();
                    let Shown::Saves(vm) = &mut *shown else {
                        return;
                    };
                    let game = vm.current_game();
                    let places = places.borrow();
                    let status = match places.iter().find(|p| p.game == game) {
                        Some(place) => {
                            pages_io::run(&command, &place.user_dir, backups, local_clock_now())
                                .unwrap_or_else(|err| format!("Could not do that: {err}"))
                        }
                        None => "Cannot find the saves of this game on this device.".to_owned(),
                    };
                    vm.reload(saves_data(&places, backups));
                    vm.set_status(&status);
                }
                // The Continue screen shows the active slot too.
                ui.screen.borrow_mut().reload(load_all(&places.borrow()));
                ui.redraw();
            }
            Cmd::Settings(SettingsCommand::Save(stored)) => {
                let saved = match &config {
                    Some(config) => {
                        settings_store::save(config, &stored).map_err(|e| e.to_string())
                    }
                    None => Err("no settings folder on this device".to_owned()),
                };
                if let Err(err) = saved {
                    if let Shown::Settings(vm) = &mut *ui.shown.borrow_mut() {
                        vm.set_status(&format!("Could not save the settings: {err}"));
                    }
                }
                ui.redraw();
            }
            Cmd::Settings(SettingsCommand::Tune) => {
                let probe = probe_tx.clone();
                std::thread::spawn(move || {
                    let _ = probe.send(pages_io::run_probe());
                });
            }
        }
    });
    let jobs: Rc<dyn Fn(Job)> = Rc::new({
        let (ui, places, cx) = (ui.clone(), places.clone(), cx.clone());
        move |job| match job {
            Job::None => {}
            Job::EditField => {
                if let Some(window) = ui.window.upgrade() {
                    window.invoke_focus_field();
                }
            }
            Job::Finish { start_steam } => {
                if start_steam {
                    work::start_steam(os);
                }
                *ui.first.borrow_mut() = None;
                // Installs and the noted dump folder may have changed: load the cards again.
                if let Some(cx) = &cx {
                    *places.borrow_mut() = self::places(os, &cx.vars);
                }
                let games = load_all(&places.borrow());
                ui.screen.borrow_mut().reload(games);
                ui.redraw();
            }
            job => {
                if let Some(cx) = &cx {
                    work::start(job, cx, event_tx.clone());
                }
            }
        }
    });
    wire(&window, &ui, run, jobs.clone());
    if cx.as_ref().is_some_and(|cx| setup::is_first_run(&cx.dirs)) {
        ui.open_first(None, steam);
    }
    // A run the app started but did not see end (closed, or left by Game Mode).
    let picked_up = cx.as_ref().and_then(|cx| {
        let game = after_crash::unfinished(&cx.dirs.logs())?;
        after_run(cx, game)
    });
    let crash_shown = picked_up.is_some();
    if let Some(page) = picked_up {
        ui.show(Shown::AfterCrash(page));
    }
    if let Some(game) = start_now(args.play, crash_shown) {
        start_game(Start {
            game,
            slot: None,
            keep: None,
        });
    }

    // Poll the controller, the game run, the probe and the first run's jobs on the UI thread;
    // all are cheap to drain.
    let mut controller = Controller::new();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(16),
        move || {
            if let Ok((game, status)) = done_rx.try_recv() {
                let games = load_all(&places.borrow());
                let mut screen = ui.screen.borrow_mut();
                screen.reload(games);
                screen.finished(&status);
                drop(screen);
                // The after-run step: the run's log and where.json; a crash shows its screen.
                if let Some(page) = cx.as_ref().and_then(|cx| after_run(cx, game)) {
                    *ui.shown.borrow_mut() = Shown::AfterCrash(page);
                }
                ui.redraw();
            }
            while let Ok(event) = event_rx.try_recv() {
                jobs(ui.first_do(|first| first.event(event)));
            }
            if let Ok(probe) = probe_rx.try_recv() {
                let result = probe.and_then(|text| {
                    let tuned = pages_io::tune_from(
                        &text,
                        &pages_io::read_reports(&relay_dirs(&places.borrow())),
                    );
                    if let (Ok(_), Some(config)) = (&tuned, &config) {
                        let _ = pages_io::save_cache(config, &text);
                    }
                    tuned
                });
                if let Shown::Settings(vm) = &mut *ui.shown.borrow_mut() {
                    vm.tuned(result);
                }
                ui.redraw();
            }
            let Some(controller) = controller.as_mut() else {
                return;
            };
            for input in controller.poll() {
                if let Some(window) = ui.window.upgrade() {
                    window.invoke_input(input_code(input));
                }
            }
        },
    );

    window.run()
}

/// The code of `input` for the window's `input` callback (the inverse of `Input::from_code`),
/// so the controller goes the same way as the keyboard.
fn input_code(input: Input) -> i32 {
    (0..=10)
        .find(|code| Input::from_code(*code) == Some(input))
        .unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    /// One headless Slint run of the page: the window built from `ui/app.slint`, drawn from the
    /// view model, driven through its own callbacks.
    #[test]
    fn headless_run_of_the_page() {
        i_slint_backend_testing::init_no_event_loop();
        let window = AppWindow::new().unwrap();
        let mut games: Vec<GameData> = Game::ALL.into_iter().map(GameData::empty).collect();
        games[1].saves.push(screen::SaveEntry {
            slot: 2,
            card: uncharted_core::savecard::SaveCard {
                game: Game::U2,
                slot: "SAVEDATA02".into(),
                picture: Some(b"not a png".to_vec()),
                title: Some("Synthetic title".into()),
                detail: None,
                saved_unix: 1_794_518_040,
                play_time_secs: None,
                device: None,
                build_id: None,
            },
        });
        let ui = Rc::new(Ui {
            screen: RefCell::new(Screen::new(games, None, NO_RELAY)),
            shown: RefCell::new(Shown::Continue),
            window: window.as_weak(),
            pictures: RefCell::default(),
            now: Box::new(|| 1_794_519_000),
            offset: Box::new(|_| 0),
            first: RefCell::new(None),
        });
        let commands = Rc::new(RefCell::new(Vec::new()));
        let run: Rc<dyn Fn(Cmd)> = Rc::new({
            let commands = commands.clone();
            move |c| commands.borrow_mut().push(c)
        });
        wire(&window, &ui, run, Rc::new(|_| {}));
        assert_eq!(window.get_mode(), 0);

        // Opens on Uncharted 2 (the only save), focus on Continue.
        assert_eq!(window.get_current_game(), 1);
        let card = window.get_card();
        assert_eq!(card.title, "Synthetic title");
        assert_eq!(
            card.lines.iter().collect::<Vec<_>>(),
            [SharedString::from("Saved today 21:14")]
        );
        // The bytes are not a PNG: no picture, and no crash.
        assert!(!card.has_picture);
        let actions: Vec<_> = window.get_actions().iter().collect();
        assert_eq!(actions.len(), 2);
        assert_eq!(
            (actions[0].label.as_str(), actions[0].hint.as_str()),
            ("Continue", "A")
        );
        assert_eq!(window.get_status(), NO_RELAY);
        assert_eq!((window.get_focus_row(), window.get_focus_index()), (0, 0));

        // Right, then down to the pages, through the window's own `input` callback.
        window.invoke_input(3);
        assert_eq!(window.get_focus_index(), 1);
        window.invoke_input(1);
        assert_eq!((window.get_focus_row(), window.get_focus_index()), (1, 1));
        window.invoke_input(6);
        assert_eq!(
            commands.borrow().last(),
            Some(&Cmd::Screen(Command::Open(Page::Settings)))
        );

        // RB to Uncharted 3: no save, Play only; A starts it.
        window.invoke_input(9);
        assert_eq!(window.get_current_game(), 2);
        assert_eq!(window.get_card().title, "Uncharted 3");
        window.invoke_input(6);
        assert_eq!(
            commands.borrow().last(),
            Some(&Cmd::Screen(Command::Start(Start {
                game: Game::U3,
                slot: None,
                keep: None,
            })))
        );

        // A click on the strip, then B opens the quit prompt.
        window.invoke_clicked(2, 1);
        assert_eq!(window.get_current_game(), 1);
        window.invoke_input(7);
        assert_eq!(window.get_overlay(), 1);
        window.invoke_clicked(5, 0);
        assert_eq!(commands.borrow().last(), Some(&Cmd::Screen(Command::Quit)));
    }

    /// The Saves and Settings pages in the same headless window: drawn, driven by keys and
    /// clicks, and B back to the Continue screen on the page's button.
    #[test]
    fn headless_run_of_the_saves_and_settings_pages() {
        i_slint_backend_testing::init_no_event_loop();
        let window = AppWindow::new().unwrap();
        let games: Vec<GameData> = Game::ALL.into_iter().map(GameData::empty).collect();
        let ui = Rc::new(Ui {
            screen: RefCell::new(Screen::new(games, None, NO_RELAY)),
            shown: RefCell::new(Shown::Continue),
            window: window.as_weak(),
            pictures: RefCell::default(),
            now: Box::new(|| 1_794_519_000),
            offset: Box::new(|_| 0),
        });
        let commands = Rc::new(RefCell::new(Vec::new()));
        let run: Rc<dyn Fn(Cmd)> = Rc::new({
            let commands = commands.clone();
            move |c| commands.borrow_mut().push(c)
        });
        wire(&window, &ui, run);
        assert_eq!(window.get_page(), 0);

        // Saves: Uncharted 2 with a save in slot 1 and one backup.
        let mut data: Vec<GameSaves> = Game::ALL.into_iter().map(GameSaves::empty).collect();
        data[1].slots[0].used = true;
        data[1].slots[0].saved_unix = 1_794_518_040;
        data[1].backups = vec![vm::saves::stamp(1_794_500_000, 1)];
        ui.show(Shown::Saves(SavesVm::new(data, Game::U2, NO_RELAY)));
        assert_eq!(window.get_page(), 1);
        assert_eq!(window.get_page_game(), 1);
        let slots: Vec<_> = window.get_save_slots().iter().collect();
        assert_eq!(slots[0].title, "Slot 1");
        assert_eq!(slots[0].mark, "Active");
        assert_eq!(
            slots[0].buttons.iter().collect::<Vec<_>>(),
            ["Active", "Copy to slot 2", "Back up", "Delete"]
        );
        assert_eq!(window.get_save_backups().row_count(), 1);
        assert!(window
            .get_saves_relay()
            .starts_with("save-relay syncs the active slot of Uncharted 2: Slot 1."));
        window.invoke_input(3);
        assert_eq!((window.get_page_row(), window.get_page_col()), (0, 1));
        window.invoke_input(6);
        assert_eq!(
            commands.borrow().last(),
            Some(&Cmd::Saves(SavesCommand::Copy {
                game: Game::U2,
                from: 1,
                to: 2
            }))
        );
        // Delete asks; the prompt's Keep button closes it.
        window.invoke_page_clicked(0, 3);
        assert!(window
            .get_saves_confirm()
            .starts_with("Delete the saves in slot 1"));
        window.invoke_page_clicked(-2, 1);
        assert_eq!(window.get_saves_confirm(), "");
        // B: back on the Saves button.
        window.invoke_input(7);
        assert_eq!(window.get_page(), 0);
        assert_eq!((window.get_focus_row(), window.get_focus_index()), (1, 0));

        // Settings, not tuned yet, on the desktop profile.
        let desktop = uncharted_core::profile::by_id("desktop").unwrap();
        let settings = SettingsVm::new(vm::settings::Stored::default(), desktop, None);
        ui.show(Shown::Settings(settings));
        assert_eq!(window.get_page(), 2);
        let cards: Vec<_> = window.get_tune_cards().iter().collect();
        assert_eq!(cards.len(), 3);
        assert_eq!(cards[1].title, "Balanced 60");
        assert!(cards[1].in_use);
        let choices: Vec<_> = window.get_choices().iter().collect();
        assert_eq!(choices.len(), 6);
        assert_eq!(
            (choices[1].label.as_str(), choices[1].value.as_str()),
            ("Frame rate", "60 FPS")
        );
        // A click on the frame rate's ">" saves 120.
        window.invoke_page_clicked(3, 1);
        assert_eq!(window.get_choices().row_data(1).unwrap().value, "120 FPS");
        assert!(matches!(
            commands.borrow().last(),
            Some(Cmd::Settings(SettingsCommand::Save(stored))) if stored.frame_rate == "120"
        ));
        window.invoke_page_clicked(1, 0);
        assert_eq!(
            commands.borrow().last(),
            Some(&Cmd::Settings(SettingsCommand::Tune))
        );
        assert!(window.get_tuning());
        // The page's Back button.
        window.invoke_page_clicked(-3, 0);
        assert_eq!(window.get_page(), 0);
        assert_eq!((window.get_focus_row(), window.get_focus_index()), (1, 1));
    }

    /// One headless Slint run of the first-run page: the client path through the window's own
    /// callbacks, as far as a job leaves the UI thread.
    #[test]
    fn headless_run_of_the_first_run() {
        i_slint_backend_testing::init_no_event_loop();
        let window = AppWindow::new().unwrap();
        let games: Vec<GameData> = Game::ALL.into_iter().map(GameData::empty).collect();
        let ui = Rc::new(Ui {
            screen: RefCell::new(Screen::new(games, None, NO_RELAY)),
            window: window.as_weak(),
            pictures: RefCell::default(),
            now: Box::new(|| 1_794_519_000),
            offset: Box::new(|_| 0),
            first: RefCell::new(None),
        });
        let commands = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::new(RefCell::new(Vec::new()));
        let run: Rc<dyn Fn(Command)> = Rc::new({
            let commands = commands.clone();
            move |c| commands.borrow_mut().push(c)
        });
        let jobs: Rc<dyn Fn(Job)> = Rc::new({
            let seen = seen.clone();
            move |j| seen.borrow_mut().push(j)
        });
        wire(&window, &ui, run, jobs.clone());
        ui.open_first(None, true);
        assert_eq!(window.get_mode(), 1);
        assert_eq!(window.get_fr_step(), "Step 1 of 4");
        assert_eq!(window.get_fr_options().row_count(), 3);
        assert_eq!(window.get_fr_focus(), 0);

        // Down, A: the client path looks for hubs.
        window.invoke_input(1);
        assert_eq!(window.get_fr_focus(), 1);
        window.invoke_input(6);
        assert_eq!(seen.borrow().last(), Some(&Job::Discover));
        assert!(commands.borrow().is_empty());
        jobs(ui.first_do(|f| f.event(firstrun::Event::Hubs(Ok(Vec::new())))));
        assert_eq!(window.get_fr_options().row_count(), 1);
        assert_ne!(window.get_fr_field(), "");
        window.invoke_fr_clicked(0);
        assert_eq!(seen.borrow().last(), Some(&Job::EditField));

        // The address typed, then the code from the keyboard's digits.
        window.invoke_field_accepted("192.168.1.20:47120".into());
        assert_eq!(window.get_fr_title(), "Pair with 192.168.1.20:47120");
        assert_eq!(window.get_fr_code().row_count(), 6);
        for code in [21, 22, 23, 24, 25, 26] {
            window.invoke_input(code);
        }
        window.invoke_input(30);
        window.invoke_input(29);
        assert_eq!(
            window.get_fr_code().iter().collect::<Vec<_>>(),
            ["1", "2", "3", "4", "5", "9"].map(SharedString::from)
        );
        window.invoke_input(6);
        assert!(matches!(
            seen.borrow().last(),
            Some(Job::PairPull { code, .. }) if code == "123459"
        ));
        assert_eq!(window.get_fr_step(), "Step 3 of 4");
        assert!(window.get_fr_options().row_count() == 0);

        // A failed pull returns to the code; B, B, B leaves for the Continue screen.
        ui.first_do(|f| f.event(firstrun::Event::Pulled(Err("no".into()))));
        assert_eq!(window.get_fr_error(), "No.");
        window.invoke_input(7);
        window.invoke_input(7);
        window.invoke_input(7);
        assert_eq!(
            seen.borrow().last(),
            Some(&Job::Finish { start_steam: false })
        );
    }

    #[test]
    fn controller_codes_round_trip() {
        for code in 0..=10 {
            let input = Input::from_code(code).unwrap();
            assert_eq!(input_code(input), code);
        }
    }

    #[test]
    fn play_arguments() {
        let start = Start {
            game: Game::U2,
            slot: Some(3),
            keep: None,
        };
        assert_eq!(play_args(&start), ["play", "u2"]);
        let start = Start {
            keep: Some(Keep::Server),
            ..start
        };
        assert_eq!(play_args(&start), ["play", "u2", "--keep", "server"]);
        let start = Start {
            keep: Some(Keep::Local),
            ..start
        };
        assert_eq!(play_args(&start), ["play", "u2", "--keep", "local"]);
    }

    #[test]
    fn status_after_a_run() {
        assert!(run_status(Game::U1, Err(std::io::Error::other("missing")))
            .starts_with("Could not start "));
        assert_eq!(
            page_status(Page::Fleet),
            "Fleet comes in a later build (roadmap 2.08)."
        );
        assert_eq!(
            keep_status(Game::U3, Keep::Local),
            "Keeping this device's save of Uncharted 3."
        );
    }
}
