//! Steam library paths, the library images cut from the player's game folder, and
//! deterministic collection artwork output. Replaces `steam_artwork`, `cover` and `crop_image`
//! of `launcher/uncharted_art.py`; the SteamGridDB download is dropped (core makes no network
//! calls).
//!
//! The five `collection-artwork` assets are the fixed raster references emitted by the former
//! Pillow generator. Keeping those exact outputs avoids changing the established artwork while
//! packaging no longer needs Python or a network service to produce them.

use crate::error::{Error, Result};
use crate::games::Game;
use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageFormat, RgbImage, Rgba, RgbaImage};
use std::path::{Path, PathBuf};

const COLLECTION_ART: [(&str, &[u8]); 5] = [
    (
        "u1.jpg",
        include_bytes!("../data/collection-artwork/u1.jpg"),
    ),
    (
        "u2.jpg",
        include_bytes!("../data/collection-artwork/u2.jpg"),
    ),
    (
        "u3.jpg",
        include_bytes!("../data/collection-artwork/u3.jpg"),
    ),
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

/// The file in a game folder an image is cut from, relative with `/`.
///
/// Decided from a real dump's `sce_sys` (CUSA02320): `icon0.png` is the collection's 512x512
/// box art, the only square picture and the only one with the title on it, so it feeds the
/// grid, the wide grid, the logo and the icon. `pic1.png` (1920x1080, the wide picture) feeds
/// the hero, as in `uncharted_art.py`. `pic0.png` (also 1920x1080, the start screen) feeds
/// nothing: it has the hero's shape and adds no portrait or square picture. The three games
/// share the dump's `sce_sys`, so the source is the same for each.
pub fn source_file(art: Art) -> &'static str {
    match art {
        Art::Hero => "sce_sys/pic1.png",
        Art::Grid | Art::WideGrid | Art::Logo | Art::Icon => "sce_sys/icon0.png",
    }
}

/// Cut and size one image for `game` from its `game_dir` (PNG bytes out, `size(art)`), the
/// framing of `steam_artwork` in `uncharted_art.py`:
///
/// - Grid and WideGrid: the box art whole and centred on a blurred, darkened copy of itself
///   (`_framed`).
/// - Hero: `pic1.png` scaled to cover the banner, centred (`_fill`); without `pic1.png` the
///   framed box art, as the Python did.
/// - Icon: the box art scaled to cover 256x256.
/// - Logo (the Python made none): the box art whole and centred on a transparent canvas, so the
///   hero shows around it.
///
/// A game folder without the source picture is `Ok(None)`: the shortcut then goes without that
/// image. A picture that is not a readable PNG or JPEG counts as missing, as `cover()` treated
/// it. Every game of the collection gets the same images (one shared `sce_sys`).
pub fn render(game_dir: &Path, _game: Game, art: Art) -> Result<Option<Vec<u8>>> {
    let (w, h) = size(art);
    let image = match art {
        Art::Hero => match load(&game_dir.join(source_file(Art::Hero)))? {
            Some(wide) => DynamicImage::ImageRgb8(fill(&wide, w, h)),
            None => match load(&game_dir.join(source_file(Art::Grid)))? {
                Some(box_art) => DynamicImage::ImageRgb8(framed(&box_art, w, h)),
                None => return Ok(None),
            },
        },
        _ => {
            let Some(box_art) = load(&game_dir.join(source_file(art)))? else {
                return Ok(None);
            };
            match art {
                Art::Logo => DynamicImage::ImageRgba8(fit_transparent(&box_art, w, h)),
                Art::Icon => DynamicImage::ImageRgb8(fill(&box_art, w, h)),
                _ => DynamicImage::ImageRgb8(framed(&box_art, w, h)),
            }
        }
    };
    encode(&image).map(Some)
}

/// The artwork editor's saved framing for a picture the player chose (`crop_image` in
/// `uncharted_art.py`): `zoom` 1 to 3 over the size that just covers the image, and the focal
/// point `x`, `y` (0 to 1 across the scaled picture) kept in frame. Values outside are clamped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crop {
    pub zoom: f64,
    pub x: f64,
    pub y: f64,
}

impl Default for Crop {
    /// No zoom, centred: what `crop_image` uses for a missing crop.
    fn default() -> Crop {
        Crop {
            zoom: 1.0,
            x: 0.5,
            y: 0.5,
        }
    }
}

/// One image cut from a picture the player chose with "Change image" (PNG or JPEG), framed by
/// `crop` (PNG bytes out, `size(art)`). As in `steam_artwork`, the picture replaces the grid,
/// the wide grid and the hero, each with its own crop; the logo and the icon are `Ok(None)`
/// (the caller keeps the game folder's). An unreadable picture is `Ok(None)` too: the Python
/// then fell back to the game folder's art.
pub fn render_custom(picture: &Path, art: Art, crop: Crop) -> Result<Option<Vec<u8>>> {
    if matches!(art, Art::Logo | Art::Icon) {
        return Ok(None);
    }
    let Some(image) = load(picture)? else {
        return Ok(None);
    };
    let (w, h) = size(art);
    encode(&DynamicImage::ImageRgb8(crop_to(&image, w, h, crop))).map(Some)
}

/// The picture at `path` as RGB, `None` when it is missing or not a readable PNG or JPEG.
fn load(path: &Path) -> Result<Option<RgbImage>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Io(format!("{}: {e}", path.display()))),
    };
    Ok(image::load_from_memory(&bytes).ok().map(|i| i.to_rgb8()))
}

fn encode(image: &DynamicImage) -> Result<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|e| Error::Io(format!("encoding a Steam image: {e}")))?;
    Ok(out.into_inner())
}

/// `math.ceil(side * scale)`, at least 1.
fn scaled(side: u32, scale: f64) -> u32 {
    ((f64::from(side) * scale).ceil() as u32).max(1)
}

/// `_fill`: scaled to cover `w`x`h` (Lanczos), centred.
fn fill(image: &RgbImage, w: u32, h: u32) -> RgbImage {
    let (iw, ih) = image.dimensions();
    let scale = (f64::from(w) / f64::from(iw)).max(f64::from(h) / f64::from(ih));
    let big = imageops::resize(
        image,
        scaled(iw, scale),
        scaled(ih, scale),
        FilterType::Lanczos3,
    );
    let (bw, bh) = big.dimensions();
    imageops::crop_imm(&big, (bw - w) / 2, (bh - h) / 2, w, h).to_image()
}

/// `_framed`: the square box art centred on a blurred copy of itself, 35% of the way toward
/// (10, 8, 6).
fn framed(art: &RgbImage, w: u32, h: u32) -> RgbImage {
    let radius = w.max(h) as f32 / 30.0;
    let mut background = imageops::fast_blur(&fill(art, w, h), radius);
    let shade = [10.0_f32, 8.0, 6.0];
    for pixel in background.pixels_mut() {
        for (channel, toward) in pixel.0.iter_mut().zip(shade) {
            *channel = (f32::from(*channel) * 0.65 + toward * 0.35) as u8;
        }
    }
    let side = w.min(h);
    let square = imageops::resize(art, side, side, FilterType::Lanczos3);
    imageops::replace(
        &mut background,
        &square,
        i64::from((w - side) / 2),
        i64::from((h - side) / 2),
    );
    background
}

/// The whole picture inside `w`x`h`, centred on a transparent canvas (`_fit` without the
/// background colour).
fn fit_transparent(image: &RgbImage, w: u32, h: u32) -> RgbaImage {
    let (iw, ih) = image.dimensions();
    let scale = (f64::from(w) / f64::from(iw)).min(f64::from(h) / f64::from(ih));
    let side = |s: u32| ((f64::from(s) * scale).round_ties_even() as u32).max(1);
    let small = DynamicImage::ImageRgb8(imageops::resize(
        image,
        side(iw).min(w),
        side(ih).min(h),
        FilterType::Lanczos3,
    ))
    .to_rgba8();
    let mut canvas = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 0]));
    let (sw, sh) = small.dimensions();
    imageops::replace(
        &mut canvas,
        &small,
        i64::from((w - sw) / 2),
        i64::from((h - sh) / 2),
    );
    canvas
}

/// `crop_image`: cover `w`x`h` times the zoom, keeping the focal point in frame.
fn crop_to(image: &RgbImage, w: u32, h: u32, crop: Crop) -> RgbImage {
    let zoom = crop.zoom.clamp(1.0, 3.0);
    let (fx, fy) = (crop.x.clamp(0.0, 1.0), crop.y.clamp(0.0, 1.0));
    let (iw, ih) = image.dimensions();
    let scale = (f64::from(w) / f64::from(iw)).max(f64::from(h) / f64::from(ih)) * zoom;
    let big = imageops::resize(
        image,
        scaled(iw, scale),
        scaled(ih, scale),
        FilterType::Lanczos3,
    );
    let (bw, bh) = big.dimensions();
    // Python's round() takes halves to even; then max(0, min(limit, v)).
    let place = |focus: f64, side: u32, out: u32| {
        let v = (focus * f64::from(side) - f64::from(out) / 2.0).round_ties_even();
        v.clamp(0.0, f64::from(side - out)) as u32
    };
    imageops::crop_imm(&big, place(fx, bw, w), place(fy, bh, h), w, h).to_image()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, Rgb};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir()
                .join(format!("uncharted-artwork-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("sce_sys")).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn save(path: &Path, image: &RgbImage) {
        image.save_with_format(path, ImageFormat::Png).unwrap();
    }

    /// A game folder with plain-colour stand-ins for the dump's pictures (no game data).
    fn game_folder(name: &str, box_art: bool, wide: bool) -> TempDir {
        let dir = TempDir::new(name);
        if box_art {
            save(
                &dir.0.join("sce_sys/icon0.png"),
                &RgbImage::from_pixel(512, 512, Rgb([220, 20, 30])),
            );
        }
        if wide {
            save(
                &dir.0.join("sce_sys/pic1.png"),
                &RgbImage::from_pixel(1920, 1080, Rgb([20, 40, 220])),
            );
        }
        dir
    }

    fn decode(bytes: &[u8]) -> DynamicImage {
        image::load_from_memory_with_format(bytes, ImageFormat::Png).unwrap()
    }

    #[test]
    fn sources_come_from_the_dump_sce_sys() {
        assert_eq!(source_file(Art::Hero), "sce_sys/pic1.png");
        for art in [Art::Grid, Art::WideGrid, Art::Logo, Art::Icon] {
            assert_eq!(source_file(art), "sce_sys/icon0.png");
        }
    }

    #[test]
    fn every_image_is_made_at_its_steam_size() {
        let dir = game_folder("sizes", true, true);
        for game in Game::ALL {
            for art in ALL {
                let png = render(&dir.0, game, art).unwrap().expect("an image");
                assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
                assert_eq!(decode(&png).dimensions(), size(art), "{art:?}");
            }
        }
    }

    #[test]
    fn framing_follows_steam_artwork() {
        let dir = game_folder("framing", true, true);
        let grid = decode(&render(&dir.0, Game::U1, Art::Grid).unwrap().unwrap()).to_rgb8();
        // The box art fills the middle 600x600; the blurred copy, 35% darker, above and below.
        assert_eq!(grid.get_pixel(300, 450).0, [220, 20, 30]);
        let shaded = |c: f32, toward: f32| (c * 0.65 + toward * 0.35) as u8;
        assert_eq!(
            grid.get_pixel(300, 5).0,
            [shaded(220.0, 10.0), shaded(20.0, 8.0), shaded(30.0, 6.0)]
        );
        let hero = decode(&render(&dir.0, Game::U1, Art::Hero).unwrap().unwrap()).to_rgb8();
        assert_eq!(hero.get_pixel(960, 310).0, [20, 40, 220]);
        let logo = decode(&render(&dir.0, Game::U1, Art::Logo).unwrap().unwrap()).to_rgba8();
        assert_eq!(logo.get_pixel(640, 360).0, [220, 20, 30, 255]);
        assert_eq!(logo.get_pixel(10, 360).0[3], 0);
        let icon = decode(&render(&dir.0, Game::U1, Art::Icon).unwrap().unwrap()).to_rgb8();
        assert_eq!(icon.get_pixel(128, 128).0, [220, 20, 30]);
    }

    #[test]
    fn hero_falls_back_to_the_framed_box_art() {
        let dir = game_folder("hero-fallback", true, false);
        let hero = render(&dir.0, Game::U2, Art::Hero).unwrap().unwrap();
        let hero = decode(&hero).to_rgb8();
        assert_eq!(hero.dimensions(), (1920, 620));
        assert_eq!(hero.get_pixel(960, 310).0, [220, 20, 30]);
        assert_ne!(hero.get_pixel(5, 310).0, [220, 20, 30]);
    }

    #[test]
    fn a_missing_source_picture_gives_none() {
        let empty = game_folder("missing", false, false);
        for art in ALL {
            assert_eq!(render(&empty.0, Game::U3, art).unwrap(), None, "{art:?}");
        }
        let wide_only = game_folder("wide-only", false, true);
        assert!(render(&wide_only.0, Game::U3, Art::Hero).unwrap().is_some());
        assert_eq!(render(&wide_only.0, Game::U3, Art::Grid).unwrap(), None);
    }

    #[test]
    fn a_damaged_picture_counts_as_missing() {
        let dir = game_folder("damaged", false, false);
        std::fs::write(dir.0.join("sce_sys/icon0.png"), b"not a png").unwrap();
        assert_eq!(render(&dir.0, Game::U1, Art::Grid).unwrap(), None);
    }

    // tests/test_uncharted_art.py: test_one_custom_image_creates_all_steam_sizes
    #[test]
    fn one_custom_image_creates_all_steam_sizes() {
        let dir = TempDir::new("custom");
        let picture = dir.0.join("collection.png");
        save(
            &picture,
            &RgbImage::from_pixel(1200, 700, Rgb([30, 180, 90])),
        );
        let made: Vec<_> = ALL
            .into_iter()
            .filter_map(|art| {
                render_custom(&picture, art, Crop::default())
                    .unwrap()
                    .map(|png| (art, decode(&png).dimensions()))
            })
            .collect();
        assert_eq!(
            made,
            [
                (Art::Grid, (600, 900)),
                (Art::WideGrid, (920, 430)),
                (Art::Hero, (1920, 620))
            ]
        );
        assert_eq!(
            render_custom(&dir.0.join("none.png"), Art::Grid, Crop::default()).unwrap(),
            None
        );
    }

    // tests/test_uncharted_art.py: test_crop_settings_keep_independent_focus
    #[test]
    fn crop_settings_keep_independent_focus() {
        let mut source = RgbImage::from_pixel(300, 100, Rgb([220, 20, 30]));
        for x in 200..300 {
            for y in 0..100 {
                source.put_pixel(x, y, Rgb([20, 40, 220]));
            }
        }
        let at = |x: f64| Crop {
            zoom: 1.0,
            x,
            y: 0.5,
        };
        let left = crop_to(&source, 100, 100, at(0.0));
        let right = crop_to(&source, 100, 100, at(1.0));
        assert_eq!(left.get_pixel(50, 50).0, [220, 20, 30]);
        assert_eq!(right.get_pixel(50, 50).0, [20, 40, 220]);
    }

    #[test]
    fn crop_clamps_zoom_and_focus() {
        let mut source = RgbImage::from_pixel(100, 100, Rgb([0, 0, 0]));
        source.put_pixel(99, 99, Rgb([255, 255, 255]));
        let wild = Crop {
            zoom: 9.0,
            x: 4.0,
            y: -2.0,
        };
        let clamped = Crop {
            zoom: 3.0,
            x: 1.0,
            y: 0.0,
        };
        assert_eq!(
            crop_to(&source, 40, 40, wild),
            crop_to(&source, 40, 40, clamped)
        );
        assert_eq!(crop_to(&source, 40, 40, clamped).dimensions(), (40, 40));
        assert_eq!(
            Crop::default(),
            Crop {
                zoom: 1.0,
                x: 0.5,
                y: 0.5
            }
        );
    }

    #[test]
    fn output_is_deterministic() {
        let dir = game_folder("deterministic", true, true);
        for art in ALL {
            assert_eq!(
                render(&dir.0, Game::U1, art).unwrap(),
                render(&dir.0, Game::U1, art).unwrap()
            );
        }
    }
}
