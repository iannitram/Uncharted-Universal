//! Fixes that follow the chapter (`uncharted_core::recipe`, issue #178).

use std::collections::BTreeMap;
use uncharted_core::games::Game;
use uncharted_core::recipe::{
    chapter_env, chapter_of, fixes_at, fixes_in, ChapterFix, CHAPTER_FIXES,
};
use uncharted_core::run_env::fix_defaults;

/// A `where.json` as `savedata::commit` writes it, with made-up save text.
fn place(game: &str, title: &str, subtitle: &str) -> String {
    serde_json::json!({
        "game": game, "title": title, "subtitle": subtitle, "detail": "",
        "saved_at": "2026-11-12T10:00:00Z",
    })
    .to_string()
}

fn u2(subtitle: &str) -> String {
    place("u2", "Uncharted 2", subtitle)
}

fn state(game: Game, where_json: Option<&str>, key: &str) -> Option<bool> {
    fixes_at(game, where_json)
        .into_iter()
        .find(|s| s.fix.key == key)
        .map(|s| s.on)
}

fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn the_flicker_fix_turns_on_in_nepal_and_off_as_the_place_changes() {
    let walk = [
        ("Borneo", false),
        ("Nepal", true),
        ("Urban Warfare", true),
        ("Getting Warmer", false),
        ("Desperate Times", true),
        ("Some other place", false),
    ];
    for (subtitle, on) in walk {
        let json = u2(subtitle);
        assert_eq!(
            state(Game::U2, Some(&json), "fix_small_buffers"),
            Some(on),
            "{subtitle}"
        );
        let got = chapter_env(&BTreeMap::new(), Game::U2, Some(&json));
        assert_eq!(
            got.get("BB_READBACK_SMALL_BUFFERS").map(String::as_str),
            Some(if on { "1" } else { "0" }),
            "{subtitle}"
        );
    }
}

#[test]
fn the_glitch_fixes_turn_off_in_the_jungle_and_back_on_elsewhere() {
    for (subtitle, on) in [
        ("Nepal", true),
        ("Borneo", false),
        ("Getting Warmer", false),
        ("Urban Warfare", true),
    ] {
        let json = u2(subtitle);
        for key in ["fix_tiny_readback", "fix_u2_gpu_results"] {
            assert_eq!(
                state(Game::U2, Some(&json), key),
                Some(on),
                "{key} {subtitle}"
            );
        }
        let got = chapter_env(&BTreeMap::new(), Game::U2, Some(&json));
        let (tiny, merge) = if on { ("1", "0") } else { ("0", "1") };
        assert_eq!(got["BB_TINY_READBACK"], tiny, "{subtitle}");
        assert_eq!(got["BB_GPU_MERGE"], merge, "{subtitle}");
    }
}

#[test]
fn chapters_match_as_a_case_insensitive_part_of_the_save_text() {
    for subtitle in ["NEPAL", "chapter 5 - urban warfare", "  Kathmandu, Nepal  "] {
        assert_eq!(
            state(Game::U2, Some(&u2(subtitle)), "fix_small_buffers"),
            Some(true),
            "{subtitle}"
        );
    }
}

#[test]
fn the_chapter_is_the_subtitle_else_the_title() {
    assert_eq!(
        chapter_of(Game::U2, &place("u2", "Nepal", "Borneo")).as_deref(),
        Some("Borneo")
    );
    assert_eq!(
        chapter_of(Game::U2, &place("u2", "Nepal", "  ")).as_deref(),
        Some("Nepal")
    );
    assert_eq!(chapter_of(Game::U2, &place("u2", "", "")), None);
}

#[test]
fn an_unknown_place_leaves_every_fix_at_its_default() {
    let other_game = place("u3", "", "Nepal");
    let no_game = r#"{"title": "", "subtitle": "Nepal"}"#.to_owned();
    for json in [
        None,
        Some("{not json"),
        Some(""),
        Some("[]"),
        Some(other_game.as_str()),
        Some(no_game.as_str()),
        Some(r#"{"game": "u2", "title": "", "subtitle": ""}"#),
    ] {
        assert!(fixes_at(Game::U2, json).is_empty(), "{json:?}");
        assert!(chapter_env(&BTreeMap::new(), Game::U2, json).is_empty());
    }
}

#[test]
fn other_games_have_no_chapter_fixes_yet() {
    for game in [Game::U1, Game::U3] {
        let json = place(game.key(), "", "Nepal");
        assert!(fixes_at(game, Some(&json)).is_empty());
    }
}

#[test]
fn a_variable_the_player_set_keeps_its_fix_out() {
    let player = env(&[("BB_READBACK_SMALL_BUFFERS", "1")]);
    let got = chapter_env(&player, Game::U2, Some(&u2("Borneo")));
    assert!(!got.contains_key("BB_READBACK_SMALL_BUFFERS"));
    assert_eq!(got["BB_TINY_READBACK"], "0");
    assert_eq!(got["BB_GPU_MERGE"], "1");
}

#[test]
fn each_fix_switches_variables_the_game_defaults_set() {
    let defaults = fix_defaults(&BTreeMap::new(), Game::U2);
    for fix in CHAPTER_FIXES {
        assert_eq!(fix.game, Game::U2);
        assert!(!fix.chapters.is_empty(), "{}", fix.key);
        for (name, value) in fix.on {
            assert_eq!(
                defaults.get(*name).map(String::as_str),
                Some(*value),
                "{}: today's default is the fix on",
                fix.key
            );
        }
        let names =
            |pairs: &[(&str, &str)]| pairs.iter().map(|(n, _)| n.to_string()).collect::<Vec<_>>();
        assert_eq!(names(fix.on), names(fix.off), "{}", fix.key);
    }
}

static WIDE: &[ChapterFix] = &[
    ChapterFix {
        key: "on_here",
        game: Game::U1,
        on: &[("BB_A", "1")],
        off: &[("BB_A", "0")],
        chapters: &["Docks", ""],
        on_in_chapters: true,
        issue: "#0",
    },
    ChapterFix {
        key: "off_here",
        game: Game::U1,
        on: &[("BB_B", "1")],
        off: &[("BB_B", "0")],
        chapters: &["Docks"],
        on_in_chapters: false,
        issue: "#0",
    },
    ChapterFix {
        key: "other_game",
        game: Game::U3,
        on: &[("BB_C", "1")],
        off: &[("BB_C", "0")],
        chapters: &["Docks"],
        on_in_chapters: true,
        issue: "#0",
    },
];

#[test]
fn a_list_keeps_its_order_and_its_game_and_an_empty_name_matches_nothing() {
    let docks = place("u1", "", "The Docks");
    let got: Vec<(&str, bool)> = fixes_in(WIDE, Game::U1, Some(&docks))
        .iter()
        .map(|s| (s.fix.key, s.on))
        .collect();
    assert_eq!(got, [("on_here", true), ("off_here", false)]);

    let fort = place("u1", "", "The Fortress");
    let got: Vec<(&str, bool)> = fixes_in(WIDE, Game::U1, Some(&fort))
        .iter()
        .map(|s| (s.fix.key, s.on))
        .collect();
    assert_eq!(got, [("on_here", false), ("off_here", true)]);
}
