//! The Saves page (roadmap 2.06): the save slots of each game (`core::saves`), the backups
//! (`core::backup`) and what save-relay syncs. One row per slot, then one row per backup; each
//! row has its own buttons, and the controller moves over them as a [`Grid`]. LB/RB change game
//! as on the Continue screen, B goes back to it.
//!
//! A slot row: "Use this slot" (makes it the active one: what the game loads and save-relay
//! syncs), "Copy to slot n" (into the first free slot; a copy never replaces saves), "Back up"
//! and "Delete" (asks first, and backs the slot up before it deletes). A backup row: "Restore to
//! slot n" (into the first free slot, so a restore never replaces saves either). Nothing here
//! touches a file: `main.rs` carries out each [`SavesCommand`] and reloads.

use crate::card::{self, Clock};
use crate::focus::Grid;
use crate::screen::Input;
use uncharted_core::games::Game;

/// One save slot of a game, as `core::saves` reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotInfo {
    /// 1 is the game's own folder, 2-10 the `Save slots` folders (`saves::slot_dir`).
    pub slot: u8,
    /// The player's name for the slot (`saves::name`), or "".
    pub name: String,
    /// The slot holds a save (`saves::used`).
    pub used: bool,
    /// The game's own description of the save (`saves::details`), or "".
    pub detail: String,
    /// The newest save time in the slot; 0 when not known or empty.
    pub saved_unix: u64,
}

/// What the page knows of one game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSaves {
    pub game: Game,
    /// The slots `saves::slots` lists: 1, every used slot and the selected one.
    pub slots: Vec<SlotInfo>,
    /// The active slot (`saves::selected`).
    pub selected: u8,
    /// The first free slot (`saves::first_free`): where a copy or a restore goes.
    pub free: Option<u8>,
    /// The backup folders of the game (`backup::list`): stamps, oldest first.
    pub backups: Vec<String>,
}

impl GameSaves {
    /// A game with only its empty slot 1 and no backup.
    pub fn empty(game: Game) -> GameSaves {
        GameSaves {
            game,
            slots: vec![SlotInfo {
                slot: 1,
                name: String::new(),
                used: false,
                detail: String::new(),
                saved_unix: 0,
            }],
            selected: 1,
            free: Some(2),
            backups: Vec::new(),
        }
    }
}

/// A button on the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Button {
    /// Make the slot the active one.
    Use(u8),
    Copy {
        from: u8,
        to: u8,
    },
    BackUp(u8),
    Delete(u8),
    Restore {
        stamp: String,
        to: u8,
    },
}

impl Button {
    fn label(&self, selected: u8) -> String {
        match self {
            Button::Use(slot) if *slot == selected => "Active".into(),
            Button::Use(_) => "Use this slot".into(),
            Button::Copy { to, .. } => format!("Copy to slot {to}"),
            Button::BackUp(_) => "Back up".into(),
            Button::Delete(_) => "Delete".into(),
            Button::Restore { to, .. } => format!("Restore to slot {to}"),
        }
    }
}

/// What the app must do after an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavesCommand {
    None,
    /// Back to the Continue screen.
    Back,
    /// `saves::select`.
    Select {
        game: Game,
        slot: u8,
    },
    /// `saves::copy` into an empty slot.
    Copy {
        game: Game,
        from: u8,
        to: u8,
    },
    /// `backup::backup_saves` of the slot's folder.
    BackUp {
        game: Game,
        slot: u8,
    },
    /// Back the slot up, then `saves::delete`.
    Delete {
        game: Game,
        slot: u8,
    },
    /// Copy the backup's files into the empty slot `to`.
    Restore {
        game: Game,
        stamp: String,
        to: u8,
    },
}

/// The name of a backup folder: the UTC time it was made and the slot it holds,
/// "20261112-213000-slot2". Names sort by time, as `backup::list` expects.
pub fn stamp(unix: u64, slot: u8) -> String {
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    let (year, month, day) = card::civil(days);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}-slot{slot}",
        secs / 3_600,
        secs % 3_600 / 60,
        secs % 60
    )
}

/// The time and slot of a [`stamp`]; `None` for a folder named another way.
pub fn stamp_parts(stamp: &str) -> Option<(u64, u8)> {
    let (date, rest) = stamp.split_once('-')?;
    let (time, slot) = rest.split_once("-slot")?;
    if date.len() != 8
        || time.len() != 6
        || !(date.bytes().chain(time.bytes())).all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let num = |s: &str| s.parse::<i64>().ok();
    let (year, month, day) = (num(&date[..4])?, num(&date[4..6])?, num(&date[6..])?);
    let (hour, minute, second) = (num(&time[..2])?, num(&time[2..4])?, num(&time[4..])?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let unix = u64::try_from(days * 86_400 + hour * 3_600 + minute * 60 + second).ok()?;
    Some((unix, slot.parse().ok()?))
}

/// Days since 1970-01-01 of a calendar date (the inverse of `card::civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The Saves page.
#[derive(Debug)]
pub struct SavesVm {
    games: Vec<GameSaves>,
    current: usize,
    focus: Grid,
    /// The slot whose Delete waits for A (confirm) or B (cancel).
    confirm: Option<u8>,
    /// save-relay's state, as the Continue screen's status line says it.
    relay: String,
    status: String,
}

impl SavesVm {
    /// The page for `games` (in `Game::ALL` order), on `game`, focus on the active slot.
    pub fn new(games: Vec<GameSaves>, game: Game, relay: &str) -> SavesVm {
        assert!(!games.is_empty(), "the Saves page needs a game");
        let current = games.iter().position(|g| g.game == game).unwrap_or(0);
        let mut vm = SavesVm {
            games,
            current,
            focus: Grid::default(),
            confirm: None,
            relay: relay.to_owned(),
            status: String::new(),
        };
        vm.focus = vm.default_focus();
        vm
    }

    pub fn current_game(&self) -> Game {
        self.games[self.current].game
    }

    #[cfg(test)]
    pub fn focus(&self) -> Grid {
        self.focus
    }

    pub fn set_status(&mut self, status: &str) {
        status.clone_into(&mut self.status);
    }

    /// New data from disk after a command: the same game stays shown, focus stays on the
    /// nearest button.
    pub fn reload(&mut self, games: Vec<GameSaves>) {
        let game = self.current_game();
        self.games = games;
        self.current = self.games.iter().position(|g| g.game == game).unwrap_or(0);
        self.focus = self.focus.clamp(&self.lens());
    }

    fn data(&self) -> &GameSaves {
        &self.games[self.current]
    }

    /// The Use button of the active slot.
    fn default_focus(&self) -> Grid {
        let data = self.data();
        let row = data
            .slots
            .iter()
            .position(|s| s.slot == data.selected)
            .unwrap_or(0);
        Grid::at(row, 0).clamp(&self.lens())
    }

    /// The buttons of each row: the slot rows, then the backup rows, newest backup first.
    pub fn rows(&self) -> Vec<Vec<Button>> {
        let data = self.data();
        let mut rows: Vec<Vec<Button>> = data
            .slots
            .iter()
            .map(|s| {
                let mut row = vec![Button::Use(s.slot)];
                if s.used {
                    if let Some(to) = data.free {
                        row.push(Button::Copy { from: s.slot, to });
                    }
                    row.push(Button::BackUp(s.slot));
                    row.push(Button::Delete(s.slot));
                }
                row
            })
            .collect();
        rows.extend(data.backups.iter().rev().map(|stamp| match data.free {
            Some(to) => vec![Button::Restore {
                stamp: stamp.clone(),
                to,
            }],
            None => Vec::new(),
        }));
        rows
    }

    fn lens(&self) -> Vec<usize> {
        self.rows().iter().map(Vec::len).collect()
    }

    fn change_game(&mut self, index: usize) {
        self.current = index;
        self.confirm = None;
        self.focus = self.default_focus();
    }

    /// Carry out one input.
    pub fn handle(&mut self, input: Input) -> SavesCommand {
        if let Some(slot) = self.confirm {
            return match input {
                Input::Activate => {
                    self.confirm = None;
                    SavesCommand::Delete {
                        game: self.current_game(),
                        slot,
                    }
                }
                Input::Back => {
                    self.confirm = None;
                    SavesCommand::None
                }
                _ => SavesCommand::None,
            };
        }
        let games = self.games.len();
        match input {
            Input::Move(nav) => self.focus = self.focus.step(nav, &self.lens()),
            Input::Tab { back } => self.focus = self.focus.tab(back, &self.lens()),
            Input::PrevGame => self.change_game((self.current + games - 1) % games),
            Input::NextGame => self.change_game((self.current + 1) % games),
            Input::Back => return SavesCommand::Back,
            Input::OtherSaves => {}
            Input::Activate => {
                let button = self
                    .rows()
                    .get(self.focus.row)
                    .and_then(|row| row.get(self.focus.col))
                    .cloned();
                if let Some(button) = button {
                    return self.activate(button);
                }
            }
        }
        SavesCommand::None
    }

    fn activate(&mut self, button: Button) -> SavesCommand {
        let game = self.current_game();
        match button {
            Button::Use(slot) if slot == self.data().selected => {
                self.status = format!("Slot {slot} is already the active one.");
                SavesCommand::None
            }
            Button::Use(slot) => SavesCommand::Select { game, slot },
            Button::Copy { from, to } => SavesCommand::Copy { game, from, to },
            Button::BackUp(slot) => SavesCommand::BackUp { game, slot },
            Button::Delete(slot) => {
                self.confirm = Some(slot);
                SavesCommand::None
            }
            Button::Restore { stamp, to } => SavesCommand::Restore { game, stamp, to },
        }
    }

    /// A click or a tap on a button: focus moves there, then it acts as A. A click on the
    /// game strip is `row` -1 with the game's index; on the prompt, `row` -2 with 0 (confirm)
    /// or 1 (cancel).
    pub fn click(&mut self, row: i32, col: usize) -> SavesCommand {
        match row {
            -2 if self.confirm.is_some() => self.handle(if col == 0 {
                Input::Activate
            } else {
                Input::Back
            }),
            -1 if self.confirm.is_none() && col < self.games.len() => {
                self.change_game(col);
                SavesCommand::None
            }
            row if row >= 0 && self.confirm.is_none() => {
                let row = row as usize;
                if self.rows().get(row).is_some_and(|r| col < r.len()) {
                    self.focus = Grid::at(row, col);
                    self.handle(Input::Activate)
                } else {
                    SavesCommand::None
                }
            }
            _ => SavesCommand::None,
        }
    }

    /// What to draw now.
    pub fn view(&self, clock: &Clock) -> SavesView {
        let data = self.data();
        let rows = self.rows();
        let labels = |row: &[Button]| row.iter().map(|b| b.label(data.selected)).collect();
        let slots = data
            .slots
            .iter()
            .zip(&rows)
            .map(|(s, row)| SavesRow {
                title: if s.name.is_empty() {
                    format!("Slot {}", s.slot)
                } else {
                    format!("Slot {}: {}", s.slot, s.name)
                },
                detail: if s.used {
                    [s.detail.clone(), card::saved_line(s.saved_unix, clock)]
                        .into_iter()
                        .filter(|t| !t.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ")
                } else {
                    "Empty".into()
                },
                mark: if s.slot == data.selected {
                    "Active".into()
                } else {
                    String::new()
                },
                buttons: labels(row),
            })
            .collect();
        let backups = data
            .backups
            .iter()
            .rev()
            .zip(&rows[data.slots.len()..])
            .map(|(stamp, row)| {
                let (title, detail) = match stamp_parts(stamp) {
                    Some((unix, slot)) => (
                        format!("Backup of slot {slot}"),
                        card::when(unix, clock).map_or_else(String::new, |w| format!("Made {w}")),
                    ),
                    None => (stamp.clone(), String::new()),
                };
                SavesRow {
                    title,
                    detail: if row.is_empty() {
                        [detail, "No free slot to restore to".into()]
                            .into_iter()
                            .filter(|t| !t.is_empty())
                            .collect::<Vec<_>>()
                            .join(" · ")
                    } else {
                        detail
                    },
                    mark: String::new(),
                    buttons: labels(row),
                }
            })
            .collect();
        SavesView {
            games: self.games.iter().map(|g| g.game.name()).collect(),
            current: self.current,
            slots,
            backups,
            focus: self.focus,
            relay: format!(
                "save-relay syncs the active slot of {}: Slot {}. {}",
                data.game.name(),
                data.selected,
                self.relay
            ),
            status: self.status.clone(),
            confirm: self.confirm.map(|slot| {
                format!(
                    "Delete the saves in slot {slot} of {}? A backup is made first.",
                    data.game.name()
                )
            }),
        }
    }
}

/// Everything the window draws on the Saves page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavesView {
    pub games: Vec<&'static str>,
    pub current: usize,
    pub slots: Vec<SavesRow>,
    /// Newest first; their focus rows follow the slots'.
    pub backups: Vec<SavesRow>,
    pub focus: Grid,
    /// What save-relay syncs, and its state.
    pub relay: String,
    pub status: String,
    /// The delete prompt's question, when it is open.
    pub confirm: Option<String>,
}

/// One row of the Saves page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavesRow {
    pub title: String,
    pub detail: String,
    /// "Active" on the active slot.
    pub mark: String,
    pub buttons: Vec<String>,
}

#[cfg(test)]
mod tests;
