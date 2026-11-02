//! Trophy data read for the runtime: the game's `.trp` files, names and grades, and the unlock
//! log. Reading only; showing trophies in the launcher is after 1.0 (SPEC.md Won'ts). Replaces
//! `scripts/trophies.py` (including its AES-128 and the `.trp` reader). The title key it
//! derives is a console secret and is never written into this repo or a log (M9): piece 1.13
//! decides how it is derived at run time.

use crate::error::{not_built, Result};
use crate::games::Game;
use std::path::Path;

/// The trophy file and the NP communication id of a game: `GAMES` in `trophies.py`. The id is
/// looked for in `npbind.dat` first, then here.
pub fn trp_file(game: Game) -> (&'static str, &'static str) {
    match game {
        Game::U1 => ("trophy01.trp", "NPWR09798_00"),
        Game::U2 => ("trophy02.trp", "NPWR09799_00"),
        Game::U3 => ("trophy03.trp", "NPWR09801_00"),
    }
}

/// A trophy's grade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    Platinum,
    Gold,
    Silver,
    Bronze,
}

impl Grade {
    /// "Platinum", "Gold", "Silver", "Bronze" (`GRADE_NAMES`).
    pub fn name(self) -> &'static str {
        match self {
            Grade::Platinum => "Platinum",
            Grade::Gold => "Gold",
            Grade::Silver => "Silver",
            Grade::Bronze => "Bronze",
        }
    }
}

/// One trophy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trophy {
    pub id: u32,
    pub grade: Grade,
    pub hidden: bool,
    pub name: String,
    pub detail: String,
}

/// A game's trophy set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrophySet {
    pub npcommid: String,
    pub trophies: Vec<Trophy>,
}

/// Unlocked and total, as the progress line shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub unlocked: u32,
    pub total: u32,
}

/// Load a game's set from its `.trp` file in `game_dir`, in language `lang` (1 = English).
/// A damaged or undecryptable file is `Error::Trophies(..)`.
pub fn load(game_dir: &Path, game: Game, lang: u32) -> Result<TrophySet> {
    not_built("trophies::load")
}

/// The trophy ids unlocked so far, from the runtime's unlock log under `user_dir`.
pub fn read_unlocks(user_dir: &Path, npcommid: &str) -> Result<Vec<u32>> {
    not_built("trophies::read_unlocks")
}

/// Unlocked over total for `set`.
pub fn progress(set: &TrophySet, unlocked: &[u32]) -> Progress {
    not_built!("trophies::progress")
}
