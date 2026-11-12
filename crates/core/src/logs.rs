//! The launcher's own log, bug reports and crash help (SPEC.md M1-M3, M9; issues #168-#170).
//! `redact` keeps game paths, user names and hex keys out of every log and report; the `Crash:`
//! line and the `known_issues.json` matcher turn a failed run into a plain sentence; the
//! playthrough log records one line per run for the M1-M3 checks. Replaces `launcher/uncharted_logs.py`
//! (rotation, summaries, `private`, `health`, the report) and #168-#170.
//!
//! Path strings are `/`-separated or native as the OS gave them; a log never holds a dump path.

use crate::error::{Error, Result};
use crate::games::Game;
use serde_json::{Map, Value};
use std::fs::{self, create_dir_all, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
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
/// The line the launcher writes when its window stops responding for `HANG_SECONDS`
/// (`NOT_RESPONDING`).
pub const NOT_RESPONDING: &str = "== Not responding";
/// The launcher log's name in the logs folder: `rotate(logs, LAUNCHER_LOG, KEEP)` before each
/// start, then write `launcher.log`.
pub const LAUNCHER_LOG: &str = "launcher";
/// The game's run log in the logs folder: the runtime's output, with its `Crash:` line
/// (`last_run.log` in the saves folder of `uncharted_logs.py`).
pub const RUN_LOG: &str = "last_run.log";
/// `health` warns below this much free space where the launcher keeps its data (5 GiB).
pub const LOW_FREE_BYTES: u64 = 5 << 30;
/// A run of this many hex digits or more is a key to `redact`.
pub const KEY_HEX_DIGITS: usize = 32;
/// How much of the run log's end `health` reads (`read_tail` in `uncharted_linux.py`): a run
/// log can grow to gigabytes, and the `Crash:` line is at its end.
const RUN_LOG_TAIL: u64 = 1 << 20;
/// What a launcher log holds when the launcher's own code failed: the Python launcher's errors
/// (`health` in `uncharted_logs.py`) and a Rust panic.
const LAUNCHER_ERRORS: [&str; 3] = [
    "Fatal Python error",
    "Traceback (most recent call last)",
    "panicked at",
];

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

impl Redact {
    /// The home folder, its last part as the user name (`/home/<user>`, `C:\Users\<user>`,
    /// `/Users/<user>`), and `dump_dirs`. (`private` in `uncharted_logs.py` took the name from
    /// the login instead.)
    pub fn for_home(home: &Path, dump_dirs: Vec<String>) -> Redact {
        Redact {
            home: home.to_string_lossy().into_owned(),
            user: home
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            dump_dirs,
        }
    }
}

/// `text` with the home folder, the user name in paths, the dump folders, and any run of 32 or
/// more hex digits (a key) replaced. Never fails; an empty `Redact` still removes hex keys.
///
/// - Each dump folder becomes `<dump>` and the home folder `~` (dump folders first, longest
///   first, so a dump inside the home folder is still `<dump>`). A folder matches with either
///   separator, JSON-escaped (`\\`), with a trailing separator or not, and ignoring ASCII case;
///   the character after it must not continue a name (a letter, digit or `_`). A folder with no
///   separator once trailing ones are cut (`/`, `C:\`, empty) is left alone.
/// - The user name, when it has two characters or more, becomes `<user>` only right after a
///   `/` or `\` and before a non-name character: `/run/media/<user>/disk`, but a user `deck`
///   stays in "Steam Deck".
/// - A run of `KEY_HEX_DIGITS` or more ASCII hex digits becomes `<key>`.
pub fn redact(text: &str, what: &Redact) -> String {
    let mut folders: Vec<(&str, &str)> = what
        .dump_dirs
        .iter()
        .map(|dir| (dir.as_str(), "<dump>"))
        .collect();
    folders.sort_by_key(|(dir, _)| std::cmp::Reverse(trim_folder(dir).len()));
    folders.push((what.home.as_str(), "~"));

    let mut out = text.to_string();
    for (folder, mark) in folders {
        let folder = trim_folder(folder);
        if !folder.contains(['/', '\\']) {
            continue;
        }
        let backslashed = folder.replace('/', "\\");
        let mut forms = vec![
            backslashed.replace('\\', "\\\\"),
            folder.to_string(),
            folder.replace('\\', "/"),
            backslashed,
        ];
        forms.sort_by_key(|form| std::cmp::Reverse(form.len()));
        forms.dedup();
        for form in forms {
            out = replace_bounded(&out, &form, mark, false);
        }
    }
    if what.user.chars().count() >= 2 {
        out = replace_bounded(&out, &what.user, "<user>", true);
    }
    remove_hex_keys(&out)
}

fn trim_folder(folder: &str) -> &str {
    folder.trim_end_matches(['/', '\\'])
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `text` with each ASCII-case-insensitive `needle` that no name character follows (and, with
/// `after_separator`, that a `/` or `\` precedes) replaced by `mark`.
fn replace_bounded(text: &str, needle: &str, mark: &str, after_separator: bool) -> String {
    if needle.is_empty() {
        return text.to_string();
    }
    // ASCII lower-casing keeps every byte offset, so offsets found in `lower` index `text`.
    let lower = text.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut from) = (0, 0);
    while let Some(found) = lower[from..].find(&needle) {
        let (start, end) = (from + found, from + found + needle.len());
        let before = !after_separator || text[..start].ends_with(['/', '\\']);
        let after = !text[end..].starts_with(is_name_char);
        if before && after {
            out.push_str(&text[copied..start]);
            out.push_str(mark);
            copied = end;
            from = end;
        } else {
            from = start + text[start..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// `text` with each run of `KEY_HEX_DIGITS` or more ASCII hex digits replaced by `<key>`.
fn remove_hex_keys(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut at) = (0, 0);
    while at < bytes.len() {
        if !bytes[at].is_ascii_hexdigit() {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && bytes[at].is_ascii_hexdigit() {
            at += 1;
        }
        if at - start >= KEY_HEX_DIGITS {
            out.push_str(&text[copied..start]);
            out.push_str("<key>");
            copied = at;
        }
    }
    out.push_str(&text[copied..]);
    out
}

#[cfg(test)]
mod redact_tests {
    use super::*;

    fn what(home: &str, user: &str, dumps: &[&str]) -> Redact {
        Redact {
            home: home.into(),
            user: user.into(),
            dump_dirs: dumps.iter().map(|d| d.to_string()).collect(),
        }
    }

    #[test]
    fn removes_home_and_user_only_in_paths() {
        // test_private_removes_home_and_user in tests/test_logs.py.
        let home = "/home/kamadha";
        let text = redact(
            &format!("{home}/Games/x and /run/media/kamadha/disk and Steam Deck kamadha"),
            &what(home, "kamadha", &[]),
        );
        assert!(!text.contains(home));
        assert!(text.contains("~/Games/x"));
        assert!(text.contains("/run/media/<user>/disk"));
        assert!(text.ends_with("kamadha"));
    }

    #[test]
    fn a_short_or_empty_user_is_kept() {
        assert_eq!(
            redact("/media/k/disk", &what("", "k", &[])),
            "/media/k/disk"
        );
        assert_eq!(redact("/media/k/disk", &Redact::default()), "/media/k/disk");
    }

    #[test]
    fn deck_stays_in_steam_deck() {
        let got = redact(
            "Steam Deck, /home/deck/x, /run/media/deck",
            &what("/home/deck", "deck", &[]),
        );
        assert_eq!(got, "Steam Deck, ~/x, /run/media/<user>");
    }

    #[test]
    fn a_longer_name_is_not_the_home_folder() {
        let got = redact(
            "/home/kam/a /home/kamadha/b /home/kam",
            &what("/home/kam", "", &[]),
        );
        assert_eq!(got, "~/a /home/kamadha/b ~");
    }

    #[test]
    fn windows_home_in_both_separators_and_json() {
        let r = what(r"C:\Users\Kam", "Kam", &[]);
        assert_eq!(
            redact(r"C:\Users\Kam\AppData c:/users/kam/x", &r),
            r"~\AppData ~/x"
        );
        assert_eq!(
            redact(r#"{"dir": "C:\\Users\\Kam\\g"}"#, &r),
            r#"{"dir": "~\\g"}"#
        );
        assert_eq!(redact(r"D:\Users\kam\x", &r), r"D:\Users\<user>\x");
    }

    #[test]
    fn a_root_folder_is_left_alone() {
        assert_eq!(
            redact(r"/usr/bin C:\x", &what("/", "", &[r"C:\"])),
            r"/usr/bin C:\x"
        );
    }

    #[test]
    fn dump_folders_beat_the_home_folder() {
        let r = what(
            "/home/kam",
            "kam",
            &["/home/kam/games/u4", "/mnt/dumps/CUSA00341/"],
        );
        assert_eq!(
            redact(
                "/home/kam/games/u4/eboot.bin /mnt/dumps/CUSA00341 /home/kam/x",
                &r
            ),
            "<dump>/eboot.bin <dump> ~/x"
        );
    }

    #[test]
    fn hex_keys_of_32_digits_or_more() {
        let key = "0123456789abcdefABCDEF0123456789";
        assert_eq!(key.len(), 32);
        assert_eq!(
            redact(&format!("key={key}."), &Redact::default()),
            "key=<key>."
        );
        assert_eq!(redact(&format!("0x{key}ff"), &Redact::default()), "0x<key>");
        let short = &key[..31];
        assert_eq!(redact(short, &Redact::default()), short);
        assert_eq!(redact("0xdeadbeef é", &Redact::default()), "0xdeadbeef é");
    }

    #[test]
    fn for_home_takes_the_user_from_the_folder() {
        let r = Redact::for_home(Path::new("/home/deck"), vec!["/d".into()]);
        assert_eq!(r, what("/home/deck", "deck", &["/d"]));
    }
}

/// The reason line of a failed run: the last line of `run_log` that starts with `Crash:`
/// (written by the runtime's fault handler), without the prefix. `None` for a clean log.
pub fn crash_line(run_log: &str) -> Option<String> {
    run_log.lines().rev().find_map(|line| {
        line.strip_prefix("Crash:")
            .map(|reason| reason.trim().to_string())
    })
}

#[cfg(test)]
mod crash_line_tests {
    use super::crash_line;

    #[test]
    fn accepts_each_runtime_crash_kind() {
        for reason in [
            "kind=assert where=fake.rs:42 text=made_up_condition",
            "kind=fault where=0x1234 text=read access to 0x5678",
            "kind=exception where=0x1234 text=exception 0xdead",
        ] {
            let log = format!("Runtime: started\nCrash: {reason}\r\nRuntime: stopped\n");
            assert_eq!(crash_line(&log).as_deref(), Some(reason));
        }
    }

    #[test]
    fn returns_only_the_last_prefixed_line() {
        assert_eq!(crash_line("Runtime: clean exit\n"), None);
        assert_eq!(
            crash_line("Fault: read\n  Crash: indented\nmessage Crash: embedded"),
            None
        );
        assert_eq!(
            crash_line("Crash: first\nCrash: second\n"),
            Some("second".into())
        );
    }
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
    let chapter = if let Some(where_) = where_ {
        if !where_.subtitle.is_empty() {
            Some(where_.subtitle.clone())
        } else if !where_.title.is_empty() {
            Some(where_.title.clone())
        } else {
            None
        }
    } else {
        None
    };

    let avg_fps = avg_fps.map(|fps| (fps * 10.0).round() / 10.0);

    PlaythroughRun {
        build: build.to_string(),
        game,
        chapter,
        preset: preset.to_string(),
        avg_fps,
        ended,
        at_unix,
    }
}

/// Append one run as a JSON line (UTF-8, not escaped to ASCII, keys sorted, ending in a
/// newline) to `<logs>/playthrough.jsonl`, creating the folder.
pub fn append(logs: &Path, run: &PlaythroughRun) -> Result<()> {
    create_dir_all(logs).map_err(|e| crate::error::Error::Launch(e.to_string()))?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join(PLAYTHROUGH_FILE))
        .map_err(|e| crate::error::Error::Launch(e.to_string()))?;

    // Create a JSON object with the run data
    let json_value = serde_json::json!({
        "at_unix": run.at_unix,
        "avg_fps": run.avg_fps,
        "build": run.build,
        "chapter": run.chapter,
        "ended": match run.ended {
            Ended::Ok => "ok",
            Ended::Crash => "crash",
            Ended::Credits => "credits",
        },
        "game": run.game.key(),
        "preset": run.preset,
    });

    let json_line = serde_json::to_string(&json_value)
        .map_err(|e| crate::error::Error::Launch(e.to_string()))?;

    // One write call for the whole line, newline included, so a crash cannot leave half a line
    // (and the next append cannot run on from it).
    let line = format!("{json_line}\n");
    file.write_all(line.as_bytes())
        .map_err(|e| crate::error::Error::Launch(e.to_string()))?;

    Ok(())
}

/// Every run in `<logs>/playthrough.jsonl`, in file order. A missing file is an empty list; a
/// line that is not a valid run (garbage, blank, not an object, cut off, unknown `game` or
/// `ended`) is skipped.
pub fn read(logs: &Path) -> Vec<PlaythroughRun> {
    let path = logs.join(PLAYTHROUGH_FILE);

    // Read the entire file content
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(_) => return vec![], // Missing file returns empty vector
    };

    let mut runs = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue; // Skip blank lines
        }

        // Parse the JSON line
        let json_value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue, // Skip invalid JSON
        };

        // Check if it's an object with all required keys
        let map = match json_value.as_object() {
            Some(map) => map,
            None => continue, // Not an object
        };

        // Validate required keys exist
        if !map.contains_key("at_unix")
            || !map.contains_key("avg_fps")
            || !map.contains_key("build")
            || !map.contains_key("chapter")
            || !map.contains_key("ended")
            || !map.contains_key("game")
            || !map.contains_key("preset")
        {
            continue;
        }

        // Parse each field
        let at_unix = match map.get("at_unix").and_then(|v| v.as_u64()) {
            Some(v) => v,
            None => continue, // Invalid at_unix
        };

        let build = match map.get("build").and_then(|v| v.as_str()) {
            Some(v) => v.to_string(),
            None => continue, // Invalid build
        };

        let preset = match map.get("preset").and_then(|v| v.as_str()) {
            Some(v) => v.to_string(),
            None => continue, // Invalid preset
        };

        let chapter = match map.get("chapter") {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Null) => None,
            _ => continue, // Invalid chapter type
        };

        let avg_fps = match map.get("avg_fps") {
            Some(Value::Number(n)) => {
                if let Some(f) = n.as_f64() {
                    Some(f)
                } else {
                    continue; // Invalid fps number
                }
            }
            Some(Value::Null) => None,
            _ => continue, // Invalid fps type
        };

        let ended_str = match map.get("ended").and_then(|v| v.as_str()) {
            Some(v) => v,
            None => continue, // Invalid ended value
        };

        let ended = match ended_str {
            "ok" => Ended::Ok,
            "crash" => Ended::Crash,
            "credits" => Ended::Credits,
            _ => continue, // Invalid ended value
        };

        let game_key = match map.get("game").and_then(|v| v.as_str()) {
            Some(v) => v,
            None => continue, // Invalid game
        };

        let game = match Game::from_key(game_key) {
            Some(g) => g,
            None => continue, // Unknown game
        };

        runs.push(PlaythroughRun {
            build,
            game,
            chapter,
            preset,
            avg_fps,
            ended,
            at_unix,
        });
    }

    runs
}

/// Per game: the newest build and chapter and the counts of runs, crashes and credits. A game
/// with no run is absent. Does not change `runs`.
pub fn summary(runs: &[PlaythroughRun]) -> std::collections::BTreeMap<Game, GameSummary> {
    use std::collections::BTreeMap;

    let mut result = BTreeMap::new();

    // Walk runs once in order and track the newest build and chapter for each game
    for run in runs {
        let game_summary = result.entry(run.game).or_insert_with(|| GameSummary {
            build: String::new(),
            chapter: None,
            runs: 0,
            crashes: 0,
            credits: 0,
        });

        // Count the run
        game_summary.runs += 1;

        // Count crashes and credits
        match run.ended {
            Ended::Crash => game_summary.crashes += 1,
            Ended::Credits => game_summary.credits += 1,
            Ended::Ok => (),
        }

        // Update build to the newest one (last in slice)
        game_summary.build = run.build.clone();

        // Update chapter only when run.chapter.is_some() and it's not None
        if let Some(ref chapter) = run.chapter {
            game_summary.chapter = Some(chapter.clone());
        }
    }

    result
}

/// Rotate `<name>.log` to `<name>.1.log` .. `<name>.<keep-1>.log` in `folder`, dropping the
/// oldest. A missing file is skipped. (`rotate` in `uncharted_logs.py`.) Every rename is tried;
/// the first that failed is the error (`Error::Io`), so a caller can still start its log.
pub fn rotate(folder: &Path, name: &str, keep: usize) -> Result<()> {
    let mut first_error = None;
    for n in (1..keep).rev() {
        let newer = if n == 1 {
            folder.join(format!("{name}.log"))
        } else {
            folder.join(format!("{name}.{}.log", n - 1))
        };
        if newer.is_file() {
            if let Err(error) = fs::rename(&newer, folder.join(format!("{name}.{n}.log"))) {
                first_error.get_or_insert(Error::from(error));
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// What the recent logs say went wrong, newest first, as short sentences (`health`): the last
/// `Crash:` line, a launcher session that did not close (no `CLOSED` line), low free space.
///
/// In order: the `Crash:` line in the last MiB of `<logs>/last_run.log` (cut to 160
/// characters); a `NOT_RESPONDING` line in `launcher.log` (this session) or `launcher.1.log`
/// (the previous one); for the previous session, a launcher error (a Python traceback or a Rust
/// panic) and a missing `CLOSED` line; `free_bytes` under `LOW_FREE_BYTES`. A missing or
/// unreadable log says nothing, so this never fails today; `Result` keeps room for that.
pub fn health(logs: &Path, free_bytes: u64) -> Result<Vec<String>> {
    let mut found = Vec::new();
    if let Some(reason) = read_tail(&logs.join(RUN_LOG), RUN_LOG_TAIL).and_then(|t| crash_line(&t))
    {
        let reason: String = reason.chars().take(160).collect();
        found.push(format!("The last game run stopped with: {reason}"));
    }
    let sessions = [
        ("launcher.log", "This launcher session", false),
        ("launcher.1.log", "The previous launcher session", true),
    ];
    for (name, when, finished) in sessions {
        let Ok(bytes) = fs::read(logs.join(name)) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        if text.contains(NOT_RESPONDING) {
            found.push(format!(
                "{when} stopped responding for a while (the log has what it was doing)."
            ));
        }
        if finished {
            if LAUNCHER_ERRORS.iter().any(|marker| text.contains(marker)) {
                found.push(format!("{when} had an error in the launcher's own code."));
            }
            if !text.contains(CLOSED) {
                found.push(format!(
                    "{when} did not close normally (a crash, or it was forced to close)."
                ));
            }
        }
    }
    if free_bytes < LOW_FREE_BYTES {
        found.push(format!(
            "Only {:.1} GB free where the launcher keeps its data.",
            free_bytes as f64 / (1u64 << 30) as f64
        ));
    }
    Ok(found)
}

/// The last `max` bytes of a file as text (UTF-8, damaged bytes replaced), starting at a whole
/// line when the start was cut; `None` when it cannot be read.
fn read_tail(path: &Path, max: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let start = size.saturating_sub(max);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let mut from = 0;
    if start > 0 {
        from = bytes
            .iter()
            .position(|&b| b == b'\n')
            .map_or(0, |at| at + 1);
    }
    Some(String::from_utf8_lossy(&bytes[from..]).into_owned())
}

/// What a bug report holds besides the logs (`create_report`). What `uncharted_logs.py` read
/// from the system itself (GPU, Vulkan devices, disk space), the caller gathers here.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReportDetails {
    /// The launcher's version, on the first line of `system.txt`.
    pub version: String,
    /// When the report was made, seconds since the Unix epoch; names the folder.
    pub at_unix: u64,
    /// System lines the caller found (OS release, CPU, GPU, Vulkan, disk space); `system.txt`
    /// adds the OS, architecture and thread count itself.
    pub system: Vec<String>,
    /// The settings, written whole (keys sorted) as `settings.json`; `Null` writes `{}`.
    pub settings: Value,
    /// Free space where the launcher keeps its data, for `health.txt`.
    pub free_bytes: u64,
}

/// The bug report: the logs, system details and settings, all through `redact`, written as a
/// folder of text files under `out`; returns its path.
///
/// The folder is `<out>/uncharted-report-<at_unix>` (made, or reused) with `system.txt`,
/// `health.txt` (`health`), `settings.json`, `launcher/<name>` for each `launcher*.log` and
/// `game/<name>` for each `last_run*.log` in `logs`. A log over `REPORT_LOG_BYTES` keeps its
/// first quarter and last three quarters, cut at whole lines, with a note between. A log that
/// cannot be read is left out; a file that cannot be written is the error (`Error::Io`).
pub fn create_report(
    logs: &Path,
    out: &Path,
    details: &ReportDetails,
    what: &Redact,
) -> Result<PathBuf> {
    let folder = out.join(format!("uncharted-report-{}", details.at_unix));
    create_dir_all(folder.join("launcher"))?;
    create_dir_all(folder.join("game"))?;

    let threads = std::thread::available_parallelism()
        .map(|n| n.to_string())
        .unwrap_or_else(|_| "?".into());
    let mut system = vec![
        format!(
            "Uncharted launcher {}, report at {} (Unix time)",
            details.version, details.at_unix
        ),
        format!(
            "OS: {} {}, {threads} threads",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    ];
    system.extend(details.system.iter().cloned());
    write_redacted(&folder.join("system.txt"), &system.join("\n"), what)?;

    let found = health(logs, details.free_bytes)?;
    let health_text = if found.is_empty() {
        "No problems found in the recent logs.".to_string()
    } else {
        found
            .iter()
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    write_redacted(&folder.join("health.txt"), &health_text, what)?;

    let settings = match &details.settings {
        Value::Null => Value::Object(Map::new()),
        other => sorted(other),
    };
    let settings = serde_json::to_string_pretty(&settings).map_err(|e| Error::Io(e.to_string()))?;
    write_redacted(&folder.join("settings.json"), &settings, what)?;

    let mut names: Vec<String> = fs::read_dir(logs)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.path().is_file())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    for name in names {
        let part = if name.starts_with(LAUNCHER_LOG) && name.ends_with(".log") {
            "launcher"
        } else if name.starts_with("last_run") && name.ends_with(".log") {
            "game"
        } else {
            continue;
        };
        let Some(text) = read_cut(&logs.join(&name), REPORT_LOG_BYTES) else {
            continue;
        };
        fs::write(folder.join(part).join(&name), redact(&text, what))?;
    }
    Ok(folder)
}

fn write_redacted(path: &Path, text: &str, what: &Redact) -> Result<()> {
    fs::write(path, redact(text, what) + "\n")?;
    Ok(())
}

/// `value` with every object's keys in sorted order, whatever map order serde_json was built
/// with (`sort_keys=True` in `uncharted_logs.py`).
fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), sorted(&map[key])))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// A log as text for a report: whole when it is at most `limit` bytes, else its first quarter
/// and last three quarters of `limit`, each cut at a whole line (so no key is split and half
/// kept), with a note between.
fn read_cut(path: &Path, limit: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    if size <= limit {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        return Some(String::from_utf8_lossy(&bytes).into_owned());
    }
    let mut head = vec![0; (limit / 4) as usize];
    file.read_exact(&mut head).ok()?;
    file.seek(SeekFrom::Start(size - limit * 3 / 4)).ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    let head = match head.iter().rposition(|&b| b == b'\n') {
        Some(at) => &head[..at],
        None => trim_hex_end(&head),
    };
    let tail = match tail.iter().position(|&b| b == b'\n') {
        Some(at) => &tail[at + 1..],
        None => trim_hex_start(&tail),
    };
    Some(format!(
        "{}\n\n[... {:.0} MB log, middle left out ...]\n\n{}",
        String::from_utf8_lossy(head),
        size as f64 / (1u64 << 20) as f64,
        String::from_utf8_lossy(tail)
    ))
}

fn trim_hex_end(bytes: &[u8]) -> &[u8] {
    let hex = bytes
        .iter()
        .rev()
        .take_while(|b| b.is_ascii_hexdigit())
        .count();
    &bytes[..bytes.len() - hex]
}

fn trim_hex_start(bytes: &[u8]) -> &[u8] {
    &bytes[bytes.iter().take_while(|b| b.is_ascii_hexdigit()).count()..]
}

#[cfg(test)]
mod cut_tests {
    use super::*;

    #[test]
    fn a_line_cut_at_either_end_is_dropped() {
        let dir = std::env::temp_dir().join(format!("uu-logs-cut-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("last_run.log");
        // 42 bytes: lines of 8, 8, 9, 9 and 8 bytes, newlines included.
        fs::write(&path, "aaaaaaa\nbbbbbbb\ncccccccc\ndddddddd\neeeeeee\n").unwrap();
        let note = "\n\n[... 0 MB log, middle left out ...]\n\n";
        // Limit 40: the first 10 bytes keep "aaaaaaa", the last 30 start inside "bbbbbbb".
        assert_eq!(
            read_cut(&path, 40).unwrap(),
            format!("aaaaaaa{note}cccccccc\ndddddddd\neeeeeee\n")
        );
        assert_eq!(
            read_cut(&path, 42).unwrap(),
            fs::read_to_string(&path).unwrap()
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_key_at_a_cut_without_lines_is_dropped() {
        assert_eq!(trim_hex_end(b"x=0123abcd"), b"x=");
        assert_eq!(trim_hex_start(b"0123 rest"), b" rest");
    }
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
