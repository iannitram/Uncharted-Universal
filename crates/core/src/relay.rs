//! Desktop adapter for the published Save Relay CLI. The helper owns all server
//! access, credentials, revision comparisons and deferred uploads.
use crate::games::Game;
use crate::launch::{Keep, Relay, Sync};
use crate::savecard::{side_by_side, SaveCard, SideBySide};
use crate::{Error, Result};
use chrono::DateTime;
use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct DesktopRelay {
    pub program: PathBuf,
    pub manifests: PathBuf,
}

fn game_id(game: Game) -> &'static str {
    match game {
        Game::U1 => "uncharted-1",
        Game::U2 => "uncharted-2",
        Game::U3 => "uncharted-3",
    }
}

// The argv of every Save Relay call. `relay/contract.rs` runs these same lists against the
// pinned release's own parser (issue #637: the fakes accepted forms the pinned rc.2 refused).

/// `pull|push <game> --manifest <file> --saves-dir <folder> --slot <n> --json [--resolve ..]`.
fn sync_args(
    phase: &str,
    game: Game,
    slot: u8,
    manifests: &Path,
    folder: &Path,
    keep: Option<Keep>,
) -> Vec<OsString> {
    let id = game_id(game);
    let mut args: Vec<OsString> = vec![phase.into(), id.into(), "--manifest".into()];
    args.push(manifests.join(format!("{id}.json")).into());
    args.push("--saves-dir".into());
    args.push(folder.into());
    args.push("--slot".into());
    args.push(slot.to_string().into());
    args.push("--json".into());
    if let Some(keep) = keep {
        args.push("--resolve".into());
        args.push(
            match keep {
                Keep::Server => "keep-server",
                Keep::Local => "keep-local",
            }
            .into(),
        );
    }
    args
}

/// The account state, with no game (save-relay `v1.0.0-rc.3` and later).
const STATUS_ARGS: [&str; 2] = ["status", "--json"];

/// Forget the account and its stored credentials.
const SIGN_OUT_ARGS: [&str; 2] = ["sign-out", "--json"];

/// Nextcloud sign-in; the app password goes on stdin, never in argv.
fn nextcloud_args<'a>(url: &'a str, account: &'a str) -> [&'a str; 8] {
    [
        "setup",
        "nextcloud",
        "--url",
        url,
        "--user",
        account,
        "--password-stdin",
        "--json",
    ]
}

/// Google Drive or Dropbox sign-in: with `--json` Save Relay answers with a code to enter on
/// any device instead of opening a browser.
fn oauth_args(service: CloudService) -> [&'static str; 3] {
    ["setup", service.id(), "--json"]
}

impl DesktopRelay {
    fn invoke(
        &self,
        phase: &str,
        game: Game,
        slot: u8,
        folder: &Path,
        keep: Option<Keep>,
    ) -> Result<Sync> {
        if !(1..=crate::saves::MOST).contains(&slot) {
            return Err(Error::Saves("Save Relay slot must be from 1 to 10.".into()));
        }
        let mut cmd = Command::new(&self.program);
        cmd.args(sync_args(phase, game, slot, &self.manifests, folder, keep))
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        let output = cmd
            .output()
            .map_err(|e| Error::Launch(format!("Could not start the bundled Save Relay: {e}")))?;
        decode(phase, game, slot, output.status.code(), &output.stdout)
    }
}

impl Relay for DesktopRelay {
    fn pull(&self, game: Game, slot: u8, folder: &Path, keep: Option<Keep>) -> Result<Sync> {
        self.invoke("pull", game, slot, folder, keep)
    }
    fn push(&self, game: Game, slot: u8, folder: &Path, keep: Option<Keep>) -> Result<Sync> {
        self.invoke("push", game, slot, folder, keep)
    }
}

fn invalid() -> Error {
    Error::Launch(
        "Save Relay returned an unexpected response. Both saves need checking before play.".into(),
    )
}

fn is_missing_vcruntime(code: Option<i32>) -> bool {
    // The Windows loader can terminate the image before main runs. The release
    // binary imports VCRUNTIME140.dll from the Microsoft x64 C++ runtime.
    code == Some(0xc0000135_u32 as i32)
}

const MISSING_VCRUNTIME: &str = "Save Relay could not load a required DLL. Install the Microsoft Visual C++ x64 Redistributable and retry.";

fn missing_vcruntime_error() -> Error {
    Error::Launch(MISSING_VCRUNTIME.into())
}

/// Save Relay's `usage` code: it did not understand the launcher's command line, so the
/// helper beside the launcher is older than the forms this launcher sends (issue #637).
const TOO_OLD: &str = "Save Relay is too old for this launcher. Reinstall Uncharted Universal to get the Save Relay it ships with.";

/// Account setup, status and sign-out are not a game start: their errors are `Saves: ..`,
/// never "Starting the game failed: ..".
fn account_error(message: impl Into<String>) -> Error {
    Error::Saves(message.into())
}

fn account_invalid() -> Error {
    account_error("Save Relay returned an unexpected response.")
}

fn credential_store_error_message(error_code: Option<&str>) -> Option<&'static str> {
    match error_code {
        Some(
            "credential_store_unavailable"
            | "credential_store_locked"
            | "credential_store_error"
            | "credential_store_failed"
            | "keyring_unavailable"
            | "keyring_locked"
            | "keychain_locked"
            | "secret_service_unavailable",
        ) => Some(
            "Save Relay could not access the system credential store. Unlock it and try again.",
        ),
        _ => None,
    }
}

fn sanitize_text(s: &str, max_chars: usize) -> Option<String> {
    let cleaned: String = s
        .chars()
        .filter(|c| !c.is_control())
        .take(max_chars)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn sanitize_multiline(s: &str, max_chars: usize) -> Option<String> {
    let cleaned: String = s
        .chars()
        .filter(|&c| c == '\n' || !c.is_control())
        .take(max_chars)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn decode(phase: &str, game: Game, slot: u8, code: Option<i32>, stdout: &[u8]) -> Result<Sync> {
    if is_missing_vcruntime(code) {
        return Err(missing_vcruntime_error());
    }
    let report: Value = serde_json::from_slice(stdout).map_err(|_| invalid())?;
    if report["command"] != phase {
        return Err(invalid());
    }
    match report["ok"].as_bool() {
        Some(false) => {
            let error_code = report["error"]["code"].as_str();
            if let Some(msg) = credential_store_error_message(error_code) {
                return Err(Error::Launch(msg.into()));
            }
            // Provider messages can contain account paths. Only display known local text.
            let message = match (code, error_code) {
                (_, Some("usage")) => TOO_OLD,
                (Some(4), Some("not_set_up")) => {
                    return Ok(Sync::LocalOnly(
                        "Save Relay needs setup; using local saves.",
                    ))
                }
                (
                    Some(4),
                    Some("auth_failed" | "reauth_needed" | "credentials" | "sign_in_failed"),
                ) => "Save Relay could not sign in. Check its account setup.",
                (Some(5), Some("server_unreachable")) => return Ok(Sync::Offline),
                _ => "Save Relay could not complete the sync. Check its setup and try again.",
            };
            return Err(Error::Launch(message.into()));
        }
        Some(true) => (),
        None => return Err(invalid()),
    }
    let data = &report["data"];
    if data["game"] != format!("{}-slot-{slot}", game_id(game)) {
        return Err(invalid());
    }
    match (code, data["action"].as_str()) {
        (Some(0), Some("noop")) => Ok(Sync::Done("already in sync".into())),
        (Some(0), Some("adopted")) => Ok(Sync::Done("matching revision recorded".into())),
        (Some(0), Some("pulled")) => Ok(Sync::Done("downloaded saves".into())),
        (Some(0), Some("pushed")) => Ok(Sync::Done("uploaded saves".into())),
        (Some(0), Some("offline")) if phase == "pull" => Ok(Sync::Offline),
        (Some(0), Some("deferred")) if phase == "push" => Ok(Sync::Offline),
        (Some(3), Some("conflict")) => Ok(Sync::Conflict(Box::new(side_by_side(
            card(game, &data["server"], &data["server_card"])?,
            card(game, &data["local"], &data["local_card"])?,
        )))),
        (Some(6), Some("held")) if phase == "pull" => Err(Error::Launch(
            "Another device has this game open. Quit it there and retry.".into(),
        )),
        _ => Err(invalid()),
    }
}

fn card(game: Game, side: &Value, card: &Value) -> Result<SaveCard> {
    let device = side["device"].as_str().ok_or_else(invalid)?;
    let saved_unix = DateTime::parse_from_rfc3339(side["time"].as_str().ok_or_else(invalid)?)
        .map_err(|_| invalid())?
        .timestamp()
        .max(0) as u64;
    Ok(SaveCard {
        game,
        slot: side["revision"]
            .as_u64()
            .map(|rev| format!("Revision {rev}"))
            .unwrap_or_default(),
        device: sanitize_text(device, 128),
        detail: card["place"].as_str().and_then(|s| sanitize_text(s, 1024)),
        play_time_secs: card["playtime_secs"].as_u64(),
        saved_unix,
        picture: None,
        title: None,
        build_id: None,
    })
}

/// Encodes a save conflict as a single-line JSON event for the launcher UI or `--relay-prompt stdio`.
pub fn conflict_event(phase: &str, pair: &SideBySide) -> Value {
    let card = |c: &SaveCard| {
        serde_json::json!({
            "device": c.device,
            "saved_unix": c.saved_unix,
            "detail": c.detail,
            "play_time_secs": c.play_time_secs,
            "slot": c.slot,
        })
    };
    serde_json::json!({
        "version": 1,
        "event": "save-conflict",
        "phase": phase,
        "game": pair.local.game.key(),
        "server": card(&pair.server),
        "local": card(&pair.local),
    })
}

/// Parses a `conflict_event` JSON line back into `(Game, SideBySide)`.
pub fn parse_conflict_event(line: &str) -> Option<(Game, SideBySide)> {
    parse_conflict_event_with_phase(line).map(|(game, _, pair)| (game, pair))
}

/// Parses a `conflict_event` JSON line back into `(Game, phase, SideBySide)`.
pub fn parse_conflict_event_with_phase(line: &str) -> Option<(Game, String, SideBySide)> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    if value["version"] != 1 || value["event"] != "save-conflict" {
        return None;
    }
    let game = Game::from_key(value["game"].as_str()?)?;
    let phase = match value["phase"].as_str() {
        Some(p @ ("pull" | "push")) => p.to_owned(),
        None => "pull".to_owned(),
        Some(_) => return None,
    };
    let parse_card = |side: &Value| -> Option<SaveCard> {
        if !side.is_object() {
            return None;
        }
        Some(SaveCard {
            game,
            slot: side["slot"]
                .as_str()
                .and_then(|s| sanitize_text(s, 128))
                .unwrap_or_default(),
            device: side["device"].as_str().and_then(|s| sanitize_text(s, 128)),
            detail: side["detail"].as_str().and_then(|s| sanitize_text(s, 1024)),
            play_time_secs: side["play_time_secs"].as_u64(),
            saved_unix: side["saved_unix"].as_u64()?,
            picture: None,
            title: None,
            build_id: None,
        })
    };
    let server = parse_card(&value["server"])?;
    let local = parse_card(&value["local"])?;
    Some((game, phase, side_by_side(server, local)))
}

/// Cloud storage provider supported by Save Relay (roadmap 9.05).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudService {
    Nextcloud,
    GoogleDrive,
    Dropbox,
}

impl CloudService {
    pub fn id(self) -> &'static str {
        match self {
            CloudService::Nextcloud => "nextcloud",
            CloudService::GoogleDrive => "gdrive",
            CloudService::Dropbox => "dropbox",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CloudService::Nextcloud => "Nextcloud",
            CloudService::GoogleDrive => "Google Drive",
            CloudService::Dropbox => "Dropbox",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "nextcloud" => Some(CloudService::Nextcloud),
            "gdrive" | "google-drive" => Some(CloudService::GoogleDrive),
            "dropbox" => Some(CloudService::Dropbox),
            _ => None,
        }
    }
}

/// Current Save Relay account state without any secret material.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RelayAccountStatus {
    pub service: Option<CloudService>,
    pub account: Option<String>,
    pub configured: bool,
}

/// Pending OAuth/QR sign-in challenge returned by Save Relay for Google Drive / Dropbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayChallenge {
    pub service: CloudService,
    pub verification_url: String,
    pub user_code: Option<String>,
    pub qr: Option<String>,
}

/// Setup & sign-in operations delegated to the `save-relay` binary (credentials stay in the
/// OS credential store managed by `save-relay`, never written to launcher files or logs).
pub trait RelaySetup {
    fn status(&self) -> Result<RelayAccountStatus>;
    fn sign_in_nextcloud(
        &self,
        url: &str,
        account: &str,
        app_password: &str,
    ) -> Result<RelayAccountStatus>;
    fn start_oauth(&self, service: CloudService) -> Result<RelayChallenge>;
    fn sign_out(&self) -> Result<()>;
}

fn decode_status(code: Option<i32>, stdout: &[u8]) -> Result<RelayAccountStatus> {
    if is_missing_vcruntime(code) {
        return Err(account_error(MISSING_VCRUNTIME));
    }
    let report: Value = serde_json::from_slice(stdout).map_err(|_| account_invalid())?;
    if !report["command"].is_null() && report["command"] != "status" {
        return Err(account_invalid());
    }
    match report["ok"].as_bool() {
        Some(false) => {
            let err_code = report["error"]["code"].as_str();
            if err_code == Some("not_set_up") {
                return Ok(RelayAccountStatus::default());
            }
            if let Some(msg) = credential_store_error_message(err_code) {
                return Err(account_error(msg));
            }
            if err_code == Some("usage") {
                return Err(account_error(TOO_OLD));
            }
            Err(account_error("Save Relay could not check account status."))
        }
        Some(true) => {
            let data = &report["data"];
            let service = data["service"].as_str().and_then(CloudService::from_id);
            let account = data["account"].as_str().and_then(|s| sanitize_text(s, 256));
            let configured = data["configured"].as_bool().unwrap_or(service.is_some());
            Ok(RelayAccountStatus {
                service,
                account,
                configured,
            })
        }
        None => Err(account_invalid()),
    }
}

fn decode_setup_error(stdout: &[u8], fallback: impl FnOnce() -> String) -> Error {
    if let Ok(report) = serde_json::from_slice::<Value>(stdout) {
        let err_code = report["error"]["code"].as_str();
        if let Some(msg) = credential_store_error_message(err_code) {
            return account_error(msg);
        }
        if err_code == Some("usage") {
            return account_error(TOO_OLD);
        }
        if matches!(
            err_code,
            Some("server_unreachable" | "network_error" | "timeout")
        ) {
            return account_error(
                "Could not reach the save sync server. Check your connection and try again.",
            );
        }
    }
    account_error(fallback())
}

fn decode_sign_in_nextcloud(
    account: &str,
    success: bool,
    code: Option<i32>,
    stdout: &[u8],
) -> Result<RelayAccountStatus> {
    if is_missing_vcruntime(code) {
        return Err(account_error(MISSING_VCRUNTIME));
    }
    let json_failed = serde_json::from_slice::<Value>(stdout)
        .ok()
        .is_some_and(|report| report["ok"].as_bool() == Some(false));
    if !success || json_failed {
        return Err(decode_setup_error(stdout, || {
            "Save Relay could not sign in to Nextcloud. Check the address, user and app password."
                .into()
        }));
    }
    let reported_account = serde_json::from_slice::<Value>(stdout)
        .ok()
        .and_then(|report| {
            report["data"]["account"]
                .as_str()
                .and_then(|s| sanitize_text(s, 256))
        });
    Ok(RelayAccountStatus {
        service: Some(CloudService::Nextcloud),
        account: reported_account.or_else(|| sanitize_text(account, 256)),
        configured: true,
    })
}

fn decode_start_oauth(
    service: CloudService,
    success: bool,
    code: Option<i32>,
    stdout: &[u8],
) -> Result<RelayChallenge> {
    if is_missing_vcruntime(code) {
        return Err(account_error(MISSING_VCRUNTIME));
    }
    let report = serde_json::from_slice::<Value>(stdout);
    let json_failed = report
        .as_ref()
        .ok()
        .is_some_and(|r| r["ok"].as_bool() == Some(false));
    if !success || json_failed {
        return Err(decode_setup_error(stdout, || {
            format!("Save Relay could not start {} sign-in.", service.name())
        }));
    }
    let report = report.map_err(|_| account_invalid())?;
    let verification_url = report["data"]["verification_url"]
        .as_str()
        .and_then(|s| sanitize_text(s, 1024))
        .ok_or_else(account_invalid)?;
    let user_code = report["data"]["user_code"]
        .as_str()
        .and_then(|s| sanitize_text(s, 128));
    let qr = report["data"]["qr"]
        .as_str()
        .or_else(|| report["data"]["qr_ascii"].as_str())
        .and_then(|s| sanitize_multiline(s, 4096));
    Ok(RelayChallenge {
        service,
        verification_url,
        user_code,
        qr,
    })
}

fn decode_sign_out(success: bool, code: Option<i32>, stdout: &[u8]) -> Result<()> {
    if is_missing_vcruntime(code) {
        return Err(account_error(MISSING_VCRUNTIME));
    }
    let json_failed = serde_json::from_slice::<Value>(stdout)
        .ok()
        .is_some_and(|report| report["ok"].as_bool() == Some(false));
    if !success || json_failed {
        return Err(decode_setup_error(stdout, || {
            "Save Relay could not sign out.".into()
        }));
    }
    Ok(())
}

fn start_failed(e: std::io::Error) -> Error {
    account_error(format!("Could not start Save Relay: {e}"))
}

impl RelaySetup for DesktopRelay {
    fn status(&self) -> Result<RelayAccountStatus> {
        let output = Command::new(&self.program)
            .args(STATUS_ARGS)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(start_failed)?;
        decode_status(output.status.code(), &output.stdout)
    }

    fn sign_in_nextcloud(
        &self,
        url: &str,
        account: &str,
        app_password: &str,
    ) -> Result<RelayAccountStatus> {
        let (url, account) = (url.trim(), account.trim());
        if url.is_empty() || account.is_empty() || app_password.is_empty() {
            return Err(account_error(
                "Enter the Nextcloud address, account and app password first.",
            ));
        }
        if url.chars().any(char::is_control)
            || account.chars().any(char::is_control)
            || app_password.contains(['\n', '\r'])
        {
            return Err(account_error(
                "Nextcloud sign-in fields cannot contain line breaks or control characters.",
            ));
        }
        // Pass the password on stdin so it never appears in argv, logs, or settings files.
        use std::io::Write;
        let mut child = Command::new(&self.program)
            .args(nextcloud_args(url, account))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(start_failed)?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "{app_password}");
        }
        let output = child
            .wait_with_output()
            .map_err(|e| account_error(format!("Save Relay setup failed: {e}")))?;
        decode_sign_in_nextcloud(
            account,
            output.status.success(),
            output.status.code(),
            &output.stdout,
        )
    }

    fn start_oauth(&self, service: CloudService) -> Result<RelayChallenge> {
        if service == CloudService::Nextcloud {
            return Err(account_error(
                "Nextcloud sign-in requires a server address, account and app password.",
            ));
        }
        let output = Command::new(&self.program)
            .args(oauth_args(service))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(start_failed)?;
        decode_start_oauth(
            service,
            output.status.success(),
            output.status.code(),
            &output.stdout,
        )
    }

    fn sign_out(&self) -> Result<()> {
        let output = Command::new(&self.program)
            .args(SIGN_OUT_ARGS)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(start_failed)?;
        decode_sign_out(
            output.status.success(),
            output.status.code(),
            &output.stdout,
        )
    }
}

#[cfg(test)]
mod contract;
#[cfg(test)]
mod tests;
