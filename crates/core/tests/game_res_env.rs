//! `BB_GAME_RES` and its heap variables (`uncharted_core::render_size::game_res_env`, issue #139).
//! The expected values are what `run.py`'s `game_res()` produces for the same input.

use std::collections::BTreeMap;
use uncharted_core::render_size::game_res_env;

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn res(text: &str) -> BTreeMap<String, String> {
    game_res_env(&map(&[("BB_GAME_RES", text)]))
}

#[test]
fn qhd_gets_the_heaps_and_extra_direct_memory() {
    let want = map(&[
        ("BB_DMEM_MB", "5440"),
        ("BB_GAME_FREE_MB", "0"),
        ("BB_GAME_MOVIE_MB", "6"),
        ("BB_GAME_RES", "2560x1440"),
        ("BB_GAME_RT2_MB", "128"),
        ("BB_GAME_RT_MB", "256"),
    ]);
    assert_eq!(res("2560x1440"), want);
}

#[test]
fn uhd_ultrawide_and_the_largest_size() {
    let uhd = res("3840x2160");
    assert_eq!(uhd["BB_GAME_RT_MB"], "960");
    assert_eq!(uhd["BB_GAME_RT2_MB"], "512");
    assert_eq!(uhd["BB_GAME_MOVIE_MB"], "14");
    assert_eq!(uhd["BB_DMEM_MB"], "6528");
    let wide = res("5120x2160");
    assert_eq!(wide["BB_GAME_RT_MB"], "1344");
    assert_eq!(wide["BB_GAME_RT2_MB"], "704");
    assert_eq!(wide["BB_DMEM_MB"], "7104");
    let max = res("7680x4320");
    assert_eq!(max["BB_GAME_RT_MB"], "4544");
    assert_eq!(max["BB_GAME_RT2_MB"], "2304");
    assert_eq!(max["BB_GAME_MOVIE_MB"], "48");
    assert_eq!(max["BB_DMEM_MB"], "11904");
}

#[test]
fn the_size_is_read_case_insensitively_and_with_the_multiplication_sign() {
    let want = res("2560x1440");
    assert_eq!(res("2560X1440"), want);
    assert_eq!(res("2560\u{d7}1440"), want);
    assert_eq!(res("3840X2160"), res("3840x2160"));
}

#[test]
fn a_size_below_1080p_has_no_pool_to_grow_and_no_direct_memory() {
    let want = map(&[
        ("BB_GAME_FREE_MB", "0"),
        ("BB_GAME_MOVIE_MB", "4"),
        ("BB_GAME_RES", "1280x720"),
        ("BB_GAME_RT2_MB", "0"),
        ("BB_GAME_RT_MB", "0"),
    ]);
    assert_eq!(res("1280x720"), want);
}

#[test]
fn nothing_is_returned_for_1080p_a_missing_a_bad_or_an_out_of_range_size() {
    for text in [
        "1920x1080",
        "big",
        "",
        "2560",
        "639x360",
        "7681x4320",
        "640x359",
        "7680x4321",
        "axb",
    ] {
        assert!(res(text).is_empty(), "{text:?}");
    }
    assert!(game_res_env(&BTreeMap::new()).is_empty());
    assert!(game_res_env(&map(&[("BB_GAME_RT_MB", "512")])).is_empty());
}

#[test]
fn an_odd_width_is_not_refused_here() {
    assert_eq!(res("2561x1440")["BB_GAME_RES"], "2561x1440");
}

#[test]
fn a_heap_variable_the_player_set_to_digits_keeps_its_value() {
    let player = map(&[
        ("BB_GAME_RES", "2560x1440"),
        ("BB_GAME_RT_MB", "512"),
        ("BB_GAME_FREE_MB", "100"),
        ("BB_DMEM_MB", "6000"),
        ("BB_GAME_MOVIE_MB", "8"),
    ]);
    let want = map(&[
        ("BB_DMEM_MB", "6000"),
        ("BB_GAME_FREE_MB", "100"),
        ("BB_GAME_MOVIE_MB", "8"),
        ("BB_GAME_RES", "2560x1440"),
        ("BB_GAME_RT2_MB", "128"),
        ("BB_GAME_RT_MB", "512"),
    ]);
    assert_eq!(game_res_env(&player), want);
}

#[test]
fn the_pool_size_uses_the_players_heap_value() {
    let got = game_res_env(&map(&[
        ("BB_GAME_RES", "2560x1440"),
        ("BB_GAME_RT2_MB", "64"),
    ]));
    assert_eq!(got["BB_GAME_RT2_MB"], "64");
    assert_eq!(got["BB_GAME_RT_MB"], "256");
    assert_eq!(got["BB_DMEM_MB"], "5376");
}
