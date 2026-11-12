//! `aspect` on real screen sizes (issue #305): render size, HUD safe area, FOV mode, fit rectangle.

use uncharted_core::aspect::{fit_rect, fov, render_size, safe_area, FovMode, Rect};
use uncharted_core::error::Error;

const FOLD_UNFOLDED: (u32, u32) = (2208, 1840);
const FOLD_COVER: (u32, u32) = (2092, 1080);
const IPAD_MINI: (u32, u32) = (2266, 1488);
const FHD: (u32, u32) = (1920, 1080);
const Z13: (u32, u32) = (2560, 1600);
const ULTRAWIDE: (u32, u32) = (3440, 1440);
const PORTRAIT: (u32, u32) = (1080, 1920);

fn rect(x: u32, y: u32, width: u32, height: u32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn render_size_fills_the_screen_at_1080() {
    assert_eq!(render_size(FOLD_UNFOLDED, 1080).unwrap(), (1296, 1080));
    assert_eq!(render_size(FOLD_COVER, 1080).unwrap(), (2092, 1080));
    assert_eq!(render_size(IPAD_MINI, 1080).unwrap(), (1644, 1080));
    assert_eq!(render_size(FHD, 1080).unwrap(), (1920, 1080));
    assert_eq!(render_size(Z13, 1080).unwrap(), (1728, 1080));
    assert_eq!(render_size(ULTRAWIDE, 1080).unwrap(), (2580, 1080));
}

#[test]
fn render_size_width_is_always_even() {
    for screen in [FOLD_UNFOLDED, FOLD_COVER, IPAD_MINI, FHD, Z13, ULTRAWIDE] {
        for height in [360, 540, 721, 900, 1080, 1441, 2160] {
            assert_eq!(render_size(screen, height).unwrap().0 % 2, 0);
        }
    }
}

#[test]
fn render_size_is_clamped() {
    // A portrait screen at 1080 would be 608 wide: below the minimum.
    assert_eq!(render_size(PORTRAIT, 1080).unwrap(), (640, 1080));
    // Ultrawide at 8K height: both sides clamp.
    assert_eq!(render_size(ULTRAWIDE, 8000).unwrap(), (7680, 4320));
    assert_eq!(render_size(FHD, 100).unwrap(), (640, 360));
    assert_eq!(render_size(FHD, u32::MAX).unwrap(), (7680, 4320));
    assert_eq!(
        render_size((u32::MAX, 1), 4320).unwrap(),
        (7680, 4320),
        "a huge aspect must not overflow"
    );
}

#[test]
fn safe_area_is_the_centred_16_9() {
    assert_eq!(safe_area(FOLD_UNFOLDED).unwrap(), rect(0, 299, 2208, 1242));
    assert_eq!(safe_area(FOLD_COVER).unwrap(), rect(86, 0, 1920, 1080));
    assert_eq!(safe_area(IPAD_MINI).unwrap(), rect(0, 107, 2266, 1274));
    assert_eq!(safe_area(FHD).unwrap(), rect(0, 0, 1920, 1080));
    assert_eq!(safe_area(Z13).unwrap(), rect(0, 80, 2560, 1440));
    assert_eq!(safe_area(ULTRAWIDE).unwrap(), rect(440, 0, 2560, 1440));
    assert_eq!(safe_area(PORTRAIT).unwrap(), rect(0, 656, 1080, 607));
}

#[test]
fn fov_mode_follows_the_screen_shape() {
    assert_eq!(fov(FHD).unwrap().mode, FovMode::Native);
    assert_eq!(fov(FOLD_COVER).unwrap().mode, FovMode::HorPlus);
    assert_eq!(fov(ULTRAWIDE).unwrap().mode, FovMode::HorPlus);
    assert_eq!(fov(FOLD_UNFOLDED).unwrap().mode, FovMode::VertPlus);
    assert_eq!(fov(IPAD_MINI).unwrap().mode, FovMode::VertPlus);
    assert_eq!(fov(Z13).unwrap().mode, FovMode::VertPlus);
    assert_eq!(fov(PORTRAIT).unwrap().mode, FovMode::VertPlus);
}

#[test]
fn fov_factors() {
    let native = fov(FHD).unwrap();
    assert_eq!((native.horizontal, native.vertical), (1.0, 1.0));

    let wide = fov(ULTRAWIDE).unwrap();
    assert!(near(wide.horizontal, 43.0 / 32.0));
    assert_eq!(wide.vertical, 1.0);

    let tall = fov(Z13).unwrap();
    assert_eq!(tall.horizontal, 1.0);
    assert!(near(tall.vertical, 10.0 / 9.0));

    let portrait = fov(PORTRAIT).unwrap();
    assert!(near(portrait.vertical, (1920.0 * 16.0) / (1080.0 * 9.0)));
}

#[test]
fn fov_tolerance_edges() {
    // 1366x768 is 0.1% off 16:9, 1360x768 0.5%, 1280x800 is 16:10.
    assert_eq!(fov((1366, 768)).unwrap().mode, FovMode::Native);
    assert_eq!(fov((1360, 768)).unwrap().mode, FovMode::Native);
    assert_eq!(fov((1280, 800)).unwrap().mode, FovMode::VertPlus);
    // 1% of 1920 is 19.2: 1939 is still native, 1940 is not.
    assert_eq!(fov((1939, 1080)).unwrap().mode, FovMode::Native);
    assert_eq!(fov((1940, 1080)).unwrap().mode, FovMode::HorPlus);
}

#[test]
fn fit_rect_letterbox_and_pillarbox() {
    // A 16:9 video.
    assert_eq!(fit_rect(Z13, (16, 9)).unwrap(), rect(0, 80, 2560, 1440));
    assert_eq!(
        fit_rect(ULTRAWIDE, (16, 9)).unwrap(),
        rect(440, 0, 2560, 1440)
    );
    // A 4:3 image on a 16:9 screen is pillarboxed; a 2.39:1 one letterboxed.
    assert_eq!(fit_rect(FHD, (4, 3)).unwrap(), rect(240, 0, 1440, 1080));
    assert_eq!(fit_rect(FHD, (239, 100)).unwrap(), rect(0, 138, 1920, 803));
    // Same shape fills the container.
    assert_eq!(fit_rect(FHD, (1920, 1080)).unwrap(), rect(0, 0, 1920, 1080));
}

#[test]
fn rectangles_stay_inside_the_container() {
    for container in [
        FOLD_UNFOLDED,
        FOLD_COVER,
        IPAD_MINI,
        FHD,
        Z13,
        ULTRAWIDE,
        PORTRAIT,
    ] {
        for aspect in [(16, 9), (4, 3), (1, 1), (21, 9), (9, 16)] {
            let r = fit_rect(container, aspect).unwrap();
            assert!(r.x + r.width <= container.0);
            assert!(r.y + r.height <= container.1);
            assert!(r.x == 0 || r.y == 0, "one axis is always full");
        }
    }
}

#[test]
fn odd_sizes() {
    // 1919x1079: the 16:9 rectangle is 1918x1079, the odd pixel goes to the far side.
    assert_eq!(safe_area((1919, 1079)).unwrap(), rect(0, 0, 1918, 1079));
    assert_eq!(safe_area((1921, 1080)).unwrap(), rect(0, 0, 1920, 1080));
    assert_eq!(fov((1919, 1079)).unwrap().mode, FovMode::Native);
    assert_eq!(render_size((1919, 1079), 1080).unwrap().0 % 2, 0);
    // 1x1 is far from 16:9 (taller), and still works.
    assert_eq!(safe_area((1, 1)).unwrap(), rect(0, 0, 1, 0));
    assert_eq!(fov((1, 1)).unwrap().mode, FovMode::VertPlus);
    assert_eq!(render_size((1, 1), 1080).unwrap(), (1080, 1080));
}

#[test]
fn zero_sizes_are_plain_errors() {
    let refused = |result: Result<_, Error>| match result {
        Err(Error::RenderSize(message)) => assert!(message.contains("zero"), "{message}"),
        Err(other) => panic!("expected RenderSize, got {other:?}"),
        Ok(_) => panic!("expected an error"),
    };
    refused(render_size((0, 0), 1080).map(|_| ()));
    refused(render_size((1920, 0), 1080).map(|_| ()));
    refused(render_size((0, 1080), 1080).map(|_| ()));
    refused(render_size(FHD, 0).map(|_| ()));
    refused(safe_area((0, 1080)).map(|_| ()));
    refused(safe_area((1920, 0)).map(|_| ()));
    refused(fov((0, 0)).map(|_| ()));
    refused(fit_rect(FHD, (0, 9)).map(|_| ()));
    refused(fit_rect(FHD, (16, 0)).map(|_| ()));
    refused(fit_rect((0, 0), (16, 9)).map(|_| ()));
}
