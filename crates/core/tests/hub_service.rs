//! The hub service (roadmap 4.05) on loopback: one listener takes a pairing and then bundle
//! pulls, telling a device it never paired, or one asking for a target it does not serve, why it
//! gets nothing. An unknown opening packet is refused before anything else is read. Every file
//! here is synthetic.
//!
//! Paired-device storage fails closed on Windows, so there pairing and pulls must fail and store
//! nothing; the full flow runs on Linux and macOS.

#![cfg(feature = "hub")]

use std::fs;
use std::io::Write;
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use uncharted_core::bundle::{self, Bundle};
use uncharted_core::error::Error;
use uncharted_core::hub::{HubService, PairingServer, SystemClock, BUNDLE_TEXT};
use uncharted_core::platform::Os;

/// A fresh scratch folder for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("uu-hub-service-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// A synthetic Linux build, published as `builds/<name>/bundle.txt`.
fn publish(builds: &Path, name: &str) -> Bundle {
    let root = builds.join(name);
    write(&root, "lib/libuncharted_runtime.so", b"synthetic runtime");
    write(&root, "shaders/0123456789abcdef.spv", b"synthetic shader");
    let b = bundle::manifest(&root, Os::Linux, 2).unwrap();
    fs::write(root.join(BUNDLE_TEXT), bundle::to_text(&b)).unwrap();
    b
}

/// A service on loopback for Linux and Android with pairing code 482913, and its address.
fn service(root: &Path) -> (HubService, TcpListener) {
    let peers = root.join("hub/peers.json");
    let builds = root.join("builds");
    let pairing = PairingServer::with_code("482913", "test-hub", &peers, SystemClock).unwrap();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    (
        HubService::new(pairing, &peers, &builds, &[Os::Linux, Os::Android]),
        listener,
    )
}

fn message(error: Error) -> String {
    match error {
        Error::Hub(message) => message,
        other => panic!("expected a hub error, got {other:?}"),
    }
}

#[test]
fn an_unknown_opening_packet_is_refused() {
    let root = scratch("unknown");
    let (mut hub, listener) = service(&root);
    let addr = listener.local_addr().unwrap();
    let device = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(addr).unwrap();
        let hello = b"UUHUB1 delete everything";
        stream
            .write_all(&(hello.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(hello).unwrap();
    });
    let error = hub.accept(&listener).unwrap_err();
    device.join().unwrap();
    assert_eq!(message(error), "a device sent an unknown request");
    assert!(!hub.pairing_spent());
    assert!(!root.join("hub/peers.json").exists());
    let _ = fs::remove_dir_all(root);
}

#[cfg(not(windows))]
#[test]
fn one_listener_pairs_a_device_and_then_serves_it_the_newest_build() {
    use uncharted_core::hub::{pair_with_service, pull_from_service, Served};

    let root = scratch("flow");
    let published = publish(&root.join("builds"), "2026-11-12");
    let (mut hub, listener) = service(&root);
    let addr = listener.local_addr().unwrap();
    assert_eq!(hub.pairing_code(), "482 913");
    let device_peers = root.join("device/peers.json");
    let staged = root.join("staged");

    let hub_thread = std::thread::spawn(move || {
        let served: Vec<_> = (0..4).map(|_| hub.accept(&listener)).collect();
        (served, hub.pairing_spent())
    });
    let hub_record = pair_with_service(addr, "482-913", "handheld", &device_peers).unwrap();
    assert_eq!(hub_record.device, "test-hub");
    let stranger = pull_from_service(addr, &hub_record, "stranger", Os::Linux, &staged);
    assert_eq!(
        message(stranger.unwrap_err()),
        "test-hub does not know this device as stranger; pair with it again"
    );
    let windows = pull_from_service(addr, &hub_record, "handheld", Os::Windows, &staged);
    assert_eq!(
        message(windows.unwrap_err()),
        "test-hub has no build ready for windows"
    );
    let pulled = pull_from_service(addr, &hub_record, "handheld", Os::Linux, &staged).unwrap();
    assert_eq!(pulled, published);
    assert_eq!(
        fs::read(staged.join("shaders/0123456789abcdef.spv")).unwrap(),
        b"synthetic shader"
    );

    let (served, spent) = hub_thread.join().unwrap();
    assert!(spent, "one pairing uses the code up");
    let mut served = served.into_iter();
    assert!(matches!(served.next(), Some(Ok(Served::Paired(peer))) if peer.device == "handheld"));
    assert!(message(served.next().unwrap().unwrap_err()).contains("not paired with this hub"));
    assert!(message(served.next().unwrap().unwrap_err()).contains("does not serve"));
    assert_eq!(
        served.next().unwrap().unwrap(),
        Served::Sent {
            device: "handheld".into(),
            bundle: published,
        }
    );
    let _ = fs::remove_dir_all(root);
}

#[cfg(windows)]
#[test]
fn on_windows_the_service_stores_nothing_and_serves_nothing() {
    use uncharted_core::hub::pair_with_service;

    let root = scratch("windows");
    publish(&root.join("builds"), "2026-11-12");
    let (mut hub, listener) = service(&root);
    let addr = listener.local_addr().unwrap();
    let device_peers = root.join("device/peers.json");
    let hub_thread = std::thread::spawn(move || hub.accept(&listener));
    let device = pair_with_service(addr, "482913", "handheld", &device_peers);
    let served = hub_thread.join().unwrap();
    for error in [device.unwrap_err(), served.unwrap_err()] {
        assert!(message(error).contains("not implemented on Windows"));
    }
    assert!(!device_peers.exists() && !root.join("hub/peers.json").exists());
    let _ = fs::remove_dir_all(root);
}
