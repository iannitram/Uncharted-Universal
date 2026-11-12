//! Launch environment assembly, including the explicit game-render size heap settings.

use std::collections::BTreeMap;
use uncharted_core::games::Game;
use uncharted_core::platform::Os;
use uncharted_core::profile::DeviceProfile;
use uncharted_core::run_env::{build_env, fix_defaults, Settings};

const PROFILE_ENV: &[(&str, &str)] = &[("BB_GAME_RES", "3840x2160"), ("PROFILE_ONLY", "profile")];
const PROFILE: DeviceProfile = DeviceProfile {
    id: "test",
    name: "Test",
    hardware: "synthetic",
    os: Os::Linux,
    env: PROFILE_ENV,
    frame_rates: &[30, 60],
    power: "",
};

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    build_env(Game::U1, &Settings::default(), &PROFILE, &map(pairs))
        .unwrap()
        .into_iter()
        .collect()
}

#[test]
fn launch_env_derives_game_heap_values_from_the_selected_game_resolution() {
    let built = env(&[("BB_GAME_RES", "2560X1440")]);

    assert_eq!(built["BB_GAME_RES"], "2560x1440");
    assert_eq!(built["BB_GAME_RT_MB"], "256");
    assert_eq!(built["BB_GAME_RT2_MB"], "128");
    assert_eq!(built["BB_GAME_FREE_MB"], "0");
    assert_eq!(built["BB_GAME_MOVIE_MB"], "6");
    assert_eq!(built["BB_DMEM_MB"], "5440");
}

#[test]
fn game_resolution_preserves_explicit_heap_values_and_player_precedence() {
    let built = env(&[
        ("BB_GAME_RES", "2560x1440"),
        ("BB_GAME_RT_MB", "512"),
        ("PROFILE_ONLY", "player"),
    ]);

    assert_eq!(built["BB_GAME_RES"], "2560x1440");
    assert_eq!(built["BB_GAME_RT_MB"], "512");
    assert_eq!(built["PROFILE_ONLY"], "player");
}

#[test]
fn invalid_or_default_game_resolution_is_removed_from_the_launch_env() {
    for value in ["bad", "1920x1080", "7681x4320"] {
        let built = env(&[("BB_GAME_RES", value)]);
        assert!(!built.contains_key("BB_GAME_RES"), "{value:?}");
    }
}

#[test]
fn per_game_defaults_are_applied_only_when_the_player_did_not_set_them() {
    let defaults = fix_defaults(&BTreeMap::new(), Game::U2);
    assert_eq!(defaults["BB_GPU_MERGE"], "0");
    assert_eq!(defaults["BB_TINY_READBACK"], "1");
    assert_eq!(defaults["BB_SCALAR_COPY64_SYNC"], "2");
    assert_eq!(defaults["BB_SCALAR_COPY64_DEFER"], "1");
    assert_eq!(defaults["BB_READBACK_SMALL_BUFFERS"], "1");

    let player = map(&[("BB_GPU_MERGE", "1"), ("BB_SCALAR_COPY64_SYNC", "5")]);
    let defaults = fix_defaults(&player, Game::U1);
    assert!(!defaults.contains_key("BB_GPU_MERGE"));
    assert!(!defaults.contains_key("BB_SCALAR_COPY64_SYNC"));
    assert!(!defaults.contains_key("BB_SCALAR_COPY64_DEFER"));
    assert_eq!(defaults["BB_TOGGLE2"], "12");
}

#[test]
fn small_buffer_readback_is_explicitly_off_for_uncharted_1_and_3() {
    for game in [Game::U1, Game::U3] {
        let defaults = fix_defaults(&BTreeMap::new(), game);
        assert_eq!(defaults["BB_READBACK_SMALL_BUFFERS"], "0", "{game:?}");

        let player = map(&[("BB_READBACK_SMALL_BUFFERS", "1")]);
        let defaults = fix_defaults(&player, game);
        assert!(
            !defaults.contains_key("BB_READBACK_SMALL_BUFFERS"),
            "{game:?}"
        );
    }

    let defaults = fix_defaults(&BTreeMap::new(), Game::U2);
    assert_eq!(defaults["BB_READBACK_SMALL_BUFFERS"], "1");
}

#[test]
fn small_buffer_player_override_is_kept_in_the_launch_env() {
    let built = env(&[("BB_READBACK_SMALL_BUFFERS", "1")]);
    assert_eq!(built["BB_READBACK_SMALL_BUFFERS"], "1");
}

fn env_for(game: Game, pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    let plain = DeviceProfile {
        env: &[],
        ..PROFILE
    };
    build_env(game, &Settings::default(), &plain, &map(pairs))
        .unwrap()
        .into_iter()
        .collect()
}

#[test]
fn fixed_renderer_values_and_vblank_default_are_set_unless_the_player_sets_them() {
    let built = env_for(Game::U2, &[]);
    assert_eq!(built["BB_VELOCITY_PS"], "f4b18264");
    assert_eq!(built["BB_UPSCALE_BEFORE_CS"], "aec539f0");
    assert_eq!(built["BB_HUDLESS_PS"], "484cdbf5");
    assert_eq!(built["BB_JITTER_PS"], "f614041e,b2a42acf,1a08df2d");
    assert_eq!(built["BB_VBLANK_HZ"], "60");

    let built = env_for(Game::U2, &[("BB_VBLANK_HZ", "30"), ("BB_HUDLESS_PS", "x")]);
    assert_eq!(built["BB_VBLANK_HZ"], "30");
    assert_eq!(built["BB_HUDLESS_PS"], "x");
}

#[test]
fn profile_vblank_beats_the_default_and_settings_beat_the_profile() {
    const HANDHELD: &[(&str, &str)] = &[("BB_VBLANK_HZ", "30"), ("BB_RESOLUTION", "1280x720")];
    let profile = DeviceProfile {
        env: HANDHELD,
        ..PROFILE
    };
    let settings = Settings {
        resolution: Some("2560x1440".to_owned()),
        upscale_quality: Some("balanced".to_owned()),
        ..Settings::default()
    };
    let built: BTreeMap<_, _> = build_env(Game::U1, &settings, &profile, &BTreeMap::new())
        .unwrap()
        .into_iter()
        .collect();
    assert_eq!(built["BB_VBLANK_HZ"], "30");
    assert_eq!(built["BB_RESOLUTION"], "2560x1440");
    assert_eq!(built["BB_UPSCALE_QUALITY"], "balanced");
}

#[test]
fn copy64_eop_readback_is_on_for_u3_only_and_the_player_wins() {
    assert_eq!(env_for(Game::U3, &[])["BB_COPY64_EOP_READBACK"], "1");
    assert!(!env_for(Game::U1, &[]).contains_key("BB_COPY64_EOP_READBACK"));
    assert!(!env_for(Game::U2, &[]).contains_key("BB_COPY64_EOP_READBACK"));
    let built = env_for(Game::U3, &[("BB_COPY64_EOP_READBACK", "0")]);
    assert_eq!(built["BB_COPY64_EOP_READBACK"], "0");
    let built = env_for(Game::U2, &[("BB_COPY64_EOP_READBACK", "1")]);
    assert_eq!(built["BB_COPY64_EOP_READBACK"], "1");
}

#[test]
fn u1_skips_the_collection_menu_unless_told_otherwise() {
    assert_eq!(env_for(Game::U1, &[])["BB_SKIP_COLLECTION_MENU"], "1");
    let built = env_for(Game::U1, &[("BB_SKIP_COLLECTION_MENU", "0")]);
    assert_eq!(built["BB_SKIP_COLLECTION_MENU"], "0");
    let built = env_for(Game::U1, &[("BB_U1_DIRECT", "1")]);
    assert!(!built.contains_key("BB_SKIP_COLLECTION_MENU"));
    assert!(!env_for(Game::U2, &[]).contains_key("BB_SKIP_COLLECTION_MENU"));
    assert!(!env_for(Game::U3, &[]).contains_key("BB_SKIP_COLLECTION_MENU"));
}

#[test]
fn game_res_sets_scene_and_movie_size_and_stale_derived_values_are_dropped() {
    let built = env_for(Game::U2, &[("BB_GAME_RES", "2560x1440")]);
    assert_eq!(built["BB_SCENE_SIZE"], "2560x1440");
    assert_eq!(built["BB_MOVIE_SIZE"], "2560x1440");
    assert_eq!(built["BB_DMEM_MB_USER"], built["BB_DMEM_MB"]);

    let built = env_for(
        Game::U2,
        &[
            ("BB_SCENE_SIZE", "1x1"),
            ("BB_RENDER_RES", "1x1"),
            ("BB_OUTPUT_RES", "1x1"),
        ],
    );
    for name in ["BB_SCENE_SIZE", "BB_RENDER_RES", "BB_OUTPUT_RES"] {
        assert!(!built.contains_key(name), "{name}");
    }
}

#[test]
fn a_direct_memory_size_the_player_set_is_remembered() {
    let built = env_for(Game::U2, &[("BB_DMEM_MB", "8192")]);
    assert_eq!(built["BB_DMEM_MB_USER"], "8192");
    assert!(!env_for(Game::U2, &[]).contains_key("BB_DMEM_MB_USER"));
}

#[test]
fn u3_defaults_match_run_py() {
    let defaults = fix_defaults(&BTreeMap::new(), Game::U3);
    assert_eq!(defaults["BB_SCALAR_COPY64_SYNC"], "0");
    assert_eq!(defaults["BB_SCALAR_COPY64_DEFER"], "0");
    assert_eq!(defaults["BB_TINY_READBACK"], "0");
    assert_eq!(defaults["BB_TOGGLE2"], "12");
    assert!(!fix_defaults(&BTreeMap::new(), Game::U2).contains_key("BB_TOGGLE2"));
}
