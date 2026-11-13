//! The Continue screen's view model (docs/continue-screen.md, roadmap 2.04): which game is
//! shown, which buttons it has, where focus is, the prompts over the screen, and what pressing a
//! button asks the app to do. Plain Rust with no window, so every state is tested with fake
//! `SaveCard`s; `main.rs` draws a [`View`] and carries out a [`Command`].

use crate::card::{self, Clock};
use crate::focus::{Focus, Nav, Row};
use uncharted_core::games::Game;
use uncharted_core::launch::Keep;
use uncharted_core::savecard::{SaveCard, SideBySide};

/// What a key, a controller button or a click means to the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Move(Nav),
    /// Tab (`back` false) or Shift+Tab.
    Tab {
        back: bool,
    },
    /// A (Cross), Enter, Space.
    Activate,
    /// B (Circle), Esc.
    Back,
    /// LB (L1), Page Up.
    PrevGame,
    /// RB (R1), Page Down.
    NextGame,
    /// Y (Triangle), O.
    OtherSaves,
}

impl Input {
    /// The integer the `.slint` file passes through its `input` callback.
    pub fn from_code(code: i32) -> Option<Input> {
        Some(match code {
            0 => Input::Move(Nav::Up),
            1 => Input::Move(Nav::Down),
            2 => Input::Move(Nav::Left),
            3 => Input::Move(Nav::Right),
            4 => Input::Tab { back: false },
            5 => Input::Tab { back: true },
            6 => Input::Activate,
            7 => Input::Back,
            8 => Input::PrevGame,
            9 => Input::NextGame,
            10 => Input::OtherSaves,
            _ => return None,
        })
    }
}

/// A button of the action row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Continue,
    Play,
    /// Other saves, with the number of saves of the game.
    OtherSaves(usize),
    /// The game is not prepared on this device: opens first run (2.05) at the right step.
    SetUp,
}

impl Action {
    pub fn label(self) -> String {
        match self {
            Action::Continue => "Continue".into(),
            Action::Play => "Play".into(),
            Action::OtherSaves(n) => format!("Other saves ({n})"),
            Action::SetUp => "Set up".into(),
        }
    }
}

/// A button of the page row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Saves,
    Settings,
    Fleet,
    About,
}

impl Page {
    pub const ALL: [Page; 4] = [Page::Saves, Page::Settings, Page::Fleet, Page::About];

    pub fn label(self) -> &'static str {
        match self {
            Page::Saves => "Saves",
            Page::Settings => "Settings",
            Page::Fleet => "Fleet",
            Page::About => "About",
        }
    }
}

/// One save of a game, with the slot it is in (1 is the game's own folder, 2-10 the
/// `Save slots` folders; `saves::slot_dir`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveEntry {
    pub slot: u8,
    pub card: SaveCard,
}

/// What the screen knows of one game on this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameData {
    pub game: Game,
    /// Every save of the game across its slots, newest first.
    pub saves: Vec<SaveEntry>,
    /// The game's selected slot (`saves::selected`): the one save-relay syncs.
    pub selected_slot: u8,
    /// Whether the game is prepared on this device.
    pub set_up: bool,
    /// The collection's `icon0.png` from the player's own installed copy, when it can be read.
    pub fallback_picture: Option<Vec<u8>>,
    /// The fleet warning for the newest save (`fleet::warning`), when its build differs from
    /// this device's.
    pub warning: Option<String>,
}

impl GameData {
    /// A game with no save, set up, nothing else known.
    pub fn empty(game: Game) -> GameData {
        GameData {
            game,
            saves: Vec::new(),
            selected_slot: 1,
            set_up: true,
            fallback_picture: None,
            warning: None,
        }
    }

    fn newest(&self) -> Option<&SaveEntry> {
        self.saves.first()
    }
}

/// A request to start a game: pull, start, push (`uncharted-universal play`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    pub game: Game,
    /// The slot to make the active one first (Continue); `None` keeps the active slot (Play).
    pub slot: Option<u8>,
    /// The answer to a sync conflict the player already gave.
    pub keep: Option<Keep>,
}

/// What the app must do after an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    None,
    Start(Start),
    /// Open first run (2.05) for the game.
    SetUp(Game),
    Open(Page),
    /// A conflict was answered without starting the game.
    Keep {
        game: Game,
        keep: Keep,
    },
    Quit,
}

/// A prompt over the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Overlay {
    None,
    /// "Quit the launcher?"
    Quit,
    /// The list of the game's saves; `index` is the focused row.
    OtherSaves {
        index: usize,
    },
    /// The side-by-side conflict prompt; `server` is the focused side, `start` what to do after.
    Conflict {
        server: bool,
        start: Option<Start>,
    },
    /// The fleet warning before Continue.
    Warning {
        start: Start,
    },
    /// The game is being started; input goes to nothing until it is back.
    Running {
        text: String,
    },
}

/// The Continue screen.
#[derive(Debug)]
pub struct Screen {
    games: Vec<GameData>,
    current: usize,
    focus: Focus,
    overlay: Overlay,
    /// The open sync conflict of each game, by index into `games`.
    conflicts: Vec<Option<SideBySide>>,
    status: String,
}

/// The game the screen opens on: the one whose newest save is the newest of the three (the
/// earlier game on a tie); with no save at all, the game last started; else Uncharted 1.
pub fn opening_game(games: &[GameData], last_started: Option<Game>) -> usize {
    let newest = games
        .iter()
        .enumerate()
        .filter_map(|(i, g)| g.newest().map(|s| (s.card.saved_unix, i)))
        .max_by(|(a, i), (b, j)| a.cmp(b).then(j.cmp(i)));
    if let Some((_, index)) = newest {
        return index;
    }
    last_started
        .and_then(|game| games.iter().position(|g| g.game == game))
        .unwrap_or(0)
}

impl Screen {
    /// The screen for `games` (in `Game::ALL` order), opened on [`opening_game`] with focus on
    /// the default button.
    pub fn new(games: Vec<GameData>, last_started: Option<Game>, status: &str) -> Screen {
        assert!(!games.is_empty(), "the Continue screen needs a game");
        let current = opening_game(&games, last_started);
        let conflicts = vec![None; games.len()];
        let mut screen = Screen {
            games,
            current,
            focus: Focus::actions(0),
            overlay: Overlay::None,
            conflicts,
            status: status.to_owned(),
        };
        screen.focus = screen.default_focus();
        screen
    }

    pub fn current_game(&self) -> Game {
        self.games[self.current].game
    }

    #[cfg(test)]
    pub fn focus(&self) -> Focus {
        self.focus
    }

    pub fn set_status(&mut self, status: &str) {
        status.clone_into(&mut self.status);
    }

    /// Back from a page (Saves, Settings): focus on that page's button, as B leaves it.
    pub fn focus_page(&mut self, page: Page) {
        if let Some(index) = Page::ALL.iter().position(|p| *p == page) {
            self.focus = Focus::pages(index);
        }
    }

    /// New data from disk (after a pull or a game run). The same game stays shown; focus stays
    /// where it was when that button is still there, else goes to the default button.
    pub fn reload(&mut self, games: Vec<GameData>) {
        let game = self.current_game();
        let focused = self.focused_button();
        self.games = games;
        self.conflicts.resize(self.games.len(), None);
        self.current = self.games.iter().position(|g| g.game == game).unwrap_or(0);
        if self.focused_button() != focused {
            self.focus = self.default_focus();
        }
    }

    /// save-relay found a conflict for `game`. When that game is shown and nothing else is
    /// open, the prompt opens at once, focus on the newer side. The save-relay pull that calls
    /// this comes with 9.01; until then only the tests raise a conflict.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_conflict(&mut self, game: Game, pair: SideBySide) {
        let Some(index) = self.games.iter().position(|g| g.game == game) else {
            return;
        };
        let server = pair.server_is_newer;
        self.conflicts[index] = Some(pair);
        if index == self.current && self.overlay == Overlay::None {
            self.overlay = Overlay::Conflict {
                server,
                start: None,
            };
        }
    }

    /// The game was started: input is ignored until [`Screen::finished`].
    pub fn running(&mut self, text: &str) {
        self.overlay = Overlay::Running {
            text: text.to_owned(),
        };
    }

    /// The game run is over; `status` says how it went.
    pub fn finished(&mut self, status: &str) {
        self.overlay = Overlay::None;
        self.set_status(status);
    }

    fn data(&self) -> &GameData {
        &self.games[self.current]
    }

    /// The action row of the shown game: Set up when it is not prepared here; else Continue
    /// when it has a save, Play always, Other saves when it has more than one.
    pub fn actions(&self) -> Vec<Action> {
        let data = self.data();
        if !data.set_up {
            return vec![Action::SetUp];
        }
        let mut actions = Vec::new();
        if !data.saves.is_empty() {
            actions.push(Action::Continue);
        }
        actions.push(Action::Play);
        if data.saves.len() > 1 {
            actions.push(Action::OtherSaves(data.saves.len()));
        }
        actions
    }

    /// Continue when the game has a save, else Play (or Set up): the first action.
    fn default_focus(&self) -> Focus {
        Focus::actions(0)
    }

    fn focused_button(&self) -> Option<Result<Action, Page>> {
        match self.focus.row {
            Row::Actions => self.actions().get(self.focus.index).copied().map(Ok),
            Row::Pages => Page::ALL.get(self.focus.index).copied().map(Err),
        }
    }

    fn change_game(&mut self, index: usize) {
        self.current = index;
        self.focus = self.default_focus();
    }

    /// Carry out one input.
    pub fn handle(&mut self, input: Input) -> Command {
        match self.overlay.clone() {
            Overlay::None => self.handle_screen(input),
            Overlay::Running { .. } => Command::None,
            Overlay::Quit => match input {
                Input::Activate => Command::Quit,
                Input::Back => {
                    self.overlay = Overlay::None;
                    Command::None
                }
                _ => Command::None,
            },
            Overlay::Warning { start } => match input {
                Input::Activate => {
                    self.overlay = Overlay::None;
                    Command::Start(start)
                }
                Input::Back => {
                    self.overlay = Overlay::None;
                    Command::None
                }
                _ => Command::None,
            },
            Overlay::OtherSaves { index } => self.handle_list(index, input),
            Overlay::Conflict { server, start } => self.handle_conflict(server, start, input),
        }
    }

    fn handle_screen(&mut self, input: Input) -> Command {
        let (actions, pages) = (self.actions().len(), Page::ALL.len());
        let games = self.games.len();
        match input {
            Input::Move(nav) => self.focus = self.focus.step(nav, actions, pages),
            Input::Tab { back } => self.focus = self.focus.tab(back, actions, pages),
            Input::PrevGame => self.change_game((self.current + games - 1) % games),
            Input::NextGame => self.change_game((self.current + 1) % games),
            Input::Back => self.overlay = Overlay::Quit,
            Input::OtherSaves => {
                if self.data().saves.len() > 1 {
                    self.overlay = Overlay::OtherSaves { index: 0 };
                }
            }
            Input::Activate => {
                return match self.focused_button() {
                    Some(Ok(action)) => self.activate(action),
                    Some(Err(page)) => Command::Open(page),
                    None => Command::None,
                }
            }
        }
        Command::None
    }

    fn activate(&mut self, action: Action) -> Command {
        let game = self.current_game();
        match action {
            Action::SetUp => Command::SetUp(game),
            Action::OtherSaves(_) => {
                self.overlay = Overlay::OtherSaves { index: 0 };
                Command::None
            }
            Action::Continue => {
                let slot = self.data().newest().map(|s| s.slot);
                let start = Start {
                    game,
                    slot,
                    keep: None,
                };
                // An open conflict asks first; the build warning shows once it is settled.
                if self.data().warning.is_some() && !self.conflicted() {
                    self.overlay = Overlay::Warning { start };
                    return Command::None;
                }
                self.start_or_ask(start)
            }
            Action::Play => self.start_or_ask(Start {
                game,
                slot: None,
                keep: None,
            }),
        }
    }

    fn conflicted(&self) -> bool {
        self.conflicts[self.current].is_some()
    }

    /// Start, unless the game has an open conflict: then the prompt opens again instead.
    fn start_or_ask(&mut self, start: Start) -> Command {
        match &self.conflicts[self.current] {
            Some(pair) => {
                self.overlay = Overlay::Conflict {
                    server: pair.server_is_newer,
                    start: Some(start),
                };
                Command::None
            }
            None => Command::Start(start),
        }
    }

    fn handle_list(&mut self, index: usize, input: Input) -> Command {
        let rows = self.data().saves.len();
        match input {
            Input::Move(Nav::Up) => {
                self.overlay = Overlay::OtherSaves {
                    index: index.saturating_sub(1),
                }
            }
            Input::Move(Nav::Down) => {
                self.overlay = Overlay::OtherSaves {
                    index: (index + 1).min(rows.saturating_sub(1)),
                }
            }
            Input::Activate => {
                let Some(entry) = self.data().saves.get(index) else {
                    return Command::None;
                };
                let start = Start {
                    game: self.current_game(),
                    slot: Some(entry.slot),
                    keep: None,
                };
                self.overlay = Overlay::None;
                return self.start_or_ask(start);
            }
            Input::Back => {
                self.overlay = Overlay::None;
                if let Some(at) = self
                    .actions()
                    .iter()
                    .position(|a| matches!(a, Action::OtherSaves(_)))
                {
                    self.focus = Focus::actions(at);
                }
            }
            _ => {}
        }
        Command::None
    }

    fn handle_conflict(&mut self, server: bool, start: Option<Start>, input: Input) -> Command {
        match input {
            Input::Move(Nav::Left) => {
                self.overlay = Overlay::Conflict {
                    server: false,
                    start,
                }
            }
            Input::Move(Nav::Right) => {
                self.overlay = Overlay::Conflict {
                    server: true,
                    start,
                }
            }
            Input::Activate => {
                let keep = if server { Keep::Server } else { Keep::Local };
                self.conflicts[self.current] = None;
                self.overlay = Overlay::None;
                return match start {
                    Some(start) => Command::Start(Start {
                        keep: Some(keep),
                        ..start
                    }),
                    None => Command::Keep {
                        game: self.current_game(),
                        keep,
                    },
                };
            }
            // Decide later: the conflict stays, and Continue and Play ask again.
            Input::Back => self.overlay = Overlay::None,
            _ => {}
        }
        Command::None
    }

    /// A click or a tap: `area` 0 an action button, 1 a page button, 2 a game in the strip, 3 a
    /// row of Other saves, 4 a side of the conflict prompt, 5 a prompt's confirm button, 6 its
    /// cancel button. Focus moves there first, then it acts as A (or B for 6).
    pub fn click(&mut self, area: i32, index: usize) -> Command {
        match (area, &self.overlay) {
            (0, Overlay::None) if index < self.actions().len() => {
                self.focus = Focus::actions(index);
                self.handle(Input::Activate)
            }
            (1, Overlay::None) if index < Page::ALL.len() => {
                self.focus = Focus::pages(index);
                self.handle(Input::Activate)
            }
            (2, Overlay::None) if index < self.games.len() => {
                self.change_game(index);
                Command::None
            }
            (3, Overlay::OtherSaves { .. }) if index < self.data().saves.len() => {
                self.overlay = Overlay::OtherSaves { index };
                self.handle(Input::Activate)
            }
            (4, Overlay::Conflict { start, .. }) if index < 2 => {
                self.overlay = Overlay::Conflict {
                    server: index == 1,
                    start: *start,
                };
                self.handle(Input::Activate)
            }
            (5, _) => self.handle(Input::Activate),
            (6, _) => self.handle(Input::Back),
            _ => Command::None,
        }
    }

    /// What to draw now.
    pub fn view(&self, clock: &Clock) -> View {
        let data = self.data();
        View {
            games: self.games.iter().map(|g| g.game.name()).collect(),
            current: self.current,
            card: self.card_view(clock),
            actions: self.actions().iter().map(|a| a.label()).collect(),
            action_hints: self
                .actions()
                .iter()
                .enumerate()
                .map(|(i, a)| match a {
                    _ if i == 0 => "A",
                    Action::OtherSaves(_) => "Y",
                    _ => "",
                })
                .collect(),
            pages: Page::ALL.iter().map(|p| p.label()).collect(),
            focus: self.focus,
            status: self.status.clone(),
            overlay: match &self.overlay {
                Overlay::None => OverlayView::None,
                Overlay::Quit => OverlayView::Quit,
                Overlay::Warning { .. } => {
                    OverlayView::Warning(data.warning.clone().unwrap_or_default())
                }
                Overlay::Running { text } => OverlayView::Running(text.clone()),
                Overlay::OtherSaves { index } => OverlayView::OtherSaves {
                    title: format!("Other saves: {}", data.game.name()),
                    rows: self.rows(clock),
                    index: *index,
                },
                Overlay::Conflict { server, .. } => match &self.conflicts[self.current] {
                    Some(pair) => OverlayView::Conflict {
                        title: format!("Two saves of {}. Keep which one?", data.game.name()),
                        sides: [
                            side_view(
                                &pair.local,
                                "This device",
                                !pair.server_is_newer
                                    && pair.local.saved_unix > pair.server.saved_unix,
                                data.selected_slot,
                                clock,
                            ),
                            side_view(
                                &pair.server,
                                "Synced",
                                pair.server_is_newer,
                                data.selected_slot,
                                clock,
                            ),
                        ],
                        server: *server,
                    },
                    None => OverlayView::None,
                },
            },
        }
    }

    fn card_view(&self, clock: &Clock) -> CardView {
        let data = self.data();
        let name = data.game.name();
        let Some(newest) = data.newest() else {
            return CardView {
                title: name.to_owned(),
                lines: vec!["No save yet".into()],
                picture: data.fallback_picture.clone(),
                picture_label: name.to_owned(),
            };
        };
        let save = &newest.card;
        let mut lines = Vec::new();
        if self.conflicted() {
            lines.push("Two saves: pick one to play".to_owned());
        }
        lines.extend(save.detail.clone());
        lines.push(card::saved_line(save.saved_unix, clock));
        lines.extend(save.device.as_deref().map(card::device_line));
        lines.extend(save.play_time_secs.map(card::play_time_line));
        let (picture, picture_label) = match &save.picture {
            Some(bytes) => (Some(bytes.clone()), String::new()),
            None => (data.fallback_picture.clone(), name.to_owned()),
        };
        CardView {
            title: save.title.clone().unwrap_or_else(|| name.to_owned()),
            lines,
            picture,
            picture_label,
        }
    }

    fn rows(&self, clock: &Clock) -> Vec<RowView> {
        let saves = &self.data().saves;
        saves
            .iter()
            .map(|entry| {
                let shared = saves.iter().filter(|s| s.slot == entry.slot).count() > 1;
                let save = &entry.card;
                RowView {
                    picture: save.picture.clone(),
                    title: save
                        .title
                        .clone()
                        .unwrap_or_else(|| save.game.name().to_owned()),
                    detail: save.detail.clone().unwrap_or_default(),
                    when: card::when(save.saved_unix, clock)
                        .unwrap_or_else(|| "time not known".into()),
                    device: save.device.clone().unwrap_or_default(),
                    slot: format!("Slot {}", entry.slot),
                    note: if shared {
                        "In the game: Load".into()
                    } else {
                        String::new()
                    },
                }
            })
            .collect()
    }
}

/// One side of the conflict prompt.
fn side_view(save: &SaveCard, heading: &str, newer: bool, slot: u8, clock: &Clock) -> SideView {
    let mut lines = Vec::new();
    lines.extend(save.detail.clone());
    let mut when = card::saved_line(save.saved_unix, clock);
    if let Some(device) = &save.device {
        when = format!("{when}, {}", card::device_line(device));
    }
    lines.push(when);
    lines.extend(save.play_time_secs.map(card::play_time_line));
    lines.push(format!(
        "Slot {slot}, {}",
        save.build_id
            .as_deref()
            .map_or_else(|| "Build not recorded".to_owned(), |b| format!("build {b}"))
    ));
    SideView {
        heading: heading.to_owned(),
        newer,
        picture: save.picture.clone(),
        title: save
            .title
            .clone()
            .unwrap_or_else(|| save.game.name().to_owned()),
        lines,
    }
}

/// Everything the window draws, as plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// The game strip.
    pub games: Vec<&'static str>,
    pub current: usize,
    pub card: CardView,
    pub actions: Vec<String>,
    /// The controller button shown on each action: A on the default one, Y on Other saves.
    pub action_hints: Vec<&'static str>,
    pub pages: Vec<&'static str>,
    pub focus: Focus,
    pub status: String,
    pub overlay: OverlayView,
}

/// The save card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardView {
    pub title: String,
    /// The lines under the title, each only when its field is known.
    pub lines: Vec<String>,
    /// PNG bytes: the save's picture, else the fallback; `None` draws a plain panel.
    pub picture: Option<Vec<u8>>,
    /// The game's name over the fallback picture or the plain panel; empty over a save's own.
    pub picture_label: String,
}

/// A row of Other saves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    pub picture: Option<Vec<u8>>,
    pub title: String,
    pub detail: String,
    pub when: String,
    pub device: String,
    pub slot: String,
    /// "In the game: Load" when two saves share the slot.
    pub note: String,
}

/// A side of the conflict prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideView {
    pub heading: String,
    pub newer: bool,
    pub picture: Option<Vec<u8>>,
    pub title: String,
    pub lines: Vec<String>,
}

/// The prompt over the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayView {
    None,
    Quit,
    OtherSaves {
        title: String,
        rows: Vec<RowView>,
        index: usize,
    },
    /// `sides[0]` is this device, `sides[1]` the synced save; `server` is the focused side.
    Conflict {
        title: String,
        sides: [SideView; 2],
        server: bool,
    },
    Warning(String),
    Running(String),
}

impl OverlayView {
    /// The number the `.slint` file compares with `overlay`.
    pub fn code(&self) -> i32 {
        match self {
            OverlayView::None => 0,
            OverlayView::Quit => 1,
            OverlayView::OtherSaves { .. } => 2,
            OverlayView::Conflict { .. } => 3,
            OverlayView::Warning(_) => 4,
            OverlayView::Running(_) => 5,
        }
    }
}

#[cfg(test)]
mod tests;
