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

/// Draw the launcher's original collection artwork in Rust.
///
/// Port of `packaging/windows/make_uncharted_art.py` (GPL-2.0-or-later,
/// repository copyright). The generator is deterministic and uses only vector shapes,
/// gradients, and seeded fractal noise. It never reads game data or accesses the network.
pub fn generate_collection_art(out_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    for (name, image) in [
        ("u1.jpg", scene(1)),
        ("u2.jpg", scene(2)),
        ("u3.jpg", scene(3)),
        ("collection_banner.jpg", banner()),
    ] {
        let file = std::fs::File::create(out_dir.join(name))?;
        image::codecs::jpeg::JpegEncoder::new_with_quality(file, 90)
            .encode_image(&image)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
    }
    render_emblem(256, false)
        .save(out_dir.join("emblem.png"))
        .map_err(|e| std::io::Error::other(e.to_string()))
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

type Rgb = image::Rgb<u8>;
type Canvas = image::RgbImage;
const CARD: (u32, u32) = (620, 660);
const BANNER: (u32, u32) = (1920, 540);

fn rgb(hex: u32) -> Rgb {
    image::Rgb([(hex >> 16) as u8, (hex >> 8) as u8, hex as u8])
}
fn blend(a: Rgb, b: Rgb, t: f32) -> Rgb {
    image::Rgb(std::array::from_fn(|i| {
        (a[i] as f32 * (1.0 - t) + b[i] as f32 * t)
            .round()
            .clamp(0., 255.) as u8
    }))
}

/// Small deterministic generator. The integer stream is fixed across targets, unlike the
/// original Pillow `effect_noise` calls whose entropy source was not seeded.
struct Noise(u64);
impl Noise {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e3779b97f4a7c15)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / 16_777_215.0
    }
    fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}
fn noise1d(n: usize, seed: u64, base: usize, octaves: usize, rough: f32) -> Vec<f32> {
    let mut rng = Noise::new(seed);
    let mut out = vec![0.; n];
    let mut amp = 1.;
    let mut cells = base;
    let mut total = 0.;
    for _ in 0..octaves {
        let knots: Vec<f32> = (0..cells + 2).map(|_| rng.signed()).collect();
        for (x, v) in out.iter_mut().enumerate() {
            let f = x as f32 / n as f32 * cells as f32;
            let i = f as usize;
            let t = (1. - (f - i as f32).mul_add(std::f32::consts::PI, 0.).cos()) / 2.;
            *v += amp * (knots[i] * (1. - t) + knots[i + 1] * t);
        }
        total += amp;
        amp *= rough;
        cells *= 2;
    }
    out.into_iter().map(|v| v / total).collect()
}
fn gradient(size: (u32, u32), stops: &[(f32, u32)]) -> Canvas {
    let (w, h) = size;
    let mut im = Canvas::new(w, h);
    for y in 0..h {
        let t = y as f32 / (h - 1).max(1) as f32;
        let mut c = rgb(stops[0].1);
        for pair in stops.windows(2) {
            if t >= pair[0].0 && t <= pair[1].0 {
                c = blend(
                    rgb(pair[0].1),
                    rgb(pair[1].1),
                    (t - pair[0].0) / (pair[1].0 - pair[0].0),
                );
                break;
            }
            if t > pair[1].0 {
                c = rgb(pair[1].1);
            }
        }
        for x in 0..w {
            im.put_pixel(x, y, c);
        }
    }
    im
}
fn poly(im: &mut Canvas, pts: &[(f32, f32)], color: Rgb) {
    let pts: Vec<_> = pts
        .iter()
        .map(|p| imageproc::point::Point::new(p.0.round() as i32, p.1.round() as i32))
        .collect();
    imageproc::drawing::draw_polygon_mut(im, &pts, color);
}
fn line(im: &mut Canvas, a: (f32, f32), b: (f32, f32), color: Rgb, width: i32) {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len = (dx * dx + dy * dy).sqrt().max(1.);
    for k in -(width / 2)..=(width / 2) {
        let o = k as f32;
        let off = (-dy / len * o, dx / len * o);
        imageproc::drawing::draw_line_segment_mut(
            im,
            (a.0 + off.0, a.1 + off.1),
            (b.0 + off.0, b.1 + off.1),
            color,
        );
    }
}
fn circle(im: &mut Canvas, c: (f32, f32), r: f32, color: Rgb) {
    imageproc::drawing::draw_filled_circle_mut(
        im,
        (c.0.round() as i32, c.1.round() as i32),
        r.max(0.) as i32,
        color,
    );
}
fn ridge(
    w: usize,
    base: f32,
    amp: f32,
    seed: u64,
    rough: f32,
    cells: usize,
    oct: usize,
) -> Vec<f32> {
    noise1d(w, seed, cells, oct, rough)
        .iter()
        .map(|v| base - amp * v)
        .collect()
}
fn silhouette(im: &mut Canvas, ys: &[f32], color: Rgb) {
    let h = im.height() as f32;
    let mut p = Vec::with_capacity(ys.len() + 2);
    p.push((0., h));
    p.extend(ys.iter().enumerate().map(|(x, y)| (x as f32, *y)));
    p.push((ys.len() as f32, h));
    poly(im, &p, color);
}
fn glow(im: &mut Canvas, c: (f32, f32), radius: f32, color: Rgb, strength: f32) {
    let x0 = (c.0 - radius).floor().max(0.) as u32;
    let x1 = (c.0 + radius).ceil().min(im.width() as f32) as u32;
    let y0 = (c.1 - radius).floor().max(0.) as u32;
    let y1 = (c.1 + radius).ceil().min(im.height() as f32) as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let dx = (x as f32 - c.0) / radius;
            let dy = (y as f32 - c.1) / radius;
            let d2 = dx * dx + dy * dy;
            if d2 <= 1.0 {
                let alpha = strength * (-4.0 * d2).exp();
                im.put_pixel(x, y, blend(*im.get_pixel(x, y), color, alpha));
            }
        }
    }
}

fn scene(which: u8) -> Canvas {
    let mut im = match which {
        1 => gradient(
            (CARD.0 * 2, CARD.1 * 2),
            &[
                (0., 0x16233a),
                (0.28, 0x4a3a55),
                (0.45, 0xb4593b),
                (0.6, 0xf0a85a),
                (0.61, 0x3c5560),
                (1., 0x0b1418),
            ],
        ),
        2 => gradient(
            (CARD.0 * 2, CARD.1 * 2),
            &[
                (0., 0x0b1426),
                (0.3, 0x283b60),
                (0.5, 0x8a7d93),
                (0.62, 0xf0b27a),
                (1., 0xe6a070),
            ],
        ),
        _ => gradient(
            (CARD.0 * 2, CARD.1 * 2),
            &[
                (0., 0x2a1410),
                (0.25, 0x6e2c1c),
                (0.45, 0xc25a2a),
                (0.6, 0xf6c26b),
                (1., 0xc87a3a),
            ],
        ),
    };
    let (w, h) = (im.width() as usize, im.height() as usize);
    let sun = if which == 1 {
        (w as f32 * 0.6, h as f32 * 0.56)
    } else if which == 2 {
        (w as f32 * 0.78, h as f32 * 0.52)
    } else {
        (w as f32 * 0.42, h as f32 * 0.5)
    };
    glow(
        &mut im,
        sun,
        w as f32 * 0.85,
        if which == 2 {
            rgb(0xffc489)
        } else {
            rgb(0xffb35e)
        },
        0.48,
    );
    glow(&mut im, sun, w as f32 * 0.09, rgb(0xfff0c0), 0.9);
    if which == 1 {
        let mut ys = ridge(w, h as f32 * 0.62, h as f32 * 0.06, 5, 0.5, 3, 5);
        silhouette(&mut im, &ys, rgb(0x7a4c4c));
        for (x, y) in ys.iter_mut().enumerate().skip(w / 2) {
            let f = ((x - w / 2) as f32 / (w / 2) as f32).powf(0.45);
            *y = h as f32 * 0.62
                - f * h as f32 * 0.2
                - h as f32 * 0.03 * noise1d(w, 9, 8, 5, 0.5)[x];
        }
        silhouette(&mut im, &ys, rgb(0x3a2a33));
        for (x, y) in [
            (w as f32 * 0.70, ys[(w as f32 * 0.72) as usize]),
            (w as f32 * 0.84, ys[(w as f32 * 0.86) as usize]),
        ] {
            let tw = w as f32 * 0.075;
            let th = h as f32 * 0.16;
            poly(
                &mut im,
                &[
                    (x - tw / 2., y - th),
                    (x + tw / 2., y - th),
                    (x + tw / 2., y),
                    (x - tw / 2., y),
                ],
                rgb(0x3a2a33),
            );
        }
        // Shore wall, crenellated towers, and dark arches on the fortress island.
        let wall_y = ys[(w as f32 * 0.70) as usize];
        imageproc::drawing::draw_filled_rect_mut(
            &mut im,
            imageproc::rect::Rect::at((w as f32 * 0.66) as i32, (wall_y - h as f32 * 0.05) as i32)
                .of_size((w as f32 * 0.29) as u32, (h as f32 * 0.05) as u32),
            rgb(0x3a2a33),
        );
        for cx in [0.735, 0.765, 0.875] {
            let x = w as f32 * cx;
            let top = wall_y - h as f32 * 0.13;
            circle(
                &mut im,
                (x, top + w as f32 * 0.006),
                w as f32 * 0.006,
                rgb(0x1a1219),
            );
            imageproc::drawing::draw_filled_rect_mut(
                &mut im,
                imageproc::rect::Rect::at(x as i32, (top + w as f32 * 0.006) as i32)
                    .of_size((w as f32 * 0.012) as u32, (h as f32 * 0.03) as u32),
                rgb(0x1a1219),
            );
        }
        // Broken horizontal strokes are the reflection path across the sea.
        let mut water = Noise::new(3);
        for _ in 0..160 {
            let y = h as f32 * 0.62 + 4.0 + water.unit() * h as f32 * 0.33;
            let t = (y - h as f32 * 0.62) / (h as f32 * 0.38);
            let half = w as f32 * (0.02 + 0.12 * t) * (0.3 + water.unit() * 0.7);
            let x = sun.0 + water.signed() * w as f32 * 0.04 * (1.0 + t * 2.0);
            let c = blend(rgb(0xffcf87), rgb(0x3c5560), t * 0.7);
            line(
                &mut im,
                (x - half, y),
                (x + half, y),
                c,
                (h as f32 * 0.003).max(1.0) as i32,
            );
        }
        let shore = ridge(w, h as f32 * 0.86, h as f32 * 0.05, 21, 0.5, 5, 5);
        silhouette(&mut im, &shore, rgb(0x101612));
        for (x, height, lean) in [
            (w as f32 * 0.1, h as f32 * 0.62, 0.32),
            (w as f32 * 0.25, h as f32 * 0.44, 0.45),
            (w as f32 * 0.95, h as f32 * 0.5, -0.35),
        ] {
            palm(
                &mut im,
                x,
                h as f32 * 1.02,
                height,
                lean,
                rgb(0x0d1210),
                31 + (x as u64),
                8,
            );
        }
    } else if which == 2 {
        for (seed, base, tall, count, col) in [
            (41, 0.55, 0.30, 4, 0x8d93ad),
            (42, 0.66, 0.24, 5, 0x55607f),
            (44, 0.76, 0.16, 6, 0x2e3650),
        ] {
            let ys = peakline(w, h as f32 * base, h as f32 * tall, count, seed);
            silhouette(&mut im, &ys, rgb(col));
            let depth = noise1d(w, seed + 7, 10, 4, 0.5);
            let cap: Vec<f32> = ys
                .iter()
                .enumerate()
                .map(|(x, y)| y + h as f32 * 0.035 * (1.6 + depth[x]))
                .collect();
            let mut m = im.clone();
            silhouette(&mut m, &cap, rgb(0xe4e2ec));
            for (x, y, p) in im.enumerate_pixels_mut() {
                let top = ys[x as usize].max(0.0) as u32;
                let cy = cap[x as usize].max(0.0) as u32;
                if y >= top && y < cy {
                    *p = *m.get_pixel(x, y);
                }
            }
        }
        let near = ridge(w, h as f32 * 0.9, h as f32 * 0.12, 46, 0.5, 6, 5);
        silhouette(&mut im, &near, rgb(0x171a28));
        let peak = (0..w).min_by(|a, b| near[*a].total_cmp(&near[*b])).unwrap();
        let base = near[peak] + 8.;
        for (dx, bw, bh) in [
            (-0.17, 0.12, 0.07),
            (-0.06, 0.16, 0.12),
            (0.08, 0.12, 0.09),
            (0.18, 0.08, 0.05),
        ] {
            let x = peak as f32 + dx * w as f32;
            let top = base - bh * h as f32;
            poly(
                &mut im,
                &[
                    (x, base),
                    (x + bw * w as f32, base),
                    (x + bw * w as f32 * 0.92, top),
                    (x + bw * w as f32 * 0.08, top),
                ],
                rgb(0x171a28),
            );
        }
        let monastery = rgb(0x171a28);
        for (dx, bw, bh) in [
            (-0.17, 0.12, 0.07),
            (-0.06, 0.16, 0.12),
            (0.08, 0.12, 0.09),
            (0.18, 0.08, 0.05),
        ] {
            let x = peak as f32 + dx * w as f32;
            let top = base - bh * h as f32;
            let inset = bw * w as f32 * 0.08;
            poly(
                &mut im,
                &[
                    (x, base + 30.),
                    (x + inset, top),
                    (x + bw * w as f32 - inset, top),
                    (x + bw * w as f32, base + 30.),
                ],
                monastery,
            );
            imageproc::drawing::draw_filled_rect_mut(
                &mut im,
                imageproc::rect::Rect::at((x + inset - 3.) as i32, (top - h as f32 * 0.012) as i32)
                    .of_size(
                        (bw * w as f32 - inset * 2. + 6.) as u32,
                        (h as f32 * 0.012) as u32,
                    ),
                monastery,
            );
        }
        for _ in 0..10 {
            let x = peak as f32 + (-0.15 + Noise::new(7).unit() * 0.4) * w as f32;
            let y = base - 0.03 * h as f32;
            imageproc::drawing::draw_filled_rect_mut(
                &mut im,
                imageproc::rect::Rect::at(x as i32, y as i32)
                    .of_size((w as f32 * 0.006) as u32, (h as f32 * 0.01) as u32),
                rgb(0xffbe6b),
            );
        }
        let mut rng = Noise::new(7);
        for i in 0..60 {
            let t = i as f32 / 60.;
            let x0 = -(w as f32) * 0.05 + w as f32 * 1.1 * t;
            let y0 = h as f32 * 0.06
                + h as f32 * 0.14 * t
                + h as f32 * 0.12 * (std::f32::consts::PI * t).sin();
            line(
                &mut im,
                (x0, y0),
                (x0 + w as f32 / 60., y0 + 2.),
                rgb(0x1a1410),
                2,
            );
            if i % 3 == 2 {
                let color = [0x2f6fb5, 0xe8e2d4, 0xc23a2c, 0x3c8b4d, 0xe3b23c][(i / 3) % 5];
                poly(
                    &mut im,
                    &[
                        (x0, y0),
                        (x0 + 24., y0 + 2.),
                        (x0 + 24., y0 + h as f32 * 0.045),
                        (x0, y0 + h as f32 * 0.045),
                    ],
                    rgb(color),
                );
            }
        }
        for _ in 0..110 {
            let x = rng.unit() * w as f32;
            let y = rng.unit() * h as f32;
            circle(&mut im, (x, y), rng.unit() * 2. + 1., rgb(0xfaf5f0));
        }
    } else {
        for i in 0..4 {
            let base = h as f32 * (0.6 + i as f32 * 0.1);
            let ys = ridge(
                w,
                base,
                h as f32 * (0.03 + i as f32 * 0.01),
                51 + i as u64,
                0.3,
                2,
                3,
            );
            silhouette(
                &mut im,
                &ys,
                rgb([0xc2703a, 0xa5552b, 0x7a3a1d, 0x3d1d10][i]),
            );
            if i == 1 {
                for (x, ph) in [
                    (0.1, 0.3),
                    (0.18, 0.22),
                    (0.27, 0.34),
                    (0.35, 0.15),
                    (0.83, 0.26),
                    (0.92, 0.33),
                ] {
                    let px = w as f32 * x;
                    let y = ys[px as usize];
                    let p = rgb(0x5a2a16);
                    let hh = h as f32 * ph;
                    line(&mut im, (px, y), (px, y - hh), p, 12);
                    line(&mut im, (px + 35., y), (px + 35., y - hh), p, 12);
                    line(&mut im, (px, y - hh), (px + 35., y - hh), p, 12);
                }
            }
        }
        let mut rng = Noise::new(3);
        for _ in 0..5 {
            let x = w as f32 * (0.66 + rng.unit() * 0.16);
            let y = h as f32 * 0.59;
            circle(&mut im, (x, y), 7., rgb(0x4a2414));
            line(&mut im, (x, y), (x + 10., y - 14.), rgb(0x4a2414), 3);
        }
        // Birds crossing the warm sky.
        let mut birds = Noise::new(57);
        for _ in 0..4 {
            let x = w as f32 * (0.5 + birds.unit() * 0.3);
            let y = h as f32 * (0.25 + birds.unit() * 0.13);
            let s = 10.0;
            line(&mut im, (x - s, y - s * 0.3), (x, y), rgb(0x3a1a10), 2);
            line(&mut im, (x, y), (x + s, y - s * 0.3), rgb(0x3a1a10), 2);
        }
    }
    image::imageops::resize(&im, CARD.0, CARD.1, image::imageops::FilterType::Lanczos3)
}
fn peakline(w: usize, base: f32, tall: f32, count: usize, seed: u64) -> Vec<f32> {
    let mut r = Noise::new(seed);
    let peaks: Vec<_> = (0..count)
        .map(|i| {
            (
                w as f32 * (i as f32 + r.unit() * 0.8) / count as f32,
                tall * (0.55 + r.unit() * 0.45),
                w as f32 * (0.18 + r.unit() * 0.14),
            )
        })
        .collect();
    let jag = noise1d(w, seed + 3, 12, 6, 0.6);
    (0..w)
        .map(|x| {
            let lift = peaks
                .iter()
                .map(|(px, t, half)| {
                    t * (1. - ((x as f32 - px).abs() / half).min(1.))
                        .max(0.)
                        .powf(1.15)
                })
                .fold(0., f32::max);
            base - lift - tall * 0.06 * jag[x]
        })
        .collect()
}
#[allow(clippy::too_many_arguments)]
fn palm(
    im: &mut Canvas,
    x: f32,
    y: f32,
    height: f32,
    lean: f32,
    color: Rgb,
    seed: u64,
    fronds: usize,
) {
    let tx = x + lean * height;
    let ty = y - height;
    let mut trunk = Vec::new();
    for i in 0..21 {
        let t = i as f32 / 20.0;
        trunk.push((x + lean * height * t * t, y - height * t));
    }
    for pair in trunk.windows(2) {
        line(
            im,
            pair[0],
            pair[1],
            color,
            (height * (0.03 - 0.014 * (pair[0].1 / height))).max(2.0) as i32,
        );
    }
    let mut r = Noise::new(seed);
    for k in 0..fronds {
        let a = (-180. + 360. * k as f32 / fronds as f32 + r.signed() * 14.).to_radians();
        let length = height * (0.3 + r.unit() * 0.12);
        let droop = 0.35 + r.unit() * 0.4;
        let mut spine = Vec::new();
        for i in 0..17 {
            let t = i as f32 / 16.0;
            spine.push((
                tx + a.cos() * length * t,
                ty + a.sin() * length * t * 0.45 + droop * length * t * t,
            ));
        }
        for pair in spine.windows(2) {
            line(
                im,
                pair[0],
                pair[1],
                color,
                (height * 0.012).max(2.0) as i32,
            );
        }
        // Tapered leaflets follow both sides of each curved frond spine.
        for i in (2..14).step_by(2) {
            let p = spine[i];
            let prev = spine[i - 1];
            let next = spine[i + 1];
            let dx = next.0 - prev.0;
            let dy = next.1 - prev.1;
            let norm = (dx * dx + dy * dy).sqrt().max(1.0);
            let t = i as f32 / 16.0;
            let leaf = height * 0.045 * (1.0 - t);
            for side in [-1.0, 1.0] {
                let end = (
                    p.0 + side * (-dy / norm) * leaf,
                    p.1 + side * (dx / norm) * leaf + leaf * 0.2,
                );
                line(im, p, end, color, (height * 0.009).max(1.0) as i32);
            }
        }
    }
}

fn banner() -> Canvas {
    let (w, h) = BANNER;
    let mut im = gradient(BANNER, &[(0., 0x3b2c1b), (0.5, 0x5a4329), (1., 0x2a1f13)]);
    let mut rng = Noise::new(77);
    for y in 0..h {
        for x in 0..w {
            let grain = rng.signed() * 5.;
            let p = im.get_pixel_mut(x, y);
            for c in 0..3 {
                p[c] = (p[c] as f32 + grain).clamp(0., 255.) as u8;
            }
        }
    }
    for x in (0..w).step_by(w as usize / 12) {
        line(
            &mut im,
            (x as f32, 0.),
            (x as f32, h as f32),
            rgb(0x6b5232),
            1,
        );
    }
    for y in (0..h).step_by(h as usize / 4) {
        line(
            &mut im,
            (0., y as f32),
            (w as f32, y as f32),
            rgb(0x6b5232),
            1,
        );
    }
    // Fractal elevation field produces irregular coastlines and a second shallow sea contour.
    let land: Vec<bool> = (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| {
                let n = noise2d(x as f32, y as f32, 240.0, 170.0, 12) * 0.62
                    + noise2d(x as f32, y as f32, 80.0, 60.0, 23) * 0.28
                    + noise2d(x as f32, y as f32, 24.0, 18.0, 37) * 0.1;
                n > 0.08
            })
        })
        .collect();
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = (y * w + x) as usize;
            if land[i] {
                let edge = [i - 1, i + 1, i - w as usize, i + w as usize]
                    .iter()
                    .any(|j| !land[*j]);
                let color = if edge { rgb(0x24180c) } else { rgb(0x7a6040) };
                im.put_pixel(x, y, color);
            } else if [i - 1, i + 1, i - w as usize, i + w as usize]
                .iter()
                .any(|j| land[*j])
            {
                im.put_pixel(x, y, rgb(0x2c2012));
            }
        }
    }
    let stops: [(f32, f32); 3] = [(0.30, 0.66), (0.55, 0.30), (0.82, 0.56)];
    for k in 0..2 {
        let (a, b) = (stops[k], stops[k + 1]);
        let mut last = None;
        for i in 0..=90 {
            let t = i as f32 / 90.;
            let mx = (a.0 + b.0) / 2.;
            let my = a.1.min(b.1) - 0.18;
            let x = (1. - t).powi(2) * a.0 + 2. * (1. - t) * t * mx + t * t * b.0;
            let y = (1. - t).powi(2) * a.1 + 2. * (1. - t) * t * my + t * t * b.1;
            let p = (x * w as f32, y * h as f32);
            if let Some(q) = last {
                line(&mut im, q, p, rgb(0xb3362a), 4);
            }
            last = Some(p);
        }
    }
    for (x, y) in stops {
        let c = (x * w as f32, y * h as f32);
        line(
            &mut im,
            (c.0 - 14., c.1 - 14.),
            (c.0 + 14., c.1 + 14.),
            rgb(0xb3362a),
            5,
        );
        line(
            &mut im,
            (c.0 + 14., c.1 - 14.),
            (c.0 - 14., c.1 + 14.),
            rgb(0xb3362a),
            5,
        );
    }
    // Compass rose landmark on the western margin, plus stylized ranges and dune marks.
    let rose = render_emblem(336, true);
    for (x, y, p) in rose.enumerate_pixels() {
        if p[3] > 0 {
            let dx = (0.07 * w as f32) as i32 + x as i32;
            let dy = (0.12 * h as f32) as i32 + y as i32;
            if dx >= 0 && dy >= 0 && dx < w as i32 && dy < h as i32 {
                let alpha = p[3] as f32 / 255.;
                let old = *im.get_pixel(dx as u32, dy as u32);
                im.put_pixel(
                    dx as u32,
                    dy as u32,
                    blend(
                        old,
                        rgb((p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32),
                        alpha,
                    ),
                );
            }
        }
    }
    for k in 0..4 {
        let x = w as f32 * 0.55 - 70. + k as f32 * 40.;
        let y = h as f32 * 0.3 - 28. + (k % 2) as f32 * 10.;
        line(&mut im, (x - 18., y + 16.), (x, y - 16.), rgb(0x2a1c0e), 3);
        line(&mut im, (x, y - 16.), (x + 18., y + 16.), rgb(0x2a1c0e), 3);
    }
    for y in (h as f32 * 0.55) as u32..h {
        let alpha = 0.75 * ((y as f32 / h as f32 - 0.55) / 0.45).clamp(0., 1.);
        for x in 0..w {
            let c = im.get_pixel_mut(x, y);
            *c = blend(*c, rgb(0x0a0704), alpha);
        }
    }
    image::imageops::resize(&im, w, h, image::imageops::FilterType::Lanczos3)
}

fn noise2d(x: f32, y: f32, sx: f32, sy: f32, seed: u64) -> f32 {
    fn hash(x: i32, y: i32, seed: u64) -> f32 {
        let mut z = (x as u64).wrapping_mul(0x9e3779b97f4a7c15)
            ^ (y as u64).wrapping_mul(0xbf58476d1ce4e5b9)
            ^ seed;
        z ^= z >> 30;
        z = z.wrapping_mul(0xbf58476d1ce4e5b9);
        z ^= z >> 27;
        z = z.wrapping_mul(0x94d049bb133111eb);
        z ^= z >> 31;
        (z >> 40) as f32 / 16_777_215.0 * 2.0 - 1.0
    }
    let fx = x / sx;
    let fy = y / sy;
    let ix = fx.floor() as i32;
    let iy = fy.floor() as i32;
    let tx = fx - ix as f32;
    let ty = fy - iy as f32;
    let sx = tx * tx * (3.0 - 2.0 * tx);
    let sy = ty * ty * (3.0 - 2.0 * ty);
    let a = hash(ix, iy, seed) * (1.0 - sx) + hash(ix + 1, iy, seed) * sx;
    let b = hash(ix, iy + 1, seed) * (1.0 - sx) + hash(ix + 1, iy + 1, seed) * sx;
    a * (1.0 - sy) + b * sy
}

fn render_emblem(output_side: u32, plain: bool) -> image::RgbaImage {
    use image::{Rgba, RgbaImage};
    use imageproc::drawing::{draw_filled_circle_mut, draw_polygon_mut};
    use imageproc::point::Point;
    let side = (output_side * 4) as i32; // 4x supersampling, matching the Python source.
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
    if !plain {
        draw_filled_circle_mut(&mut im, (c as i32, c as i32), r as i32, dark);
        draw_filled_circle_mut(
            &mut im,
            (c as i32, c as i32),
            (r * 0.94) as i32,
            Rgba([26, 18, 10, 255]),
        );
    }
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
    image::imageops::resize(
        &im,
        output_side,
        output_side,
        image::imageops::FilterType::Lanczos3,
    )
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
