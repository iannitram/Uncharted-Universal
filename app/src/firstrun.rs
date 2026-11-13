//! The guided first run (roadmap 2.05), as a view model: plain Rust, tested without a window.
//!
//! Two paths lead to Add to Steam. The hub path (this device has the game files): pick the dump,
//! check it (`dump::check`), prepare each game with progress and errors shown. The client path
//! (the games come from the hub): find or type the hub, type its six-digit pairing code, pair and
//! pull the prepared games (`setup::client::pair_and_pull`) with the bytes shown. Then Add to
//! Steam: the launcher's shortcut and artwork, and when Steam is open, a prompt to close it first
//! and start it again afterwards.
//!
//! The page is one vertical list of buttons (the D-pad or stick moves, A acts, B goes back); the
//! pairing code is six digit boxes (Left and Right move, Up and Down turn a digit, the keyboard
//! types). Work that takes time is a [`Job`] the caller runs off the UI thread; what it finds
//! comes back as an [`Event`].

use crate::focus::{self, Digits};
use crate::screen::Input;
use std::path::PathBuf;
use uncharted_core::dump::DumpReport;
use uncharted_core::games::Game;
use uncharted_core::setup::client::{self, HubChoice, Pulled};
use uncharted_core::setup::{self, bytes_text, Prepared};

/// How many steps the page counts: how to set up, the files or the hub, preparing or pulling,
/// Steam.
pub const STEPS: usize = 4;

/// A key the first-run page reads: an [`Input`] of the Continue screen, a typed digit (the
/// pairing code) or Backspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Input(Input),
    Digit(u8),
    Backspace,
}

impl Key {
    /// The integer the `.slint` file passes through its `input` callback: the [`Input`] codes
    /// 0-10, the digits 20-29 and Backspace 30.
    pub fn from_code(code: i32) -> Option<Key> {
        match code {
            20..=29 => u8::try_from(code - 20).ok().map(Key::Digit),
            30 => Some(Key::Backspace),
            _ => Input::from_code(code).map(Key::Input),
        }
    }
}

/// Work for the caller to run off the UI thread (or, for the last two, to do at once).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    None,
    /// Look for dump folders (`setup::search_roots`, `setup::dump_candidates`).
    Scan,
    /// `dump::check` the folder.
    Check(PathBuf),
    /// `setup::prepare_all` of the plan.
    Prepare(Vec<(Game, PathBuf)>),
    /// `hub::discover`, as `client::choices`.
    Discover,
    /// `client::pair_and_pull` with the hub, the code typed.
    PairPull {
        hub: HubChoice,
        code: String,
    },
    /// Add to Steam, unless Steam is open (then [`Event::SteamOpen`]).
    AddToSteam,
    /// Ask Steam to quit, wait for it, then add to Steam.
    CloseSteamAndAdd,
    /// Give the text field the keyboard.
    EditField,
    /// Back to the Continue screen, which loads again; `start_steam` starts Steam first.
    Finish {
        start_steam: bool,
    },
}

/// What a job came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Candidates(Vec<PathBuf>),
    Checked(PathBuf, Result<DumpReport, String>),
    /// Preparing game `index` of the plan has begun.
    Preparing {
        index: usize,
        game: Game,
    },
    Prepared(Vec<Prepared>),
    Hubs(Result<Vec<HubChoice>, String>),
    /// Bytes in the staging folder so far.
    PullBytes(u64),
    Pulled(Result<Pulled, String>),
    SteamOpen,
    /// Added to Steam: what was written, in a sentence; or why not.
    Added(Result<String, String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Welcome,
    PickDump {
        candidates: Vec<PathBuf>,
        searching: bool,
    },
    Checking(PathBuf),
    Report {
        lines: Vec<String>,
        plan: Vec<(Game, PathBuf)>,
    },
    Preparing {
        plan: Vec<(Game, PathBuf)>,
        index: usize,
        game: Game,
    },
    Prepared {
        lines: Vec<String>,
        /// The games that failed, to try again.
        retry: Vec<(Game, PathBuf)>,
    },
    FindHub {
        hubs: Vec<HubChoice>,
        searching: bool,
        note: String,
    },
    Code {
        hub: HubChoice,
        digits: Digits,
    },
    Pulling {
        hub: HubChoice,
        bytes: u64,
    },
    Pulled {
        lines: Vec<String>,
    },
    Steam,
    SteamOpen,
    Adding {
        closing: bool,
    },
    Done {
        lines: Vec<String>,
    },
}

/// What the page shows.
#[derive(Debug, Clone, PartialEq)]
pub struct FirstRunView {
    /// "Step 2 of 4".
    pub step: String,
    pub title: String,
    pub lines: Vec<String>,
    /// The buttons, top to bottom.
    pub options: Vec<String>,
    /// The focused button.
    pub focus: usize,
    /// The text field's placeholder; empty when the step has no field.
    pub field: String,
    /// The six code boxes ("" for an empty one) and the cursor; empty when the step has none.
    pub code: Vec<String>,
    pub code_cursor: usize,
    /// 0.0 to 1.0, or `None` for no bar.
    pub progress: Option<f32>,
    /// Work is going on: keys other than the controller's movement do nothing.
    pub busy: bool,
    /// The last error, in a sentence; empty when none.
    pub error: String,
    /// What the buttons do: "A chooses, B goes back".
    pub hint: String,
}

/// The first-run page.
pub struct FirstRun {
    step: Step,
    focus: usize,
    error: String,
    /// The game whose card's Set up opened the page: only that one is prepared.
    only: Option<Game>,
    /// This device can have a Steam shortcut (Windows, Linux, Mac).
    steam: bool,
    /// Steam was closed for the shortcut and is not started again yet.
    steam_closed: bool,
    /// The games prepared on this page so far.
    ready: Vec<Game>,
}

const HUB: &str = "This device has the game files";
const CLIENT: &str = "Get the games from my hub";
const NOT_NOW: &str = "Not now";
const TYPE_FOLDER: &str = "Type the folder";
const TYPE_HUB: &str = "Type the hub's address";
const CHOOSE_ANOTHER: &str = "Choose another folder";
const TRY_AGAIN: &str = "Try again";
const NEXT: &str = "Next";
const ADD: &str = "Add to Steam";
const CLOSE_AND_ADD: &str = "Close Steam and add";
const BACK: &str = "Back";
const START_STEAM: &str = "Start Steam again";
const OPEN: &str = "Open the launcher";

impl FirstRun {
    /// The page at its first step. `only` is the game whose Set up button opened it; `steam`
    /// whether this device can have a Steam shortcut.
    pub fn new(only: Option<Game>, steam: bool) -> FirstRun {
        FirstRun {
            step: Step::Welcome,
            focus: 0,
            error: String::new(),
            only,
            steam,
            steam_closed: false,
            ready: Vec::new(),
        }
    }

    fn go(&mut self, step: Step) {
        self.step = step;
        self.focus = 0;
        self.error.clear();
    }

    fn options(&self) -> Vec<String> {
        let own = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        match &self.step {
            Step::Welcome => own(&[HUB, CLIENT, NOT_NOW]),
            Step::PickDump { candidates, .. } => candidates
                .iter()
                .map(|c| c.display().to_string())
                .chain([TYPE_FOLDER.to_owned()])
                .collect(),
            Step::Report { plan, .. } if !plan.is_empty() => {
                vec![prepare_label(plan), CHOOSE_ANOTHER.to_owned()]
            }
            Step::Report { .. } => own(&[CHOOSE_ANOTHER]),
            Step::Prepared { retry, .. } => {
                let mut items = Vec::new();
                if !self.ready.is_empty() {
                    items.push(NEXT);
                }
                if !retry.is_empty() {
                    items.push(TRY_AGAIN);
                }
                items.push(CHOOSE_ANOTHER);
                own(&items)
            }
            Step::FindHub { hubs, .. } => hubs
                .iter()
                .map(|h| h.label.clone())
                .chain([TYPE_HUB.to_owned()])
                .collect(),
            Step::Pulled { .. } => own(&[NEXT]),
            Step::Steam => own(&[ADD, NOT_NOW]),
            Step::SteamOpen => own(&[CLOSE_AND_ADD, BACK]),
            Step::Done { .. } if self.steam_closed => own(&[START_STEAM, OPEN]),
            Step::Done { .. } => own(&[OPEN]),
            Step::Checking(_)
            | Step::Preparing { .. }
            | Step::Code { .. }
            | Step::Pulling { .. }
            | Step::Adding { .. } => Vec::new(),
        }
    }

    /// A key from the keyboard or the controller.
    pub fn handle(&mut self, key: Key) -> Job {
        if let Step::Code { digits, .. } = &mut self.step {
            match key {
                Key::Input(Input::Move(nav)) => *digits = digits.step(nav),
                Key::Digit(d) => *digits = digits.type_digit(d),
                Key::Backspace => *digits = digits.backspace(),
                Key::Input(Input::Activate) => return self.pair(),
                Key::Input(Input::Back) => return self.back(),
                Key::Input(_) => {}
            }
            return Job::None;
        }
        let len = self.options().len();
        match key {
            Key::Input(Input::Move(nav)) => self.focus = focus::list_step(self.focus, nav, len),
            Key::Input(Input::Tab { back }) => self.focus = focus::list_tab(self.focus, back, len),
            Key::Input(Input::Activate) => return self.choose(self.focus),
            Key::Input(Input::Back) => return self.back(),
            _ => {}
        }
        Job::None
    }

    /// A click or tap on button `index`.
    pub fn click(&mut self, index: usize) -> Job {
        if index >= self.options().len() {
            return Job::None;
        }
        self.focus = index;
        self.choose(index)
    }

    /// Enter in the text field: a dump folder, or a hub's address.
    pub fn field(&mut self, text: &str) -> Job {
        let text = text.trim();
        match &self.step {
            Step::PickDump { .. } if text.is_empty() => {
                self.error = "Type the folder that holds the game files.".into();
                Job::None
            }
            Step::PickDump { .. } => self.check(PathBuf::from(text)),
            Step::FindHub { .. } => match client::hub_address(text) {
                Ok(hub) => {
                    self.go(Step::Code {
                        hub,
                        digits: Digits::default(),
                    });
                    Job::None
                }
                Err(err) => {
                    self.error = sentence(&message(&err));
                    Job::None
                }
            },
            _ => Job::None,
        }
    }

    fn check(&mut self, folder: PathBuf) -> Job {
        self.go(Step::Checking(folder.clone()));
        Job::Check(folder)
    }

    fn pair(&mut self) -> Job {
        let Step::Code { hub, digits } = &self.step else {
            return Job::None;
        };
        let Some(code) = digits.code() else {
            self.error = "Type all six digits of the code the hub shows.".into();
            return Job::None;
        };
        let hub = hub.clone();
        self.go(Step::Pulling {
            hub: hub.clone(),
            bytes: 0,
        });
        Job::PairPull { hub, code }
    }

    fn steam_or_done(&mut self, lines: Vec<String>) -> Job {
        if self.steam {
            self.go(Step::Steam);
        } else {
            self.go(Step::Done { lines });
        }
        Job::None
    }

    fn choose(&mut self, index: usize) -> Job {
        let options = self.options();
        let Some(label) = options.get(index).map(String::as_str) else {
            return Job::None;
        };
        match (&self.step, label) {
            (Step::Welcome, HUB) => {
                self.go(Step::PickDump {
                    candidates: Vec::new(),
                    searching: true,
                });
                Job::Scan
            }
            (Step::Welcome, CLIENT) => {
                self.go(Step::FindHub {
                    hubs: Vec::new(),
                    searching: true,
                    note: String::new(),
                });
                Job::Discover
            }
            (Step::Welcome, _) => Job::Finish { start_steam: false },
            (Step::PickDump { .. }, TYPE_FOLDER) | (Step::FindHub { .. }, TYPE_HUB) => {
                Job::EditField
            }
            (Step::PickDump { candidates, .. }, _) => match candidates.get(index) {
                Some(folder) => self.check(folder.clone()),
                None => Job::None,
            },
            (Step::Report { .. } | Step::Prepared { .. }, CHOOSE_ANOTHER) => self.rescan(),
            (Step::Report { plan, .. }, _) | (Step::Prepared { retry: plan, .. }, TRY_AGAIN) => {
                let plan = plan.clone();
                self.start_prepare(plan)
            }
            (Step::Prepared { lines, .. }, _) | (Step::Pulled { lines }, _) => {
                let lines = lines.clone();
                self.steam_or_done(lines)
            }
            (Step::FindHub { hubs, .. }, _) => {
                if let Some(hub) = hubs.get(index) {
                    let hub = hub.clone();
                    self.go(Step::Code {
                        hub,
                        digits: Digits::default(),
                    });
                }
                Job::None
            }
            (Step::Steam, ADD) => {
                self.go(Step::Adding { closing: false });
                Job::AddToSteam
            }
            (Step::Steam, _) => {
                self.go(Step::Done {
                    lines: vec!["The launcher is not in Steam. Add it later with \
                         uncharted-universal setup steam."
                        .into()],
                });
                Job::None
            }
            (Step::SteamOpen, CLOSE_AND_ADD) => {
                self.steam_closed = true;
                self.go(Step::Adding { closing: true });
                Job::CloseSteamAndAdd
            }
            (Step::SteamOpen, _) => {
                self.go(Step::Steam);
                Job::None
            }
            (Step::Done { .. }, START_STEAM) => Job::Finish { start_steam: true },
            (Step::Done { .. }, _) => Job::Finish { start_steam: false },
            _ => Job::None,
        }
    }

    fn start_prepare(&mut self, plan: Vec<(Game, PathBuf)>) -> Job {
        let Some(&(game, _)) = plan.first() else {
            return Job::None;
        };
        self.go(Step::Preparing {
            plan: plan.clone(),
            index: 0,
            game,
        });
        Job::Prepare(plan)
    }

    fn rescan(&mut self) -> Job {
        self.go(Step::PickDump {
            candidates: Vec::new(),
            searching: true,
        });
        Job::Scan
    }

    /// B (Circle), Esc: one step back; from the first step, leave. Work that is going on is not
    /// stopped.
    fn back(&mut self) -> Job {
        match &self.step {
            Step::Welcome => Job::Finish { start_steam: false },
            Step::PickDump { .. } | Step::FindHub { .. } => {
                self.go(Step::Welcome);
                Job::None
            }
            Step::Report { .. } | Step::Prepared { .. } => self.rescan(),
            Step::Code { .. } => {
                self.go(Step::FindHub {
                    hubs: Vec::new(),
                    searching: true,
                    note: String::new(),
                });
                Job::Discover
            }
            Step::SteamOpen => {
                self.go(Step::Steam);
                Job::None
            }
            Step::Steam => self.choose(1),
            Step::Done { .. } => Job::Finish {
                start_steam: self.steam_closed,
            },
            Step::Checking(_)
            | Step::Preparing { .. }
            | Step::Pulling { .. }
            | Step::Pulled { .. }
            | Step::Adding { .. } => Job::None,
        }
    }

    /// What a job came to. An event for a step the page has left is dropped.
    pub fn event(&mut self, event: Event) -> Job {
        match (event, &mut self.step) {
            (Event::Candidates(found), Step::PickDump { .. }) => {
                self.step = Step::PickDump {
                    candidates: found,
                    searching: false,
                };
                self.focus = 0;
            }
            (Event::Checked(folder, result), Step::Checking(at)) if *at == folder => match result {
                Ok(report) => {
                    let plan = if setup::can_prepare(&report) {
                        setup::prepare_plan(&report, self.only)
                    } else {
                        Vec::new()
                    };
                    let lines = report_lines(&folder, &report, self.only, &plan);
                    self.go(Step::Report { lines, plan });
                }
                Err(err) => {
                    self.go(Step::Report {
                        lines: vec![format!("Checked {}.", folder.display())],
                        plan: Vec::new(),
                    });
                    self.error = sentence(&err);
                }
            },
            (
                Event::Preparing { index, game },
                Step::Preparing {
                    index: at,
                    game: now,
                    ..
                },
            ) => {
                (*at, *now) = (index, game);
            }
            (Event::Prepared(done), Step::Preparing { plan, .. }) => {
                let plan = std::mem::take(plan);
                self.prepared(&plan, &done);
            }
            (Event::Hubs(result), Step::FindHub { .. }) => {
                let (hubs, note) = match result {
                    Ok(hubs) if hubs.is_empty() => (
                        hubs,
                        "No hub answered on the home network. Type its address instead.".into(),
                    ),
                    Ok(hubs) => (hubs, String::new()),
                    Err(err) => (
                        Vec::new(),
                        format!(
                            "Could not look for hubs: {} Type its address instead.",
                            sentence(&err)
                        ),
                    ),
                };
                self.step = Step::FindHub {
                    hubs,
                    searching: false,
                    note,
                };
                self.focus = 0;
            }
            (Event::PullBytes(now), Step::Pulling { bytes, .. }) => *bytes = now,
            (Event::Pulled(result), Step::Pulling { hub, .. }) => match result {
                Ok(pulled) => {
                    let lines = pulled_lines(&pulled);
                    self.go(Step::Pulled { lines });
                }
                Err(err) => {
                    let hub = hub.clone();
                    self.go(Step::Code {
                        hub,
                        digits: Digits::default(),
                    });
                    self.error = sentence(&err);
                }
            },
            (Event::SteamOpen, Step::Adding { .. }) => self.go(Step::SteamOpen),
            (Event::Added(result), Step::Adding { .. }) => match result {
                Ok(text) => {
                    let mut lines = vec![text];
                    if self.steam_closed {
                        lines.push("Steam was closed for this; start it again below.".into());
                    }
                    self.go(Step::Done { lines });
                }
                Err(err) => {
                    self.go(Step::Steam);
                    self.error = sentence(&err);
                }
            },
            _ => {}
        }
        Job::None
    }

    /// The results of a prepare: a line per game, and the failed games kept to try again.
    fn prepared(&mut self, plan: &[(Game, PathBuf)], done: &[Prepared]) {
        let mut lines = Vec::new();
        let mut retry = Vec::new();
        for prepared in done {
            match &prepared.result {
                Ok(_) => {
                    if !self.ready.contains(&prepared.game) {
                        self.ready.push(prepared.game);
                    }
                    lines.push(format!("{}: ready.", prepared.game.name()));
                }
                Err(err) => {
                    lines.push(format!("{}: {}", prepared.game.name(), sentence(err)));
                    retry.extend(plan.iter().filter(|(g, _)| *g == prepared.game).cloned());
                }
            }
        }
        self.go(Step::Prepared { lines, retry });
    }

    /// The page as it is now.
    pub fn view(&self) -> FirstRunView {
        let options = self.options();
        let (number, title, mut lines) = match &self.step {
            Step::Welcome => (
                1,
                "Set up Uncharted".to_owned(),
                vec![
                    "The hub is the device that holds your own dump of the game: it checks the \
                     files and prepares each game. Your other devices get the prepared games from \
                     it over the home network."
                        .to_owned(),
                    "Is this the hub, or does it get the games from one?".to_owned(),
                ],
            ),
            Step::PickDump {
                candidates,
                searching,
            } => (
                2,
                "Find the game files".to_owned(),
                vec![if *searching {
                    "Looking for a dump in Games, Downloads, Desktop and your home folder..."
                        .to_owned()
                } else if candidates.is_empty() {
                    "No dump found nearby. Type the folder that holds sce_sys and eboot.bin."
                        .to_owned()
                } else {
                    "Choose the folder of your dump, or type another.".to_owned()
                }],
            ),
            Step::Checking(folder) => (
                2,
                "Checking the files".to_owned(),
                vec![format!("Checking {}...", folder.display())],
            ),
            Step::Report { lines, .. } => (2, "The files".to_owned(), lines.clone()),
            Step::Preparing { plan, index, game } => (
                3,
                "Preparing".to_owned(),
                vec![
                    format!(
                        "Preparing {} ({} of {})...",
                        game.name(),
                        index + 1,
                        plan.len()
                    ),
                    "This copies and patches the game files; it can take a few minutes.".into(),
                ],
            ),
            Step::Prepared { lines, .. } => (3, "Prepared".to_owned(), lines.clone()),
            Step::FindHub {
                searching, note, ..
            } => (
                2,
                "Find your hub".to_owned(),
                vec![if *searching {
                    "Looking for hubs on the home network...".to_owned()
                } else if note.is_empty() {
                    "Choose your hub, or type its address.".to_owned()
                } else {
                    note.clone()
                }],
            ),
            Step::Code { hub, .. } => (
                2,
                format!("Pair with {}", hub.label),
                vec!["Type the six-digit code the hub shows.".to_owned()],
            ),
            Step::Pulling { hub, bytes } => (
                3,
                "Getting the games".to_owned(),
                vec![format!(
                    "Pairing with {} and getting the prepared games: {} so far...",
                    hub.label,
                    bytes_text(*bytes)
                )],
            ),
            Step::Pulled { lines } => (3, "Got the games".to_owned(), lines.clone()),
            Step::Steam => (
                4,
                "Add to Steam".to_owned(),
                vec![
                    "Add the launcher to Steam, with its artwork, so it starts from the library \
                     and from Game Mode."
                        .to_owned(),
                ],
            ),
            Step::SteamOpen => (
                4,
                "Steam is open".to_owned(),
                vec![
                    "Steam writes its own shortcuts when it closes, so it has to be closed while \
                     the launcher is added. Close it now? It can start again afterwards."
                        .to_owned(),
                ],
            ),
            Step::Adding { closing } => (
                4,
                "Adding to Steam".to_owned(),
                vec![if *closing {
                    "Closing Steam, then adding the launcher...".to_owned()
                } else {
                    "Adding the launcher...".to_owned()
                }],
            ),
            Step::Done { lines } => (4, "All set".to_owned(), lines.clone()),
        };
        let (code, code_cursor) = match &self.step {
            Step::Code { digits, .. } => (
                digits
                    .digits
                    .iter()
                    .map(|d| d.map(|d| d.to_string()).unwrap_or_default())
                    .collect(),
                digits.cursor,
            ),
            _ => (Vec::new(), 0),
        };
        let field = match &self.step {
            Step::PickDump { .. } => "The dump folder, as /home/deck/Games/CUSA02320",
            Step::FindHub { .. } => "The hub's address and port, as 192.168.1.20:47120",
            _ => "",
        };
        let progress = match &self.step {
            Step::Preparing { plan, index, .. } if !plan.is_empty() => {
                Some(*index as f32 / plan.len() as f32)
            }
            _ => None,
        };
        let busy = matches!(
            self.step,
            Step::Checking(_)
                | Step::Preparing { .. }
                | Step::Pulling { .. }
                | Step::Adding { .. }
                | Step::PickDump {
                    searching: true,
                    ..
                }
                | Step::FindHub {
                    searching: true,
                    ..
                }
        );
        let hint = if !code.is_empty() {
            "Left and Right move, Up and Down change a digit, A pairs, B goes back"
        } else if busy {
            "Please wait"
        } else {
            "A chooses, B goes back"
        };
        if lines.is_empty() {
            lines.push(String::new());
        }
        FirstRunView {
            step: format!("Step {number} of {STEPS}"),
            title,
            lines,
            focus: self.focus.min(options.len().saturating_sub(1)),
            options,
            field: field.to_owned(),
            code,
            code_cursor,
            progress,
            busy,
            error: self.error.clone(),
            hint: hint.to_owned(),
        }
    }
}

/// "Prepare Uncharted 2" or "Prepare the 3 games".
fn prepare_label(plan: &[(Game, PathBuf)]) -> String {
    match plan {
        [(game, _)] => format!("Prepare {}", game.name()),
        _ => format!("Prepare the {} games", plan.len()),
    }
}

/// The lines of the dump check: what was found, then every problem.
fn report_lines(
    folder: &std::path::Path,
    report: &DumpReport,
    only: Option<Game>,
    plan: &[(Game, PathBuf)],
) -> Vec<String> {
    let mut lines = vec![format!(
        "{}{}",
        folder.display(),
        report
            .title_id
            .as_deref()
            .map(|t| format!(" ({t})"))
            .unwrap_or_default()
    )];
    for found in &report.games {
        lines.push(format!(
            "{}: found{}",
            found.game.name(),
            found
                .app_ver
                .as_deref()
                .map(|v| format!(", version {v}"))
                .unwrap_or_default()
        ));
    }
    lines.extend(report.problems.iter().cloned());
    if let Some(game) = only {
        if setup::can_prepare(report) && plan.is_empty() {
            lines.push(format!("{} is not in this dump.", game.name()));
        }
    }
    if plan.is_empty() {
        lines.push("Nothing here can be prepared; choose another folder.".into());
    }
    lines
}

/// The lines after a pull.
fn pulled_lines(pulled: &Pulled) -> Vec<String> {
    let names: Vec<&str> = pulled.games.iter().map(|g| g.name()).collect();
    let mut lines = vec![format!(
        "Installed {} ({}).",
        names.join(", "),
        bytes_text(pulled.bytes)
    )];
    if let Some(note) = &pulled.peer_note {
        lines.push(format!(
            "The hub was not remembered ({note}); the next update pairs again."
        ));
    }
    lines
}

/// The sentence of a core error for the page: a hub's or Steam's own words without the
/// "Build hub:" prefix of its `Display`.
pub fn message(err: &uncharted_core::Error) -> String {
    match err {
        uncharted_core::Error::Hub(m) => m.clone(),
        other => other.to_string(),
    }
}

/// `text` with a capital first letter and a full stop.
fn sentence(text: &str) -> String {
    let text = text.trim();
    let mut chars = text.chars();
    let mut out: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => return String::new(),
    };
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

#[cfg(test)]
mod tests;
