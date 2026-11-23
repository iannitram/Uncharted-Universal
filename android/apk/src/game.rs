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

/// Live Stage 7.04 (`#525`) download progress snapshot mirrored from
/// `uncharted_core::game_download::DownloadProgress`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DownloadProgressSnapshot {
    pub completed_files: usize,
    pub total_files: usize,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub current_file: Option<String>,
}

impl DownloadProgressSnapshot {
    /// Whole-percent completion in `0..=100`.
    pub fn percent(&self) -> u8 {
        match self
            .downloaded_bytes
            .saturating_mul(100)
            .checked_div(self.total_bytes)
        {
            Some(pct) => pct.min(100) as u8,
            None => {
                u8::from(self.completed_files >= self.total_files && self.total_files > 0) * 100
            }
        }
    }

    /// True when all expected files and bytes have completed verification.
    pub fn is_complete(&self) -> bool {
        self.total_files > 0
            && self.completed_files >= self.total_files
            && self.downloaded_bytes >= self.total_bytes
    }
}

/// Lifecycle phase of the Stage 7.04 per-game Dropbox download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadPhase {
    Idle,
    Downloading,
    Paused,
    Cancelled,
    Completed,
    Failed(String),
}

/// Gamepad- and UI-bindable state for the Stage 7.04 resumable download (`#525`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stage704DownloadState {
    pub game: AndroidGame,
    pub phase: DownloadPhase,
    pub progress: DownloadProgressSnapshot,
    pub pause_requested: bool,
    pub cancel_requested: bool,
}

impl Stage704DownloadState {
    pub fn new(game: AndroidGame) -> Self {
        Self {
            game,
            phase: DownloadPhase::Idle,
            progress: DownloadProgressSnapshot::default(),
            pause_requested: false,
            cancel_requested: false,
        }
    }

    pub fn begin(&mut self, total_files: usize, total_bytes: u64) {
        self.phase = DownloadPhase::Downloading;
        self.pause_requested = false;
        self.cancel_requested = false;
        self.progress = DownloadProgressSnapshot {
            completed_files: 0,
            total_files,
            downloaded_bytes: 0,
            total_bytes,
            current_file: None,
        };
    }

    pub fn apply_progress(&mut self, progress: DownloadProgressSnapshot) {
        self.progress = progress;
        if self.phase == DownloadPhase::Idle {
            self.phase = DownloadPhase::Downloading;
        }
    }

    pub fn request_pause(&mut self) {
        if self.phase == DownloadPhase::Downloading {
            self.pause_requested = true;
        }
    }

    pub fn request_resume(&mut self) {
        if matches!(
            self.phase,
            DownloadPhase::Paused | DownloadPhase::Failed(_) | DownloadPhase::Idle
        ) {
            self.pause_requested = false;
            self.cancel_requested = false;
            self.phase = DownloadPhase::Downloading;
        }
    }

    pub fn request_cancel(&mut self) {
        if matches!(
            self.phase,
            DownloadPhase::Downloading | DownloadPhase::Paused
        ) {
            self.cancel_requested = true;
        }
    }

    pub fn mark_paused(&mut self) {
        self.pause_requested = false;
        self.phase = DownloadPhase::Paused;
    }

    pub fn mark_cancelled(&mut self) {
        self.pause_requested = false;
        self.cancel_requested = false;
        self.phase = DownloadPhase::Cancelled;
    }

    pub fn mark_completed(&mut self) {
        self.pause_requested = false;
        self.cancel_requested = false;
        self.progress.current_file = None;
        self.phase = DownloadPhase::Completed;
    }

    pub fn mark_failed(&mut self, error: impl Into<String>) {
        self.pause_requested = false;
        self.phase = DownloadPhase::Failed(error.into());
    }

    /// Handles controller/keyboard input while the Stage 7.04 download screen is active.
    pub fn handle_input(&mut self, input: LauncherInput) {
        match input {
            LauncherInput::Activate => match self.phase {
                DownloadPhase::Downloading => self.request_pause(),
                DownloadPhase::Paused | DownloadPhase::Failed(_) => self.request_resume(),
                DownloadPhase::Idle | DownloadPhase::Cancelled | DownloadPhase::Completed => {}
            },
            LauncherInput::Back => self.request_cancel(),
            _ => {}
        }
    }
}

/// Gamepad-navigable state machine for the thin APK first-boot prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstBootState {
    pub game: AndroidGame,
    pub prompt_text: &'static str,
    pub focused_button: PromptButton,
    pub decision: FirstBootDecision,
    pub download: Stage704DownloadState,
}

impl FirstBootState {
    /// Evaluates first-boot readiness for `game` given whether prepared game files exist.
    pub fn evaluate(game: AndroidGame, game_files_present: bool) -> Self {
        let mut download = Stage704DownloadState::new(game);
        let decision = if game_files_present {
            download.mark_completed();
            FirstBootDecision::AlreadyInstalled { game }
        } else {
            FirstBootDecision::PromptDownload
        };
        Self {
            game,
            prompt_text: FIRST_BOOT_DOWNLOAD_PROMPT,
            focused_button: PromptButton::Download,
            decision,
            download,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_704_download_state_tracks_progress_pause_resume_and_cancel() {
        let mut first_boot = FirstBootState::evaluate(AndroidGame::U2, false);
        assert_eq!(first_boot.download.phase, DownloadPhase::Idle);

        assert_eq!(
            first_boot.handle_input(LauncherInput::Activate),
            FirstBootDecision::ReadyForStage704Download {
                game: AndroidGame::U2
            }
        );

        let dl = &mut first_boot.download;
        dl.begin(2, 1000);
        assert_eq!(dl.phase, DownloadPhase::Downloading);
        dl.apply_progress(DownloadProgressSnapshot {
            completed_files: 1,
            total_files: 2,
            downloaded_bytes: 500,
            total_bytes: 1000,
            current_file: Some("USRDIR/pak01.psarc".into()),
        });
        assert_eq!(dl.progress.percent(), 50);
        assert!(!dl.progress.is_complete());

        dl.handle_input(LauncherInput::Activate);
        assert!(dl.pause_requested);
        dl.mark_paused();
        assert_eq!(dl.phase, DownloadPhase::Paused);

        dl.handle_input(LauncherInput::Activate);
        assert_eq!(dl.phase, DownloadPhase::Downloading);
        assert!(!dl.pause_requested);

        dl.handle_input(LauncherInput::Back);
        assert!(dl.cancel_requested);
        dl.mark_cancelled();
        assert_eq!(dl.phase, DownloadPhase::Cancelled);
    }
}
