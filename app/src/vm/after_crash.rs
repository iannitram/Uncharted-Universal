//! The after-run step and the after-crash screen (roadmap 2.07; replaces closed #176, takes in
//! #185; SPEC.md M1-M3, M10).
//!
//! When a game run ends, [`after_run`] reads the run's `Crash:` line (`logs::last_crash`) and
//! the game's `where.json`, appends the run to `playthrough.jsonl` and, for a crash, returns the
//! [`AfterCrash`] screen: the game, the chapter and a plain sentence. A known issue
//! (`logs::match_known`) offers "Turn on <fix> and play again" when it has a toggle (the toggle
//! is saved, then the same game starts) and a link to its issue; an unknown crash offers
//! "Report it", which copies the redacted Copy log for GitHub text (`logs::issue_text`, with the
//! game and chapter) and opens the bug form (`logs::bug_form_url`).
//!
//! The app notes each run it starts in `<logs>/last_run.json`. A run the app did not see end
//! (the app was closed, or Game Mode left it) is still `"done": false` there, so the next start
//! runs the after-run step for it ([`unfinished`]) and shows its screen once.

use crate::strings::{fill, Lang, Text};
use serde_json::{json, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use uncharted_core::games::Game;
use uncharted_core::logs::{self, Ended, KnownIssue, Redact, ReportDetails, RunInfo, WHERE_FILE};

/// The app's note of the last run it started, in the logs folder.
pub const RUN_MARK: &str = "last_run.json";

/// Note that a run of `game` starts now (`{"game": "<key>", "done": false}`).
pub fn mark_started(logs: &Path, game: Game) -> io::Result<()> {
    write_mark(logs, game, false)
}

/// The game of a run the app started but did not see end; `None` when the last run was seen
/// to its end or there is no readable note.
pub fn unfinished(logs: &Path) -> Option<Game> {
    let text = fs::read_to_string(logs.join(RUN_MARK)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    if value.get("done").and_then(Value::as_bool) != Some(false) {
        return None;
    }
    Game::from_key(value.get("game")?.as_str()?)
}

fn write_mark(logs: &Path, game: Game, done: bool) -> io::Result<()> {
    fs::create_dir_all(logs)?;
    let text = json!({"game": game.key(), "done": done}).to_string();
    fs::write(logs.join(RUN_MARK), text + "\n")
}

/// What the after-run step reads besides the run itself.
#[derive(Debug, Clone)]
pub struct Context {
    /// The launcher's logs folder (`AppDirs::logs`): `last_run.log`, `playthrough.jsonl`.
    pub logs: PathBuf,
    /// The build recorded in `playthrough.jsonl`.
    pub build: String,
    /// The settings preset recorded in `playthrough.jsonl`.
    pub preset: String,
    /// `known_issues.json`, loaded once.
    pub issues: Vec<KnownIssue>,
    /// The version, system lines and settings for the bug-report text.
    pub details: ReportDetails,
    /// What the bug-report text leaves out.
    pub redact: Redact,
}

/// The after-run step for a run of `game` that has ended; `saves` is the game's saves folder
/// (where the runtime keeps `where.json`). Appends the run to `playthrough.jsonl` (`Ended::Crash`
/// when the run log has a `Crash:` line, else `Ended::Ok`), marks the run seen, and returns the
/// screen for a crash. Crash help never stops the launcher: a log that cannot be written is
/// reported on stderr and the step goes on.
pub fn after_run(ctx: &Context, game: Game, saves: &Path, at_unix: u64) -> Option<AfterCrash> {
    let crash = logs::last_crash(&ctx.logs);
    let where_ = logs::load_where(&saves.join(WHERE_FILE));
    let ended = if crash.is_some() {
        Ended::Crash
    } else {
        Ended::Ok
    };
    let run = logs::record(
        &ctx.build,
        game,
        where_.as_ref(),
        &ctx.preset,
        None,
        ended,
        at_unix,
    );
    if let Err(error) = logs::append(&ctx.logs, &run) {
        eprintln!("could not add the run to the playthrough log: {error}");
    }
    if let Err(error) = write_mark(&ctx.logs, game, true) {
        eprintln!("could not note the run as seen: {error}");
    }
    let crash = crash?;
    let location = where_.as_ref().map(|w| w.location());
    let known = logs::match_known(&ctx.issues, game, location.as_deref(), &crash).cloned();
    Some(AfterCrash {
        game,
        chapter: run.chapter,
        crash,
        known,
    })
}

/// What the app does for the screen's buttons; the real one saves settings, starts games,
/// opens the browser and fills the clipboard, the tests' one records the calls.
pub trait Host {
    /// Save the player's `BB_*` setting `key` as `value`.
    fn set_setting(&mut self, key: &str, value: &str) -> Result<(), String>;
    /// Start `game` (with the saved settings).
    fn play(&mut self, game: Game) -> Result<(), String>;
    /// Open a link in the browser.
    fn open_url(&mut self, url: &str);
    /// Put a text on the clipboard.
    fn copy_text(&mut self, text: &str);
}

/// One button of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// "Turn on <fix> and play again": a known issue with a toggle.
    PlayWithFix,
    PlayAgain,
    /// The known issue's page.
    OpenIssue,
    /// An unknown crash: the bug form and the log on the clipboard.
    Report,
    Back,
}

/// What a button press leads to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pressed {
    /// The screen stays, with this note under the buttons (empty for none).
    Stay(String),
    /// The game was started again.
    Playing,
    /// Back to the home page.
    Home,
}

/// The screen's texts in one language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    pub heading: String,
    /// The game's name.
    pub game: String,
    pub chapter: String,
    /// The plain sentence: the known issue's summary, or that the crash is new.
    pub summary: String,
    /// The `Crash:` line, as the game said it.
    pub detail: String,
    /// The labels of [`AfterCrash::buttons`], in order.
    pub buttons: Vec<String>,
}

/// A crashed run, as the after-crash screen shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AfterCrash {
    pub game: Game,
    /// From `where.json` (`Where::chapter`); `None` before the first save.
    pub chapter: Option<String>,
    /// The `Crash:` line without its prefix.
    pub crash: String,
    /// The matching entry of `known_issues.json`.
    pub known: Option<KnownIssue>,
}

impl AfterCrash {
    /// The buttons, the first one focused: a known issue with a toggle: fix and play, its
    /// page, back; a known issue without one: play again, its page (when it has a link), back;
    /// an unknown crash: report, play again, back.
    pub fn buttons(&self) -> Vec<Button> {
        match &self.known {
            Some(issue) => {
                let mut buttons = vec![if issue.toggle.is_some() {
                    Button::PlayWithFix
                } else {
                    Button::PlayAgain
                }];
                if !issue.issue.is_empty() {
                    buttons.push(Button::OpenIssue);
                }
                buttons.push(Button::Back);
                buttons
            }
            None => vec![Button::Report, Button::PlayAgain, Button::Back],
        }
    }

    /// The texts in `lang`. The known issue's summary is the English of `known_issues.json`.
    pub fn screen(&self, lang: Lang) -> Screen {
        let chapter = match &self.chapter {
            Some(chapter) => fill(Text::Chapter.get(lang), chapter),
            None => Text::NoChapter.get(lang).to_string(),
        };
        let summary = match &self.known {
            Some(issue) => {
                let mut summary = fill(Text::Known.get(lang), &issue.summary);
                if let Some(version) = &issue.fixed_in {
                    summary.push(' ');
                    summary.push_str(&fill(Text::FixedIn.get(lang), version));
                }
                summary
            }
            None => Text::Unknown.get(lang).to_string(),
        };
        Screen {
            heading: Text::Heading.get(lang).to_string(),
            game: self.game.name().to_string(),
            chapter,
            summary,
            detail: fill(Text::Detail.get(lang), &self.crash),
            buttons: self
                .buttons()
                .into_iter()
                .map(|button| self.label(button, lang))
                .collect(),
        }
    }

    fn label(&self, button: Button, lang: Lang) -> String {
        match button {
            Button::PlayWithFix => {
                let key = self
                    .known
                    .as_ref()
                    .and_then(|issue| issue.toggle.as_ref())
                    .map_or("", |toggle| toggle.key.as_str());
                fill(Text::PlayWithFix.get(lang), &fix_name(key))
            }
            Button::PlayAgain => Text::PlayAgain.get(lang).to_string(),
            Button::OpenIssue => Text::OpenIssue.get(lang).to_string(),
            Button::Report => Text::Report.get(lang).to_string(),
            Button::Back => Text::Back.get(lang).to_string(),
        }
    }

    /// The game, chapter and crash for the bug report.
    pub fn run_info(&self) -> RunInfo {
        RunInfo {
            game: self.game,
            chapter: self.chapter.clone(),
            crash: Some(self.crash.clone()),
        }
    }

    /// Do what `button` does. The fix is saved before the game starts again; a failure stays
    /// on the screen with a note.
    pub fn press(&self, button: Button, ctx: &Context, host: &mut dyn Host, lang: Lang) -> Pressed {
        match button {
            Button::PlayWithFix => {
                if let Some(toggle) = self.known.as_ref().and_then(|i| i.toggle.as_ref()) {
                    if let Err(error) = host.set_setting(&toggle.key, &toggle.value) {
                        return Pressed::Stay(fill(Text::SaveFailed.get(lang), &error));
                    }
                }
                self.play(host, lang)
            }
            Button::PlayAgain => self.play(host, lang),
            Button::OpenIssue => {
                if let Some(issue) = &self.known {
                    host.open_url(&issue.issue);
                }
                Pressed::Stay(String::new())
            }
            Button::Report => {
                let run = self.run_info();
                host.copy_text(&logs::issue_text(
                    &ctx.logs,
                    &ctx.details,
                    Some(&run),
                    &ctx.redact,
                ));
                host.open_url(&logs::bug_form_url(&run));
                Pressed::Stay(Text::Copied.get(lang).to_string())
            }
            Button::Back => Pressed::Home,
        }
    }

    fn play(&self, host: &mut dyn Host, lang: Lang) -> Pressed {
        match host.play(self.game) {
            Ok(()) => Pressed::Playing,
            Err(error) => Pressed::Stay(fill(Text::StartFailed.get(lang), &error)),
        }
    }
}

/// A toggle's variable as the player reads it: `BB_SAFE_SHADERS` is "safe shaders".
pub fn fix_name(key: &str) -> String {
    let name = key.strip_prefix("BB_").unwrap_or(key);
    name.split('_')
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests;
