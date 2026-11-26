//! The argv `relay.rs` builds, run against the pinned Save Relay release's own parser (#637).
//!
//! The fake-CLI tests in `tests.rs` check how replies are read; a fake written to match
//! `relay.rs` accepts whatever `relay.rs` sends, which is how `status --json` shipped against a
//! release that refused it. Here each call goes to the real binary named by
//! `SAVE_RELAY_CONTRACT_BIN` (CI fetches the one this OS's `components.lock.json` pins) on a
//! throwaway profile, and every reply must be an answer, not a `usage` error. Without the
//! variable the test is skipped.
//!
//! Nothing here reaches past the throwaway profile and loopback: Nextcloud sign-in goes to a
//! closed 127.0.0.1 port, so it fails before anything is stored; sign-out meets an unreadable
//! config file first, so it stops before the system credential store. Google Drive and Dropbox
//! sign-in would contact the provider and store a pending sign-in in the system credential
//! store, so their argv is checked against the binary's own `help` instead.
use super::*;
use serde_json::json;
use std::io::Write;
use std::net::TcpListener;

/// A fresh folder under the system temp folder, removed on drop.
struct Profile(PathBuf);

impl Profile {
    fn new() -> Profile {
        let path = std::env::temp_dir().join(format!(
            "uncharted-relay-contract-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&path);
        for dir in [
            "home", "roaming", "local", "config", "data", "state", "cache", "saves",
        ] {
            std::fs::create_dir_all(path.join(dir)).unwrap();
        }
        Profile(path)
    }

    /// Where each OS's Save Relay keeps `save-relay/config.json` under this profile.
    fn config_files(&self) -> [PathBuf; 3] {
        [
            self.0.join("roaming/save-relay/config.json"),
            self.0.join("config/save-relay/config.json"),
            self.0
                .join("home/Library/Application Support/save-relay/config.json"),
        ]
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Reply {
    code: Option<i32>,
    stdout: Vec<u8>,
    json: Value,
}

fn run(bin: &Path, profile: &Profile, args: &[OsString], stdin: &str) -> Reply {
    let mut cmd = Command::new(bin);
    cmd.args(args);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("SAVE_RELAY_") {
            cmd.env_remove(&name);
        }
    }
    let p = &profile.0;
    cmd.env("HOME", p.join("home"))
        .env("USERPROFILE", p.join("home"))
        .env("APPDATA", p.join("roaming"))
        .env("LOCALAPPDATA", p.join("local"))
        .env("XDG_CONFIG_HOME", p.join("config"))
        .env("XDG_DATA_HOME", p.join("data"))
        .env("XDG_STATE_HOME", p.join("state"))
        .env("XDG_CACHE_HOME", p.join("cache"))
        .env_remove("HOMEDRIVE")
        .env_remove("HOMEPATH")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .current_dir(p)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().expect("start the pinned save-relay");
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(stdin.as_bytes());
    }
    let output = child.wait_with_output().expect("wait for save-relay");
    let json = std::str::from_utf8(&output.stdout)
        .ok()
        .and_then(|text| text.lines().next())
        .and_then(|line| serde_json::from_str(line).ok())
        .unwrap_or(Value::Null);
    Reply {
        code: output.status.code(),
        stdout: output.stdout,
        json,
    }
}

fn argv(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

/// The reply is Save Relay's answer to `command`, not a complaint about the command line.
fn assert_understood(what: &str, reply: &Reply, command: &str) {
    assert_eq!(
        reply.json["command"], command,
        "{what}: no {command} envelope (exit {:?}): {}",
        reply.code, reply.json
    );
    assert_ne!(
        reply.json["error"]["code"], "usage",
        "{what}: the pinned Save Relay does not accept this argv (exit {:?}): {}",
        reply.code, reply.json
    );
}

fn contract_bin() -> Option<PathBuf> {
    let Some(bin) = std::env::var_os("SAVE_RELAY_CONTRACT_BIN") else {
        eprintln!("SAVE_RELAY_CONTRACT_BIN is not set: the Save Relay argv contract is skipped");
        return None;
    };
    let bin = PathBuf::from(bin);
    assert!(
        bin.is_file(),
        "SAVE_RELAY_CONTRACT_BIN is set but {} is not a file",
        bin.display()
    );
    Some(bin)
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn pinned_save_relay_accepts_every_launcher_argv() {
    let Some(bin) = contract_bin() else { return };

    // The binary is the one this OS's lock pins.
    let os = match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        _ => "linux",
    };
    let lock: Value = serde_json::from_slice(
        &std::fs::read(repo().join(format!("packaging/{os}/components.lock.json"))).unwrap(),
    )
    .unwrap();
    let pinned = lock["save-relay"]["version"].as_str().unwrap();
    let profile = Profile::new();
    let version = run(&bin, &profile, &argv(&["version", "--json"]), "");
    assert_eq!(version.code, Some(0), "{}", version.json);
    assert_eq!(
        Some(pinned.trim_start_matches('v')),
        version.json["data"]["version"].as_str(),
        "SAVE_RELAY_CONTRACT_BIN is not the pinned {pinned}"
    );

    // Account status, with no game: a fresh profile is "not configured".
    let status = run(&bin, &profile, &argv(&STATUS_ARGS), "");
    assert_understood("status", &status, "status");
    assert_eq!(
        decode_status(status.code, &status.stdout).unwrap(),
        RelayAccountStatus::default()
    );

    // Play's pull and push, every game, with and without a conflict answer: "needs setup".
    let manifests = repo().join("packaging/save-relay");
    let saves = profile.0.join("saves");
    for game in [Game::U1, Game::U2, Game::U3] {
        for phase in ["pull", "push"] {
            for keep in [None, Some(Keep::Server), Some(Keep::Local)] {
                let args = sync_args(phase, game, 2, &manifests, &saves, keep);
                let reply = run(&bin, &profile, &args, "");
                let what = format!("{phase} {} {keep:?}", game_id(game));
                assert_understood(&what, &reply, phase);
                assert_eq!(
                    decode(phase, game, 2, reply.code, &reply.stdout).unwrap(),
                    Sync::LocalOnly("Save Relay needs setup; using local saves."),
                    "{what}: {}",
                    reply.json
                );
            }
        }
    }

    // Nextcloud sign-in against a closed loopback port: the login check fails before anything
    // is stored. The password on stdin is a dummy.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = format!("http://127.0.0.1:{port}");
    let args = argv(&nextcloud_args(&url, "contract-test"));
    let reply = run(&bin, &profile, &args, "dummy-app-password\n");
    assert_understood("setup nextcloud", &reply, "setup");
    assert_eq!(reply.json["ok"], json!(false), "{}", reply.json);
    let err = decode_sign_in_nextcloud("contract-test", false, reply.code, &reply.stdout)
        .unwrap_err()
        .to_string();
    assert!(err.contains("Could not reach"), "{err}: {}", reply.json);
    for config in profile.config_files() {
        assert!(!config.exists(), "{} was written", config.display());
    }

    // Sign-out reads the config before it deletes any credential; an unreadable one stops it
    // there, so the system credential store is never touched.
    for config in profile.config_files() {
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(&config, "not json").unwrap();
    }
    let reply = run(&bin, &profile, &argv(&SIGN_OUT_ARGS), "");
    assert_understood("sign-out", &reply, "sign-out");
    assert_eq!(
        reply.json["ok"],
        json!(false),
        "sign-out got past the unreadable config: {}",
        reply.json
    );

    // Google Drive and Dropbox: the positional service form must be in the binary's own usage.
    let help = run(&bin, &profile, &argv(&["help"]), "");
    assert_eq!(help.code, Some(0));
    let help = String::from_utf8_lossy(&help.stdout).into_owned();
    for service in [CloudService::GoogleDrive, CloudService::Dropbox] {
        let [command, id, json_flag] = oauth_args(service);
        assert_eq!((command, json_flag), ("setup", "--json"));
        let positional = help.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("setup ")
                && line
                    .replace("--service gdrive|dropbox", "")
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                    .any(|word| word == id)
        });
        assert!(
            positional,
            "the pinned Save Relay's usage has no positional `setup {id}`:\n{help}"
        );
    }
    assert!(help.contains("[--json]"), "{help}");
}
