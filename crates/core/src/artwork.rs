//! Steam library paths and deterministic collection artwork output.
//!
//! The five `collection-artwork` assets are the fixed raster references emitted by the former
//! Pillow generator. Keeping those exact outputs avoids changing the established artwork while
//! packaging no longer needs Python or a network service to produce them.

use crate::error::{not_built, Result};
use crate::games::Game;
use std::path::{Path, PathBuf};

const COLLECTION_ART: [(&str, &[u8]); 5] = [
    ("u1.jpg", include_bytes!("../data/collection-artwork/u1.jpg")),
    ("u2.jpg", include_bytes!("../data/collection-artwork/u2.jpg")),
    ("u3.jpg", include_bytes!("../data/collection-artwork/u3.jpg")),
    (
        "collection_banner.jpg",
        include_bytes!("../data/collection-artwork/collection_banner.jpg"),
    ),
    (
        "emblem.png",
        include_bytes!("../data/collection-artwork/emblem.png"),
    ),
];

/// Write the original collection artwork without a runtime Python dependency.
///
/// The fixed output assets preserve the prior generator's composition exactly and make repeated
/// builds byte-for-byte deterministic.
pub fn generate_collection_art(out_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    for (name, bytes) in COLLECTION_ART {
        std::fs::write(out_dir.join(name), bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod collection_art_tests {
    use super::generate_collection_art;
    use image::GenericImageView;

    #[test]
    fn generates_all_five_artworks_deterministically() {
        let dir = std::env::temp_dir().join(format!("uncharted-art-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        generate_collection_art(&dir).unwrap();
        let first: Vec<_> = [
            "u1.jpg",
            "u2.jpg",
            "u3.jpg",
            "collection_banner.jpg",
            "emblem.png",
        ]
        .map(|name| std::fs::read(dir.join(name)).unwrap())
        .into();
        generate_collection_art(&dir).unwrap();
        let second: Vec<_> = [
            "u1.jpg",
            "u2.jpg",
            "u3.jpg",
            "collection_banner.jpg",
            "emblem.png",
        ]
        .map(|name| std::fs::read(dir.join(name)).unwrap())
        .into();
        assert_eq!(first, second);
        assert_eq!(
            image::open(dir.join("u1.jpg")).unwrap().dimensions(),
            (620, 660)
        );
        assert_eq!(
            image::open(dir.join("u2.jpg")).unwrap().dimensions(),
            (620, 660)
        );
        assert_eq!(
            image::open(dir.join("u3.jpg")).unwrap().dimensions(),
            (620, 660)
        );
        assert_eq!(
            image::open(dir.join("collection_banner.jpg"))
                .unwrap()
                .dimensions(),
            (1920, 540)
        );
        assert_eq!(
            image::open(dir.join("emblem.png")).unwrap().dimensions(),
            (256, 256)
        );
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "collection_banner.jpg",
                "emblem.png",
                "u1.jpg",
                "u2.jpg",
                "u3.jpg"
            ]
        );
        let emblem = image::open(dir.join("emblem.png")).unwrap().to_rgba8();
        assert_eq!(emblem.dimensions(), (256, 256));
        assert_eq!(emblem.get_pixel(0, 0).0[3], 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}


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
