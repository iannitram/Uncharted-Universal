//! Log rotation, the health check and the bug report (`uncharted_core::logs`, roadmap 1.26).
//! Ported from `tests/test_logs.py` (`LogTests`), the expected results of
//! `launcher/uncharted_logs.py`.

use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uncharted_core::logs::{
    create_report, health, rotate, Redact, ReportDetails, CLOSED, KEEP, LAUNCHER_LOG,
    NOT_RESPONDING, REPORT_LOG_BYTES, RUN_LOG,
};

/// Plenty of space, whatever the test machine has (the Python test mocked `disk_usage`).
const PLENTY: u64 = 100 << 30;
const HOME: &str = "/home/kamadha";
const KEY: &str = "00112233445566778899aabbccddeeff";

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("uu-logs-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("logs")).unwrap();
        TempDir(dir)
    }

    fn logs(&self) -> PathBuf {
        self.0.join("logs")
    }

    fn write(&self, name: &str, text: &str) {
        fs::write(self.logs().join(name), text).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn redact_home() -> Redact {
    Redact::for_home(Path::new(HOME), vec!["/mnt/dumps/CUSA00341-app".into()])
}

fn details(version: &str, settings: serde_json::Value) -> ReportDetails {
    ReportDetails {
        version: version.into(),
        at_unix: 1_793_527_200,
        system: vec![format!("Launcher: {HOME}/Apps/Uncharted.AppImage")],
        settings,
        free_bytes: PLENTY,
    }
}

/// Every file under `folder`, as (path relative to it with `/`, text).
fn files(folder: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let name = path.strip_prefix(folder).unwrap().to_string_lossy();
                let name = name.replace('\\', "/");
                found.push((name, fs::read_to_string(&path).unwrap()));
            }
        }
    }
    found.sort();
    found
}

fn read(files: &[(String, String)], name: &str) -> String {
    files
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{name} is not in the report"))
        .1
        .clone()
}

#[test]
fn rotate_keeps_five() {
    let dir = TempDir::new("rotate");
    let folder = dir.logs();
    for run in 0..7 {
        rotate(&folder, LAUNCHER_LOG, KEEP).unwrap();
        fs::write(folder.join("launcher.log"), format!("run {run}")).unwrap();
    }
    let mut names: Vec<String> = fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "launcher.1.log",
            "launcher.2.log",
            "launcher.3.log",
            "launcher.4.log",
            "launcher.log"
        ]
    );
    assert_eq!(
        fs::read_to_string(folder.join("launcher.4.log")).unwrap(),
        "run 2"
    );
}

#[test]
fn rotate_of_an_empty_or_missing_folder_is_fine() {
    let dir = TempDir::new("rotate-missing");
    rotate(&dir.logs(), LAUNCHER_LOG, KEEP).unwrap();
    rotate(&dir.0.join("not-there"), LAUNCHER_LOG, KEEP).unwrap();
    rotate(&dir.logs(), LAUNCHER_LOG, 0).unwrap();
}

#[test]
fn health_finds_crash_hang_and_unclean_exit() {
    let dir = TempDir::new("health-all");
    dir.write(
        RUN_LOG,
        "Run: x\nCrash: kind=fault where=0x1234 text=bad thing\nRuntime: stopped\n",
    );
    dir.write("launcher.log", "== Uncharted launcher\n");
    dir.write(
        "launcher.1.log",
        &format!("== Uncharted launcher\n[ 3.0s] {NOT_RESPONDING} for 10 s\n"),
    );
    let found = health(&dir.logs(), PLENTY).unwrap();
    assert_eq!(found.len(), 3, "{found:?}");
    assert_eq!(
        found[0],
        "The last game run stopped with: kind=fault where=0x1234 text=bad thing"
    );
    assert!(found[1].contains("stopped responding"));
    assert!(found[2].contains("did not close normally"));
}

#[test]
fn health_quiet_after_a_normal_session() {
    let dir = TempDir::new("health-quiet");
    dir.write(
        "launcher.1.log",
        &format!("== Uncharted launcher\n[ 9.0s] {CLOSED} normally\n"),
    );
    dir.write("launcher.log", "== Uncharted launcher\n");
    dir.write(RUN_LOG, "Run: x\nRuntime: clean exit\n");
    assert_eq!(health(&dir.logs(), PLENTY).unwrap(), Vec::<String>::new());
}

#[test]
fn health_of_an_empty_folder_is_quiet() {
    let dir = TempDir::new("health-empty");
    assert!(health(&dir.logs(), PLENTY).unwrap().is_empty());
    assert!(health(&dir.0.join("not-there"), PLENTY).unwrap().is_empty());
}

#[test]
fn health_warns_about_little_disk_space() {
    let dir = TempDir::new("health-disk");
    assert_eq!(
        health(&dir.logs(), 2 << 30).unwrap(),
        ["Only 2.0 GB free where the launcher keeps its data."]
    );
}

#[test]
fn health_names_a_launcher_error_in_the_previous_session() {
    let dir = TempDir::new("health-error");
    dir.write(
        "launcher.1.log",
        &format!("thread 'main' panicked at app/src/main.rs:1:1\n{CLOSED} normally\n"),
    );
    assert_eq!(
        health(&dir.logs(), PLENTY).unwrap(),
        ["The previous launcher session had an error in the launcher's own code."]
    );
}

#[test]
fn health_reads_a_huge_run_log_from_its_end() {
    let dir = TempDir::new("health-huge");
    let mut text = String::from("Crash: an old line beyond the last MiB\n");
    text.push_str(&"spam every frame\n".repeat(200_000)); // 3.4 MB
    text.push_str("Crash: kind=assert where=fake.rs:42 text=the real crash\n");
    dir.write(RUN_LOG, &text);
    assert_eq!(
        health(&dir.logs(), PLENTY).unwrap(),
        ["The last game run stopped with: kind=assert where=fake.rs:42 text=the real crash"]
    );
}

#[test]
fn huge_game_log_keeps_its_start_and_end() {
    // test_huge_game_log_is_read_from_its_end: the Python test lowered REPORT_LOG_BYTES; this
    // one writes a log a little over the real limit.
    let dir = TempDir::new("report-huge");
    let line = "spam every frame\n";
    let mut text = String::from("Run: first line\n");
    text.push_str(&line.repeat(REPORT_LOG_BYTES as usize / line.len() + 1000));
    text.push_str("Crash: kind=fault where=0x1 text=the real crash\n");
    dir.write(RUN_LOG, &text);
    let folder = create_report(
        &dir.logs(),
        &dir.0,
        &details("v", json!({})),
        &redact_home(),
    )
    .unwrap();
    let kept = fs::read_to_string(folder.join("game").join(RUN_LOG)).unwrap();
    assert!(kept.contains("middle left out"));
    assert!(kept.starts_with("Run: first line\n"));
    assert!(kept
        .trim_end()
        .ends_with("Crash: kind=fault where=0x1 text=the real crash"));
    assert!((kept.len() as u64) <= REPORT_LOG_BYTES + 100);
}

#[test]
fn report_holds_logs_system_and_settings() {
    let dir = TempDir::new("report-all");
    dir.write(RUN_LOG, &format!("Run: {HOME}/game\n"));
    dir.write("launcher.log", "== Uncharted launcher\n");
    dir.write(
        "launcher.1.log",
        &format!("== Uncharted launcher\n{CLOSED}\n"),
    );
    dir.write("playthrough.jsonl", "{}\n");
    let settings = json!({"game_dir": format!("{HOME}/g"), "frame_rate": "60"});
    let folder = create_report(
        &dir.logs(),
        &dir.0,
        &details("0.3.6-linux.99", settings),
        &redact_home(),
    )
    .unwrap();
    assert_eq!(folder, dir.0.join("uncharted-report-1793527200"));
    let report = files(&folder);
    let names: Vec<&str> = report.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "game/last_run.log",
            "health.txt",
            "launcher/launcher.1.log",
            "launcher/launcher.log",
            "settings.json",
            "system.txt"
        ]
    );
    let settings = read(&report, "settings.json");
    assert!(!settings.contains(HOME));
    assert_eq!(
        settings,
        "{\n  \"frame_rate\": \"60\",\n  \"game_dir\": \"~/g\"\n}\n"
    );
    assert!(read(&report, "game/last_run.log").contains("~/game"));
    let system = read(&report, "system.txt");
    assert!(system.contains("0.3.6-linux.99"));
    assert!(system.contains("Launcher: ~/Apps/Uncharted.AppImage"));
    assert!(system.contains(std::env::consts::OS));
    assert_eq!(
        read(&report, "health.txt"),
        "No problems found in the recent logs.\n"
    );
}

#[test]
fn report_has_no_dump_path_and_no_key() {
    let dir = TempDir::new("report-private");
    let dump = "/mnt/dumps/CUSA00341-app";
    let windows_dump = r"D:\Dumps\U4";
    dir.write(
        RUN_LOG,
        &format!("Run: {dump}/eboot.bin\nkey {KEY}\nCrash: kind=fault where=0x1 text=x\n"),
    );
    dir.write(
        "launcher.log",
        &format!("game_dir: {dump} (found)\nfrom {windows_dump}\\eboot.bin\nKEY={KEY}\n"),
    );
    dir.write("launcher.1.log", &format!("{CLOSED} normally {KEY}\n"));
    let settings = json!({"game_dir": dump, "update_dir": windows_dump, "keys": [KEY]});
    let mut what = redact_home();
    what.dump_dirs.push(windows_dump.into());
    let folder = create_report(&dir.logs(), &dir.0, &details("v", settings), &what).unwrap();
    let report = files(&folder);
    assert_eq!(report.len(), 6);
    for (name, text) in &report {
        let lower = text.to_ascii_lowercase();
        assert!(!text.contains(dump), "{name} holds the dump path");
        assert!(!lower.contains("dumps"), "{name} holds the dump path");
        assert!(!lower.contains(KEY), "{name} holds the key");
        assert!(!text.contains(HOME), "{name} holds the home folder");
    }
    assert!(read(&report, "game/last_run.log").contains("Run: <dump>/eboot.bin"));
    assert!(read(&report, "launcher/launcher.log").contains("KEY=<key>"));
    assert!(read(&report, "settings.json").contains("\"update_dir\": \"<dump>\""));
}
