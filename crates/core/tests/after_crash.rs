//! What the after-crash screen (roadmap 2.07, closed #176) reads from `uncharted_core::logs`:
//! `where.json`, the Copy log for GitHub text with the game and chapter, and the bug-form link.
//! Expected results from `issue_text` in `launcher/uncharted_logs.py`; the chapter is new.

use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uncharted_core::games::Game;
use uncharted_core::logs::{
    bug_form_url, crash_line, issue_text, load_where, Redact, ReportDetails, RunInfo, BUG_FORM,
    ISSUE_CHARS, ISSUE_LOG_HEAD, RUN_LOG, WHERE_FILE,
};

const HOME: &str = "/home/kamadha";
const KEY: &str = "00112233445566778899aabbccddeeff";

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir =
            std::env::temp_dir().join(format!("uu-after-crash-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn details() -> ReportDetails {
    ReportDetails {
        version: "0.9.0".into(),
        at_unix: 1_793_527_200,
        system: vec![format!("Launcher: {HOME}/Apps/Uncharted.AppImage")],
        settings: json!({"upscale": "fsr", "game_dir": format!("{HOME}/Games/dump")}),
        free_bytes: 100 << 30,
    }
}

fn redact_home() -> Redact {
    Redact::for_home(Path::new(HOME), vec![])
}

#[test]
fn an_unknown_crash_reports_game_chapter_and_crash() {
    let dir = TempDir::new("unknown");
    dir.write(
        RUN_LOG,
        &format!("build b7\nloading {HOME}/Games/dump/big2.bin\nkey {KEY}\nCrash: access violation at guest offset 0x123456\n"),
    );
    let where_ = load_where(&dir.write(
        WHERE_FILE,
        r#"{"game":"u2","title":"Chapter 13","subtitle":"Train-wreck","detail":"","saved_at":"2026-11-12T10:00:00Z"}"#,
    ))
    .unwrap();
    let run = RunInfo {
        game: Game::U2,
        chapter: where_.chapter(),
        crash: crash_line(&fs::read_to_string(dir.0.join(RUN_LOG)).unwrap()),
    };
    let text = issue_text(&dir.0, &details(), Some(&run), &redact_home());
    assert!(text.starts_with("Uncharted launcher 0.9.0, copied at 1793527200 (Unix time)\n"));
    assert!(text.contains("\nGame: Uncharted 2\n"));
    assert!(text.contains("\nChapter: Train-wreck\n"));
    assert!(text.contains("\nCrash: access violation at guest offset 0x123456\n"));
    assert!(text.contains(r#"Settings: {"game_dir":"~/Games/dump","upscale":"fsr"}"#));
    assert!(text.contains("== Health check\n- The last game run stopped with: access violation"));
    assert!(text.contains(
        "== Game log (last_run.log)\nbuild b7\nloading ~/Games/dump/big2.bin\nkey <key>\n"
    ));
    assert!(!text.contains(HOME));
    assert!(!text.contains(KEY));
}

#[test]
fn no_save_yet_and_no_log_say_so() {
    let dir = TempDir::new("empty");
    let run = RunInfo {
        game: Game::U1,
        chapter: None,
        crash: None,
    };
    let text = issue_text(&dir.0, &details(), Some(&run), &redact_home());
    assert!(text.contains("\nChapter: not known (no save yet)\n"));
    assert!(!text.contains("\nCrash:"));
    assert!(text.contains("== Health check\nNo problems found."));
    assert!(text.ends_with("== Game log\n(no game run logged yet)"));
    assert_eq!(load_where(&dir.0.join(WHERE_FILE)), None);
}

#[test]
fn a_long_log_keeps_its_start_and_its_end() {
    let dir = TempDir::new("long");
    let line = "x".repeat(99);
    let lines: Vec<String> = (0..2_000).map(|n| format!("{n:04} {line}")).collect();
    dir.write(RUN_LOG, &(lines.join("\n") + "\n"));
    let text = issue_text(&dir.0, &details(), None, &redact_home());
    assert!(text.chars().count() <= ISSUE_CHARS);
    assert!(text.contains(&format!("{}\n[... ", lines[ISSUE_LOG_HEAD - 1])));
    assert!(text.contains(" lines left out ...]\n"));
    assert!(text.ends_with(&lines[1_999]));
    assert!(!text.contains(&lines[ISSUE_LOG_HEAD]));
    assert!(!text.contains("\nGame: "));
}

#[test]
fn the_bug_form_link_names_game_and_chapter() {
    let run = RunInfo {
        game: Game::U2,
        chapter: Some("Train-wreck".into()),
        crash: Some("access violation".into()),
    };
    assert_eq!(
        bug_form_url(&run),
        format!("{BUG_FORM}&title=Uncharted%202%2C%20Train-wreck%3A%20access%20violation&game=Uncharted%202&where=Train-wreck")
    );
    let no_chapter = RunInfo {
        chapter: None,
        ..run
    };
    assert!(!bug_form_url(&no_chapter).contains("&where="));
}
