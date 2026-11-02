//! The playthrough log: one JSON line per run (`uncharted_core::logs`, issue #170).

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use uncharted_core::games::Game;
use uncharted_core::logs::{
    append, read, record, summary, Ended, GameSummary, PlaythroughRun, Where, PLAYTHROUGH_FILE,
};

const AT: u64 = 1_793_527_200;

fn run(game: Game, build: &str, chapter: Option<&str>, ended: Ended) -> PlaythroughRun {
    PlaythroughRun {
        build: build.into(),
        game,
        chapter: chapter.map(String::from),
        preset: "balanced".into(),
        avg_fps: Some(59.9),
        ended,
        at_unix: AT,
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir =
            std::env::temp_dir().join(format!("uu-playthrough-{}-{name}", std::process::id()));
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

fn make(where_: Option<&Where>, fps: Option<f64>, ended: Ended) -> PlaythroughRun {
    record("b7", Game::U2, where_, "quality", fps, ended, AT)
}

fn place(title: &str, subtitle: &str) -> Where {
    Where {
        title: title.into(),
        subtitle: subtitle.into(),
    }
}

#[test]
fn record_fills_every_field() {
    let got = make(Some(&place("T", "S")), Some(59.94), Ended::Ok);
    let want = PlaythroughRun {
        build: "b7".into(),
        game: Game::U2,
        chapter: Some("S".into()),
        preset: "quality".into(),
        avg_fps: Some(59.9),
        ended: Ended::Ok,
        at_unix: AT,
    };
    assert_eq!(got, want);
}

#[test]
fn the_chapter_falls_back_to_the_title() {
    assert_eq!(
        make(Some(&place("T", "")), None, Ended::Ok)
            .chapter
            .as_deref(),
        Some("T")
    );
}

#[test]
fn the_chapter_is_none_when_there_is_nothing() {
    assert_eq!(make(Some(&place("", "")), None, Ended::Ok).chapter, None);
    assert_eq!(make(Some(&Where::default()), None, Ended::Ok).chapter, None);
    assert_eq!(make(None, None, Ended::Ok).chapter, None);
}

#[test]
fn the_frame_rate_is_rounded_to_one_decimal_and_none_is_kept() {
    assert_eq!(make(None, Some(30.04), Ended::Ok).avg_fps, Some(30.0));
    assert_eq!(make(None, Some(30.06), Ended::Ok).avg_fps, Some(30.1));
    assert_eq!(make(None, None, Ended::Ok).avg_fps, None);
}

#[test]
fn record_keeps_how_the_run_ended() {
    for ended in [Ended::Ok, Ended::Crash, Ended::Credits] {
        assert_eq!(make(None, None, ended).ended, ended);
    }
}

#[test]
fn append_writes_one_sorted_unescaped_line_and_creates_the_folder() {
    let dir = TempDir::new("append");
    let logs = dir.0.join("a").join("b");
    append(&logs, &run(Game::U1, "b1", Some("Café"), Ended::Ok)).unwrap();
    append(&logs, &run(Game::U3, "b1", None, Ended::Credits)).unwrap();
    assert_eq!(PLAYTHROUGH_FILE, "playthrough.jsonl");
    let text = fs::read_to_string(logs.join(PLAYTHROUGH_FILE)).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0],
        r#"{"at_unix":1793527200,"avg_fps":59.9,"build":"b1","chapter":"Café","ended":"ok","game":"u1","preset":"balanced"}"#
    );
    assert_eq!(
        lines[1],
        r#"{"at_unix":1793527200,"avg_fps":59.9,"build":"b1","chapter":null,"ended":"credits","game":"u3","preset":"balanced"}"#
    );
    assert!(text.ends_with('\n'));
}

#[test]
fn read_of_a_missing_file_is_empty() {
    let dir = TempDir::new("missing");
    assert!(read(&dir.0.join("none")).is_empty());
}

#[test]
fn read_gives_back_what_append_wrote() {
    let dir = TempDir::new("round");
    let runs = [
        run(Game::U1, "b1", None, Ended::Ok),
        run(Game::U2, "b2", Some("X"), Ended::Crash),
        run(Game::U3, "b3", Some("Café"), Ended::Credits),
    ];
    for r in &runs {
        append(&dir.0, r).unwrap();
    }
    assert_eq!(read(&dir.0), runs);
}

#[test]
fn read_skips_lines_that_are_not_a_valid_run() {
    let dir = TempDir::new("bad");
    let good = r#"{"build": "b1", "game": "u1", "chapter": null, "preset": "balanced", "avg_fps": 59.9, "ended": "ok", "at_unix": 1793527200}"#;
    let unknown_game = good.replace(r#""u1""#, r#""u9""#);
    let unknown_ended = good.replace(r#""ok""#, r#""quit""#);
    let text =
        format!("garbage\n[1, 2]\n\"str\"\n\n{unknown_game}\n{unknown_ended}\n{good}\n{{\"cut\": ");
    fs::write(dir.0.join(PLAYTHROUGH_FILE), text).unwrap();
    assert_eq!(read(&dir.0), [run(Game::U1, "b1", None, Ended::Ok)]);
}

#[test]
fn summary_of_nothing_is_empty() {
    assert!(summary(&[]).is_empty());
}

#[test]
fn summary_counts_per_game_and_takes_the_newest_build() {
    let runs = [
        run(Game::U1, "old", Some("A"), Ended::Ok),
        run(Game::U2, "x", Some("M"), Ended::Crash),
        run(Game::U1, "new", Some("B"), Ended::Crash),
        run(Game::U1, "newest", Some("C"), Ended::Ok),
    ];
    let got = summary(&runs);
    assert_eq!(
        got.keys().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from([Game::U1, Game::U2])
    );
    let u1 = GameSummary {
        build: "newest".into(),
        chapter: Some("C".into()),
        runs: 3,
        crashes: 1,
        credits: 0,
    };
    let u2 = GameSummary {
        build: "x".into(),
        chapter: Some("M".into()),
        runs: 1,
        crashes: 1,
        credits: 0,
    };
    assert_eq!(got[&Game::U1], u1);
    assert_eq!(got[&Game::U2], u2);
}

#[test]
fn summary_chapter_is_the_newest_that_is_not_none() {
    let runs = [
        run(Game::U1, "a", Some("A"), Ended::Ok),
        run(Game::U1, "b", Some("B"), Ended::Ok),
        run(Game::U1, "c", None, Ended::Ok),
    ];
    let got = &summary(&runs)[&Game::U1];
    assert_eq!(got.chapter.as_deref(), Some("B"));
    assert_eq!(got.build, "c");
}

#[test]
fn summary_chapter_is_none_when_every_run_has_none() {
    let runs = [
        run(Game::U3, "a", None, Ended::Ok),
        run(Game::U3, "b", None, Ended::Ok),
    ];
    assert_eq!(summary(&runs)[&Game::U3].chapter, None);
}

#[test]
fn summary_counts_the_runs_where_the_credits_rolled() {
    let runs = [
        run(Game::U2, "a", Some("A"), Ended::Credits),
        run(Game::U2, "b", Some("B"), Ended::Ok),
        run(Game::U2, "c", Some("C"), Ended::Credits),
    ];
    let got = &summary(&runs)[&Game::U2];
    assert_eq!((got.runs, got.crashes, got.credits), (3, 0, 2));
}
