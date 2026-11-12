//! The launcher (ADR 0003): the Continue screen (docs/continue-screen.md, roadmap 2.04). One
//! card per game from `core::savecard`, Continue and Play, Other saves, the sync-conflict
//! prompt and the fleet warning; keyboard, mouse and controller move the same focus.
//!
//! The view model is `screen.rs` (tested without a window); this file draws its `View` into the
//! Slint window and carries out its `Command`s. Continue and Play run
//! `uncharted-universal play <game>` (the command line, roadmap 2.02) beside this program and
//! wait for it: that command pulls the save, starts the game and pushes the save.

mod card;
mod focus;
mod input;
mod load;
mod screen;

use card::Clock;
use input::Controller;
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
use uncharted_core::launch::Keep;
use uncharted_core::platform::{AppDirs, Os, Vars};
use uncharted_core::saves;

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

/// The screen and the window, redrawn after every change.
struct Ui {
    screen: RefCell<Screen>,
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
    }

    fn input(&self, input: Input) -> Command {
        let command = self.screen.borrow_mut().handle(input);
        self.redraw();
        command
    }

    fn click(&self, area: i32, index: i32) -> Command {
        let Ok(index) = usize::try_from(index) else {
            return Command::None;
        };
        let command = self.screen.borrow_mut().click(area, index);
        self.redraw();
        command
    }
}

/// Connect the window's callbacks to the screen; every command goes to `run`.
fn wire(window: &AppWindow, ui: &Rc<Ui>, run: Rc<dyn Fn(Command)>) {
    window.on_input({
        let (ui, run) = (ui.clone(), run.clone());
        move |code| {
            if let Some(input) = Input::from_code(code) {
                run(ui.input(input));
            }
        }
    });
    window.on_clicked({
        let ui = ui.clone();
        move |area, index| run(ui.click(area, index))
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

fn page_status(page: Page) -> String {
    let piece = match page {
        Page::Saves | Page::Settings => "2.06",
        Page::Fleet => "2.08",
        Page::About => "2.09",
    };
    format!("{} comes in a later build (roadmap {piece}).", page.label())
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
    let (places, status, config) = match Vars::from_lookup(os, |k| std::env::var(k).ok()) {
        Ok(vars) => (
            places(os, &vars),
            NO_RELAY.to_owned(),
            Some(AppDirs::new(&vars).config),
        ),
        Err(err) => (Vec::new(), format!("Cannot find the saves: {err}"), None),
    };
    let places = Rc::new(places);
    let last = config.as_deref().and_then(load::last_started);
    let screen = Screen::new(load_all(&places), last, &status);
    let ui = Rc::new(Ui {
        screen: RefCell::new(screen),
        window: window.as_weak(),
        pictures: RefCell::default(),
        now: Box::new(local_clock_now),
        offset: Box::new(local_offset),
    });

    // A finished game run comes back from its thread through this channel.
    let (done_tx, done_rx) = mpsc::channel::<String>();

    let run: Rc<dyn Fn(Command)> = Rc::new({
        let (ui, places) = (ui.clone(), places.clone());
        move |command| match command {
            Command::None => {}
            Command::Quit => {
                let _ = slint::quit_event_loop();
            }
            Command::Open(page) => {
                ui.screen.borrow_mut().set_status(&page_status(page));
                ui.redraw();
            }
            Command::SetUp(game) => {
                ui.screen.borrow_mut().set_status(&format!(
                    "Setting up {} on this device comes with the guided first run (roadmap 2.05).",
                    game.name()
                ));
                ui.redraw();
            }
            Command::Keep { game, keep } => {
                ui.screen.borrow_mut().set_status(&keep_status(game, keep));
                ui.redraw();
            }
            Command::Start(start) => {
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
        }
    });
    wire(&window, &ui, run);

    // Poll the controller and the game run on the UI thread; both are cheap to drain.
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
            window: window.as_weak(),
            pictures: RefCell::default(),
            now: Box::new(|| 1_794_519_000),
            offset: Box::new(|_| 0),
        });
        let commands = Rc::new(RefCell::new(Vec::new()));
        let run: Rc<dyn Fn(Command)> = Rc::new({
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
            Some(&Command::Open(Page::Settings))
        );

        // RB to Uncharted 3: no save, Play only; A starts it.
        window.invoke_input(9);
        assert_eq!(window.get_current_game(), 2);
        assert_eq!(window.get_card().title, "Uncharted 3");
        window.invoke_input(6);
        assert_eq!(
            commands.borrow().last(),
            Some(&Command::Start(Start {
                game: Game::U3,
                slot: None,
                keep: None,
            }))
        );

        // A click on the strip, then B opens the quit prompt.
        window.invoke_clicked(2, 1);
        assert_eq!(window.get_current_game(), 1);
        window.invoke_input(7);
        assert_eq!(window.get_overlay(), 1);
        window.invoke_clicked(5, 0);
        assert_eq!(commands.borrow().last(), Some(&Command::Quit));
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
