//! Pixel Fold M21 unit tests (Stage 7.07): 6:5 unfolded, tall & landscape cover screens,
//! and mid-game fold/unfold resize/restart decisions.

use uncharted_core::aspect::{
    evaluate_resize, fit_rect, fov, render_size, resize_needs_restart, safe_area, FovMode, Rect,
    RESIZE_RESTART_THRESHOLD_PX,
};
use uncharted_core::error::Error;
use uncharted_core::games::Game;
use uncharted_core::render_size::{automatic_size, render_target_growth};

const FOLD_UNFOLDED_6_5: (u32, u32) = (2208, 1840);
const FOLD_UNFOLDED_ROTATED_5_6: (u32, u32) = (1840, 2208);
const FOLD_COVER_LANDSCAPE: (u32, u32) = (2092, 1080);
const FOLD_COVER_TALL: (u32, u32) = (1080, 2092);

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn unfolded_6_5_uses_vert_plus_and_centred_safe_area() {
    assert_eq!(FOLD_UNFOLDED_6_5.0 * 5, FOLD_UNFOLDED_6_5.1 * 6);

    let f = fov(FOLD_UNFOLDED_6_5).unwrap();
    assert_eq!(f.mode, FovMode::VertPlus);
    assert_eq!(f.horizontal, 1.0);
    assert!(near(f.vertical, 40.0 / 27.0));
    assert!(f.vertical > 1.45);

    let hud = safe_area(FOLD_UNFOLDED_6_5).unwrap();
    assert_eq!(
        hud,
        Rect {
            x: 0,
            y: 299,
            width: 2208,
            height: 1242,
        }
    );
    assert_eq!(
        fit_rect(FOLD_UNFOLDED_6_5, (1920, 1080)).unwrap(),
        hud,
        "16:9 pre-rendered video should letterbox cleanly to the same safe band"
    );
    assert_eq!(render_size(FOLD_UNFOLDED_6_5, 1080).unwrap(), (1296, 1080));

    for game in Game::ALL {
        let auto = automatic_size(FOLD_UNFOLDED_6_5, Some(game), 1200).unwrap();
        assert_eq!(auto, FOLD_UNFOLDED_6_5);
        assert_eq!(auto.0 % 16, 0);
        assert_eq!(auto.1 % 4, 0);
        let (rt, rt2) = render_target_growth(auto);
        assert!(rt + rt2 <= 1200);
    }

    // Portrait orientation of the unfolded 6:5 panel (5:6) also stays VertPlus and valid.
    let rotated_fov = fov(FOLD_UNFOLDED_ROTATED_5_6).unwrap();
    assert_eq!(rotated_fov.mode, FovMode::VertPlus);
    let rotated_hud = safe_area(FOLD_UNFOLDED_ROTATED_5_6).unwrap();
    assert_eq!(rotated_hud.x, 0);
    assert_eq!(rotated_hud.width, 1840);
    assert!(rotated_hud.y + rotated_hud.height <= 2208);
    let rotated_auto = automatic_size(FOLD_UNFOLDED_ROTATED_5_6, Some(Game::U3), 1200).unwrap();
    assert_eq!(rotated_auto.0 % 16, 0);
    assert_eq!(rotated_auto.1 % 4, 0);
}

#[test]
fn tall_and_landscape_cover_screens_adapt_fov_safe_area_and_render_size() {
    // Tall portrait cover screen (9:17.4): VertPlus + letterboxed 16:9 safe area & video.
    let tall_fov = fov(FOLD_COVER_TALL).unwrap();
    assert_eq!(tall_fov.mode, FovMode::VertPlus);
    assert_eq!(tall_fov.horizontal, 1.0);
    assert!(near(tall_fov.vertical, (2092.0 * 16.0) / (1080.0 * 9.0)));

    let tall_hud = safe_area(FOLD_COVER_TALL).unwrap();
    assert_eq!(
        tall_hud,
        Rect {
            x: 0,
            y: 742,
            width: 1080,
            height: 607,
        }
    );
    assert_eq!(fit_rect(FOLD_COVER_TALL, (1920, 1080)).unwrap(), tall_hud);
    assert_eq!(render_size(FOLD_COVER_TALL, 1080).unwrap(), (640, 1080));

    for game in Game::ALL {
        let auto = automatic_size(FOLD_COVER_TALL, Some(game), 1200).unwrap();
        assert_eq!(auto.0 % 16, 0);
        assert_eq!(auto.1 % 4, 0);
        let (rt, rt2) = render_target_growth(auto);
        assert!(rt + rt2 <= 1200);
    }

    // Landscape cover screen (~17.4:9): HorPlus + pillarboxed 16:9 safe area & video.
    let wide_fov = fov(FOLD_COVER_LANDSCAPE).unwrap();
    assert_eq!(wide_fov.mode, FovMode::HorPlus);
    assert!(near(wide_fov.horizontal, 523.0 / 480.0));
    assert_eq!(wide_fov.vertical, 1.0);

    let wide_hud = safe_area(FOLD_COVER_LANDSCAPE).unwrap();
    assert_eq!(
        wide_hud,
        Rect {
            x: 86,
            y: 0,
            width: 1920,
            height: 1080,
        }
    );
    assert_eq!(
        fit_rect(FOLD_COVER_LANDSCAPE, (1920, 1080)).unwrap(),
        wide_hud
    );
    assert_eq!(
        render_size(FOLD_COVER_LANDSCAPE, 1080).unwrap(),
        (2092, 1080)
    );
    for game in Game::ALL {
        assert_eq!(
            automatic_size(FOLD_COVER_LANDSCAPE, Some(game), 1200).unwrap(),
            (2096, 1080)
        );
    }
}

#[test]
fn mid_game_fold_unfold_triggers_restart_prompt_and_preserves_aspect() {
    // 1. Start on landscape cover, unfold mid-game to 6:5.
    let start_wide = automatic_size(FOLD_COVER_LANDSCAPE, Some(Game::U2), 1200).unwrap();
    let unfold = evaluate_resize(start_wide, FOLD_UNFOLDED_6_5).unwrap();
    assert!(unfold.needs_restart);
    assert_eq!(unfold.fov.mode, FovMode::VertPlus);
    assert_eq!(unfold.safe_area, safe_area(FOLD_UNFOLDED_6_5).unwrap());
    assert_eq!(unfold.presenter_rect.x, 0);
    assert!(unfold.presenter_rect.y > 0);
    assert_eq!(unfold.presenter_rect.width, FOLD_UNFOLDED_6_5.0);
    assert!(unfold.presenter_rect.y + unfold.presenter_rect.height <= FOLD_UNFOLDED_6_5.1);

    // 2. Start unfolded 6:5, fold mid-game to landscape cover.
    let start_unfolded = automatic_size(FOLD_UNFOLDED_6_5, Some(Game::U3), 1200).unwrap();
    let fold_wide = evaluate_resize(start_unfolded, FOLD_COVER_LANDSCAPE).unwrap();
    assert!(fold_wide.needs_restart);
    assert_eq!(fold_wide.fov.mode, FovMode::HorPlus);
    assert_eq!(
        fold_wide.safe_area,
        safe_area(FOLD_COVER_LANDSCAPE).unwrap()
    );
    assert!(fold_wide.presenter_rect.x > 0);
    assert_eq!(fold_wide.presenter_rect.y, 0);
    assert_eq!(fold_wide.presenter_rect.height, FOLD_COVER_LANDSCAPE.1);
    assert!(fold_wide.presenter_rect.x + fold_wide.presenter_rect.width <= FOLD_COVER_LANDSCAPE.0);

    // 3. Tall cover -> unfolded 6:5 and unfolded 6:5 -> tall cover.
    let start_tall = automatic_size(FOLD_COVER_TALL, Some(Game::U1), 1200).unwrap();
    let tall_to_unfolded = evaluate_resize(start_tall, FOLD_UNFOLDED_6_5).unwrap();
    assert!(tall_to_unfolded.needs_restart);
    assert!(tall_to_unfolded.presenter_rect.x > 0);
    assert_eq!(tall_to_unfolded.presenter_rect.height, FOLD_UNFOLDED_6_5.1);
    assert!(
        tall_to_unfolded.presenter_rect.x + tall_to_unfolded.presenter_rect.width
            <= FOLD_UNFOLDED_6_5.0
    );

    let unfolded_to_tall = evaluate_resize(start_unfolded, FOLD_COVER_TALL).unwrap();
    assert!(unfolded_to_tall.needs_restart);
    assert_eq!(unfolded_to_tall.presenter_rect.x, 0);
    assert!(unfolded_to_tall.presenter_rect.y > 0);
    assert_eq!(unfolded_to_tall.presenter_rect.width, FOLD_COVER_TALL.0);
    assert!(
        unfolded_to_tall.presenter_rect.y + unfolded_to_tall.presenter_rect.height
            <= FOLD_COVER_TALL.1
    );

    // 4. Cover orientation rotation (tall <-> landscape) also offers restart.
    assert!(resize_needs_restart(FOLD_COVER_TALL, FOLD_COVER_LANDSCAPE));
    assert!(resize_needs_restart(FOLD_COVER_LANDSCAPE, FOLD_COVER_TALL));
}

#[test]
fn resize_threshold_ignores_small_insets_and_uninitialized_sizes() {
    assert_eq!(RESIZE_RESTART_THRESHOLD_PX, 16);

    // System-bar / gesture inset jitter within <= 16 px does not prompt a restart.
    let jitter_unfolded = evaluate_resize(
        FOLD_UNFOLDED_6_5,
        (
            FOLD_UNFOLDED_6_5.0,
            FOLD_UNFOLDED_6_5.1 - RESIZE_RESTART_THRESHOLD_PX,
        ),
    )
    .unwrap();
    assert!(!jitter_unfolded.needs_restart);

    let jitter_cover = evaluate_resize(
        FOLD_COVER_LANDSCAPE,
        (
            FOLD_COVER_LANDSCAPE.0 - RESIZE_RESTART_THRESHOLD_PX,
            FOLD_COVER_LANDSCAPE.1,
        ),
    )
    .unwrap();
    assert!(!jitter_cover.needs_restart);

    // Crossing > 16 px triggers the restart prompt.
    assert!(resize_needs_restart(
        FOLD_UNFOLDED_6_5,
        (
            FOLD_UNFOLDED_6_5.0,
            FOLD_UNFOLDED_6_5.1 - (RESIZE_RESTART_THRESHOLD_PX + 1),
        ),
    ));
    assert!(resize_needs_restart(
        FOLD_COVER_TALL,
        (
            FOLD_COVER_TALL.0 - (RESIZE_RESTART_THRESHOLD_PX + 1),
            FOLD_COVER_TALL.1,
        ),
    ));

    // Zero / uninitialized startup or current sizes never trigger a spurious restart.
    assert!(!resize_needs_restart((0, 0), FOLD_UNFOLDED_6_5));
    assert!(!resize_needs_restart(FOLD_UNFOLDED_6_5, (0, 0)));
    assert!(!resize_needs_restart((0, 1840), FOLD_UNFOLDED_6_5));
    assert!(matches!(
        evaluate_resize((0, 0), FOLD_UNFOLDED_6_5),
        Err(Error::RenderSize(_))
    ));
    assert!(matches!(
        evaluate_resize(FOLD_UNFOLDED_6_5, (0, 1840)),
        Err(Error::RenderSize(_))
    ));
}
