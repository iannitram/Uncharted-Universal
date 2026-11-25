//! Per-game thin APK identity, M9 no-bundled-assets guard, and Stage 7.04 first-boot
//! download prompt hook for Stage 7.03 (`#524`).

use crate::gamepad::{LauncherInput, Nav};
use crate::save_relay::{game_id, GameFilesBackend, GameFilesResult, SaveRelayOutcome};
use std::fs;
use std::path::{Path, PathBuf};

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

    /// Per-game data directory name (`u1data`, `u2data`, `u3data`).
    pub const fn data_dir_name(self) -> &'static str {
        match self {
            Self::U1 => "u1data",
            Self::U2 => "u2data",
            Self::U3 => "u3data",
        }
    }

    /// Local per-game directory under `app_storage_dir` (`<app_storage_dir>/games/<game-id>`).
    pub fn game_dir(self, app_storage_dir: &Path) -> PathBuf {
        app_storage_dir.join("games").join(game_id(self))
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

/// Manifest filename stored in the per-game directory when game files are downloaded.
pub const GAME_FILES_MANIFEST_NAME: &str = "game-files.json";

fn dir_has_part_files(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if dir_has_part_files(&path) {
                return true;
            }
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".part"))
        {
            return true;
        }
    }
    false
}

/// Returns `true` when `u1data`/`u2data`/`u3data` and `game-files.json` exist under
/// `<app_storage_dir>/games/<game-id>` with no leftover `.part` files.
pub fn local_game_files_complete(app_storage_dir: &Path, game: AndroidGame) -> bool {
    let dest = game.game_dir(app_storage_dir);
    let data_dir = dest.join(game.data_dir_name());
    let manifest_path = dest.join(GAME_FILES_MANIFEST_NAME);
    data_dir.is_dir() && manifest_path.is_file() && !dir_has_part_files(&dest)
}

/// Checks local game-file completeness including SHA-256 and byte-size verification via `save-relay`.
pub fn local_game_files_complete_with_backend(
    app_storage_dir: &Path,
    game: AndroidGame,
    backend: &mut dyn GameFilesBackend,
) -> bool {
    let dest = game.game_dir(app_storage_dir);
    let data_dir = dest.join(game.data_dir_name());
    let manifest_path = dest.join(GAME_FILES_MANIFEST_NAME);
    if !data_dir.is_dir() || !manifest_path.is_file() {
        return false;
    }
    let Ok(manifest_json) = fs::read_to_string(&manifest_path) else {
        return false;
    };
    let Some(dest_str) = dest.to_str() else {
        return false;
    };
    let status = backend.game_files_status(game, dest_str, &manifest_json);
    status.outcome == SaveRelayOutcome::Done && status.complete && status.partial_files == 0
}

/// Gamepad- and UI-bindable state for the Stage 7.04 resumable download (`#525`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stage704DownloadState {
    pub game: AndroidGame,
    pub phase: DownloadPhase,
    pub progress: DownloadProgressSnapshot,
    pub partial_files: u64,
    pub reauth_needed: bool,
    pub offline_retry: bool,
    pub pause_requested: bool,
    pub cancel_requested: bool,
}

impl Stage704DownloadState {
    pub fn new(game: AndroidGame) -> Self {
        Self {
            game,
            phase: DownloadPhase::Idle,
            progress: DownloadProgressSnapshot::default(),
            partial_files: 0,
            reauth_needed: false,
            offline_retry: false,
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

    /// Updates progress counters from a `save-relay` `game-files-status` result (including `.part`
    /// bytes already on disk after battery death or force-close).
    pub fn sync_status_from_result(&mut self, status: &GameFilesResult) {
        self.partial_files = status.partial_files;
        self.progress.total_files = status.total_files as usize;
        self.progress.completed_files = status.completed_files as usize;
        self.progress.downloaded_bytes = status.bytes_on_disk;
        self.progress.total_bytes = status.total_bytes;
        if status.outcome == SaveRelayOutcome::Done && status.complete {
            self.reauth_needed = false;
            self.offline_retry = false;
            self.mark_completed();
        }
    }

    /// Runs or resumes the per-game asset download through `save-relay#305`, preserving `.part`
    /// byte-offset progress and surfacing `reauth_needed` / offline retry states cleanly.
    pub fn run_resumable_download<B: GameFilesBackend + ?Sized>(
        &mut self,
        backend: &mut B,
        dest_dir: &str,
        manifest_json: &str,
    ) -> &DownloadPhase {
        let pre_status = backend.game_files_status(self.game, dest_dir, manifest_json);
        if pre_status.outcome == SaveRelayOutcome::Done {
            self.sync_status_from_result(&pre_status);
            if pre_status.complete {
                return &self.phase;
            }
        }

        self.reauth_needed = false;
        self.offline_retry = false;
        self.pause_requested = false;
        self.cancel_requested = false;
        self.phase = DownloadPhase::Downloading;

        let res = backend.download_game_files(self.game, dest_dir, manifest_json);
        if res.outcome == SaveRelayOutcome::Done && res.complete {
            self.partial_files = 0;
            self.progress.total_files = res.total_files as usize;
            self.progress.completed_files = res.total_files as usize;
            self.progress.downloaded_bytes = res.total_bytes;
            self.progress.total_bytes = res.total_bytes;
            self.mark_completed();
            return &self.phase;
        }

        // Refresh on-disk `.part` progress so UI reflects bytes saved before interruption.
        let post_status = backend.game_files_status(self.game, dest_dir, manifest_json);
        if post_status.outcome == SaveRelayOutcome::Done {
            self.partial_files = post_status.partial_files;
            self.progress.total_files = post_status.total_files as usize;
            self.progress.completed_files = post_status.completed_files as usize;
            self.progress.downloaded_bytes = post_status.bytes_on_disk;
            self.progress.total_bytes = post_status.total_bytes;
        }

        if res.reauth_needed || res.error_code == "reauth_needed" {
            self.reauth_needed = true;
            self.mark_failed(if res.message.is_empty() {
                "Dropbox session expired; please sign in again.".to_owned()
            } else {
                res.message
            });
        } else if res.outcome == SaveRelayOutcome::Offline || res.error_code == "server_unreachable"
        {
            self.offline_retry = true;
            self.mark_failed(if res.message.is_empty() {
                "Dropbox is unreachable; retry when online.".to_owned()
            } else {
                res.message
            });
        } else {
            self.mark_failed(if res.message.is_empty() {
                format!("Download failed ({})", res.error_code)
            } else {
                res.message
            });
        }
        &self.phase
    }

    /// Retries or resumes an interrupted, offline, or re-authenticated download from `.part` state.
    pub fn retry_or_resume<B: GameFilesBackend + ?Sized>(
        &mut self,
        backend: &mut B,
        dest_dir: &str,
        manifest_json: &str,
    ) -> &DownloadPhase {
        self.request_resume();
        self.run_resumable_download(backend, dest_dir, manifest_json)
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

    /// Evaluates first-boot readiness from `app_storage_dir` (`u1data`/`u2data`/`u3data` + manifest).
    pub fn evaluate_from_storage(game: AndroidGame, app_storage_dir: &Path) -> Self {
        Self::evaluate(game, local_game_files_complete(app_storage_dir, game))
    }

    /// Evaluates first-boot readiness using `save-relay`'s `game_files_status` SHA-256 check,
    /// populating any existing `.part` resume progress on `self.download`.
    pub fn evaluate_with_backend<B: GameFilesBackend + ?Sized>(
        game: AndroidGame,
        app_storage_dir: &Path,
        backend: &mut B,
    ) -> Self {
        let dest = game.game_dir(app_storage_dir);
        let data_dir = dest.join(game.data_dir_name());
        let manifest_path = dest.join(GAME_FILES_MANIFEST_NAME);
        let mut state = Self::evaluate(game, false);
        if let (Some(dest_str), Ok(manifest_json)) =
            (dest.to_str(), fs::read_to_string(&manifest_path))
        {
            let status = backend.game_files_status(game, dest_str, &manifest_json);
            if status.outcome == SaveRelayOutcome::Done {
                state.download.sync_status_from_result(&status);
                if data_dir.is_dir() && status.complete && status.partial_files == 0 {
                    state.decision = FirstBootDecision::AlreadyInstalled { game };
                }
            }
        }
        state
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

    struct SimulatedDropboxBackend {
        total_files: u64,
        total_bytes: u64,
        bytes_on_disk: u64,
        completed_files: u64,
        partial_files: u64,
        fail_mode: Option<&'static str>,
        download_calls: usize,
        status_calls: usize,
    }

    impl GameFilesBackend for SimulatedDropboxBackend {
        fn game_files_status(
            &mut self,
            game: AndroidGame,
            _dest_dir: &str,
            _manifest_json: &str,
        ) -> GameFilesResult {
            self.status_calls += 1;
            let complete = self.completed_files == self.total_files && self.partial_files == 0;
            GameFilesResult {
                outcome: SaveRelayOutcome::Done,
                game_id: game_id(game).to_owned(),
                total_files: self.total_files,
                completed_files: self.completed_files,
                downloaded_files: 0,
                skipped_files: 0,
                partial_files: self.partial_files,
                bytes_on_disk: self.bytes_on_disk,
                bytes_downloaded: 0,
                total_bytes: self.total_bytes,
                complete,
                reauth_needed: false,
                error_code: String::new(),
                message: String::new(),
            }
        }

        fn download_game_files(
            &mut self,
            game: AndroidGame,
            _dest_dir: &str,
            _manifest_json: &str,
        ) -> GameFilesResult {
            self.download_calls += 1;
            if let Some(mode) = self.fail_mode.take() {
                // Simulate partial `.part` progress written before battery death / auth expiry.
                self.bytes_on_disk = self.total_bytes / 2;
                self.completed_files = 1;
                self.partial_files = 1;
                return match mode {
                    "reauth_needed" => {
                        GameFilesResult::error("reauth_needed", "Dropbox token expired", true)
                    }
                    "server_unreachable" => GameFilesResult {
                        outcome: SaveRelayOutcome::Offline,
                        error_code: "server_unreachable".to_owned(),
                        message: "offline".to_owned(),
                        ..GameFilesResult::error("server_unreachable", "offline", false)
                    },
                    _ => GameFilesResult::error(mode, "simulated failure", false),
                };
            }
            let remaining = self.total_bytes.saturating_sub(self.bytes_on_disk);
            let skipped = self.completed_files;
            let downloaded = self.total_files.saturating_sub(skipped);
            self.bytes_on_disk = self.total_bytes;
            self.completed_files = self.total_files;
            self.partial_files = 0;
            GameFilesResult {
                outcome: SaveRelayOutcome::Done,
                game_id: game_id(game).to_owned(),
                total_files: self.total_files,
                completed_files: self.total_files,
                downloaded_files: downloaded,
                skipped_files: skipped,
                partial_files: 0,
                bytes_on_disk: self.total_bytes,
                bytes_downloaded: remaining,
                total_bytes: self.total_bytes,
                complete: true,
                reauth_needed: false,
                error_code: String::new(),
                message: "game files ready".to_owned(),
            }
        }
    }

    #[test]
    fn first_boot_prompt_triggers_when_game_data_or_manifest_missing_or_partial() {
        let base = std::env::temp_dir().join("uu_apk_test_first_boot_storage");
        let _ = fs::remove_dir_all(&base);

        for game in AndroidGame::ALL {
            // 1. Empty storage -> PromptDownload with exact prompt text.
            let state = FirstBootState::evaluate_from_storage(game, &base);
            assert_eq!(state.prompt_text, FIRST_BOOT_DOWNLOAD_PROMPT);
            assert_eq!(state.decision, FirstBootDecision::PromptDownload);

            // 2. Data dir exists without manifest -> still PromptDownload.
            let dest = game.game_dir(&base);
            fs::create_dir_all(dest.join(game.data_dir_name())).unwrap();
            assert_eq!(
                FirstBootState::evaluate_from_storage(game, &base).decision,
                FirstBootDecision::PromptDownload
            );

            // 3. Manifest exists, but `.part` file remains -> still PromptDownload.
            fs::write(dest.join(GAME_FILES_MANIFEST_NAME), "{}").unwrap();
            fs::write(
                dest.join(game.data_dir_name()).join("a.psarc.part"),
                b"partial",
            )
            .unwrap();
            assert_eq!(
                FirstBootState::evaluate_from_storage(game, &base).decision,
                FirstBootDecision::PromptDownload
            );

            // 4. Clean complete directory + manifest -> AlreadyInstalled.
            fs::remove_file(dest.join(game.data_dir_name()).join("a.psarc.part")).unwrap();
            assert_eq!(
                FirstBootState::evaluate_from_storage(game, &base).decision,
                FirstBootDecision::AlreadyInstalled { game }
            );
        }

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn resumable_download_recovers_from_part_after_offline_and_reauth_and_skips_completed() {
        let mut backend = SimulatedDropboxBackend {
            total_files: 2,
            total_bytes: 8192,
            bytes_on_disk: 0,
            completed_files: 0,
            partial_files: 0,
            fail_mode: Some("server_unreachable"),
            download_calls: 0,
            status_calls: 0,
        };

        let mut dl = Stage704DownloadState::new(AndroidGame::U1);
        dl.run_resumable_download(&mut backend, "/tmp/u1", "{}");
        assert!(dl.offline_retry);
        assert!(!dl.reauth_needed);
        assert_eq!(dl.partial_files, 1);
        assert_eq!(dl.progress.downloaded_bytes, 4096);
        assert_eq!(dl.progress.percent(), 50);
        assert!(matches!(dl.phase, DownloadPhase::Failed(_)));

        // Simulate reauth_needed on next attempt, then successful resume.
        backend.fail_mode = Some("reauth_needed");
        dl.retry_or_resume(&mut backend, "/tmp/u1", "{}");
        assert!(dl.reauth_needed);
        assert!(!dl.offline_retry);
        assert_eq!(dl.progress.downloaded_bytes, 4096);

        dl.retry_or_resume(&mut backend, "/tmp/u1", "{}");
        assert_eq!(dl.phase, DownloadPhase::Completed);
        assert!(!dl.reauth_needed);
        assert!(!dl.offline_retry);
        assert!(dl.progress.is_complete());
        assert_eq!(dl.progress.percent(), 100);
        let calls_before_skip = backend.download_calls;

        // Re-running when already complete skips network download via status check.
        dl.run_resumable_download(&mut backend, "/tmp/u1", "{}");
        assert_eq!(dl.phase, DownloadPhase::Completed);
        assert_eq!(backend.download_calls, calls_before_skip);
    }
}
