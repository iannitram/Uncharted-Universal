//! Desktop adapter for the published Save Relay CLI. The helper owns all server
//! access, credentials, revision comparisons and deferred uploads.
use crate::games::Game;
use crate::launch::{Keep, Relay, Sync};
use crate::savecard::{side_by_side, SaveCard, SideBySide};
use crate::{Error, Result};
use chrono::DateTime;
use serde_json::Value;
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
        let id = game_id(game);
        let mut cmd = Command::new(&self.program);
        cmd.args([phase, id, "--manifest"])
            .arg(self.manifests.join(format!("{id}.json")))
            .arg("--saves-dir")
            .arg(folder)
            .arg("--slot")
            .arg(slot.to_string())
            .arg("--json")
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        if let Some(keep) = keep {
            cmd.args([
                "--resolve",
                match keep {
                    Keep::Server => "keep-server",
                    Keep::Local => "keep-local",
                },
            ]);
        }
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

fn decode(phase: &str, game: Game, slot: u8, code: Option<i32>, stdout: &[u8]) -> Result<Sync> {
    // The Windows loader can terminate the image before main runs. The rc.2
    // binary imports VCRUNTIME140.dll from the Microsoft x64 C++ runtime.
    if code == Some(0xc0000135_u32 as i32) {
        return Err(Error::Launch("Save Relay could not load a required DLL. Install the Microsoft Visual C++ x64 Redistributable and retry.".into()));
    }
    let report: Value = serde_json::from_slice(stdout).map_err(|_| invalid())?;
    if report["command"] != phase {
        return Err(invalid());
    }
    match report["ok"].as_bool() {
        Some(false) => {
            // Provider messages can contain account paths. Only display known local text.
            let message = match (code, report["error"]["code"].as_str()) {
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
        device: Some(
            device
                .chars()
                .filter(|c| !c.is_control())
                .take(128)
                .collect(),
        ),
        detail: card["place"]
            .as_str()
            .map(|s| s.chars().filter(|c| !c.is_control()).take(1024).collect()),
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
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    if value["version"] != 1 || value["event"] != "save-conflict" {
        return None;
    }
    let game = Game::from_key(value["game"].as_str()?)?;
    let parse_card = |side: &Value| -> Option<SaveCard> {
        Some(SaveCard {
            game,
            slot: side["slot"].as_str().unwrap_or_default().to_owned(),
            device: side["device"].as_str().map(str::to_owned),
            detail: side["detail"].as_str().map(str::to_owned),
            play_time_secs: side["play_time_secs"].as_u64(),
            saved_unix: side["saved_unix"].as_u64()?,
            picture: None,
            title: None,
            build_id: None,
        })
    };
    let server = parse_card(&value["server"])?;
    let local = parse_card(&value["local"])?;
    Some((game, side_by_side(server, local)))
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

impl RelaySetup for DesktopRelay {
    fn status(&self) -> Result<RelayAccountStatus> {
        let output = Command::new(&self.program)
            .args(["status", "--json"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| Error::Launch(format!("Could not start Save Relay: {e}")))?;
        let report: Value = serde_json::from_slice(&output.stdout).map_err(|_| invalid())?;
        let data = &report["data"];
        let service = data["service"].as_str().and_then(CloudService::from_id);
        let account = data["account"].as_str().map(str::to_owned);
        let configured = data["configured"].as_bool().unwrap_or(service.is_some());
        Ok(RelayAccountStatus {
            service,
            account,
            configured,
        })
    }

    fn sign_in_nextcloud(
        &self,
        url: &str,
        account: &str,
        app_password: &str,
    ) -> Result<RelayAccountStatus> {
        // Pass the password on stdin so it never appears in argv, logs, or settings files.
        use std::io::Write;
        let mut child = Command::new(&self.program)
            .args([
                "setup",
                "nextcloud",
                "--url",
                url,
                "--user",
                account,
                "--password-stdin",
                "--json",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Launch(format!("Could not start Save Relay: {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "{app_password}");
        }
        let output = child
            .wait_with_output()
            .map_err(|e| Error::Launch(format!("Save Relay setup failed: {e}")))?;
        if !output.status.success() {
            return Err(Error::Launch(
                "Save Relay could not sign in to Nextcloud. Check the address, user and app password."
                    .into(),
            ));
        }
        Ok(RelayAccountStatus {
            service: Some(CloudService::Nextcloud),
            account: Some(account.to_owned()),
            configured: true,
        })
    }

    fn start_oauth(&self, service: CloudService) -> Result<RelayChallenge> {
        let output = Command::new(&self.program)
            .args(["setup", service.id(), "--json"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| Error::Launch(format!("Could not start Save Relay: {e}")))?;
        if !output.status.success() {
            return Err(Error::Launch(format!(
                "Save Relay could not start {} sign-in.",
                service.name()
            )));
        }
        let report: Value = serde_json::from_slice(&output.stdout).map_err(|_| invalid())?;
        let verification_url = report["data"]["verification_url"]
            .as_str()
            .ok_or_else(invalid)?
            .to_owned();
        let user_code = report["data"]["user_code"].as_str().map(str::to_owned);
        Ok(RelayChallenge {
            service,
            verification_url,
            user_code,
        })
    }

    fn sign_out(&self) -> Result<()> {
        let output = Command::new(&self.program)
            .args(["sign-out", "--json"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| Error::Launch(format!("Could not start Save Relay: {e}")))?;
        if !output.status.success() {
            return Err(Error::Launch("Save Relay could not sign out.".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
