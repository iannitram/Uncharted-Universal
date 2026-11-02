//! The save card the Continue screen shows (SPEC.md M10): one save's picture, title and detail,
//! when it was saved, the device and the build that made it. Read-only: nothing here writes a
//! save. A field the files do not give is `None`, never an error, so the card always shows
//! (with the game's own `icon0.png` as the fallback picture). New module, no Python
//! counterpart; the shape is copied from iannitram/totk-native's `savecard.rs` (MIT).

use crate::error::{not_built, Result};
use crate::games::Game;
use std::path::Path;

/// One save, as the Continue screen and the conflict prompt show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveCard {
    pub game: Game,
    /// The save's entry name ("SAVEDATA01-..."), as found on disk.
    pub slot: String,
    /// The save's picture: its `sce_sys/icon0.png`, bytes as stored.
    pub picture: Option<Vec<u8>>,
    /// `TITLE` of the save's `sce_sys/param.sfo`.
    pub title: Option<String>,
    /// The save's detail line from the same file.
    pub detail: Option<String>,
    /// Seconds since the Unix epoch of the newest file in the save.
    pub saved_unix: u64,
    /// Play time in seconds, when the files give it.
    pub play_time_secs: Option<u64>,
    /// The device that made the save (filled in by the launcher from the fleet records).
    pub device: Option<String>,
    /// The build that made it (`bundle::Bundle::build_id`), same source.
    pub build_id: Option<String>,
}

/// Both sides of a sync conflict, for the side-by-side prompt (M10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideBySide {
    pub server: SaveCard,
    pub local: SaveCard,
    /// Which one is newer: true when the server's `saved_unix` is larger.
    pub server_is_newer: bool,
}

/// The card for one save folder of `game`. No `param.sfo` and no picture still gives a card;
/// an unreadable folder is `Error::Saves("no save at <path>")`.
pub fn read_card(game: Game, save_dir: &Path) -> Result<SaveCard> {
    not_built("savecard::read_card")
}

/// The newest card among the saves in a game's save root, the largest `saved_unix` winning, the
/// lower name on a tie. `Ok(None)` when there is no save.
pub fn latest(game: Game, save_root: &Path) -> Result<Option<SaveCard>> {
    not_built("savecard::latest")
}

/// The pair the conflict prompt shows.
pub fn side_by_side(server: SaveCard, local: SaveCard) -> SideBySide {
    not_built!("savecard::side_by_side")
}
