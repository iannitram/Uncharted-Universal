//! Bundle serve and pull (roadmap 4.03): a fake hub on loopback serves the newest bundle for a
//! target, shader translations included, to a paired device over the sealed stream; the device
//! stages it with resume and checks it before install. A tampered file, an unpaired device, a
//! driver pipeline cache, a target mismatch and a non-home address all fail. The flow follows
//! iannitram/totk-native's hub serve/pull tests (MIT); every file here is synthetic.

#![cfg(feature = "hub")]

use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::time::Duration;
use uncharted_core::bundle::{self, Bundle, Kind};
use uncharted_core::error::{Error, Result};
use uncharted_core::hub::{
    pair_limited, pull, serve, PairingLimiter, Peer, SystemClock, TcpTransport, Transport,
    BUNDLE_TEXT,
};
use uncharted_core::platform::Os;

const TIMEOUT: Duration = Duration::from_secs(20);
const MARKER: &str = ".uu-transfer-incomplete";

/// A fresh scratch folder for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("uu-hub-bundle-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// A build of synthetic runtime files and shader translations, published as
/// `builds/<name>/bundle.txt` the way a hub does after preparing.
fn publish(builds: &Path, name: &str, target: Os, patch_level: u32, tag: &str) -> Bundle {
    let root = builds.join(name);
    write(
        &root,
        "lib/libuncharted_runtime.so",
        format!("synthetic runtime {tag}").as_bytes(),
    );
    write(&root, "settings/runtime.toml", b"scale = 1\n");
    write(
        &root,
        "shaders/0123456789abcdef.spv",
        format!("synthetic shader translation {tag}").as_bytes(),
    );
    // Larger than one transfer chunk, so resume has something to save.
    let big: Vec<u8> = (0..200_000u32).map(|n| (n % 251) as u8).collect();
    write(&root, "shaders/large.spv", &big);
    let b = bundle::manifest(&root, target, patch_level).unwrap();
    fs::write(root.join(BUNDLE_TEXT), bundle::to_text(&b)).unwrap();
    b
}

/// Pairs a hub and a device over TCP loopback with one code; returns (hub's record of the
/// device, device's record of the hub).
fn paired(code: &str) -> (Peer, Peer) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let code_hub = code.to_string();
    let hub = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut t = TcpTransport::from_stream(stream).unwrap();
        pair_limited(
            &mut t,
            &code_hub,
            "strix-halo",
            &mut PairingLimiter::default(),
            &SystemClock,
        )
        .unwrap()
    });
    let mut t = TcpTransport::connect(addr, TIMEOUT).unwrap();
    let device = pair_limited(
        &mut t,
        code,
        "handheld",
        &mut PairingLimiter::default(),
        &SystemClock,
    )
    .unwrap();
    (hub.join().unwrap(), device)
}

/// What the device side does to its stream: nothing, or flip one byte it reads at `flip_at`.
/// It also counts the bytes read, which shows how much a resumed pull asked for.
struct Tap {
    inner: TcpTransport,
    read: u64,
    flip_at: Option<u64>,
}

impl io::Read for Tap {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if let Some(at) = self.flip_at {
            if (self.read..self.read + n as u64).contains(&at) {
                buf[(at - self.read) as usize] ^= 0x40;
            }
        }
        self.read += n as u64;
        Ok(n)
    }
}

impl io::Write for Tap {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl Transport for Tap {}

struct Run {
    hub: Result<Bundle>,
    device: Result<Bundle>,
    /// Bytes the device read from the hub.
    read: u64,
}

/// One fake-hub session on loopback: the hub accepts one device and serves `builds` for
/// `hub_target`; the device pulls `device_target` into `staged`.
#[allow(clippy::too_many_arguments)]
fn session(
    hub_peer: &Peer,
    builds: &Path,
    hub_target: Os,
    device_peer: &Peer,
    device_target: Os,
    staged: &Path,
    flip_at: Option<u64>,
) -> Run {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let hub_peer = hub_peer.clone();
    let builds = builds.to_path_buf();
    let hub = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut t = TcpTransport::from_stream(stream).unwrap();
        t.set_timeout(TIMEOUT).unwrap();
        serve(&mut t, &hub_peer, &builds, hub_target)
    });
    let inner = TcpTransport::connect(addr, TIMEOUT).unwrap();
    inner.set_timeout(TIMEOUT).unwrap();
    let mut tap = Tap {
        inner,
        read: 0,
        flip_at,
    };
    let device = pull(&mut tap, device_peer, device_target, staged);
    let read = tap.read;
    drop(tap);
    Run {
        hub: hub.join().unwrap(),
        device,
        read,
    }
}

fn hub_message(result: &Result<Bundle>) -> String {
    match result {
        Err(Error::Hub(message)) => message.clone(),
        other => panic!("expected a hub error, got {other:?}"),
    }
}

#[test]
fn a_fake_hub_on_loopback_serves_the_newest_bundle_with_its_shader_translations() {
    let root = scratch("serve");
    let builds = root.join("builds");
    publish(&builds, "2026-11-01", Os::Linux, 3, "old");
    publish(&builds, "2026-11-02-windows", Os::Windows, 9, "windows");
    let newest = publish(&builds, "2026-11-12", Os::Linux, 4, "new");
    // A driver pipeline cache next to the build is never listed, so it never travels.
    write(
        &builds.join("2026-11-12"),
        "cache/vulkan_driver.vkc",
        b"driver blob",
    );
    let (hub_peer, device_peer) = paired("482913");
    let staged = root.join("staged");

    let run = session(
        &hub_peer,
        &builds,
        Os::Linux,
        &device_peer,
        Os::Linux,
        &staged,
        None,
    );
    let pulled = run.device.unwrap();
    assert_eq!(run.hub.unwrap(), newest);
    assert_eq!(pulled, newest);
    assert_eq!(pulled.patch_level, 4);
    let shaders: Vec<&str> = pulled
        .files
        .iter()
        .filter(|f| f.kind == Kind::ShaderTranslation)
        .map(|f| f.path.as_str())
        .collect();
    assert_eq!(
        shaders,
        ["shaders/0123456789abcdef.spv", "shaders/large.spv"]
    );
    assert_eq!(
        fs::read(staged.join("shaders/0123456789abcdef.spv")).unwrap(),
        b"synthetic shader translation new"
    );
    assert!(!staged.join("cache").exists());
    assert!(!staged.join(MARKER).exists());
    assert!(!staged.join(BUNDLE_TEXT).exists());
    bundle::verify(&pulled, &staged).unwrap();

    // The staged build installs over an old one and keeps the device's saves.
    let install = root.join("install");
    write(&install, "saves/slot0.sav", b"synthetic save");
    bundle::install(&pulled, &staged, &install).unwrap();
    assert_eq!(
        fs::read(install.join("saves/slot0.sav")).unwrap(),
        b"synthetic save"
    );
    assert!(install.join("shaders/large.spv").is_file());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_pull_resumes_and_replaces_a_damaged_staged_file() {
    let root = scratch("resume");
    let builds = root.join("builds");
    let b = publish(&builds, "build", Os::Linux, 1, "resume");
    let (hub_peer, device_peer) = paired("604117");
    let staged = root.join("staged");

    let full = session(
        &hub_peer,
        &builds,
        Os::Linux,
        &device_peer,
        Os::Linux,
        &staged,
        None,
    );
    assert_eq!(full.device.unwrap(), b);
    assert!(full.read > 200_000);

    // Everything is staged already: only the manifest and the skips go over the wire.
    let again = session(
        &hub_peer,
        &builds,
        Os::Linux,
        &device_peer,
        Os::Linux,
        &staged,
        None,
    );
    assert_eq!(again.device.unwrap(), b);
    assert!(again.read < 10_000, "resume read {} bytes", again.read);

    // A damaged staged file is fetched again; the large one is still skipped.
    write(&staged, "shaders/0123456789abcdef.spv", b"damaged");
    let repaired = session(
        &hub_peer,
        &builds,
        Os::Linux,
        &device_peer,
        Os::Linux,
        &staged,
        None,
    );
    assert_eq!(repaired.device.unwrap(), b);
    assert!(
        repaired.read < 10_000,
        "repair read {} bytes",
        repaired.read
    );
    assert_eq!(
        fs::read(staged.join("shaders/0123456789abcdef.spv")).unwrap(),
        b"synthetic shader translation resume"
    );
    bundle::verify(&b, &staged).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_file_tampered_on_the_hub_fails_and_leaves_the_staging_marked_incomplete() {
    let root = scratch("tampered-hub");
    let builds = root.join("builds");
    let b = publish(&builds, "build", Os::Linux, 1, "tamper");
    // Same size, different bytes: only the hash can tell.
    write(
        &builds.join("build"),
        "shaders/0123456789abcdef.spv",
        b"synthetic shader translation TAMPER",
    );
    let (hub_peer, device_peer) = paired("118822");
    let staged = root.join("staged");

    let run = session(
        &hub_peer,
        &builds,
        Os::Linux,
        &device_peer,
        Os::Linux,
        &staged,
        None,
    );
    assert!(hub_message(&run.hub).contains("shaders/0123456789abcdef.spv"));
    assert!(run.device.is_err());
    assert!(staged.join(MARKER).exists());
    let install = root.join("install");
    fs::create_dir_all(&install).unwrap();
    assert!(
        hub_message(&bundle::install(&b, &staged, &install).map(|_| b.clone()))
            .contains("incomplete")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_byte_tampered_in_transit_fails_the_sealed_stream() {
    let root = scratch("tampered-wire");
    let builds = root.join("builds");
    publish(&builds, "build", Os::Linux, 1, "wire");
    let (hub_peer, device_peer) = paired("907531");
    let staged = root.join("staged");

    // Byte 5000 is inside the sealed file data that follows the manifest frame.
    let run = session(
        &hub_peer,
        &builds,
        Os::Linux,
        &device_peer,
        Os::Linux,
        &staged,
        Some(5_000),
    );
    // The hub may already have written everything into the socket buffers, so only the
    // device's side is certain to fail.
    assert!(hub_message(&run.device).contains("not paired"));
    assert!(staged.join(MARKER).exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_unpaired_device_is_refused_before_the_builds_are_read() {
    let root = scratch("unpaired");
    let (hub_peer, _) = paired("246813");
    // Paired with some other hub: a real key, but not this hub's.
    let (_, stranger) = paired("135792");
    let staged = root.join("staged");

    // The builds folder does not even exist: the refusal must come first.
    let run = session(
        &hub_peer,
        &root.join("no-builds-here"),
        Os::Linux,
        &stranger,
        Os::Linux,
        &staged,
        None,
    );
    assert!(hub_message(&run.hub).contains("not paired"));
    assert!(run.device.is_err());
    assert!(!staged.join("shaders").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_bundle_listing_a_driver_pipeline_cache_is_never_served() {
    let root = scratch("driver-cache");
    let builds = root.join("builds");
    let build = builds.join("build");
    write(&build, "cache/vulkan_driver.vkc", b"driver blob");
    write(&build, "lib/libuncharted_runtime.so", b"runtime");
    // A hand-edited bundle text that lists the driver cache.
    let hash = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    let text = format!(
        "uncharted-bundle 1\ntarget linux\npatches 0\nbuild abc\nfile shader 11 {} cache/vulkan_driver.vkc\nfile install 7 {} lib/libuncharted_runtime.so\n",
        hash(b"driver blob"),
        hash(b"runtime")
    );
    fs::write(build.join(BUNDLE_TEXT), text).unwrap();
    assert!(bundle::manifest(&build, Os::Linux, 0).is_err());

    let (hub_peer, device_peer) = paired("314159");
    let staged = root.join("staged");
    let run = session(
        &hub_peer,
        &builds,
        Os::Linux,
        &device_peer,
        Os::Linux,
        &staged,
        None,
    );
    assert!(hub_message(&run.hub).contains("cache/vulkan_driver.vkc"));
    assert!(run.device.is_err());
    assert!(!staged.join("cache").exists());
    assert!(!staged.join("lib").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_request_for_another_target_is_refused() {
    let root = scratch("target");
    let builds = root.join("builds");
    publish(&builds, "build", Os::Linux, 1, "target");
    let (hub_peer, device_peer) = paired("271828");
    let staged = root.join("staged");

    let run = session(
        &hub_peer,
        &builds,
        Os::Windows,
        &device_peer,
        Os::Linux,
        &staged,
        None,
    );
    assert!(hub_message(&run.hub).contains("not authorized"));
    assert!(run.device.is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_hub_outside_the_home_network_is_refused_before_connecting() {
    for addr in ["8.8.8.8", "2001:4860:4860::8888", "100.64.0.1"] {
        let addr = SocketAddr::new(addr.parse::<IpAddr>().unwrap(), 41234);
        match TcpTransport::connect(addr, Duration::from_millis(1)) {
            Err(Error::Hub(message)) => assert!(message.contains("home network"), "{message}"),
            Err(other) => panic!("{addr}: expected the home-network refusal, got {other:?}"),
            Ok(_) => panic!("{addr}: connected outside the home network"),
        }
    }
}
