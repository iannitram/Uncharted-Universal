//! Android Save Relay C-ABI/JNI integration, slot-aware manifest & save-dir paths,
//! setup/OAuth browser sign-in flow, and side-by-side conflict UI state machine (`#250`).
//!
//! Mirrors `android/save_relay_bridge.h` and `crates/core/src/saves.rs` without external
//! crate dependencies so `uncharted_android_apk` stays self-contained.

use crate::game::AndroidGame;
use crate::gamepad::{LauncherInput, Nav};
use core::ffi::{c_char, c_int, c_void};
use std::fmt;

/// Minimum save slot index (`1..=10`).
pub const SAVE_SLOT_MIN: u8 = 1;
/// Maximum save slot index (`1..=10`).
pub const SAVE_SLOT_MAX: u8 = 10;

/// Opaque HTTP sink passed by `save-relay` into the host HTTP callback.
#[repr(C)]
pub struct SaveRelayHttpSink {
    _private: [u8; 0],
}

/// C-ABI `SaveRelayHttpRequest` matching `include/save_relay.h` and `android/save_relay_bridge.h`.
#[repr(C)]
pub struct SaveRelayHttpRequest {
    pub method: *const c_char,
    pub url: *const c_char,
    pub headers: *const c_char,
    pub body: *const u8,
    pub body_len: usize,
    pub max_response_bytes: usize,
}

/// C-ABI `SaveRelayHost` matching `include/save_relay.h` and `android/save_relay_bridge.h`.
#[repr(C)]
pub struct SaveRelayHost {
    pub user_data: *mut c_void,
    pub home: *const c_char,
    pub data: *const c_char,
    pub config: *const c_char,
    pub documents: *const c_char,
    pub cred_get: Option<
        unsafe extern "C" fn(
            user_data: *mut c_void,
            account: *const c_char,
            buf: *mut u8,
            cap: usize,
            len: *mut usize,
        ) -> c_int,
    >,
    pub cred_set: Option<
        unsafe extern "C" fn(
            user_data: *mut c_void,
            account: *const c_char,
            secret: *const c_char,
        ) -> c_int,
    >,
    pub cred_delete:
        Option<unsafe extern "C" fn(user_data: *mut c_void, account: *const c_char) -> c_int>,
    pub http: Option<
        unsafe extern "C" fn(
            user_data: *mut c_void,
            request: *const SaveRelayHttpRequest,
            sink: *mut SaveRelayHttpSink,
        ) -> c_int,
    >,
}

/// Injectable C-ABI function table matching `BbSaveRelayCAbi` in `android/save_relay_bridge.h`.
#[repr(C)]
pub struct BbSaveRelayCAbi {
    pub init: Option<unsafe extern "C" fn(host: *const SaveRelayHost) -> *mut c_char>,
    pub pull: Option<
        unsafe extern "C" fn(
            manifest_json: *const c_char,
            saves_dir: *const c_char,
            resolve: *const c_char,
        ) -> *mut c_char,
    >,
    pub push: Option<
        unsafe extern "C" fn(
            manifest_json: *const c_char,
            saves_dir: *const c_char,
            resolve: *const c_char,
        ) -> *mut c_char,
    >,
    pub status: Option<
        unsafe extern "C" fn(manifest_json: *const c_char, saves_dir: *const c_char) -> *mut c_char,
    >,
    pub setup: Option<
        unsafe extern "C" fn(
            server: *const c_char,
            user: *const c_char,
            password: *const c_char,
            device: *const c_char,
            root: *const c_char,
        ) -> *mut c_char,
    >,
    pub sign_in_start: Option<
        unsafe extern "C" fn(
            service: *const c_char,
            redirect_uri: *const c_char,
            device: *const c_char,
            root: *const c_char,
        ) -> *mut c_char,
    >,
    pub sign_in_finish: Option<unsafe extern "C" fn(redirect_url: *const c_char) -> *mut c_char>,
    pub free_str: Option<unsafe extern "C" fn(pointer: *mut c_char)>,
    pub http_respond: Option<
        unsafe extern "C" fn(
            sink: *mut SaveRelayHttpSink,
            status: u16,
            headers: *const c_char,
            body: *const u8,
            body_len: usize,
        ) -> c_int,
    >,
}

/// Mirrors `BbSaveRelaySideCard` in `android/save_relay_bridge.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BbSaveRelaySideCard {
    pub present: c_int,
    pub device: [c_char; 129],
    pub time: [c_char; 64],
    pub revision: u64,
    pub has_revision: c_int,
    pub place: [c_char; 256],
    pub playtime_secs: u64,
    pub has_playtime: c_int,
}

/// Mirrors `BbSaveRelayHeldInfo` in `android/save_relay_bridge.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BbSaveRelayHeldInfo {
    pub present: c_int,
    pub device: [c_char; 129],
    pub since: [c_char; 64],
}

/// Mirrors `BbSaveRelaySyncRequest` in `android/save_relay_bridge.h`.
#[repr(C)]
pub struct BbSaveRelaySyncRequest {
    pub game: u32,
    pub slot: u32,
    pub user_dir: *const c_char,
    pub saves_dir_override: *const c_char,
    pub manifest_path: *const c_char,
    pub manifest_json_override: *const c_char,
    pub resolve: u32,
}

/// Mirrors `BbSaveRelaySyncResult` in `android/save_relay_bridge.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BbSaveRelaySyncResult {
    pub outcome: i32,
    pub action: [c_char; 32],
    pub game_id: [c_char; 80],
    pub revision: u64,
    pub has_revision: c_int,
    pub server_is_newer: c_int,
    pub server: BbSaveRelaySideCard,
    pub local: BbSaveRelaySideCard,
    pub held: BbSaveRelayHeldInfo,
    pub error_code: [c_char; 64],
    pub message: [c_char; 512],
}

/// Mirrors `BbSaveRelaySignInChallenge` in `android/save_relay_bridge.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BbSaveRelaySignInChallenge {
    pub service: [c_char; 32],
    pub verification_url: [c_char; 1024],
    pub user_code: [c_char; 64],
    pub message: [c_char; 512],
}

/// C-ABI entry points exported by `android/save_relay_bridge.cpp`.
pub mod ffi {
    use super::*;

    extern "C" {
        pub fn bb_android_save_relay_init(
            host: *const SaveRelayHost,
            abi: *const BbSaveRelayCAbi,
            err_buf: *mut c_char,
            err_cap: usize,
        ) -> c_int;
        pub fn bb_android_save_relay_pull_before_play(
            req: *const BbSaveRelaySyncRequest,
            abi: *const BbSaveRelayCAbi,
            out_result: *mut BbSaveRelaySyncResult,
        ) -> c_int;
        pub fn bb_android_save_relay_push_after_exit(
            req: *const BbSaveRelaySyncRequest,
            abi: *const BbSaveRelayCAbi,
            out_result: *mut BbSaveRelaySyncResult,
        ) -> c_int;
        pub fn bb_android_save_relay_status(
            req: *const BbSaveRelaySyncRequest,
            abi: *const BbSaveRelayCAbi,
            out_result: *mut BbSaveRelaySyncResult,
        ) -> c_int;
        pub fn bb_android_save_relay_setup_nextcloud(
            server: *const c_char,
            user: *const c_char,
            password: *const c_char,
            device: *const c_char,
            root: *const c_char,
            abi: *const BbSaveRelayCAbi,
            err_buf: *mut c_char,
            err_cap: usize,
        ) -> c_int;
        pub fn bb_android_save_relay_sign_in_start(
            service: *const c_char,
            redirect_uri: *const c_char,
            device: *const c_char,
            root: *const c_char,
            abi: *const BbSaveRelayCAbi,
            out_challenge: *mut BbSaveRelaySignInChallenge,
            err_buf: *mut c_char,
            err_cap: usize,
        ) -> c_int;
        pub fn bb_android_save_relay_sign_in_finish(
            redirect_url: *const c_char,
            abi: *const BbSaveRelayCAbi,
            err_buf: *mut c_char,
            err_cap: usize,
        ) -> c_int;
    }
}

/// Resolution choice for a Save Relay conflict (`BbSaveRelayResolve`).
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveRelayResolve {
    None = 0,
    KeepServer = 1,
    KeepLocal = 2,
}

/// Outcome returned by the Save Relay bridge (`BbSaveRelayOutcome`).
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveRelayOutcome {
    Unavailable = -2,
    Error = -1,
    Done = 0,
    Offline = 1,
    LocalOnly = 2,
    Conflict = 3,
    Held = 4,
}

impl SaveRelayOutcome {
    pub const fn from_raw(code: i32) -> Self {
        match code {
            0 => Self::Done,
            1 => Self::Offline,
            2 => Self::LocalOnly,
            3 => Self::Conflict,
            4 => Self::Held,
            -2 => Self::Unavailable,
            _ => Self::Error,
        }
    }

    /// Returns `true` when the player can proceed into the game (either synced,
    /// offline fallback, or unconfigured local-only fallback).
    pub const fn allows_local_play(self) -> bool {
        matches!(self, Self::Done | Self::Offline | Self::LocalOnly)
    }
}

/// Sync phase for pull-before-play vs push-after-exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncPhase {
    PullBeforePlay,
    PushAfterExit,
    Status,
}

/// Supported Save Relay cloud providers on Android.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupProvider {
    Nextcloud,
    Gdrive,
    Dropbox,
}

impl SetupProvider {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Nextcloud => "nextcloud",
            Self::Gdrive => "gdrive",
            Self::Dropbox => "dropbox",
        }
    }

    pub const fn uses_browser_oauth(self) -> bool {
        matches!(self, Self::Gdrive | Self::Dropbox)
    }
}

/// Errors produced by Android Save Relay validation or bridge calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveRelayError {
    InvalidSlot(u8),
    InvalidPath(String),
    InvalidCredentials(String),
    UnsupportedOAuthProvider(SetupProvider),
    BridgeFailed { code: String, message: String },
}

impl fmt::Display for SaveRelayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSlot(slot) => write!(
                f,
                "Save Relay slot {slot} is invalid (must be {SAVE_SLOT_MIN}..={SAVE_SLOT_MAX})"
            ),
            Self::InvalidPath(msg) => write!(f, "invalid Save Relay path: {msg}"),
            Self::InvalidCredentials(msg) => write!(f, "invalid Save Relay credentials: {msg}"),
            Self::UnsupportedOAuthProvider(p) => {
                write!(f, "{} does not use browser OAuth sign-in", p.key())
            }
            Self::BridgeFailed { code, message } => write!(f, "{code}: {message}"),
        }
    }
}

/// Canonical Save Relay game id (`"uncharted-1"`, `"uncharted-2"`, `"uncharted-3"`).
pub const fn game_id(game: AndroidGame) -> &'static str {
    match game {
        AndroidGame::U1 => "uncharted-1",
        AndroidGame::U2 => "uncharted-2",
        AndroidGame::U3 => "uncharted-3",
    }
}

/// Numeric game id matching `BbAndroidSaveGame` (`1`, `2`, `3`).
pub const fn game_code(game: AndroidGame) -> u32 {
    match game {
        AndroidGame::U1 => 1,
        AndroidGame::U2 => 2,
        AndroidGame::U3 => 3,
    }
}

fn validate_slot(slot: u8) -> Result<u8, SaveRelayError> {
    if !(SAVE_SLOT_MIN..=SAVE_SLOT_MAX).contains(&slot) {
        return Err(SaveRelayError::InvalidSlot(slot));
    }
    Ok(slot)
}

fn validate_clean_dir(dir: &str) -> Result<&str, SaveRelayError> {
    let trimmed = dir.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() || dir.contains('\0') {
        return Err(SaveRelayError::InvalidPath("directory is empty".to_owned()));
    }
    if dir.split(['/', '\\']).any(|seg| seg == "..") {
        return Err(SaveRelayError::InvalidPath(
            "path traversal ('..') is forbidden".to_owned(),
        ));
    }
    Ok(trimmed)
}

/// Computes the slot-aware saves directory matching `android/save_relay_bridge.h`
/// and `crates/core/src/saves.rs`:
/// - slot 1: `"<user_dir>/Uncharted N"`
/// - slot 2..=10: `"<user_dir>/Save slots/Uncharted N/Slot <slot>"`
pub fn slot_saves_dir(
    user_dir: &str,
    game: AndroidGame,
    slot: u8,
) -> Result<String, SaveRelayError> {
    let base = validate_clean_dir(user_dir)?;
    let slot = validate_slot(slot)?;
    let folder = game.app_label();
    if slot == 1 {
        Ok(format!("{base}/{folder}"))
    } else {
        Ok(format!("{base}/Save slots/{folder}/Slot {slot}"))
    }
}

/// Computes the slot-aware manifest file path under `manifests_dir`:
/// - slot 0: `"<manifests_dir>/uncharted-<n>.json"` (base manifest)
/// - slot 1..=10: `"<manifests_dir>/uncharted-<n>-slot-<slot>.json"`
pub fn manifest_path(
    manifests_dir: &str,
    game: AndroidGame,
    slot: u8,
) -> Result<String, SaveRelayError> {
    let base = validate_clean_dir(manifests_dir)?;
    let id = game_id(game);
    if slot == 0 {
        Ok(format!("{base}/{id}.json"))
    } else {
        let slot = validate_slot(slot)?;
        Ok(format!("{base}/{id}-slot-{slot}.json"))
    }
}

/// Generates the format-1 save-relay manifest JSON for `game` and `slot` (`1..=10`).
pub fn manifest_json(game: AndroidGame, slot: u8) -> Result<String, SaveRelayError> {
    let slot = validate_slot(slot)?;
    let (id, title, prefix) = match game {
        AndroidGame::U1 => ("uncharted-1", "Uncharted: Drake's Fortune", "SAVEDATA01"),
        AndroidGame::U2 => ("uncharted-2", "Uncharted 2: Among Thieves", "SAVEDATA02"),
        AndroidGame::U3 => (
            "uncharted-3",
            "Uncharted 3: Drake's Deception",
            "SAVEDATA03",
        ),
    };
    Ok(format!(
        r#"{{"format":1,"id":"{id}-slot-{slot}","name":"{title}","saves":{{}},"saves_from":"launcher","include":["{prefix}*/**"],"exclude":["**/*.tmp","**/.save-relay-*"]}}"#
    ))
}

/// One side of a save-relay conflict card (`BbSaveRelaySideCard`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideCard {
    pub device: String,
    pub time: String,
    pub revision: Option<u64>,
    pub place: Option<String>,
    pub playtime_secs: Option<u64>,
}

/// Open-elsewhere lock info (`BbSaveRelayHeldInfo`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldInfo {
    pub device: String,
    pub since: String,
}

/// Result of a Save Relay sync operation (`pull`, `push`, or `status`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncResult {
    pub outcome: SaveRelayOutcome,
    pub action: String,
    pub game_id: String,
    pub revision: Option<u64>,
    pub server_is_newer: bool,
    pub server: Option<SideCard>,
    pub local: Option<SideCard>,
    pub held: Option<HeldInfo>,
    pub error_code: String,
    pub message: String,
}

/// Focused action in the side-by-side conflict UI modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictChoice {
    KeepServer,
    KeepLocal,
    Cancel,
}

impl ConflictChoice {
    pub const fn to_resolve(self) -> Option<SaveRelayResolve> {
        match self {
            Self::KeepServer => Some(SaveRelayResolve::KeepServer),
            Self::KeepLocal => Some(SaveRelayResolve::KeepLocal),
            Self::Cancel => None,
        }
    }
}

/// Side-by-side conflict UI state shown when both server and local saves changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictPrompt {
    pub phase: SyncPhase,
    pub game: AndroidGame,
    pub slot: u8,
    pub server_is_newer: bool,
    pub server: SideCard,
    pub local: SideCard,
    pub message: String,
    pub selected: ConflictChoice,
}

impl ConflictPrompt {
    pub fn from_sync_result(
        phase: SyncPhase,
        game: AndroidGame,
        slot: u8,
        res: &SyncResult,
    ) -> Option<Self> {
        if res.outcome != SaveRelayOutcome::Conflict {
            return None;
        }
        let server = res.server.clone()?;
        let local = res.local.clone()?;
        // Default selection highlights whichever side is newer.
        let selected = if res.server_is_newer {
            ConflictChoice::KeepServer
        } else {
            ConflictChoice::KeepLocal
        };
        Some(Self {
            phase,
            game,
            slot,
            server_is_newer: res.server_is_newer,
            server,
            local,
            message: res.message.clone(),
            selected,
        })
    }

    /// Applies a launcher gamepad/keyboard input to the side-by-side conflict modal.
    /// Returns `Some(choice)` when the player confirms or cancels.
    pub fn handle_input(&mut self, input: LauncherInput) -> Option<ConflictChoice> {
        match input {
            LauncherInput::Move(Nav::Left) => {
                self.selected = ConflictChoice::KeepServer;
                None
            }
            LauncherInput::Move(Nav::Right) => {
                self.selected = ConflictChoice::KeepLocal;
                None
            }
            LauncherInput::Move(Nav::Down) => {
                self.selected = ConflictChoice::Cancel;
                None
            }
            LauncherInput::Move(Nav::Up) => {
                if self.selected == ConflictChoice::Cancel {
                    self.selected = if self.server_is_newer {
                        ConflictChoice::KeepServer
                    } else {
                        ConflictChoice::KeepLocal
                    };
                }
                None
            }
            LauncherInput::Activate => Some(self.selected),
            LauncherInput::Back => {
                self.selected = ConflictChoice::Cancel;
                Some(ConflictChoice::Cancel)
            }
            LauncherInput::Tab { .. }
            | LauncherInput::PrevGame
            | LauncherInput::NextGame
            | LauncherInput::OtherSaves => None,
        }
    }
}

/// High-level launcher decision produced by pull-before-play or push-after-exit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncDecision {
    /// Sync succeeded, or fell back cleanly to offline / local-only mode.
    Proceed {
        outcome: SaveRelayOutcome,
        message: String,
    },
    /// Both sides changed; show side-by-side conflict UI before continuing.
    ShowConflict(ConflictPrompt),
    /// Another device currently holds the open-elsewhere lock.
    ShowHeld { held: HeldInfo, message: String },
    /// Fatal/auth error that requires user attention or setup check.
    SyncError { code: String, message: String },
}

/// OAuth browser sign-in challenge returned by `save_relay_sign_in_start`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInChallenge {
    pub service: SetupProvider,
    pub verification_url: String,
    pub user_code: Option<String>,
    pub message: String,
}

/// Setup / OAuth sign-in UI state machine for the Android launcher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupFlowState {
    Unconfigured,
    OAuthAwaitingBrowser {
        challenge: SignInChallenge,
        redirect_uri: String,
    },
    Configured {
        provider: SetupProvider,
        device: String,
    },
    Failed {
        message: String,
    },
}

/// Backend abstraction so both the C/JNI bridge and host unit tests can drive [`SaveRelayFlow`].
pub trait SaveRelayBackend {
    fn sync(
        &mut self,
        phase: SyncPhase,
        game: AndroidGame,
        slot: u8,
        user_dir: &str,
        resolve: SaveRelayResolve,
    ) -> SyncResult;

    fn setup_nextcloud(
        &mut self,
        server: &str,
        user: &str,
        password: &str,
        device: Option<&str>,
        root: Option<&str>,
    ) -> Result<(), SaveRelayError>;

    fn sign_in_start(
        &mut self,
        service: SetupProvider,
        redirect_uri: &str,
        device: Option<&str>,
        root: Option<&str>,
    ) -> Result<SignInChallenge, SaveRelayError>;

    fn sign_in_finish(&mut self, redirect_url: &str) -> Result<(), SaveRelayError>;
}

/// Coordinates Android launcher pull-before-play, push-after-exit, offline/local-only
/// fallback, setup/OAuth sign-in, and side-by-side conflict resolution.
pub struct SaveRelayFlow<B: SaveRelayBackend> {
    backend: B,
    user_dir: String,
    device_name: String,
    pub setup: SetupFlowState,
}

impl<B: SaveRelayBackend> SaveRelayFlow<B> {
    pub fn new(
        backend: B,
        user_dir: impl Into<String>,
        device_name: impl Into<String>,
        setup: SetupFlowState,
    ) -> Self {
        Self {
            backend,
            user_dir: user_dir.into(),
            device_name: device_name.into(),
            setup,
        }
    }

    pub fn pull_before_play(
        &mut self,
        game: AndroidGame,
        slot: u8,
        resolve: SaveRelayResolve,
    ) -> SyncDecision {
        self.run_sync(SyncPhase::PullBeforePlay, game, slot, resolve)
    }

    pub fn push_after_exit(
        &mut self,
        game: AndroidGame,
        slot: u8,
        resolve: SaveRelayResolve,
    ) -> SyncDecision {
        self.run_sync(SyncPhase::PushAfterExit, game, slot, resolve)
    }

    pub fn status(&mut self, game: AndroidGame, slot: u8) -> SyncDecision {
        self.run_sync(SyncPhase::Status, game, slot, SaveRelayResolve::None)
    }

    fn run_sync(
        &mut self,
        phase: SyncPhase,
        game: AndroidGame,
        slot: u8,
        resolve: SaveRelayResolve,
    ) -> SyncDecision {
        if let Err(err) = validate_slot(slot) {
            return SyncDecision::SyncError {
                code: "invalid_slot".to_owned(),
                message: err.to_string(),
            };
        }
        let res = self
            .backend
            .sync(phase, game, slot, &self.user_dir, resolve);
        if res.outcome.allows_local_play() {
            return SyncDecision::Proceed {
                outcome: res.outcome,
                message: res.message,
            };
        }
        if res.outcome == SaveRelayOutcome::Conflict {
            if let Some(prompt) = ConflictPrompt::from_sync_result(phase, game, slot, &res) {
                return SyncDecision::ShowConflict(prompt);
            }
            return SyncDecision::SyncError {
                code: "invalid_conflict".to_owned(),
                message: "Save Relay returned an incomplete conflict payload.".to_owned(),
            };
        }
        if res.outcome == SaveRelayOutcome::Held {
            if let Some(held) = res.held {
                return SyncDecision::ShowHeld {
                    held,
                    message: res.message,
                };
            }
        }
        SyncDecision::SyncError {
            code: res.error_code,
            message: res.message,
        }
    }

    pub fn configure_nextcloud(
        &mut self,
        server: &str,
        user: &str,
        password: &str,
        root: Option<&str>,
    ) -> Result<(), SaveRelayError> {
        if server.trim().is_empty() || user.trim().is_empty() || password.is_empty() {
            return Err(SaveRelayError::InvalidCredentials(
                "server, user, and password are required".to_owned(),
            ));
        }
        if password.contains(['\n', '\r']) {
            return Err(SaveRelayError::InvalidCredentials(
                "password must be a single line".to_owned(),
            ));
        }
        let res = self.backend.setup_nextcloud(
            server.trim(),
            user.trim(),
            password,
            Some(&self.device_name),
            root,
        );
        match res {
            Ok(()) => {
                self.setup = SetupFlowState::Configured {
                    provider: SetupProvider::Nextcloud,
                    device: self.device_name.clone(),
                };
                Ok(())
            }
            Err(err) => {
                self.setup = SetupFlowState::Failed {
                    message: err.to_string(),
                };
                Err(err)
            }
        }
    }

    pub fn start_oauth_sign_in(
        &mut self,
        service: SetupProvider,
        redirect_uri: &str,
        root: Option<&str>,
    ) -> Result<SignInChallenge, SaveRelayError> {
        if !service.uses_browser_oauth() {
            return Err(SaveRelayError::UnsupportedOAuthProvider(service));
        }
        if redirect_uri.trim().is_empty() {
            return Err(SaveRelayError::InvalidCredentials(
                "redirect URI is required".to_owned(),
            ));
        }
        let challenge = self.backend.sign_in_start(
            service,
            redirect_uri.trim(),
            Some(&self.device_name),
            root,
        )?;
        self.setup = SetupFlowState::OAuthAwaitingBrowser {
            challenge: challenge.clone(),
            redirect_uri: redirect_uri.trim().to_owned(),
        };
        Ok(challenge)
    }

    pub fn finish_oauth_sign_in(&mut self, redirect_url: &str) -> Result<(), SaveRelayError> {
        let pending_service = match &self.setup {
            SetupFlowState::OAuthAwaitingBrowser { challenge, .. } => challenge.service,
            _ => {
                return Err(SaveRelayError::InvalidCredentials(
                    "no OAuth browser sign-in is pending".to_owned(),
                ))
            }
        };
        if redirect_url.trim().is_empty() {
            return Err(SaveRelayError::InvalidCredentials(
                "redirect URL is required".to_owned(),
            ));
        }
        match self.backend.sign_in_finish(redirect_url.trim()) {
            Ok(()) => {
                self.setup = SetupFlowState::Configured {
                    provider: pending_service,
                    device: self.device_name.clone(),
                };
                Ok(())
            }
            Err(err) => {
                self.setup = SetupFlowState::Failed {
                    message: err.to_string(),
                };
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockBackend {
        next_sync: Option<SyncResult>,
        last_phase: Option<SyncPhase>,
        last_resolve: Option<SaveRelayResolve>,
        fail_setup: bool,
        fail_oauth_finish: bool,
    }

    impl MockBackend {
        fn new() -> Self {
            Self {
                next_sync: None,
                last_phase: None,
                last_resolve: None,
                fail_setup: false,
                fail_oauth_finish: false,
            }
        }
    }

    impl SaveRelayBackend for MockBackend {
        fn sync(
            &mut self,
            phase: SyncPhase,
            game: AndroidGame,
            slot: u8,
            _user_dir: &str,
            resolve: SaveRelayResolve,
        ) -> SyncResult {
            self.last_phase = Some(phase);
            self.last_resolve = Some(resolve);
            self.next_sync.clone().unwrap_or(SyncResult {
                outcome: SaveRelayOutcome::Done,
                action: "noop".to_owned(),
                game_id: format!("{}-slot-{slot}", game_id(game)),
                revision: Some(1),
                server_is_newer: false,
                server: None,
                local: None,
                held: None,
                error_code: String::new(),
                message: "already in sync".to_owned(),
            })
        }

        fn setup_nextcloud(
            &mut self,
            _server: &str,
            _user: &str,
            _password: &str,
            _device: Option<&str>,
            _root: Option<&str>,
        ) -> Result<(), SaveRelayError> {
            if self.fail_setup {
                Err(SaveRelayError::BridgeFailed {
                    code: "auth_failed".to_owned(),
                    message: "bad credentials".to_owned(),
                })
            } else {
                Ok(())
            }
        }

        fn sign_in_start(
            &mut self,
            service: SetupProvider,
            redirect_uri: &str,
            _device: Option<&str>,
            _root: Option<&str>,
        ) -> Result<SignInChallenge, SaveRelayError> {
            Ok(SignInChallenge {
                service,
                verification_url: format!(
                    "https://accounts.example/auth?redirect_uri={redirect_uri}&state=xyz"
                ),
                user_code: None,
                message: "Open browser".to_owned(),
            })
        }

        fn sign_in_finish(&mut self, _redirect_url: &str) -> Result<(), SaveRelayError> {
            if self.fail_oauth_finish {
                Err(SaveRelayError::BridgeFailed {
                    code: "sign_in_failed".to_owned(),
                    message: "state mismatch".to_owned(),
                })
            } else {
                Ok(())
            }
        }
    }

    fn sample_side(device: &str, time: &str, rev: u64, place: &str) -> SideCard {
        SideCard {
            device: device.to_owned(),
            time: time.to_owned(),
            revision: Some(rev),
            place: Some(place.to_owned()),
            playtime_secs: Some(3600),
        }
    }

    #[test]
    fn slot_paths_and_manifests_match_desktop_contract() {
        assert_eq!(
            slot_saves_dir("/data/user/0/org.uncharted/files/user/", AndroidGame::U1, 1).unwrap(),
            "/data/user/0/org.uncharted/files/user/Uncharted 1"
        );
        assert_eq!(
            slot_saves_dir("/data/user/0/org.uncharted/files/user", AndroidGame::U3, 7).unwrap(),
            "/data/user/0/org.uncharted/files/user/Save slots/Uncharted 3/Slot 7"
        );
        assert!(slot_saves_dir("/user", AndroidGame::U1, 0).is_err());
        assert!(slot_saves_dir("/user", AndroidGame::U1, 11).is_err());
        assert!(slot_saves_dir("/user/../escape", AndroidGame::U1, 1).is_err());

        assert_eq!(
            manifest_path("/app/manifests/", AndroidGame::U2, 0).unwrap(),
            "/app/manifests/uncharted-2.json"
        );
        assert_eq!(
            manifest_path("/app/manifests", AndroidGame::U2, 4).unwrap(),
            "/app/manifests/uncharted-2-slot-4.json"
        );

        let json = manifest_json(AndroidGame::U2, 3).unwrap();
        assert!(json.contains(r#""id":"uncharted-2-slot-3""#));
        assert!(json.contains(r#""saves_from":"launcher""#));
        assert!(json.contains(r#""SAVEDATA02*/**""#));
    }

    #[test]
    fn pull_before_play_offline_and_local_only_allow_play() {
        let mut backend = MockBackend::new();
        backend.next_sync = Some(SyncResult {
            outcome: SaveRelayOutcome::Offline,
            action: "offline".to_owned(),
            game_id: "uncharted-1-slot-1".to_owned(),
            revision: None,
            server_is_newer: false,
            server: None,
            local: None,
            held: None,
            error_code: "server_unreachable".to_owned(),
            message: "Offline; using local saves.".to_owned(),
        });
        let mut flow = SaveRelayFlow::new(
            backend,
            "/data/user/0/org.uncharted/files/user",
            "pixel-fold",
            SetupFlowState::Unconfigured,
        );
        match flow.pull_before_play(AndroidGame::U1, 1, SaveRelayResolve::None) {
            SyncDecision::Proceed { outcome, .. } => assert_eq!(outcome, SaveRelayOutcome::Offline),
            other => panic!("expected Proceed, got {other:?}"),
        }

        flow.backend.next_sync.as_mut().unwrap().outcome = SaveRelayOutcome::LocalOnly;
        match flow.pull_before_play(AndroidGame::U1, 1, SaveRelayResolve::None) {
            SyncDecision::Proceed { outcome, .. } => {
                assert_eq!(outcome, SaveRelayOutcome::LocalOnly)
            }
            other => panic!("expected Proceed, got {other:?}"),
        }
    }

    #[test]
    fn conflict_ui_navigation_and_resolution_flow() {
        let mut backend = MockBackend::new();
        backend.next_sync = Some(SyncResult {
            outcome: SaveRelayOutcome::Conflict,
            action: "conflict".to_owned(),
            game_id: "uncharted-1-slot-2".to_owned(),
            revision: None,
            server_is_newer: true,
            server: Some(sample_side(
                "strix-halo",
                "2026-11-22T21:30:00Z",
                9,
                "Chapter 12 - Heading Upriver",
            )),
            local: Some(sample_side(
                "pixel-fold",
                "2026-11-22T20:15:00Z",
                8,
                "Chapter 10 - The Customs House",
            )),
            held: None,
            error_code: String::new(),
            message: "Both sides changed.".to_owned(),
        });

        let mut flow = SaveRelayFlow::new(
            backend,
            "/data/user/0/org.uncharted/files/user",
            "pixel-fold",
            SetupFlowState::Configured {
                provider: SetupProvider::Nextcloud,
                device: "pixel-fold".to_owned(),
            },
        );

        let mut prompt = match flow.pull_before_play(AndroidGame::U1, 2, SaveRelayResolve::None) {
            SyncDecision::ShowConflict(p) => p,
            other => panic!("expected ShowConflict, got {other:?}"),
        };
        assert_eq!(prompt.selected, ConflictChoice::KeepServer);
        assert_eq!(prompt.handle_input(LauncherInput::Move(Nav::Right)), None);
        assert_eq!(prompt.selected, ConflictChoice::KeepLocal);
        assert_eq!(
            prompt.handle_input(LauncherInput::Activate),
            Some(ConflictChoice::KeepLocal)
        );

        // Re-run pull with chosen KeepLocal resolution.
        flow.backend.next_sync = Some(SyncResult {
            outcome: SaveRelayOutcome::Done,
            action: "pulled".to_owned(),
            game_id: "uncharted-1-slot-2".to_owned(),
            revision: Some(8),
            server_is_newer: false,
            server: None,
            local: None,
            held: None,
            error_code: String::new(),
            message: "Kept local saves.".to_owned(),
        });
        let resolve = prompt.selected.to_resolve().unwrap();
        match flow.pull_before_play(AndroidGame::U1, 2, resolve) {
            SyncDecision::Proceed { outcome, .. } => assert_eq!(outcome, SaveRelayOutcome::Done),
            other => panic!("expected Proceed, got {other:?}"),
        }
        assert_eq!(flow.backend.last_resolve, Some(SaveRelayResolve::KeepLocal));
    }

    #[test]
    fn setup_nextcloud_and_oauth_pkce_flow() {
        let mut flow = SaveRelayFlow::new(
            MockBackend::new(),
            "/data/user/0/org.uncharted/files/user",
            "pixel-fold",
            SetupFlowState::Unconfigured,
        );

        assert!(flow
            .configure_nextcloud("https://cloud.example", "nate", "bad\npass", None)
            .is_err());
        flow.configure_nextcloud("https://cloud.example", "nate", "app-pass", Some("saves"))
            .unwrap();
        assert_eq!(
            flow.setup,
            SetupFlowState::Configured {
                provider: SetupProvider::Nextcloud,
                device: "pixel-fold".to_owned(),
            }
        );

        // Nextcloud cannot use browser OAuth.
        assert!(matches!(
            flow.start_oauth_sign_in(SetupProvider::Nextcloud, "uncharted://oauth", None),
            Err(SaveRelayError::UnsupportedOAuthProvider(
                SetupProvider::Nextcloud
            ))
        ));

        // Google Drive PKCE start -> finish.
        let challenge = flow
            .start_oauth_sign_in(SetupProvider::Gdrive, "uncharted://oauth", None)
            .unwrap();
        assert_eq!(challenge.service, SetupProvider::Gdrive);
        assert!(challenge
            .verification_url
            .contains("redirect_uri=uncharted://oauth"));

        flow.finish_oauth_sign_in("uncharted://oauth?code=abc&state=xyz")
            .unwrap();
        assert_eq!(
            flow.setup,
            SetupFlowState::Configured {
                provider: SetupProvider::Gdrive,
                device: "pixel-fold".to_owned(),
            }
        );
    }
}
