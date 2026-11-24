//! The Trophies (achievements) page (Issue #12): per-game trophy list, grades, icons, unlock
//! timestamps, and progress summary. Plain Rust, no Slint types, so every state is tested without
//! a window.

use crate::card::{self, Clock};
use crate::focus::{FocusRing, Nav};
use crate::load::GameTrophies;
use std::fs;
use uncharted_core::games::Game;

/// What an input or click on the Trophies page asks the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrophiesCommand {
    None,
    Back,
}

/// One trophy row as the UI draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrophyRow {
    pub name: String,
    pub detail: String,
    pub grade: String,
    pub unlocked: bool,
    pub when: String,
    pub icon: Option<Vec<u8>>,
}

/// One game's trophy page state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameTrophiesPage {
    pub game: Game,
    pub title: String,
    pub summary: String,
    pub rows: Vec<TrophyRow>,
    pub empty_text: String,
}

/// View model for the Trophies page.
#[derive(Debug)]
pub struct TrophiesVm {
    pages: Vec<GameTrophiesPage>,
    current: usize,
    focus: FocusRing,
}

impl TrophiesVm {
    /// Build the Trophies page for `games`, opened on `current` game.
    pub fn new(games: Vec<GameTrophies>, current: Game, clock: &Clock) -> Self {
        let current_idx = games.iter().position(|g| g.game == current).unwrap_or(0);
        let pages: Vec<GameTrophiesPage> =
            games.into_iter().map(|g| build_page(g, clock)).collect();
        let count = pages.get(current_idx).map_or(0, |p| p.rows.len());
        Self {
            pages,
            current: current_idx,
            focus: FocusRing::new(count + 1),
        }
    }

    pub fn games(&self) -> Vec<&'static str> {
        self.pages.iter().map(|p| p.game.name()).collect()
    }

    pub fn current(&self) -> usize {
        self.current
    }

    pub fn page(&self) -> &GameTrophiesPage {
        &self.pages[self.current]
    }

    /// Focused item: `0..rows.len()` for a row, `rows.len()` for Back.
    pub fn focused(&self) -> usize {
        self.focus.index()
    }

    pub fn back_index(&self) -> usize {
        self.page().rows.len()
    }

    pub fn select_game(&mut self, index: usize) {
        if index < self.pages.len() && index != self.current {
            self.current = index;
            self.focus = FocusRing::new(self.pages[self.current].rows.len() + 1);
        }
    }

    pub fn prev_game(&mut self) {
        if !self.pages.is_empty() {
            let next = if self.current == 0 {
                self.pages.len() - 1
            } else {
                self.current - 1
            };
            self.select_game(next);
        }
    }

    pub fn next_game(&mut self) {
        if !self.pages.is_empty() {
            let next = (self.current + 1) % self.pages.len();
            self.select_game(next);
        }
    }

    pub fn on_nav(&mut self, nav: Nav) {
        match nav {
            Nav::Up => self.focus.step(Nav::Up),
            Nav::Down => self.focus.step(Nav::Down),
            Nav::Left => self.prev_game(),
            Nav::Right => self.next_game(),
        }
    }

    pub fn activate(&self) -> TrophiesCommand {
        if self.focus.index() == self.back_index() {
            TrophiesCommand::Back
        } else {
            TrophiesCommand::None
        }
    }

    pub fn click(&mut self, row: i32, col: usize) -> TrophiesCommand {
        if row == -3 {
            return TrophiesCommand::Back;
        }
        if row == -1 {
            self.select_game(col);
            return TrophiesCommand::None;
        }
        if let Ok(idx) = usize::try_from(row) {
            if idx < self.page().rows.len() {
                self.focus.set(idx);
            }
        }
        TrophiesCommand::None
    }
}

fn build_page(game: GameTrophies, clock: &Clock) -> GameTrophiesPage {
    let mut rows = Vec::new();
    let (summary, empty_text) = if let Some(set) = &game.set {
        let p = &game.progress;
        let summary = format!(
            "{} / {} unlocked ({}%)  ·  {}P  {}G  {}S  {}B",
            p.unlocked,
            p.total,
            p.percent,
            p.earned.platinum,
            p.earned.gold,
            p.earned.silver,
            p.earned.bronze,
        );
        for trophy in &set.trophies {
            let unlock_time = game.unlocked.get(&trophy.id).copied();
            let unlocked = unlock_time.is_some();
            let (name, detail) = if trophy.hidden && !unlocked {
                ("Hidden trophy".to_owned(), String::new())
            } else {
                (trophy.name.clone(), trophy.detail.clone())
            };
            let when = unlock_time
                .map(|ts| card::when(ts, clock).unwrap_or_else(|| "Unlocked".to_owned()))
                .unwrap_or_else(|| "Locked".to_owned());
            let icon = game
                .trophy_dir
                .as_ref()
                .and_then(|dir| fs::read(dir.join(format!("TROP{:03}.PNG", trophy.id))).ok());
            rows.push(TrophyRow {
                name,
                detail,
                grade: trophy.grade.name().to_owned(),
                unlocked,
                when,
                icon,
            });
        }
        let empty = if rows.is_empty() {
            "No trophies in this set.".to_owned()
        } else {
            String::new()
        };
        (summary, empty)
    } else if !game.unlocked.is_empty() {
        let summary = format!("{} unlocked (trophy pack not cached)", game.unlocked.len());
        for (&id, &ts) in &game.unlocked {
            rows.push(TrophyRow {
                name: format!("Trophy {id}"),
                detail: String::new(),
                grade: String::new(),
                unlocked: true,
                when: card::when(ts, clock).unwrap_or_else(|| "Unlocked".to_owned()),
                icon: None,
            });
        }
        (summary, String::new())
    } else {
        (
            "0 unlocked".to_owned(),
            "No trophy set cached yet. Prepare the game or unlock a trophy in-game.".to_owned(),
        )
    };

    GameTrophiesPage {
        game: game.game,
        title: game.title,
        summary,
        rows,
        empty_text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use uncharted_core::trophies::{self, Grade, Trophy, TrophySet};

    fn clock() -> Clock<'static> {
        Clock {
            now: 1_794_519_000,
            offset: &|_| 0,
        }
    }

    fn sample_set(npcommid: &str) -> TrophySet {
        TrophySet {
            npcommid: npcommid.to_owned(),
            title: "Test Game".to_owned(),
            detail: "Set detail".to_owned(),
            groups: 1,
            trophies: vec![
                Trophy {
                    id: 0,
                    grade: Grade::Platinum,
                    hidden: false,
                    group: String::new(),
                    name: "Platinum".to_owned(),
                    detail: "All trophies".to_owned(),
                },
                Trophy {
                    id: 1,
                    grade: Grade::Bronze,
                    hidden: false,
                    group: String::new(),
                    name: "First Treasure".to_owned(),
                    detail: "Find a treasure".to_owned(),
                },
                Trophy {
                    id: 2,
                    grade: Grade::Gold,
                    hidden: true,
                    group: String::new(),
                    name: "Secret Gold".to_owned(),
                    detail: "Secret detail".to_owned(),
                },
            ],
            icons: BTreeMap::new(),
        }
    }

    #[test]
    fn renders_set_progress_hides_locked_hidden_trophies_and_formats_unlocks() {
        let set = sample_set("NPWR09798_00");
        let unlocked = BTreeMap::from([(1, 1_794_518_040)]);
        let progress = trophies::progress(&set, &unlocked);
        let u1 = GameTrophies {
            game: Game::U1,
            title: set.title.clone(),
            set: Some(set),
            unlocked,
            progress,
            trophy_dir: None,
        };
        let u2 = GameTrophies {
            game: Game::U2,
            title: Game::U2.name().to_owned(),
            set: None,
            unlocked: BTreeMap::from([(5, 1_794_518_040)]),
            progress: trophies::Progress {
                unlocked: 1,
                ..Default::default()
            },
            trophy_dir: None,
        };
        let mut vm = TrophiesVm::new(vec![u1, u2], Game::U1, &clock());
        assert_eq!(vm.current(), 0);
        assert_eq!(vm.page().summary, "1 / 3 unlocked (33%)  ·  0P  0G  0S  1B");
        assert_eq!(vm.page().rows.len(), 3);
        assert!(!vm.page().rows[0].unlocked);
        assert_eq!(vm.page().rows[0].when, "Locked");
        assert!(vm.page().rows[1].unlocked);
        assert_eq!(vm.page().rows[1].name, "First Treasure");
        assert_eq!(vm.page().rows[1].when, "today 21:14");
        // Hidden locked trophy hides name & detail.
        assert_eq!(vm.page().rows[2].name, "Hidden trophy");
        assert_eq!(vm.page().rows[2].detail, "");

        // Switching to U2 shows fallback "Trophy 5" from the unlock log.
        vm.on_nav(Nav::Right);
        assert_eq!(vm.current(), 1);
        assert_eq!(vm.page().rows.len(), 1);
        assert_eq!(vm.page().rows[0].name, "Trophy 5");
        assert_eq!(vm.page().rows[0].when, "today 21:14");

        // Navigating to Back and activating returns Back.
        vm.on_nav(Nav::Down);
        assert_eq!(vm.focused(), vm.back_index());
        assert_eq!(vm.activate(), TrophiesCommand::Back);
        assert_eq!(vm.click(-3, 0), TrophiesCommand::Back);
    }
}
