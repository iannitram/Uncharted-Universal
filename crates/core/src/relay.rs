//! Desktop adapter for the published Save Relay CLI. The helper owns all server
//! access, credentials, revision comparisons and deferred uploads.
use crate::games::Game;
use crate::launch::{Keep, Relay, Sync};
use crate::savecard::{side_by_side, SaveCard};
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
    fn invoke(&self, phase: &str, game: Game, folder: &Path, keep: Option<Keep>) -> Result<Sync> {
        let id = game_id(game);
        let mut cmd = Command::new(&self.program);
        cmd.args([phase, id, "--manifest"])
            .arg(self.manifests.join(format!("{id}.json")))
            .arg("--saves-dir")
            .arg(folder)
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
        decode(phase, game, output.status.code(), &output.stdout)
    }
}

impl Relay for DesktopRelay {
    fn pull(&self, game: Game, folder: &Path, keep: Option<Keep>) -> Result<Sync> {
        self.invoke("pull", game, folder, keep)
    }
    fn push(&self, game: Game, folder: &Path, keep: Option<Keep>) -> Result<Sync> {
        self.invoke("push", game, folder, keep)
    }
}

fn invalid() -> Error {
    Error::Launch(
        "Save Relay returned an unexpected response. Both saves need checking before play.".into(),
    )
}

fn decode(phase: &str, game: Game, code: Option<i32>, stdout: &[u8]) -> Result<Sync> {
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
    if data["game"] != game_id(game) {
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

#[cfg(test)]
mod tests;
