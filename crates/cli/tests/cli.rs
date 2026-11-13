//! Smoke run of the built `uncharted-universal` binary: every command once, with its exit
//! status, on a synthetic dump in a temp folder (no game files, no Steam, no network).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_uncharted-universal");

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("uu-cli-smoke-{}-{label}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `param.sfo` with `TITLE_ID` CUSA02320 and `APP_VER` 01.00.
fn sfo() -> Vec<u8> {
    let pairs = [("TITLE_ID", "CUSA02320"), ("APP_VER", "01.00")];
    let (mut keys, mut values, mut entries) = (Vec::new(), Vec::new(), Vec::new());
    for (key, value) in pairs {
        let key_offset = keys.len() as u16;
        keys.extend_from_slice(key.as_bytes());
        keys.push(0);
        let value_offset = values.len() as u32;
        values.extend_from_slice(value.as_bytes());
        values.push(0);
        let len = (value.len() + 1) as u32;
        entries.extend_from_slice(&key_offset.to_le_bytes());
        entries.extend_from_slice(&0x0204u16.to_le_bytes());
        entries.extend_from_slice(&len.to_le_bytes());
        entries.extend_from_slice(&len.to_le_bytes());
        entries.extend_from_slice(&value_offset.to_le_bytes());
    }
    let key_base = 20 + entries.len();
    let value_base = key_base + keys.len();
    let mut data = b"\0PSF".to_vec();
    data.extend_from_slice(&0x0000_0101u32.to_le_bytes());
    data.extend_from_slice(&(key_base as u32).to_le_bytes());
    data.extend_from_slice(&(value_base as u32).to_le_bytes());
    data.extend_from_slice(&(pairs.len() as u32).to_le_bytes());
    data.extend(entries);
    data.extend(keys);
    data.extend(values);
    data
}

fn dump(root: &Path) -> PathBuf {
    let dump = root.join("dump");
    for name in ["sce_sys", "u1data", "u2data", "u3data"] {
        fs::create_dir_all(dump.join(name)).unwrap();
    }
    for exe in [
        "eboot.bin",
        "big2-ps4_Shipping.elf",
        "big3-ps4_Shipping.elf",
    ] {
        fs::write(dump.join(exe), b"\x7fELF\x02fake header").unwrap();
    }
    fs::write(dump.join("sce_sys/param.sfo"), sfo()).unwrap();
    dump
}

/// Runs the binary with a clean environment whose user folders are under `root`.
fn uu(root: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(BIN);
    command.args(args).env_clear();
    for (name, sub) in [
        ("HOME", "home"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CONFIG_HOME", "config"),
        ("USERPROFILE", "home"),
        ("LOCALAPPDATA", "data"),
        ("APPDATA", "config"),
    ] {
        command.env(name, root.join(sub));
    }
    // Windows needs its system folder to start any process.
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    command.output().unwrap()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap()
}

#[test]
fn every_command_runs_with_its_documented_exit_status() {
    let root = TempDir::new("all");
    let dump = dump(&root.0);
    let dump_arg = dump.display().to_string();
    let empty = root.0.join("empty");
    fs::create_dir_all(&empty).unwrap();
    let art = root.0.join("art").display().to_string();

    let help = uu(&root.0, &["--help"]);
    assert_eq!(code(&help), 0);
    let text = String::from_utf8_lossy(&help.stdout);
    for name in [
        "check",
        "prepare",
        "play",
        "steam-add",
        "patches",
        "hub",
        "fleet",
        "generate-art",
    ] {
        assert!(text.contains(name), "{name} missing from --help");
    }

    let check = uu(&root.0, &["check", &dump_arg]);
    assert_eq!(code(&check), 0, "{check:?}");
    assert!(String::from_utf8_lossy(&check.stdout).contains("ready to prepare"));
    assert_eq!(
        code(&uu(&root.0, &["check", &empty.display().to_string()])),
        1
    );

    assert_eq!(code(&uu(&root.0, &["frobnicate"])), 2);
    assert_eq!(code(&uu(&root.0, &["prepare"])), 2);
    assert_eq!(code(&uu(&root.0, &["play", "u4"])), 2);
    assert_eq!(code(&uu(&root.0, &["hub"])), 2);
    // Without the hub feature `hub serve` is not built (3); with it, there is no builds folder
    // yet (and Windows refuses paired-device storage), so it fails (1). It never listens here.
    let serve = uu(
        &root.0,
        &["hub", "serve", "--bind", "127.0.0.1", "--port", "0"],
    );
    let expected = if cfg!(feature = "hub") { 1 } else { 3 };
    assert_eq!(code(&serve), expected, "{serve:?}");

    let patches = uu(
        &root.0,
        &[
            "patches",
            &dump_arg,
            "--patches-dir",
            &empty.display().to_string(),
        ],
    );
    assert_eq!(code(&patches), 0, "{patches:?}");
    let fleet = uu(&root.0, &["fleet"]);
    assert_eq!(code(&fleet), 0, "{fleet:?}");
    assert!(String::from_utf8_lossy(&fleet.stdout).contains("no paired devices yet"));

    let generate = uu(&root.0, &["generate-art", &art]);
    assert_eq!(code(&generate), 0, "{generate:?}");
    assert!(Path::new(&art).join("u1.jpg").is_file());

    // Commands whose core piece is still a stub end with status 3 and name it; once the piece
    // is built they get further (prepare then fails on the fake ELF headers, status 1).
    let prepare = uu(&root.0, &["prepare", "u1", "--dump", &dump_arg]);
    assert!(matches!(code(&prepare), 1 | 3), "{prepare:?}");
    // No stub panics reach the screen as a panic.
    assert!(!String::from_utf8_lossy(&prepare.stderr).contains("panicked"));
    let steam = uu(
        &root.0,
        &["steam-add", "--userdata", &empty.display().to_string()],
    );
    assert!(matches!(code(&steam), 0 | 1 | 3), "{steam:?}");
    assert!(!String::from_utf8_lossy(&steam.stderr).contains("panicked"));
    // Without --userdata it looks for Steam under the (empty) home folder: a stub that panics
    // today (`steam::userdata_roots`) is reported as not built, later as no Steam found.
    let steam = uu(&root.0, &["steam-add"]);
    assert!(matches!(code(&steam), 1 | 3), "{steam:?}");
    assert!(!String::from_utf8_lossy(&steam.stderr).contains("panicked"));
}
