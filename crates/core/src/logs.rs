//! The launcher's own log, bug reports and crash help (SPEC.md M1-M3, M9; issues #168-#170).
//! `redact` keeps game paths, user names and hex keys out of every log and report; the `Crash:`
//! line and the `known_issues.json` matcher turn a failed run into a plain sentence; the
//! playthrough log records one line per run for the M1-M3 checks. Replaces `launcher/uncharted_logs.py`
//! (rotation, summaries, `private`, `health`, the report) and #168-#170.
//!
//! Path strings are `/`-separated or native as the OS gave them; a log never holds a dump path.

use crate::error::{not_built, Result};
use crate::games::Game;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// How many launcher logs are kept (`KEEP` in `uncharted_logs.py`): `launcher.log` and
/// `launcher.1.log` .. `launcher.4.log`.
pub const KEEP: usize = 5;
/// The window counts as not responding after this many seconds (`HANG_SECONDS`).
pub const HANG_SECONDS: u64 = 10;
/// Each log in a bug report is cut to this many bytes (`REPORT_LOG_BYTES`).
pub const REPORT_LOG_BYTES: u64 = 8 << 20;
/// The line a clean launcher exit writes (`CLOSED`).
pub const CLOSED: &str = "== Launcher closed";

/// What to take out of a text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Redact {
    /// The home folder, shown as `~`.
    pub home: String,
    /// The user name, shown as `<user>` when it appears in a path (two characters or more).
    pub user: String,
    /// Dump folders (`dump::FoundGame::folder`), shown as `<dump>`.
    pub dump_dirs: Vec<String>,
}

/// `text` with the home folder, the user name in paths, the dump folders, and any run of 32 or
/// more hex digits (a key) replaced. Never fails; an empty `Redact` still removes hex keys.
pub fn redact(text: &str, what: &Redact) -> String {
    not_built!("logs::redact")
}

/// The reason line of a failed run: the last line of `run_log` that starts with `Crash:`
/// (written by the runtime's fault handler), without the prefix. `None` for a clean log.
pub fn crash_line(run_log: &str) -> Option<String> {
    not_built!("logs::crash_line")
}

/// The setting a known issue's workaround turns on (`toggle` in `known_issues.json`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toggle {
    pub key: String,
    pub value: String,
}

/// One entry of `known_issues.json` (#169).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownIssue {
    pub id: String,
    /// The text to find in the `Crash:` line (case-sensitive).
    pub signature: String,
    /// Which game it concerns; `None` for all (`"any"` or no `game` key in the file).
    pub game: Option<Game>,
    /// Where in the game it happens; empty means anywhere. Each is matched case-insensitively
    /// as a substring of the player's location.
    pub locations: Vec<String>,
    /// The plain sentence for the player.
    pub summary: String,
    /// The build that fixed it, if any.
    pub fixed_in: Option<String>,
    /// The setting that works around it, if any.
    pub toggle: Option<Toggle>,
    /// The link to the issue; empty when the file has none.
    pub issue: String,
}

/// Parse the text of `known_issues.json`: a list of objects with `id`, `signature`, optional
/// `game` (`"u1"`, `"u2"`, `"u3"` or `"any"`), `locations`, `summary`, `fixed_in`, `toggle`
/// (`{"key", "value"}`) and `issue`. An entry that is not an object, lacks a non-empty `id` or
/// `signature`, or names an unknown `game` is skipped. Text that is not JSON, or JSON that is
/// not a list, is `Error::Launch("known_issues.json is damaged")`.
pub fn parse_known_issues(json: &str) -> Result<Vec<KnownIssue>> {
    let parsed: Value = serde_json::from_str(json)
        .map_err(|_| crate::error::Error::Launch("known_issues.json is damaged".to_string()))?;

    let array = parsed
        .as_array()
        .ok_or_else(|| crate::error::Error::Launch("known_issues.json is damaged".to_string()))?;

    let mut result = Vec::new();

    for value in array {
        if let Value::Object(map) = value {
            // Check for required fields
            let id = match map.get("id").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => continue,
            };

            let signature = match map.get("signature").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => continue,
            };

            // Parse game field
            // A missing game or "any" is None; any other value that is not a known game skips the entry.
            let game = match map.get("game") {
                None => None,
                Some(Value::String(game_str)) if game_str == "any" => None,
                Some(Value::String(game_str)) => match Game::from_key(game_str) {
                    Some(game) => Some(game),
                    None => continue,
                },
                Some(_) => continue,
            };

            // Parse locations field
            let locations = match map.get("locations") {
                Some(Value::Array(arr)) => arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect(),
                _ => Vec::new(),
            };

            // Parse other fields with defaults
            let summary = map
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let fixed_in = map
                .get("fixed_in")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let toggle = match map.get("toggle") {
                Some(Value::Object(toggle_map)) => {
                    let key = toggle_map
                        .get("key")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let value = toggle_map
                        .get("value")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    if let (Some(key), Some(value)) = (key, value) {
                        Some(Toggle { key, value })
                    } else {
                        None
                    }
                }
                _ => None,
            };

            let issue = map
                .get("issue")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            result.push(KnownIssue {
                id,
                signature,
                game,
                locations,
                summary,
                fixed_in,
                toggle,
                issue,
            });
        }
    }

    Ok(result)
}

/// Read and parse a `known_issues.json`; a missing, unreadable or damaged file is an empty
/// list (crash help must never break the launcher).
pub fn load_known_issues(path: &Path) -> Vec<KnownIssue> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return Vec::new(),
    };

    parse_known_issues(&content).unwrap_or_default()
}

/// The known issue that explains a `Crash:` line for `game` at `location`, or `None`. An issue
/// matches when its `signature` is in `crash_line`, its `game` is `None` or equals `game`, and
/// it has no `locations` or one of them is in `location` (case-insensitive; no location given
/// counts as ""). An empty `crash_line` matches nothing. An issue with locations beats one
/// without; otherwise the first in list order wins.
pub fn match_known<'a>(
    issues: &'a [KnownIssue],
    game: Game,
    location: Option<&str>,
    crash_line: &str,
) -> Option<&'a KnownIssue> {
    if crash_line.is_empty() {
        return None;
    }

    let location_lower = location.unwrap_or("").to_lowercase();

    let mut best_match: Option<&'a KnownIssue> = None;
    let mut best_has_locations = false;

    for issue in issues {
        // Check signature match (case-sensitive substring)
        if !crash_line.contains(&issue.signature) {
            continue;
        }

        // Check game match
        if let Some(issue_game) = issue.game {
            if issue_game != game {
                continue;
            }
        }

        // Check location match
        let location_match = if issue.locations.is_empty() {
            // No locations specified, so it matches anywhere
            true
        } else {
            // Check if any of the issue's locations match the current location
            issue
                .locations
                .iter()
                .any(|loc| location_lower.contains(&loc.to_lowercase()))
        };

        if !location_match {
            continue;
        }

        // This is a valid match - check if it's better than our current best
        let has_locations = !issue.locations.is_empty();

        // If we don't have a match yet, or this one has locations and the current doesn't,
        // or they're both without locations and this is first in order
        if best_match.is_none() || (has_locations && !best_has_locations) {
            best_match = Some(issue);
            best_has_locations = has_locations;
        }
    }

    best_match
}

/// The file `append` and `read` use inside the logs folder.
pub const PLAYTHROUGH_FILE: &str = "playthrough.jsonl";

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// The player quit; no crash and no credits.
    Ok,
    /// The run crashed.
    Crash,
    /// The credits rolled (the M1-M3 evidence that a game plays start to finish).
    Credits,
}

/// Where the game was when the run ended (the runtime's `where.json`, #168); an empty text
/// means the file had none.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Where {
    pub title: String,
    pub subtitle: String,
}

/// One line of `playthrough.jsonl` (#170): one game run. In the file its keys are `build`,
/// `game` (`Game::key`), `chapter`, `preset`, `avg_fps`, `ended` (`"ok"`, `"crash"`,
/// `"credits"`) and `at_unix`, sorted, one JSON object per line.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaythroughRun {
    pub build: String,
    pub game: Game,
    /// The chapter reached: the subtitle, else the title, else `None`.
    pub chapter: Option<String>,
    pub preset: String,
    /// Average frames per second, rounded to one decimal; `None` when not measured.
    pub avg_fps: Option<f64>,
    pub ended: Ended,
    /// Seconds since the Unix epoch.
    pub at_unix: u64,
}

/// What the log says about one game (`summary`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSummary {
    /// The build of the newest run (the last in the list).
    pub build: String,
    /// The chapter of the newest run that has one; `None` when no run has.
    pub chapter: Option<String>,
    pub runs: usize,
    pub crashes: usize,
    pub credits: usize,
}

/// Build a [`PlaythroughRun`]: `chapter` from `where_` (subtitle, else title, else `None`),
/// `avg_fps` rounded to one decimal.
pub fn record(
    build: &str,
    game: Game,
    where_: Option<&Where>,
    preset: &str,
    avg_fps: Option<f64>,
    ended: Ended,
    at_unix: u64,
) -> PlaythroughRun {
    not_built!("logs::record")
}

/// Append one run as a JSON line (UTF-8, not escaped to ASCII, keys sorted, ending in a
/// newline) to `<logs>/playthrough.jsonl`, creating the folder.
pub fn append(logs: &Path, run: &PlaythroughRun) -> Result<()> {
    not_built("logs::append")
}

/// Every run in `<logs>/playthrough.jsonl`, in file order. A missing file is an empty list; a
/// line that is not a valid run (garbage, blank, not an object, cut off, unknown `game` or
/// `ended`) is skipped.
pub fn read(logs: &Path) -> Vec<PlaythroughRun> {
    not_built!("logs::read")
}

/// Per game: the newest build and chapter and the counts of runs, crashes and credits. A game
/// with no run is absent. Does not change `runs`.
pub fn summary(runs: &[PlaythroughRun]) -> std::collections::BTreeMap<Game, GameSummary> {
    not_built!("logs::summary")
}

/// Rotate `<name>.log` to `<name>.1.log` .. `<name>.<keep-1>.log` in `folder`, dropping the
/// oldest. A missing file is skipped. (`rotate` in `uncharted_logs.py`.)
pub fn rotate(folder: &Path, name: &str, keep: usize) -> Result<()> {
    not_built("logs::rotate")
}

/// What the recent logs say went wrong, newest first, as short sentences (`health`): the last
/// `Crash:` line, a launcher session that did not close (no `CLOSED` line), low free space.
pub fn health(logs: &Path, free_bytes: u64) -> Result<Vec<String>> {
    not_built("logs::health")
}

/// The bug report: the logs, system details and settings, all through `redact`, written as a
/// folder of text files under `out`; returns its path.
pub fn create_report(logs: &Path, out: &Path, what: &Redact) -> Result<PathBuf> {
    not_built("logs::create_report")
}

#[cfg(test)]
mod known_issue_game_tests {
    use super::*;

    #[test]
    fn a_game_that_is_not_text_skips_the_entry() {
        let got = parse_known_issues(
            r#"[{"id": "n", "signature": "s", "game": 5}, {"id": "ok", "signature": "s"}]"#,
        )
        .unwrap();
        let ids: Vec<&str> = got.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["ok"]);
    }
}
