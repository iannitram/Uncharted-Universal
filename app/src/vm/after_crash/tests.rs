//! The after-crash screen with a fake run log, `where.json` and host (the done-when of closed
//! #176: the screen's text and the toggle set for a known crash; the report text with the
//! chapter for an unknown one; #185: one playthrough record per run end).

use super::*;
use crate::settings_file;
use serde_json::json;
use uncharted_core::logs::{parse_known_issues, read, RUN_LOG};

const AT: u64 = 1_793_527_200;
const HOME: &str = "/home/kamadha";

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir =
            std::env::temp_dir().join(format!("uu-app-after-crash-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("logs")).unwrap();
        fs::create_dir_all(dir.join("saves")).unwrap();
        TempDir(dir)
    }

    fn logs(&self) -> PathBuf {
        self.0.join("logs")
    }

    fn saves(&self) -> PathBuf {
        self.0.join("saves")
    }

    fn run_log(&self, text: &str) {
        fs::write(self.logs().join(RUN_LOG), text).unwrap();
    }

    fn where_json(&self, title: &str, subtitle: &str) {
        let text = json!({"game": "u2", "title": title, "subtitle": subtitle, "detail": "",
                          "saved_at": "2026-11-12T10:00:00Z"});
        fs::write(self.saves().join(WHERE_FILE), text.to_string()).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One known issue with a toggle, as `known_issues.json` would hold it.
const FIXABLE: &str = r#"[{
    "id": "u2-trainwreck-shader",
    "signature": "shader cache mismatch",
    "game": "u2",
    "locations": ["train-wreck"],
    "summary": "A shader cache mismatch stops U2 at the train wreck.",
    "fixed_in": null,
    "toggle": {"key": "BB_SAFE_SHADERS", "value": "1"},
    "issue": "https://github.com/iannitram/Uncharted-Universal/issues/999"
}]"#;

fn context(dir: &TempDir, issues: Vec<KnownIssue>) -> Context {
    Context {
        logs: dir.logs(),
        build: "b7".into(),
        preset: "balanced".into(),
        issues,
        details: ReportDetails {
            version: "0.9.0".into(),
            at_unix: AT,
            system: vec![format!("Launcher: {HOME}/Apps/uncharted-app")],
            settings: json!({"extra": {"BB_SAFE_SHADERS": "0"}}),
            free_bytes: 100 << 30,
        },
        redact: Redact::for_home(Path::new(HOME), vec![]),
    }
}

/// A host that saves settings to a real `settings.json` and records everything else.
struct FakeHost {
    settings: PathBuf,
    calls: Vec<String>,
    copied: Vec<String>,
    opened: Vec<String>,
    fail_save: bool,
}

impl FakeHost {
    fn new(dir: &TempDir) -> FakeHost {
        FakeHost {
            settings: dir.0.join("config").join(settings_file::SETTINGS_FILE),
            calls: Vec::new(),
            copied: Vec::new(),
            opened: Vec::new(),
            fail_save: false,
        }
    }
}

impl Host for FakeHost {
    fn set_setting(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.calls.push(format!("set {key}={value}"));
        if self.fail_save {
            return Err("read-only folder".into());
        }
        settings_file::set_extra(&self.settings, key, value).map_err(|e| e.to_string())
    }

    fn play(&mut self, game: Game) -> Result<(), String> {
        self.calls.push(format!("play {}", game.key()));
        Ok(())
    }

    fn open_url(&mut self, url: &str) {
        self.calls.push("open".into());
        self.opened.push(url.into());
    }

    fn copy_text(&mut self, text: &str) {
        self.calls.push("copy".into());
        self.copied.push(text.into());
    }
}

#[test]
fn a_known_crash_shows_the_fix_and_sets_the_toggle_before_playing() {
    let dir = TempDir::new("known");
    dir.run_log("build b7\nloading\nCrash: GPU fault: shader cache mismatch in pass 3\n");
    dir.where_json("Chapter 13", "Train-wreck");
    let ctx = context(&dir, parse_known_issues(FIXABLE).unwrap());

    let screen = after_run(&ctx, Game::U2, &dir.saves(), AT).expect("a crash shows the screen");
    assert_eq!(screen.known.as_ref().unwrap().id, "u2-trainwreck-shader");
    assert_eq!(
        screen.buttons(),
        [Button::PlayWithFix, Button::OpenIssue, Button::Back]
    );
    assert_eq!(
        screen.screen(Lang::En),
        Screen {
            heading: "The game stopped".into(),
            game: "Uncharted 2".into(),
            chapter: "Chapter: Train-wreck".into(),
            summary:
                "This is a known problem: A shader cache mismatch stops U2 at the train wreck."
                    .into(),
            detail: "What the game said: GPU fault: shader cache mismatch in pass 3".into(),
            buttons: vec![
                "Turn on safe shaders and play again".into(),
                "Open the issue page".into(),
                "Back".into(),
            ],
        }
    );

    let mut host = FakeHost::new(&dir);
    let got = screen.press(Button::PlayWithFix, &ctx, &mut host, Lang::En);
    assert_eq!(got, Pressed::Playing);
    assert_eq!(host.calls, ["set BB_SAFE_SHADERS=1", "play u2"]);
    assert_eq!(
        settings_file::extra(&host.settings)
            .get("BB_SAFE_SHADERS")
            .map(String::as_str),
        Some("1")
    );

    let got = screen.press(Button::OpenIssue, &ctx, &mut host, Lang::En);
    assert_eq!(got, Pressed::Stay(String::new()));
    assert_eq!(
        host.opened,
        ["https://github.com/iannitram/Uncharted-Universal/issues/999"]
    );
    assert_eq!(
        screen.press(Button::Back, &ctx, &mut host, Lang::En),
        Pressed::Home
    );
}

#[test]
fn the_run_is_added_to_the_playthrough_log_and_marked_seen() {
    let dir = TempDir::new("playthrough");
    let ctx = context(&dir, parse_known_issues(FIXABLE).unwrap());
    mark_started(&dir.logs(), Game::U2).unwrap();
    assert_eq!(unfinished(&dir.logs()), Some(Game::U2));

    dir.run_log("Crash: GPU fault: shader cache mismatch\n");
    dir.where_json("Chapter 13", "Train-wreck");
    assert!(after_run(&ctx, Game::U2, &dir.saves(), AT).is_some());
    assert_eq!(unfinished(&dir.logs()), None);

    dir.run_log("build b7\nthe player quit\n");
    assert_eq!(after_run(&ctx, Game::U2, &dir.saves(), AT + 60), None);

    let runs = read(&dir.logs());
    let got: Vec<(Ended, Option<&str>, u64)> = runs
        .iter()
        .map(|run| (run.ended, run.chapter.as_deref(), run.at_unix))
        .collect();
    assert_eq!(
        got,
        [
            (Ended::Crash, Some("Train-wreck"), AT),
            (Ended::Ok, Some("Train-wreck"), AT + 60),
        ]
    );
    assert!(runs
        .iter()
        .all(|run| run.game == Game::U2 && run.build == "b7"));
}

#[test]
fn an_unknown_crash_reports_with_the_game_and_chapter() {
    let dir = TempDir::new("unknown");
    dir.run_log(&format!(
        "build b7\nloading {HOME}/Games/dump\nCrash: access violation at guest offset 0x123456\n"
    ));
    dir.where_json("Chapter 13", "Train-wreck");
    let ctx = context(&dir, parse_known_issues(FIXABLE).unwrap());

    let screen = after_run(&ctx, Game::U2, &dir.saves(), AT).unwrap();
    assert_eq!(screen.known, None);
    assert_eq!(
        screen.buttons(),
        [Button::Report, Button::PlayAgain, Button::Back]
    );
    let texts = screen.screen(Lang::En);
    assert_eq!(
        texts.summary,
        "This crash is new to us. Report it and we can look into it."
    );
    assert_eq!(texts.buttons, ["Report it", "Play again", "Back"]);

    let mut host = FakeHost::new(&dir);
    let got = screen.press(Button::Report, &ctx, &mut host, Lang::En);
    assert_eq!(
        got,
        Pressed::Stay("The log is copied. Paste it into the Log field of the form.".into())
    );
    assert_eq!(host.calls, ["copy", "open"]);
    let report = &host.copied[0];
    assert!(report.contains("\nGame: Uncharted 2\n"), "{report}");
    assert!(report.contains("\nChapter: Train-wreck\n"), "{report}");
    assert!(report.contains("\nCrash: access violation at guest offset 0x123456\n"));
    assert!(report.contains(r#"Settings: {"extra":{"BB_SAFE_SHADERS":"0"}}"#));
    assert!(report.contains("loading ~/Games/dump"));
    assert!(!report.contains(HOME));
    assert_eq!(host.opened, [logs::bug_form_url(&screen.run_info())]);
    assert!(host.opened[0].contains("&where=Train-wreck"));

    assert_eq!(
        screen.press(Button::PlayAgain, &ctx, &mut host, Lang::En),
        Pressed::Playing
    );
    assert_eq!(host.calls.last().unwrap(), "play u2");
}

#[test]
fn a_shipped_known_issue_without_a_toggle_offers_play_again_and_its_page() {
    let dir = TempDir::new("shipped");
    let issues = parse_known_issues(include_str!(
        "../../../../crates/core/data/known_issues.json"
    ))
    .unwrap();
    dir.run_log("Crash: assertion failed: iSubMesh < m_numSubMeshInstances\n");
    dir.where_json("Chapter 4", "Plane-wrecked");
    let ctx = context(&dir, issues);

    let screen = after_run(&ctx, Game::U1, &dir.saves(), AT).unwrap();
    assert_eq!(screen.known.as_ref().unwrap().id, "u1-submesh-assertion");
    assert_eq!(
        screen.buttons(),
        [Button::PlayAgain, Button::OpenIssue, Button::Back]
    );
    let mut host = FakeHost::new(&dir);
    screen.press(Button::OpenIssue, &ctx, &mut host, Lang::En);
    assert_eq!(
        host.opened,
        ["https://github.com/iannitram/Uncharted-Universal/issues/110"]
    );
}

#[test]
fn a_setting_that_cannot_be_saved_does_not_start_the_game() {
    let dir = TempDir::new("save-fails");
    dir.run_log("Crash: shader cache mismatch\n");
    let ctx = context(&dir, parse_known_issues(FIXABLE).unwrap());
    let screen = after_run(&ctx, Game::U2, &dir.saves(), AT).unwrap();
    // No where.json: no chapter, and the location rule of the issue does not match.
    assert_eq!(screen.chapter, None);
    assert_eq!(screen.known, None);
    assert_eq!(
        screen.screen(Lang::En).chapter,
        "Chapter: not known yet (no save)"
    );

    let fixable = AfterCrash {
        known: parse_known_issues(FIXABLE).unwrap().pop(),
        ..screen
    };
    let mut host = FakeHost::new(&dir);
    host.fail_save = true;
    let got = fixable.press(Button::PlayWithFix, &ctx, &mut host, Lang::De);
    assert_eq!(
        got,
        Pressed::Stay("Die Einstellung konnte nicht gespeichert werden: read-only folder".into())
    );
    assert_eq!(host.calls, ["set BB_SAFE_SHADERS=1"]);
}

#[test]
fn a_clean_run_or_a_damaged_note_shows_nothing() {
    let dir = TempDir::new("clean");
    let ctx = context(&dir, Vec::new());
    assert_eq!(after_run(&ctx, Game::U3, &dir.saves(), AT), None);
    assert_eq!(read(&dir.logs())[0].ended, Ended::Ok);
    fs::write(
        dir.logs().join(RUN_MARK),
        "{\"game\": \"u9\", \"done\": false}",
    )
    .unwrap();
    assert_eq!(unfinished(&dir.logs()), None);
    fs::write(dir.logs().join(RUN_MARK), "garbage").unwrap();
    assert_eq!(unfinished(&dir.logs()), None);
}

#[test]
fn every_language_fills_the_screen() {
    let screen = AfterCrash {
        game: Game::U2,
        chapter: Some("Train-wreck".into()),
        crash: "x".into(),
        known: parse_known_issues(FIXABLE).unwrap().pop(),
    };
    for lang in Lang::ALL {
        let texts = screen.screen(lang);
        assert!(texts.chapter.contains("Train-wreck"), "{lang:?}");
        assert!(texts.buttons[0].contains("safe shaders"), "{lang:?}");
        assert!(!texts.heading.is_empty() && !texts.summary.contains("{}"));
    }
    assert_eq!(
        screen.screen(Lang::Ja).buttons[0],
        "safe shaders をオンにしてもう一度プレイ"
    );
}

#[test]
fn fix_names_read_as_words() {
    assert_eq!(fix_name("BB_SAFE_SHADERS"), "safe shaders");
    assert_eq!(fix_name("VSYNC"), "vsync");
    assert_eq!(fix_name(""), "");
}
