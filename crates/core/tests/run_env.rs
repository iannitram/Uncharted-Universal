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
