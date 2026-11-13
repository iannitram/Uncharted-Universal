//! The launcher (ADR 0003): the Continue screen (docs/continue-screen.md, roadmap 2.04). One
//! card per game from `core::savecard`, Continue and Play, Other saves, the sync-conflict
//! prompt and the fleet warning; keyboard, mouse and controller move the same focus. Its Saves
//! and Settings buttons open those pages (roadmap 2.06, `src/vm/`). The page row's About opens
//! About (2.09): version, patches, licences, the Slint notice, and the language button (put
//! there until Settings existed, owner ruling 2026-11-12; it has not moved yet). The Continue
//! screen's own texts are English until they are in the catalogues.
//!
//! The view models are `screen.rs` and `vm/` (tested without a window); this file draws their
//! views into the Slint window and carries out their commands, the files through `pages_io.rs`
//! and `settings_store.rs`. Continue and Play run
//! `uncharted-universal play <game>` (the command line, roadmap 2.02) beside this program and
//! wait for it: that command pulls the save, starts the game and pushes the save.

mod card;
mod focus;
mod input;
mod lang;
mod load;
mod pages_io;
mod screen;
mod settings_store;
mod vm;

use card::Clock;
use focus::{AboutFocus, Effect, Nav};
use input::Controller;
use lang::Texts;
use screen::{Command, GameData, Input, OverlayView, Page, Screen, Start, View};
use slint::{ComponentHandle, Image, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Stdio;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uncharted_core::games::Game;
use uncharted_core::language::{self, Language};
use uncharted_core::launch::Keep;
use uncharted_core::patches::{Patch, Selection, Target};
use uncharted_core::platform::{AppDirs, Os, Vars};
use uncharted_core::saves;
use vm::saves::{GameSaves, SavesCommand, SavesView};
use vm::settings::{SettingsCommand, SettingsView};
use vm::{SavesVm, SettingsVm};

slint::include_modules!();

/// The status line until save-relay is linked (roadmap 9.01).
const NO_RELAY: &str = "Saves stay on this device: save-relay is not in this build yet";

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

/// The interface language and what About shows, outside the window.
struct Lang {
    /// `ui_language`: "" is Automatic.
    setting: String,
    /// Where the setting is saved; `None` when the platform folders are not set.
    settings_file: Option<PathBuf>,
    system_locale: Option<String>,
    texts: Texts,
    /// The chosen game's patches and identity, for About. Preparation picks the game's patches;
    /// until then the list is empty.
    patches: Vec<Patch>,
    target: Target,
    selection: Selection,
}

impl Lang {
    fn new(setting: String, settings_file: Option<PathBuf>, system_locale: Option<String>) -> Lang {
        let language = language::resolve(&setting, system_locale.as_deref());
        Lang {
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
        }
    }

    fn language(&self) -> Language {
        self.texts.language()
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

    /// Push the language and everything About shows that Rust builds, in the current language.
    fn show(&self, window: &AppWindow) {
        if let Err(err) = slint::select_bundled_translation(self.language().code()) {
            eprintln!("could not select the interface language: {err}");
        }
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
        window.set_patches(model(patches));
        window.set_shipped_licences(licence_model(vm::about::SHIPPED, &self.texts));
        window.set_learned_licences(licence_model(vm::about::LEARNED_FROM, &self.texts));
    }
}

fn licence_model(list: &[vm::about::Licence], texts: &Texts) -> ModelRc<LicenceRow> {
    model(
        vm::about::licence_rows(list, texts)
            .into_iter()
            .map(|row| LicenceRow {
                name: row.name.into(),
                licence: row.licence.into(),
                role: row.role.into(),
            })
            .collect(),
    )
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

/// What the window shows over the Continue screen.
enum Shown {
    Continue,
    Saves(SavesVm),
    Settings(SettingsVm),
}

/// `page` of the window while About is open (over the Continue screen, whatever `Shown` is).
const ABOUT_PAGE: i32 = 3;

impl Shown {
    /// `page` of the window: 0 the Continue screen, 1 Saves, 2 Settings ([`ABOUT_PAGE`] is 3).
    fn code(&self) -> i32 {
        match self {
            Shown::Continue => 0,
            Shown::Saves(_) => 1,
            Shown::Settings(_) => 2,
        }
    }
}

/// A command of the screen or of a page, for `main` to carry out.
#[derive(Debug, Clone, PartialEq)]
enum Cmd {
    Screen(Command),
    Saves(SavesCommand),
    Settings(SettingsCommand),
}

/// The screen, the page over it and the window, redrawn after every change.
struct Ui {
    screen: RefCell<Screen>,
    shown: RefCell<Shown>,
    lang: RefCell<Lang>,
    /// About's focus while About is open over the Continue screen.
    about: RefCell<Option<AboutFocus>>,
    window: slint::Weak<AppWindow>,
    pictures: RefCell<Pictures>,
    now: Box<dyn Fn() -> u64>,
    offset: Box<dyn Fn(u64) -> i64>,
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
        let shown = self.shown.borrow();
        match &*shown {
            Shown::Continue => {}
            Shown::Saves(vm) => apply_saves(&window, &vm.view(&clock)),
            Shown::Settings(vm) => apply_settings(&window, &vm.view()),
        }
        let about = self.about.borrow();
        window.set_page(if about.is_some() {
            ABOUT_PAGE
        } else {
            shown.code()
        });
        window.set_about_focus(about.as_ref().map_or(0, |a| a.focused() as i32));
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
        let about = self.about.borrow_mut().as_mut().map(|about| match input {
            Input::Move(nav) => about.nav(nav),
            Input::Tab { back } => about.nav(if back { Nav::Left } else { Nav::Right }),
            Input::Activate => about.activate(),
            Input::Back => Effect::Close,
            _ => Effect::None,
        });
        if let Some(effect) = about {
            self.about_effect(effect);
            return Cmd::Screen(Command::None);
        }
        let command = match &mut *self.shown.borrow_mut() {
            Shown::Continue => Cmd::Screen(self.screen.borrow_mut().handle(input)),
            Shown::Saves(vm) => Cmd::Saves(vm.handle(input)),
            Shown::Settings(vm) => Cmd::Settings(vm.handle(input)),
        };
        let command = self.opened(command);
        self.after(&command);
        command
    }

    /// `area` 7 is a button of About (`AboutFocus::click`); the others are `Screen::click`'s.
    fn click(&self, area: i32, index: i32) -> Cmd {
        let Ok(index) = usize::try_from(index) else {
            return Cmd::Screen(Command::None);
        };
        let about = self.about.borrow_mut().as_mut().map(|about| match area {
            7 => about.click(index),
            _ => Effect::None,
        });
        if let Some(effect) = about {
            self.about_effect(effect);
            return Cmd::Screen(Command::None);
        }
        let command = Cmd::Screen(self.screen.borrow_mut().click(area, index));
        self.opened(command)
    }

    /// About opens here; every other command goes on to the app.
    fn opened(&self, command: Cmd) -> Cmd {
        if command == Cmd::Screen(Command::Open(Page::About)) {
            *self.about.borrow_mut() = Some(AboutFocus::default());
            self.redraw();
            return Cmd::Screen(Command::None);
        }
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
        };
        self.after(&command);
        command
    }

    /// Back closes the page; then redraw.
    fn after(&self, command: &Cmd) {
        match command {
            Cmd::Saves(SavesCommand::Back) => self.close(Page::Saves),
            Cmd::Settings(SettingsCommand::Back) => self.close(Page::Settings),
            _ => {}
        }
        self.redraw();
    }

    fn about_effect(&self, effect: Effect) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        match effect {
            Effect::None => {}
            Effect::Focus(index) => window.set_about_focus(index as i32),
            Effect::Scroll(steps) => window.invoke_scroll_about(steps),
            Effect::NextLanguage => {
                let mut lang = self.lang.borrow_mut();
                lang.next_language();
                lang.show(&window);
            }
            Effect::Close => {
                *self.about.borrow_mut() = None;
                self.redraw();
            }
        }
    }
}

/// Connect the window's callbacks to the screen and the pages; every command goes to `run`.
fn wire(window: &AppWindow, ui: &Rc<Ui>, run: Rc<dyn Fn(Cmd)>) {
    window.on_input({
        let (ui, run) = (ui.clone(), run.clone());
        move |code| {
            if let Some(input) = Input::from_code(code) {
                run(ui.input(input));
            }
        }
    });
    window.on_clicked({
        let (ui, run) = (ui.clone(), run.clone());
        move |area, index| run(ui.click(area, index))
    });
    window.on_page_clicked({
        let ui = ui.clone();
        move |row, col| run(ui.page_click(row, col))
    });
    ui.lang.borrow().show(window);
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
                dump: load::dump_dir(game, |k| std::env::var(k).ok()),
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

/// The arguments of `uncharted-universal play` for `start`.
fn play_args(start: &Start) -> Vec<String> {
    let mut args = vec!["play".to_owned(), start.game.key().to_owned()];
    if let Some(keep) = start.keep {
        args.push("--keep".into());
        args.push(match keep {
            Keep::Server => "server".into(),
            Keep::Local => "local".into(),
        });
    }
    args
}

/// The status line after a run of the play command.
fn run_status(game: Game, result: std::io::Result<std::process::Output>) -> String {
    match result {
        Err(err) => format!("Could not start {}: {err}", cli_path().display()),
        Ok(out) if out.status.success() => format!("{} closed.", game.name()),
        Ok(out) => String::from_utf8_lossy(&out.stderr)
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "{} stopped with exit status {}.",
                    game.name(),
                    out.status.code().unwrap_or(-1)
                )
            }),
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
    let window = AppWindow::new()?;
    let os = Os::current();
    let (places, status, dirs) = match Vars::from_lookup(os, |k| std::env::var(k).ok()) {
        Ok(vars) => (
            places(os, &vars),
            NO_RELAY.to_owned(),
            Some(AppDirs::new(&vars)),
        ),
        Err(err) => (Vec::new(), format!("Cannot find the saves: {err}"), None),
    };
    let settings_file = dirs.as_ref().map(AppDirs::settings);
    let setting = settings_file
        .as_deref()
        .map(language::read_setting)
        .unwrap_or_default();
    let lang = Lang::new(setting, settings_file, sys_locale::get_locale());
    let places = Rc::new(places);
    let config = dirs.as_ref().map(|d| d.config.clone());
    let backups = dirs.as_ref().map(AppDirs::backups);
    let detected = pages_io::detected_profile(os);
    let last = config.as_deref().and_then(load::last_started);
    let screen = Screen::new(load_all(&places), last, &status);
    let ui = Rc::new(Ui {
        screen: RefCell::new(screen),
        shown: RefCell::new(Shown::Continue),
        lang: RefCell::new(lang),
        about: RefCell::default(),
        window: window.as_weak(),
        pictures: RefCell::default(),
        now: Box::new(local_clock_now),
        offset: Box::new(local_offset),
    });

    // A finished game run comes back from its thread through this channel; the hardware probe
    // through the other.
    let (done_tx, done_rx) = mpsc::channel::<String>();
    let (probe_tx, probe_rx) = mpsc::channel::<Result<String, String>>();

    let run: Rc<dyn Fn(Cmd)> = Rc::new({
        let (ui, places, config) = (ui.clone(), places.clone(), config.clone());
        move |command| match command {
            Cmd::Screen(Command::None)
            | Cmd::Saves(SavesCommand::None | SavesCommand::Back)
            | Cmd::Settings(SettingsCommand::None | SettingsCommand::Back) => {}
            Cmd::Screen(Command::Quit) => {
                let _ = slint::quit_event_loop();
            }
            Cmd::Screen(Command::Open(Page::Saves)) => match &backups {
                Some(backups) if !places.is_empty() => {
                    let game = ui.screen.borrow().current_game();
                    let vm = SavesVm::new(saves_data(&places, backups), game, NO_RELAY);
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
                        pages_io::tune_from(&probe, &pages_io::read_reports(&relay_dirs(&places)))
                            .ok()
                    });
                ui.show(Shown::Settings(SettingsVm::new(stored, detected, tune)));
            }
            Cmd::Screen(Command::Open(page)) => {
                ui.screen.borrow_mut().set_status(&page_status(page));
                ui.redraw();
            }
            Cmd::Screen(Command::SetUp(game)) => {
                ui.screen.borrow_mut().set_status(&format!(
                    "Setting up {} on this device comes with the guided first run (roadmap 2.05).",
                    game.name()
                ));
                ui.redraw();
            }
            Cmd::Screen(Command::Keep { game, keep }) => {
                ui.screen.borrow_mut().set_status(&keep_status(game, keep));
                ui.redraw();
            }
            Cmd::Screen(Command::Start(start)) => {
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
                ui.screen
                    .borrow_mut()
                    .running(&format!("Starting {}...", start.game.name()));
                ui.redraw();
                let done = done_tx.clone();
                std::thread::spawn(move || {
                    let result = std::process::Command::new(cli_path())
                        .args(play_args(&start))
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::piped())
                        .output();
                    let _ = done.send(run_status(start.game, result));
                });
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
                ui.screen.borrow_mut().reload(load_all(&places));
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
    wire(&window, &ui, run);

    // Poll the controller, the game run and the probe on the UI thread; all are cheap to drain.
    let mut controller = Controller::new();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(16),
        move || {
            if let Ok(status) = done_rx.try_recv() {
                let games = load_all(&places);
                let mut screen = ui.screen.borrow_mut();
                screen.reload(games);
                screen.finished(&status);
                drop(screen);
                ui.redraw();
            }
            if let Ok(probe) = probe_rx.try_recv() {
                let result = probe.and_then(|text| {
                    let tuned =
                        pages_io::tune_from(&text, &pages_io::read_reports(&relay_dirs(&places)));
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
    use i_slint_backend_testing::ElementHandle;
    use slint::Model;

    fn shows(window: &AppWindow, label: &str) -> bool {
        !ElementHandle::find_by_accessible_label(window, label)
            .collect::<Vec<_>>()
            .is_empty()
    }

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
            lang: RefCell::new(Lang::new("en".into(), None, None)),
            about: RefCell::default(),
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
            lang: RefCell::new(Lang::new("en".into(), None, None)),
            about: RefCell::default(),
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

    // The testing backend, and with it Slint's translation choice, is per thread: this test's
    // language switches do not reach the one above.
    #[test]
    fn about_and_the_language_button_in_the_window() {
        i_slint_backend_testing::init_no_event_loop();
        let window = AppWindow::new().unwrap();
        let dir = std::env::temp_dir().join(format!("uncharted-app-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("settings.json");
        let games = Game::ALL.into_iter().map(GameData::empty).collect();
        let ui = Rc::new(Ui {
            screen: RefCell::new(Screen::new(games, None, NO_RELAY)),
            lang: RefCell::new(Lang::new(
                String::new(),
                Some(file.clone()),
                Some("ko_KR".into()),
            )),
            about: RefCell::default(),
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

        // Automatic with an unsupported locale is English. The Slint notice is on About only.
        assert_eq!(window.get_language_name(), "Automatic (English)");
        assert_eq!(window.get_page(), 0);
        assert!(!shows(&window, "Built with Slint"));
        assert_eq!(
            window.get_shipped_licences().row_count(),
            vm::about::SHIPPED.len()
        );

        // Down to the page row, right to About, A: About opens, focus on the language button.
        window.invoke_input(1);
        for _ in 0..3 {
            window.invoke_input(3);
        }
        window.invoke_input(6);
        assert_eq!(window.get_page(), ABOUT_PAGE);
        assert_eq!(window.get_about_focus(), 0);
        assert!(
            commands
                .borrow()
                .iter()
                .all(|c| *c == Cmd::Screen(Command::None)),
            "About opens in the window"
        );
        // Only what is on screen is found: the top of About, not yet its end.
        assert!(shows(&window, "Patches"));
        assert!(!shows(&window, "Built with Slint"));
        // Down scrolls to the end (and stops there); the Slint notice is at the bottom.
        for _ in 0..40 {
            window.invoke_input(1);
        }
        assert!(shows(&window, "Built with Slint"));
        assert!(!shows(&window, "Patches"));
        window.invoke_input(0);
        assert!(shows(&window, "Back"), "the Back button stays on screen");
        // Right to Back, A: the Continue screen, focus still on About.
        window.invoke_input(3);
        assert_eq!(window.get_about_focus(), 1);
        window.invoke_input(6);
        assert_eq!(window.get_page(), 0);
        assert_eq!((window.get_focus_row(), window.get_focus_index()), (1, 3));

        // Open it again; the language button steps English -> Spanish, Slint's own texts
        // follow, and the choice is saved.
        window.invoke_input(6);
        window.invoke_clicked(7, 0);
        assert_eq!(window.get_language_name(), "English");
        assert_eq!(language::read_setting(&file), "en");
        window.invoke_input(6);
        assert_eq!(window.get_language_name(), "Español");
        assert_eq!(language::read_setting(&file), "es");
        assert!(shows(&window, "Volver"), "Slint texts are in Spanish");
        assert!(!shows(&window, "Back"));
        let roles = window.get_shipped_licences();
        assert_eq!(
            roles.row_data(0).unwrap().role,
            "Motor gráfico (nuestro fork, en gpu/)"
        );

        // Every language switches without an error.
        for _ in 0..6 {
            window.invoke_input(6);
        }
        assert_eq!(language::read_setting(&file), "");
        assert_eq!(window.get_language_name(), "Automatic (English)");
        // B goes back.
        window.invoke_input(7);
        assert_eq!(window.get_page(), 0);
        let _ = std::fs::remove_dir_all(&dir);
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
