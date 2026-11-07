//! Known-issue list and crash matcher (`uncharted_core::logs`, issue #169).

use std::fs;
use std::path::PathBuf;
use uncharted_core::games::Game;
use uncharted_core::logs::{
    load_known_issues, match_known, parse_known_issues, KnownIssue, Toggle,
};

const GENERAL: &str = r#"{"id": "a", "signature": "Assertion failed", "game": "any", "locations": [],
  "summary": "general", "fixed_in": null, "toggle": null, "issue": "u/1"}"#;
const LOCATED: &str = r#"{"id": "b", "signature": "Assertion failed", "game": "any",
  "locations": ["Docks", "Nepal"], "summary": "located", "fixed_in": "0.3.2",
  "toggle": {"key": "fix", "value": "on"}, "issue": "u/2"}"#;
const U2_ONLY: &str = r#"{"id": "c", "signature": "GPU hang", "game": "u2", "summary": "u2 only"}"#;

fn parse(entries: &[&str]) -> Vec<KnownIssue> {
    parse_known_issues(&format!("[{}]", entries.join(","))).unwrap()
}

fn ids(issues: &[KnownIssue]) -> Vec<&str> {
    issues.iter().map(|i| i.id.as_str()).collect()
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("uu-known-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn damaged_json_is_a_launch_error() {
    for text in ["{not json", "", r#"{"id": "a", "signature": "x"}"#, "5"] {
        let err = parse_known_issues(text).unwrap_err();
        assert_eq!(err.code(), "launch");
        assert!(err.to_string().contains("known_issues.json is damaged"));
    }
}

#[test]
fn load_gives_an_empty_list_for_a_missing_or_damaged_file() {
    let dir = TempDir::new("load");
    assert!(load_known_issues(&dir.0.join("nope.json")).is_empty());
    for (name, text) in [
        ("bad.json", "{not json"),
        ("obj.json", r#"{"id": "a", "signature": "x"}"#),
    ] {
        fs::write(dir.0.join(name), text).unwrap();
        assert!(load_known_issues(&dir.0.join(name)).is_empty());
    }
}

#[test]
fn load_reads_a_valid_file_in_order() {
    let dir = TempDir::new("valid");
    let path = dir.0.join("issues.json");
    fs::write(&path, format!("[{GENERAL},{LOCATED}]")).unwrap();
    assert_eq!(ids(&load_known_issues(&path)), ["a", "b"]);
}

#[test]
fn load_reads_the_shipped_known_issues_file() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/known_issues.json");
    let issues = load_known_issues(&path);
    assert_eq!(
        issues.len(),
        3,
        "shipped entries should reflect verified crash contexts"
    );
    assert!(issues
        .iter()
        .all(|issue| issue.issue.starts_with("https://github.com/")));
}

#[test]
fn a_valid_list_parses_in_order_with_every_field() {
    let got = parse(&[GENERAL, LOCATED]);
    assert_eq!(ids(&got), ["a", "b"]);
    assert_eq!(got[0].game, None);
    assert!(got[0].locations.is_empty());
    assert_eq!(got[0].fixed_in, None);
    assert_eq!(got[0].toggle, None);
    assert_eq!(got[0].summary, "general");
    assert_eq!(got[1].signature, "Assertion failed");
    assert_eq!(got[1].locations, ["Docks", "Nepal"]);
    assert_eq!(got[1].fixed_in.as_deref(), Some("0.3.2"));
    assert_eq!(
        got[1].toggle,
        Some(Toggle {
            key: "fix".into(),
            value: "on".into()
        })
    );
    assert_eq!(got[1].issue, "u/2");
}

#[test]
fn missing_optional_keys_take_their_defaults() {
    let got = parse(&[U2_ONLY]);
    assert_eq!(got[0].game, Some(Game::U2));
    assert!(got[0].locations.is_empty());
    assert_eq!(got[0].fixed_in, None);
    assert_eq!(got[0].toggle, None);
    assert_eq!(got[0].issue, "");
}

#[test]
fn entries_without_id_or_signature_or_with_an_unknown_game_are_skipped() {
    let got = parse(&[
        r#"{"signature": "s"}"#,
        r#"{"id": "x"}"#,
        r#"{"id": "", "signature": "s"}"#,
        r#"{"id": "z", "signature": "s", "game": "u9"}"#,
        "5",
        r#""text""#,
        r#"{"id": "ok", "signature": "s"}"#,
    ]);
    assert_eq!(ids(&got), ["ok"]);
}

fn run_match<'a>(
    line: &str,
    game: Game,
    location: Option<&str>,
    issues: &'a [KnownIssue],
) -> Option<&'a str> {
    match_known(issues, game, location, line).map(|i| i.id.as_str())
}

#[test]
fn an_empty_line_matches_nothing() {
    assert_eq!(
        run_match("", Game::U1, Some("Docks"), &parse(&[GENERAL])),
        None
    );
}

#[test]
fn the_signature_is_a_case_sensitive_substring() {
    let issues = parse(&[GENERAL]);
    assert_eq!(
        run_match("x: Assertion failed at y", Game::U1, None, &issues),
        Some("a")
    );
    assert_eq!(run_match("assertion failed", Game::U1, None, &issues), None);
    assert_eq!(
        run_match("segfault", Game::U1, None, &parse(&[GENERAL, LOCATED])),
        None
    );
}

#[test]
fn the_game_must_equal_or_be_any() {
    let u2 = parse(&[U2_ONLY]);
    assert_eq!(run_match("GPU hang", Game::U1, None, &u2), None);
    assert_eq!(run_match("GPU hang", Game::U2, None, &u2), Some("c"));
    assert_eq!(
        run_match("Assertion failed", Game::U3, None, &parse(&[GENERAL])),
        Some("a")
    );
}

#[test]
fn locations_match_as_a_case_insensitive_substring() {
    let issues = parse(&[LOCATED]);
    assert_eq!(
        run_match(
            "Assertion failed",
            Game::U1,
            Some("the DOCKS level"),
            &issues
        ),
        Some("b")
    );
    assert_eq!(
        run_match(
            "Assertion failed",
            Game::U1,
            Some("kathmandu, nepal"),
            &issues
        ),
        Some("b")
    );
}

#[test]
fn an_entry_with_locations_needs_a_location_that_fits() {
    let issues = parse(&[LOCATED]);
    for location in [Some("Desert"), None, Some("")] {
        assert_eq!(
            run_match("Assertion failed", Game::U1, location, &issues),
            None
        );
    }
}

#[test]
fn an_entry_without_locations_matches_anywhere() {
    let issues = parse(&[U2_ONLY]);
    assert_eq!(run_match("GPU hang", Game::U2, None, &issues), Some("c"));
    assert_eq!(
        run_match("GPU hang", Game::U2, Some("Docks"), &issues),
        Some("c")
    );
}

#[test]
fn a_located_entry_beats_a_general_one_in_either_order() {
    for entries in [[GENERAL, LOCATED], [LOCATED, GENERAL]] {
        let issues = parse(&entries);
        assert_eq!(
            run_match("Assertion failed", Game::U1, Some("Docks"), &issues),
            Some("b")
        );
    }
}

#[test]
fn the_general_entry_is_used_when_the_location_does_not_fit() {
    let issues = parse(&[LOCATED, GENERAL]);
    assert_eq!(
        run_match("Assertion failed", Game::U1, Some("Desert"), &issues),
        Some("a")
    );
}

#[test]
fn the_first_in_the_list_wins_among_equals() {
    let twin = GENERAL.replace(r#""id": "a""#, r#""id": "a2""#);
    let twin_located = LOCATED.replace(r#""id": "b""#, r#""id": "b2""#);
    let line = "Assertion failed";
    assert_eq!(
        run_match(line, Game::U1, None, &parse(&[GENERAL, &twin])),
        Some("a")
    );
    assert_eq!(
        run_match(line, Game::U1, None, &parse(&[&twin, GENERAL])),
        Some("a2")
    );
    let issues = parse(&[GENERAL, LOCATED, &twin_located]);
    assert_eq!(run_match(line, Game::U1, Some("Docks"), &issues), Some("b"));
}

#[test]
fn the_match_is_the_entry_in_the_slice() {
    let issues = parse(&[GENERAL, LOCATED]);
    let got = match_known(&issues, Game::U1, Some("Docks"), "Assertion failed").unwrap();
    assert!(std::ptr::eq(got, &issues[1]));
}
