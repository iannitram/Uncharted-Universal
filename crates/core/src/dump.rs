//! The early check on the player's dump, before any prepare (SPEC.md M11; issue #172 folds in):
//! folder layout, whether it is decrypted, which games and revisions it holds. Only file names
//! and `param.sfo` are read; nothing is copied, uploaded or logged (M9). Replaces the checks
//! scattered through `run.py` (`locate`, `find_executable`) and `launcher/` start-up code.

use crate::error::{not_built, Result};
use crate::games::Game;
use std::path::{Path, PathBuf};

/// One game found in a dump folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundGame {
    pub game: Game,
    /// The folder holding that game's executable (a collection dump has all three in one folder;
    /// a game dumped on its own has its own: `BB_GAME_DIR_U1` .. `_U3` in `run.py`).
    pub folder: PathBuf,
    /// `APP_VER` from `sce_sys/param.sfo`, if readable.
    pub app_ver: Option<String>,
}

/// What `check` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DumpReport {
    /// `TITLE_ID` from `param.sfo` ("CUSA02320"), if readable.
    pub title_id: Option<String>,
    pub games: Vec<FoundGame>,
    /// False when an executable is still an encrypted container (a `.pkg` from PSN, or an
    /// encrypted SELF) instead of a decrypted dump.
    pub decrypted: bool,
    /// Plain sentences for the first-run page, in order; empty when all is well.
    pub problems: Vec<String>,
}

/// Check a dump folder. A folder with no executable of any game is `Error::BadDump(..)`; a
/// revision the port does not support is listed in `problems` and left to
/// `games::check_revision` to refuse at prepare.
pub fn check(folder: &Path) -> Result<DumpReport> {
    not_built("dump::check")
}
