//! The three games of the collection, and which executable revisions the port supports.
//! Replaces `NDC_IDS` / `NDC_ELFS` in `scripts/patches.py` and `EXECUTABLES` in `run.py`.
//!
//! NOT KNOWN from the Python: which of the four title ids is which region or edition. Every
//! script treats the four as one family ("all three games share these title IDs",
//! `scripts/patches.py`), and the three games are told apart by executable name, not by title
//! id. `title_region` is therefore a stub; the dump check (piece 1.05) fills it from real
//! `param.sfo` files.

use crate::error::{not_built, Result};

/// One of the collection's games.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Game {
    /// Uncharted: Drake's Fortune.
    U1,
    /// Uncharted 2: Among Thieves.
    U2,
    /// Uncharted 3: Drake's Deception.
    U3,
}

/// The collection's title ids: `NDC_IDS` in `scripts/patches.py`. All three games ship under
/// each of them.
pub const TITLE_IDS: [&str; 4] = ["CUSA02320", "CUSA02343", "CUSA02344", "CUSA02826"];

impl Game {
    /// Every game, in order.
    pub const ALL: [Game; 3] = [Game::U1, Game::U2, Game::U3];

    /// "u1", "u2", "u3": the keys of `EXECUTABLES` in `run.py` and of `BB_*_U1` variables.
    pub fn key(self) -> &'static str {
        match self {
            Game::U1 => "u1",
            Game::U2 => "u2",
            Game::U3 => "u3",
        }
    }

    /// The inverse of [`Game::key`], case-insensitive.
    pub fn from_key(key: &str) -> Option<Game> {
        Game::ALL
            .into_iter()
            .find(|g| g.key().eq_ignore_ascii_case(key))
    }

    /// "Uncharted 1", "Uncharted 2", "Uncharted 3": the folder names of `SAVE_FOLDERS` in `run.py`.
    pub fn name(self) -> &'static str {
        match self {
            Game::U1 => "Uncharted 1",
            Game::U2 => "Uncharted 2",
            Game::U3 => "Uncharted 3",
        }
    }

    /// The executable in the dump's game folder (`EXECUTABLES` in `run.py`, `NDC_ELFS` in
    /// `scripts/patches.py`, which is also the patch files' `AppElf`).
    pub fn executable(self) -> &'static str {
        match self {
            Game::U1 => "eboot.bin",
            Game::U2 => "big2-ps4_Shipping.elf",
            Game::U3 => "big3-ps4_Shipping.elf",
        }
    }

    /// The game an executable file name belongs to (case-sensitive, as the dump has it).
    pub fn from_executable(name: &str) -> Option<Game> {
        Game::ALL.into_iter().find(|g| g.executable() == name)
    }
}

/// True for one of [`TITLE_IDS`].
pub fn is_collection_title_id(title_id: &str) -> bool {
    TITLE_IDS.contains(&title_id)
}

/// The `APP_VER` values the port supports per game, newest last: the code map's `SUPPORTED`.
/// EMPTY until the device runs fill it (docs/code-map.md, "Not verified yet").
/// `scripts/cheats.py` carries executable hashes for 01.00 and 01.02 of all three games and
/// README.md says 1.00, but neither says the port plays both, so nothing is claimed here.
pub const SUPPORTED: [(Game, &[&str]); 3] = [(Game::U1, &[]), (Game::U2, &[]), (Game::U3, &[])];

/// The `APP_VER` values in [`SUPPORTED`] for `game`.
pub fn supported(game: Game) -> &'static [&'static str] {
    SUPPORTED
        .iter()
        .find(|(g, _)| *g == game)
        .map_or(&[], |(_, vers)| *vers)
}

/// The region or edition a collection title id stands for (the code map's "title ids to game
/// and region"); the value is the region code as `param.sfo` and the dump spell it. Which id
/// is which is not known yet (see the module docs), so this is a stub; `Error::BadDump` for an
/// id outside [`TITLE_IDS`].
pub fn title_region(title_id: &str) -> Result<&'static str> {
    not_built("games::title_region")
}

/// The revision gate (SPEC.md M11): `Ok` when `app_ver` is in [`supported`] for `game`,
/// else `Error::WrongRevision` with the plain refusal.
pub fn check_revision(game: Game, app_ver: &str) -> Result<()> {
    not_built("games::check_revision")
}
