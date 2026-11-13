//! A fake hub on loopback (roadmap 4.05): the built binary runs `hub serve` on 127.0.0.1 with a
//! synthetic build, and a second copy, with its own user folders, pairs with it (a wrong code
//! first) and pulls that build with `hub pull`. A pull for a target the hub does not serve and a
//! pull under a name the hub never paired both fail with the reason. Every file is synthetic;
//! nothing leaves loopback and mDNS is not used (`--bind 127.0.0.1` skips the announcement).
//!
//! Paired-device storage fails closed on Windows until its ACL work lands, so there the commands
//! must fail before listening or connecting, and store nothing.

#![cfg(feature = "hub")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_uncharted-universal");

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("uu-hub-loopback-{}-{label}", std::process::id()));
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

/// The binary with a clean environment whose user folders are under `root`.
fn command(root: &Path, args: &[&str]) -> Command {
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
    command
}

/// Kills the hub when the test ends early, so a failed assertion leaves no listener behind.
#[cfg(not(windows))]
struct Running(std::process::Child);

#[cfg(not(windows))]
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn uu(root: &Path, args: &[&str]) -> Output {
    command(root, args).output().unwrap()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// A synthetic Linux build published as `builds/<name>/bundle.txt`, the way a hub does after
/// preparing. Returns the build folder.
fn publish(builds: &Path, name: &str) -> PathBuf {
    use uncharted_core::{bundle, hub::BUNDLE_TEXT, platform::Os};

    let root = builds.join(name);
    write(&root, "lib/libuncharted_runtime.so", b"synthetic runtime");
    write(&root, "settings/runtime.toml", b"scale = 1\n");
    write(
        &root,
        "shaders/0123456789abcdef.spv",
        b"synthetic shader translation",
    );
    // Larger than one transfer chunk.
    let big: Vec<u8> = (0..200_000u32).map(|n| (n % 251) as u8).collect();
    write(&root, "shaders/large.spv", &big);
    let b = bundle::manifest(&root, Os::Linux, 4).unwrap();
    fs::write(root.join(BUNDLE_TEXT), bundle::to_text(&b)).unwrap();
    root
}

#[cfg(not(windows))]
#[test]
fn a_device_pairs_with_and_pulls_from_a_fake_hub_on_loopback() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let hub_root = TempDir::new("hub");
    let device_root = TempDir::new("device");
    let builds = hub_root.0.join("builds");
    let build = publish(&builds, "2026-11-12");
    let builds_arg = builds.display().to_string();

    // Wrong code, right code, two refused pulls, one good pull: five connections, then it ends.
    let mut hub = Running(
        command(
            &hub_root.0,
            &[
                "hub",
                "serve",
                "--builds",
                &builds_arg,
                "--role",
                "strix-halo",
                "--name",
                "test-hub",
                "--bind",
                "127.0.0.1",
                "--port",
                "0",
                "--stop-after",
                "5",
            ],
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap(),
    );
    let (lines, read) = mpsc::channel();
    let stdout = hub.0.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut all = Vec::new();
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            let _ = lines.send(line.clone());
            all.push(line);
        }
        all
    });
    let next = || read.recv_timeout(Duration::from_secs(30)).unwrap();
    let listening = next();
    let addr = listening
        .split_once(" listening on ")
        .and_then(|(_, rest)| rest.split_once(','))
        .map(|(addr, _)| addr.to_string())
        .unwrap_or_else(|| panic!("no address in {listening:?}"));
    assert!(listening.starts_with("hub test-hub (strix-halo) listening on 127.0.0.1:"));
    assert!(listening.contains("serving linux, android"));
    let code_line = next();
    let shown = code_line
        .strip_prefix("pairing code: ")
        .and_then(|rest| rest.get(..7))
        .unwrap_or_else(|| panic!("no code in {code_line:?}"));
    let typed = shown.replace(' ', "");
    assert_eq!(typed.len(), 6);

    let wrong = if typed == "000000" {
        "111111"
    } else {
        "000000"
    };
    let pair = |code: &str| {
        uu(
            &device_root.0,
            &["hub", "pair", code, "--hub", &addr, "--name", "handheld"],
        )
    };
    let refused = pair(wrong);
    assert_eq!(code(&refused), 1, "{refused:?}");
    assert!(stderr(&refused).contains("did not match"), "{refused:?}");
    assert!(!device_root
        .0
        .join("config/uncharted-universal/peers.json")
        .exists());

    let paired = pair(&typed);
    assert_eq!(code(&paired), 0, "{paired:?}");
    assert!(String::from_utf8_lossy(&paired.stdout)
        .contains(&format!("paired with test-hub at {addr} as handheld")));

    let staged = device_root.0.join("staged");
    let staged_arg = staged.display().to_string();
    let pull = |extra: &[&str]| {
        let mut list = vec!["hub", "pull", "--hub", addr.as_str(), "--into", &staged_arg];
        list.extend_from_slice(extra);
        uu(&device_root.0, &list)
    };
    let windows = pull(&["--target", "windows", "--name", "handheld"]);
    assert_eq!(code(&windows), 1, "{windows:?}");
    assert!(stderr(&windows).contains("test-hub has no build ready for windows"));
    let stranger = pull(&["--target", "linux", "--name", "stranger"]);
    assert_eq!(code(&stranger), 1, "{stranger:?}");
    assert!(stderr(&stranger).contains("does not know this device as stranger"));
    assert!(!staged.join("lib/libuncharted_runtime.so").exists());

    let good = pull(&["--target", "linux", "--name", "handheld"]);
    assert_eq!(code(&good), 0, "{good:?}");
    let said = String::from_utf8_lossy(&good.stdout).into_owned();
    assert!(
        said.contains("for linux from test-hub (patch level 4, 4 files, 2 shader"),
        "{said}"
    );
    for file in [
        "lib/libuncharted_runtime.so",
        "settings/runtime.toml",
        "shaders/0123456789abcdef.spv",
        "shaders/large.spv",
    ] {
        assert_eq!(
            fs::read(staged.join(file)).unwrap(),
            fs::read(build.join(file)).unwrap(),
            "{file}"
        );
    }
    assert!(!staged.join(".uu-transfer-incomplete").exists());

    // The fifth connection was the last: the hub ends by itself (a hang fails the test).
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = hub.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "the hub did not stop after five connections"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "{status:?}");
    let said = reader.join().unwrap().join("\n");
    assert!(
        said.contains("paired with handheld; the code is used up"),
        "{said}"
    );
    assert!(
        said.contains("for linux (patch level 4, 4 files) to handheld"),
        "{said}"
    );
    let mut errors = String::new();
    std::io::Read::read_to_string(&mut hub.0.stderr.take().unwrap(), &mut errors).unwrap();
    assert_eq!(errors.matches("a connection failed").count(), 3, "{errors}");
}

#[cfg(windows)]
#[test]
fn on_windows_the_hub_commands_fail_closed_before_any_connection() {
    let hub_root = TempDir::new("hub-windows");
    let builds = hub_root.0.join("builds");
    publish(&builds, "2026-11-12");
    let serve = uu(
        &hub_root.0,
        &[
            "hub",
            "serve",
            "--builds",
            &builds.display().to_string(),
            "--role",
            "strix-halo",
            "--bind",
            "127.0.0.1",
            "--port",
            "0",
            "--stop-after",
            "1",
        ],
    );
    let device_root = TempDir::new("device-windows");
    let pair = uu(
        &device_root.0,
        &["hub", "pair", "483921", "--hub", "127.0.0.1:9"],
    );
    let pull = uu(&device_root.0, &["hub", "pull", "--hub", "127.0.0.1:9"]);
    for output in [&serve, &pair, &pull] {
        assert_eq!(code(output), 1, "{output:?}");
        assert!(
            stderr(output).contains("not implemented on Windows"),
            "{output:?}"
        );
        assert!(output.stdout.is_empty(), "{output:?}");
    }
    for root in [&hub_root.0, &device_root.0] {
        assert!(!root.join("config/uncharted-universal/peers.json").exists());
    }
}
