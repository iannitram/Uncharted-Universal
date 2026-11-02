//! The launcher's own log, bug reports and crash help (SPEC.md M1-M3, M9; issues #168-#170).
//! `redact` keeps game paths, user names and hex keys out of every log and report; the `Crash:`
//! line and the `known_issues.json` matcher turn a failed run into a plain sentence; the
//! playthrough log records chapters for the M1-M3 checks. Replaces `launcher/uncharted_logs.py`
//! (rotation, summaries, `private`, `health`, the report) and #168-#170.
//!
//! Path strings are `/`-separated or native as the OS gave them; a log never holds a dump path.

use crate::error::{not_built, Result};
use crate::games::Game;
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

/// One entry of `known_issues.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownIssue {
    pub id: String,
    /// Which game it concerns; `None` for all.
    pub game: Option<Game>,
    /// The text to find in the `Crash:` line.
    pub contains: String,
    /// The plain sentence for the player.
    pub message: String,
}

/// Parse `known_issues.json` (a list of objects with `id`, optional `game`, `contains`,
/// `message`). Bad JSON is `Error::Launch("known_issues.json is damaged")`.
pub fn parse_known_issues(json: &str) -> Result<Vec<KnownIssue>> {
    not_built("logs::parse_known_issues")
}

/// The known issues that match a `Crash:` line for `game`, in file order.
pub fn match_known<'a>(
    issues: &'a [KnownIssue],
    game: Game,
    crash_line: &str,
) -> Vec<&'a KnownIssue> {
    not_built!("logs::match_known")
}

/// One line of `playthrough.jsonl` (#170): a chapter reached, a crash, a save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaythroughEvent {
    pub game: Game,
    /// Seconds since the Unix epoch.
    pub at_unix: u64,
    /// "chapter", "crash", "save", "credits".
    pub kind: String,
    pub detail: String,
}

/// Append one event as a JSON line to `<logs>/playthrough.jsonl`.
pub fn append_playthrough(logs: &Path, event: &PlaythroughEvent) -> Result<()> {
    not_built("logs::append_playthrough")
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
