//! Default-off guard for Uncharted 3 readback flags (Issue #191).
//!
//! Enforces the 2026-11-02 owner ruling ("Keep ours off until measured"): both
//! `BB_TINY_READBACK` and `BB_READBACK_SMALL_BUFFERS` must explicitly default to `"0"`
//! for Uncharted 3 across `fix_defaults`, `build_env` (for every device profile),
//! `recipe` chapter fixes, and `launch::Session::env`, unless explicitly overridden.
//!
//! Explicit `"0"` (not merely omitting the variable) is required because
//! `gpu/shadps4/video_core/renderer_vulkan/vk_rasterizer.cpp` enables
//! `NoteSmallWrittenBuffers` whenever `BB_READBACK_SMALL_BUFFERS` is unset.

use std::collections::BTreeMap;
use std::path::PathBuf;
use uncharted_core::games::Game;
use uncharted_core::launch::{Paths, Session};
use uncharted_core::profile;
use uncharted_core::recipe::{chapter_env, fixes_at, CHAPTER_FIXES};
use uncharted_core::run_env::{build_env, fix_defaults, Settings};

fn env_map(pairs: Vec<(String, String)>) -> BTreeMap<String, String> {
    pairs.into_iter().collect()
}

fn where_json(game: &str, title: &str, subtitle: &str) -> String {
    serde_json::json!({
        "game": game,
        "title": title,
        "subtitle": subtitle,
        "detail": "",
        "saved_at": "2026-11-02T12:00:00Z",
    })
    .to_string()
}

#[test]
fn u3_fix_defaults_explicitly_sets_both_readback_flags_off() {
    let defaults = fix_defaults(&BTreeMap::new(), Game::U3);
    assert_eq!(
        defaults.get("BB_TINY_READBACK").map(String::as_str),
        Some("0"),
        "BB_TINY_READBACK must explicitly default to 0 for U3"
    );
    assert_eq!(
        defaults
            .get("BB_READBACK_SMALL_BUFFERS")
            .map(String::as_str),
        Some("0"),
        "BB_READBACK_SMALL_BUFFERS must explicitly default to 0 for U3"
    );
}

#[test]
fn u3_build_env_keeps_both_readback_flags_off_across_all_profiles() {
    for dev in profile::builtin() {
        let built =
            env_map(build_env(Game::U3, &Settings::default(), dev, &BTreeMap::new()).unwrap());
        assert_eq!(
            built.get("BB_TINY_READBACK").map(String::as_str),
            Some("0"),
            "profile {}",
            dev.id
        );
        assert_eq!(
            built.get("BB_READBACK_SMALL_BUFFERS").map(String::as_str),
            Some("0"),
            "profile {}",
            dev.id
        );
        assert!(
            !built.contains_key("BB_READBACK_SMALL_GATE"),
            "profile {} should not enable BB_READBACK_SMALL_GATE by default",
            dev.id
        );
    }
}

#[test]
fn u3_chapter_fixes_do_not_enable_readback_flags() {
    for fix in CHAPTER_FIXES {
        if fix.game == Game::U3 {
            for (name, _) in fix.on.iter().chain(fix.off) {
                assert!(
                    !matches!(*name, "BB_TINY_READBACK" | "BB_READBACK_SMALL_BUFFERS"),
                    "unexpected U3 chapter fix {} touching {name}",
                    fix.key
                );
            }
        }
    }

    for chapter in [
        "Nepal",
        "Borneo",
        "Urban Warfare",
        "London Underground",
        "Chateau",
        "Yemen",
        "Rub' al Khali",
        "Iram of the Pillars",
    ] {
        let place = where_json("u3", "Uncharted 3", chapter);
        assert!(fixes_at(Game::U3, Some(&place)).is_empty(), "{chapter}");
        let env = chapter_env(&BTreeMap::new(), Game::U3, Some(&place));
        assert!(!env.contains_key("BB_TINY_READBACK"), "{chapter}");
        assert!(!env.contains_key("BB_READBACK_SMALL_BUFFERS"), "{chapter}");
    }
}

#[test]
fn u3_launch_session_env_keeps_both_readback_flags_off() {
    let profile = profile::by_id("strix-halo-tablet").unwrap();
    let settings = Settings::default();
    let player_env = BTreeMap::new();
    let paths = Paths::in_data_dir(&PathBuf::from("/tmp/u3-readback-guard-test"));
    let session = Session {
        settings: &settings,
        profile,
        player_env: &player_env,
        paths: &paths,
        os: profile.os,
    };

    let built = env_map(session.env(Game::U3).unwrap());
    assert_eq!(built.get("BB_TINY_READBACK").map(String::as_str), Some("0"));
    assert_eq!(
        built.get("BB_READBACK_SMALL_BUFFERS").map(String::as_str),
        Some("0")
    );
    assert_eq!(
        built.get("BB_COPY64_EOP_READBACK").map(String::as_str),
        Some("1")
    );
}

#[test]
fn u3_explicit_overrides_for_ab_matrix_are_preserved() {
    let profile = profile::by_id("strix-halo-tablet").unwrap();
    let matrix = [
        ("0", "0"),
        ("1", "0"),
        ("0", "1"),
        ("1", "1"),
        ("0", "sync"),
        ("1", "sync"),
    ];

    for (tiny, small) in matrix {
        let player: BTreeMap<String, String> = [
            ("BB_TINY_READBACK".to_owned(), tiny.to_owned()),
            ("BB_READBACK_SMALL_BUFFERS".to_owned(), small.to_owned()),
        ]
        .into_iter()
        .collect();

        let defaults = fix_defaults(&player, Game::U3);
        assert!(!defaults.contains_key("BB_TINY_READBACK"));
        assert!(!defaults.contains_key("BB_READBACK_SMALL_BUFFERS"));

        let built_player =
            env_map(build_env(Game::U3, &Settings::default(), profile, &player).unwrap());
        assert_eq!(built_player["BB_TINY_READBACK"], tiny);
        assert_eq!(built_player["BB_READBACK_SMALL_BUFFERS"], small);

        let mut settings = Settings::default();
        settings
            .extra
            .insert("BB_TINY_READBACK".to_owned(), tiny.to_owned());
        settings
            .extra
            .insert("BB_READBACK_SMALL_BUFFERS".to_owned(), small.to_owned());
        let built_settings =
            env_map(build_env(Game::U3, &settings, profile, &BTreeMap::new()).unwrap());
        assert_eq!(built_settings["BB_TINY_READBACK"], tiny);
        assert_eq!(built_settings["BB_READBACK_SMALL_BUFFERS"], small);
    }

    // Player env overrides settings.extra when both are provided.
    let mut settings = Settings::default();
    settings
        .extra
        .insert("BB_TINY_READBACK".to_owned(), "0".to_owned());
    settings
        .extra
        .insert("BB_READBACK_SMALL_BUFFERS".to_owned(), "0".to_owned());
    let player: BTreeMap<String, String> = [
        ("BB_TINY_READBACK".to_owned(), "1".to_owned()),
        ("BB_READBACK_SMALL_BUFFERS".to_owned(), "1".to_owned()),
    ]
    .into_iter()
    .collect();
    let built = env_map(build_env(Game::U3, &settings, profile, &player).unwrap());
    assert_eq!(built["BB_TINY_READBACK"], "1");
    assert_eq!(built["BB_READBACK_SMALL_BUFFERS"], "1");
}
