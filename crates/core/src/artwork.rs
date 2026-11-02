//! The Steam library images for our shortcut (SPEC.md M13): their file names under
//! `userdata/<user>/config/grid/` and their sizes, and where each is cut from. The images come
//! from each game's own `sce_sys/pic0.png`, `pic1.png` and `icon0.png` on the hub; nothing is
//! downloaded and nothing is kept in this repo (SPEC.md M9, Won'ts). Replaces
//! `launcher/uncharted_art.py` (`steam_artwork`, `cover`, `_framed`, `_fill`); its SteamGridDB
//! download (`steamgriddb_artwork`) is dropped.
//!
//! The file names and the logo and icon sizes follow iannitram/totk-native's `artwork.rs`
//! (MIT); grid, wide grid and hero sizes are those of `steam_artwork` in `uncharted_art.py`.

use crate::error::{not_built, Result};
use crate::games::Game;
use std::path::{Path, PathBuf};

/// One of the images Steam shows for a non-Steam game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Art {
    /// The portrait capsule in the library grid.
    Grid,
    /// The wide capsule (recent games).
    WideGrid,
    /// The banner across the top of the game's page.
    Hero,
    /// The logo drawn over the hero.
    Logo,
    /// The small icon.
    Icon,
}

/// Every `Art`, in the order above.
pub const ALL: [Art; 5] = [Art::Grid, Art::WideGrid, Art::Hero, Art::Logo, Art::Icon];

/// The file name Steam reads for `art` of the shortcut `app_id` (decimal): Grid `<id>p.png`,
/// WideGrid `<id>.png`, Hero `<id>_hero.png`, Logo `<id>_logo.png`, Icon `<id>_icon.png`.
pub fn file_name(art: Art, app_id: u32) -> String {
    match art {
        Art::Grid => format!("{app_id}p.png"),
        Art::WideGrid => format!("{app_id}.png"),
        Art::Hero => format!("{app_id}_hero.png"),
        Art::Logo => format!("{app_id}_logo.png"),
        Art::Icon => format!("{app_id}_icon.png"),
    }
}

/// The size in pixels (width, height) we make `art` at: Grid 600x900, WideGrid 920x430,
/// Hero 1920x620, Logo 1280x720, Icon 256x256.
pub fn size(art: Art) -> (u32, u32) {
    match art {
        Art::Grid => (600, 900),
        Art::WideGrid => (920, 430),
        Art::Hero => (1920, 620),
        Art::Logo => (1280, 720),
        Art::Icon => (256, 256),
    }
}

/// The grid folder of one Steam user folder (`userdata/<id>`): `<user_dir>/config/grid`.
pub fn grid_dir(user_dir: &Path) -> PathBuf {
    user_dir.join("config").join("grid")
}

/// Every image's full path for `app_id` in `user_dir`, in `ALL` order.
pub fn paths(user_dir: &Path, app_id: u32) -> Vec<(Art, PathBuf)> {
    ALL.into_iter()
        .map(|art| (art, grid_dir(user_dir).join(file_name(art, app_id))))
        .collect()
}

/// The file in a game folder an image is cut from, relative with `/`:
/// the hero from `sce_sys/pic1.png` (the wide picture), the others from `sce_sys/pic0.png` and
/// `sce_sys/icon0.png`. Which of pic0 and icon0 serves the grid, logo and icon is decided by the
/// artwork issue (1.22) from the dump's real files; `uncharted_art.py` framed the cover for both
/// capsules and used `pic1` for the hero.
pub fn source_file(art: Art) -> &'static str {
    not_built!("artwork::source_file")
}

/// Cut and size one image for `game` from its `game_dir` (PNG bytes out). A game folder without
/// the source picture is `Ok(None)`: the shortcut then goes without that image. Decoding and
/// encoding PNG without a crate is the issue's decision (a pure-Rust crate may be added).
pub fn render(game_dir: &Path, game: Game, art: Art) -> Result<Option<Vec<u8>>> {
    not_built("artwork::render")
}
