//! Per-game thin APK identity, M9 no-bundled-assets guard, and Stage 7.04 first-boot
//! download prompt hook for Stage 7.03 (`#524`).

use crate::gamepad::{LauncherInput, Nav};

/// Exact first-boot prompt text handed off to Stage 7.04 (`#525`).
pub const FIRST_BOOT_DOWNLOAD_PROMPT: &str =
    "Would you like to download the rest of the game files?";

/// Which per-game thin Android APK is being built or launched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AndroidGame {
    U1,
    U2,
    U3,
}

impl AndroidGame {
    /// All three collection games, in order.
    pub const ALL: [Self; 3] = [Self::U1, Self::U2, Self::U3];

    /// Short key matching `crates/core/src/games.rs::Game::key`.
    pub const fn key(self) -> &'static str {
        match self {
            Self::U1 => "u1",
            Self::U2 => "u2",
            Self::U3 => "u3",
        }
    }

    /// Parses `"u1"`, `"u2"`, or `"u3"` case-insensitively.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|g| g.key().eq_ignore_ascii_case(key.trim()))
    }

    /// Distinct Android `package` / `applicationId` for this thin APK.
    pub const fn application_id(self) -> &'static str {
        match self {
            Self::U1 => "org.uncharted.universal.u1",
            Self::U2 => "org.uncharted.universal.u2",
            Self::U3 => "org.uncharted.universal.u3",
        }
    }

    /// Distinct Android `android:label` for this thin APK.
    pub const fn app_label(self) -> &'static str {
        match self {
            Self::U1 => "Uncharted 1",
            Self::U2 => "Uncharted 2",
            Self::U3 => "Uncharted 3",
        }
    }

    /// Expected guest executable name for this game (`crates/core/src/games.rs`).
    pub const fn executable(self) -> &'static str {
        match self {
            Self::U1 => "eboot.bin",
            Self::U2 => "big2-ps4_Shipping.elf",
            Self::U3 => "big3-ps4_Shipping.elf",
        }
    }

    /// Returns the compile-time selected game when built with `--features game-u1|game-u2|game-u3`.
    pub const fn compiled_default() -> Option<Self> {
        #[cfg(feature = "game-u1")]
        {
            return Some(Self::U1);
        }
        #[cfg(all(not(feature = "game-u1"), feature = "game-u2"))]
        {
            return Some(Self::U2);
        }
        #[cfg(all(
            not(feature = "game-u1"),
            not(feature = "game-u2"),
            feature = "game-u3"
        ))]
        {
            return Some(Self::U3);
        }
        #[allow(unreachable_code)]
        None
    }
}

/// Focused action button on the Stage 7.03 -> 7.04 first-boot download prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptButton {
    Download,
    Later,
}

/// State of the first-boot game-file check and Stage 7.04 handoff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstBootDecision {
    /// Game files are missing; UI is showing [`FIRST_BOOT_DOWNLOAD_PROMPT`].
    PromptDownload,
    /// User accepted the prompt; hand off to Stage 7.04 resumable Dropbox download (`#525`).
    ReadyForStage704Download { game: AndroidGame },
    /// User declined or pressed Back on the first-boot prompt.
    DeclinedWait,
    /// Game files are already present under `app_storage_dir`; ready to launch.
    AlreadyInstalled { game: AndroidGame },
}

/// Gamepad-navigable state machine for the thin APK first-boot prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstBootState {
    pub game: AndroidGame,
    pub prompt_text: &'static str,
    pub focused_button: PromptButton,
    pub decision: FirstBootDecision,
}

impl FirstBootState {
    /// Evaluates first-boot readiness for `game` given whether prepared game files exist.
    pub fn evaluate(game: AndroidGame, game_files_present: bool) -> Self {
        let decision = if game_files_present {
            FirstBootDecision::AlreadyInstalled { game }
        } else {
            FirstBootDecision::PromptDownload
        };
        Self {
            game,
            prompt_text: FIRST_BOOT_DOWNLOAD_PROMPT,
            focused_button: PromptButton::Download,
            decision,
        }
    }

    /// Applies a Slint [`LauncherInput`] from gamepad/keyboard navigation.
    pub fn handle_input(&mut self, input: LauncherInput) -> FirstBootDecision {
        if self.decision != FirstBootDecision::PromptDownload {
            return self.decision;
        }
        match input {
            LauncherInput::Move(Nav::Left | Nav::Up) => {
                self.focused_button = PromptButton::Download;
            }
            LauncherInput::Move(Nav::Right | Nav::Down) => {
                self.focused_button = PromptButton::Later;
            }
            LauncherInput::Tab { .. } => {
                self.focused_button = match self.focused_button {
                    PromptButton::Download => PromptButton::Later,
                    PromptButton::Later => PromptButton::Download,
                };
            }
            LauncherInput::Activate => {
                self.decision = match self.focused_button {
                    PromptButton::Download => {
                        FirstBootDecision::ReadyForStage704Download { game: self.game }
                    }
                    PromptButton::Later => FirstBootDecision::DeclinedWait,
                };
            }
            LauncherInput::Back => {
                self.decision = FirstBootDecision::DeclinedWait;
            }
            LauncherInput::PrevGame | LauncherInput::NextGame | LauncherInput::OtherSaves => {}
        }
        self.decision
    }
}
