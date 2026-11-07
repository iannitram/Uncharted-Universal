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

/// Write the original launcher collection artwork to `out_dir`.
///
/// The four cover images are the checked-in output of the original generator in
/// `packaging/windows/make_uncharted_art.py` (GPL-2.0-or-later, repository copyright).
/// Keeping these originals as the source of truth preserves every pixel and avoids depending
/// on Pillow's implementation-specific noise, blur and JPEG encoder behavior. The compass is
/// rasterized below in Rust. No game data or network access is involved.
pub fn generate_collection_art(out_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    for (name, bytes) in [
        (
            "u1.jpg",
            include_bytes!("../../../launcher/covers/u1.jpg").as_slice(),
        ),
        (
            "u2.jpg",
            include_bytes!("../../../launcher/covers/u2.jpg").as_slice(),
        ),
        (
            "u3.jpg",
            include_bytes!("../../../launcher/covers/u3.jpg").as_slice(),
        ),
        (
            "collection_banner.jpg",
            include_bytes!("../../../launcher/covers/collection_banner.jpg").as_slice(),
        ),
    ] {
        std::fs::write(out_dir.join(name), bytes)?;
    }
    render_emblem()
        .save(out_dir.join("emblem.png"))
        .map_err(|e| std::io::Error::other(e.to_string()))
}

#[cfg(test)]
mod collection_art_tests {
    use super::generate_collection_art;

    #[test]
    fn writes_exact_cover_sources_and_transparent_compass() {
        let dir = std::env::temp_dir().join(format!("uncharted-art-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        generate_collection_art(&dir).unwrap();
        for (name, source) in [
            (
                "u1.jpg",
                include_bytes!("../../../launcher/covers/u1.jpg").as_slice(),
            ),
            (
                "u2.jpg",
                include_bytes!("../../../launcher/covers/u2.jpg").as_slice(),
            ),
            (
                "u3.jpg",
                include_bytes!("../../../launcher/covers/u3.jpg").as_slice(),
            ),
            (
                "collection_banner.jpg",
                include_bytes!("../../../launcher/covers/collection_banner.jpg").as_slice(),
            ),
        ] {
            assert_eq!(std::fs::read(dir.join(name)).unwrap(), source);
        }
        let emblem = image::open(dir.join("emblem.png")).unwrap().to_rgba8();
        assert_eq!(emblem.dimensions(), (256, 256));
        assert_eq!(emblem.get_pixel(0, 0).0[3], 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn render_emblem() -> image::RgbaImage {
    use image::{Rgba, RgbaImage};
    use imageproc::drawing::{draw_filled_circle_mut, draw_polygon_mut};
    use imageproc::point::Point;
    let side = 1024i32; // 4x supersampling, matching the Python source.
    let c = side as f32 / 2.0;
    let r = side as f32 * 0.46;
    let brass = Rgba([201, 148, 74, 255]);
    let dark = Rgba([91, 61, 26, 255]);
    let light = Rgba([244, 212, 143, 255]);
    let mut im = RgbaImage::new(side as u32, side as u32);
    let at = |deg: f32, radius: f32| {
        let a = (deg - 90.0).to_radians();
        (c + radius * a.cos(), c + radius * a.sin())
    };
    draw_filled_circle_mut(&mut im, (c as i32, c as i32), r as i32, dark);
    draw_filled_circle_mut(
        &mut im,
        (c as i32, c as i32),
        (r * 0.94) as i32,
        Rgba([26, 18, 10, 255]),
    );
    for (radius, color, width) in [
        (r * 0.86, brass, 8),
        (r * 0.8, Rgba([201, 148, 74, 200]), 3),
    ] {
        let n = 2048;
        for i in 0..n {
            let a = std::f32::consts::TAU * i as f32 / n as f32;
            let b = std::f32::consts::TAU * (i + 1) as f32 / n as f32;
            thick_line(
                &mut im,
                (c + radius * a.cos(), c + radius * a.sin()),
                (c + radius * b.cos(), c + radius * b.sin()),
                color,
                width,
            );
        }
    }
    for k in 0..72 {
        let inner = r * if k % 6 == 0 { 0.74 } else { 0.77 };
        let (x0, y0) = at((k * 5) as f32, inner);
        let (x1, y1) = at((k * 5) as f32, r * 0.8);
        thick_line(&mut im, (x0, y0), (x1, y1), brass, 4);
    }
    for k in 0..8 {
        let a = k as f32 * 45.0;
        let length = if k % 2 == 0 { 0.72 } else { 0.5 };
        let width = if k % 2 == 0 { 0.1 } else { 0.08 };
        let tip = at(a, r * length);
        let left = at(a - 90.0, r * width);
        let right = at(a + 90.0, r * width);
        let p = |v: (f32, f32)| Point::new(v.0.round() as i32, v.1.round() as i32);
        draw_polygon_mut(
            &mut im,
            &[p(tip), p(left), Point::new(c as i32, c as i32)],
            if k % 2 == 0 { light } else { brass },
        );
        draw_polygon_mut(
            &mut im,
            &[p(tip), p(right), Point::new(c as i32, c as i32)],
            if k % 2 == 0 { brass } else { dark },
        );
    }
    draw_filled_circle_mut(&mut im, (c as i32, c as i32), (r * 0.07) as i32, light);
    let n = at(0.0, r * 0.93);
    let p = |x: f32, y: f32| Point::new(x.round() as i32, y.round() as i32);
    draw_polygon_mut(
        &mut im,
        &[
            p(n.0, n.1 - r * 0.06),
            p(n.0 - r * 0.05, n.1 + r * 0.05),
            p(n.0 + r * 0.05, n.1 + r * 0.05),
        ],
        Rgba([179, 54, 42, 255]),
    );
    image::imageops::resize(&im, 256, 256, image::imageops::FilterType::Lanczos3)
}

fn thick_line(
    image: &mut image::RgbaImage,
    a: (f32, f32),
    b: (f32, f32),
    color: image::Rgba<u8>,
    width: i32,
) {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let length = (dx * dx + dy * dy).sqrt().max(1.0);
    let (nx, ny) = (-dy / length, dx / length);
    for offset in -(width / 2)..=(width / 2) {
        let offset = offset as f32;
        imageproc::drawing::draw_line_segment_mut(
            image,
            (a.0 + nx * offset, a.1 + ny * offset),
            (b.0 + nx * offset, b.1 + ny * offset),
            color,
        );
    }
}

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
