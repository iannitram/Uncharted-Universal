//! Hub discovery (roadmap 4.01): the TXT schema, the home-network filter, the announce checks
//! and a fake hub on loopback that a device finds and reaches. Ported from
//! iannitram/totk-native's `crates/core/tests/hub_discover.rs` (MIT) without the Switch TXT parts.

#![cfg(feature = "hub")]

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::time::{Duration, Instant};
use uncharted_core::hub::{
    announce, discover, discover_on, found, pair_limited, parse_txt, txt, withdraw, Announcement,
    Found, Mdns, MemoryMdns, PairingLimiter, Record, SystemClock, TcpTransport, TXT_HUB,
    TXT_TARGETS,
};
use uncharted_core::platform::Os;
use uncharted_core::recipe::{self, Hub};

fn pairs(txt: &[(&str, &str)]) -> Vec<(String, String)> {
    txt.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn record(txt: &[(&str, &str)], addrs: &[&str], port: u16) -> Record {
    Record {
        txt: pairs(txt),
        addrs: addrs.iter().map(|a| ip(a)).collect(),
        port,
    }
}

#[test]
fn txt_round_trips_for_every_hub_in_the_recipe_table() {
    for hub in [Hub::StrixHalo, Hub::Desktop, Hub::Mac] {
        let targets = recipe::serves(hub);
        let record = txt(hub, targets);
        assert_eq!(
            record,
            vec![
                (TXT_HUB.to_string(), hub.key().to_string()),
                (
                    TXT_TARGETS.to_string(),
                    targets
                        .iter()
                        .map(|os| os.key())
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ]
        );
        assert_eq!(parse_txt(&record), Some((hub, targets.to_vec())));
    }
    assert_eq!(
        txt(Hub::StrixHalo, &[Os::Linux, Os::Android]),
        pairs(&[("hub", "strix-halo"), ("targets", "linux,android")])
    );
}

#[test]
fn malformed_txt_is_dropped() {
    for bad in [
        vec![],
        vec![("targets", "linux")],
        vec![("hub", "strix-halo")],
        vec![("hub", ""), ("targets", "linux")],
        vec![("hub", "switch-dock"), ("targets", "linux")],
        vec![("hub", "strix-halo"), ("targets", "")],
        vec![("hub", "strix-halo"), ("targets", "linux,")],
        vec![("hub", "strix-halo"), ("targets", "linux,switch")],
        vec![("hub", "strix-halo"), ("targets", "Linux")],
        vec![("hub", "strix-halo"), ("targets", "linux, android")],
        vec![("hub", "strix-halo"), ("targets", "linux,linux")],
    ] {
        assert_eq!(parse_txt(&pairs(&bad)), None, "{bad:?}");
        assert!(found([record(&bad, &["192.168.1.20"], 4710)]).is_empty());
    }
    let good = [("hub", "desktop"), ("targets", "windows")];
    assert!(
        found([record(&good, &["192.168.1.20"], 0)]).is_empty(),
        "port 0"
    );
    assert_eq!(found([record(&good, &["192.168.1.20"], 4710)]).len(), 1);
}

#[test]
fn addresses_outside_the_home_network_are_refused() {
    let hub = [("hub", "strix-halo"), ("targets", "linux,android")];
    let hubs = found([
        record(
            &hub,
            &[
                "8.8.8.8",
                "192.168.1.20",
                "100.64.0.1",
                "2001:4860:4860::8888",
                "fd00::20",
                "::ffff:1.1.1.1",
            ],
            4710,
        ),
        record(
            &[("hub", "desktop"), ("targets", "windows")],
            &["203.0.113.5"],
            4710,
        ),
    ]);
    let addrs: Vec<IpAddr> = hubs.iter().map(|f| f.addr).collect();
    assert_eq!(addrs, [ip("192.168.1.20"), ip("fd00::20")]);
    assert!(hubs.iter().all(|f| f.hub == "strix-halo"));
}

#[test]
fn repeated_sightings_are_merged_and_sorted() {
    let strix = [("hub", "strix-halo"), ("targets", "linux,android")];
    let desktop = [("hub", "desktop"), ("targets", "windows")];
    let hubs = found([
        record(&strix, &["192.168.1.20", "10.0.0.20"], 4710),
        record(&desktop, &["192.168.1.30"], 4710),
        record(&strix, &["10.0.0.20"], 4710),
        // A second record for the same hub, address and port: the first one wins.
        record(
            &[("hub", "strix-halo"), ("targets", "linux")],
            &["192.168.1.20"],
            4710,
        ),
        record(&strix, &["192.168.1.20"], 4711),
    ]);
    let seen: Vec<(&str, String, u16, Vec<Os>)> = hubs
        .iter()
        .map(|f| {
            (
                f.hub.as_str(),
                f.addr.to_string(),
                f.port,
                f.targets.clone(),
            )
        })
        .collect();
    let both = vec![Os::Linux, Os::Android];
    assert_eq!(
        seen,
        [
            ("desktop", "192.168.1.30".into(), 4710, vec![Os::Windows]),
            ("strix-halo", "10.0.0.20".into(), 4710, both.clone()),
            ("strix-halo", "192.168.1.20".into(), 4710, both.clone()),
            ("strix-halo", "192.168.1.20".into(), 4711, both),
        ]
    );
}

#[test]
fn no_hubs_means_an_empty_list_after_the_wait() {
    let net = MemoryMdns::new();
    let started = Instant::now();
    assert!(discover_on(&net, 60).unwrap().is_empty());
    assert!(started.elapsed() >= Duration::from_millis(60));
}

#[test]
fn zero_wait_does_not_start_mdns() {
    assert!(discover(0).unwrap().is_empty());
    assert!(discover_on(&MemoryMdns::new(), 0).unwrap().is_empty());
}

#[test]
fn a_hub_refuses_to_announce_what_no_device_would_accept() {
    let net = MemoryMdns::new();
    let good = Announcement {
        hub: Hub::StrixHalo,
        targets: vec![Os::Linux, Os::Android],
        port: 4710,
        addrs: vec![ip("192.168.1.20")],
    };
    for (bad, message) in [
        (
            Announcement {
                addrs: vec![ip("192.168.1.20"), ip("8.8.8.8")],
                ..good.clone()
            },
            "outside the home network",
        ),
        (
            Announcement {
                targets: vec![],
                ..good.clone()
            },
            "at least one target",
        ),
        (
            Announcement {
                targets: vec![Os::Linux, Os::Linux],
                ..good.clone()
            },
            "same target twice",
        ),
        (
            Announcement {
                port: 0,
                ..good.clone()
            },
            "its port",
        ),
    ] {
        let error = announce(&net, &bad).unwrap_err();
        assert_eq!(error.code(), "hub");
        assert!(error.to_string().contains(message), "{error}");
    }
    assert!(net.browse(Duration::ZERO).unwrap().is_empty());
    announce(&net, &good).unwrap();
    assert_eq!(net.browse(Duration::ZERO).unwrap().len(), 1);
}

/// A fake hub on loopback: a TCP listener on 127.0.0.1 announced through `net`. It accepts one
/// device and pairs with it, as the hub's accept loop will. Each side sends its own name and
/// gets back the other's.
fn fake_hub(net: &dyn Mdns, code: &'static str) -> std::thread::JoinHandle<String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    announce(
        net,
        &Announcement {
            hub: Hub::StrixHalo,
            targets: recipe::serves(Hub::StrixHalo).to_vec(),
            port,
            addrs: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
        },
    )
    .unwrap();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut device = TcpTransport::from_stream(stream).unwrap();
        let peer = pair_limited(
            &mut device,
            code,
            "strix-halo",
            &mut PairingLimiter::default(),
            &SystemClock,
        )
        .unwrap();
        format!("{peer:?}")
    })
}

/// The device side: find the strix-halo hub, connect to it over the checked transport, pair.
fn find_and_pair(hubs: &[Found], code: &str) -> String {
    let hub = hubs
        .iter()
        .find(|f| f.hub == "strix-halo" && f.addr.is_loopback())
        .expect("the fake hub was discovered");
    assert_eq!(hub.targets, [Os::Linux, Os::Android]);
    let mut t =
        TcpTransport::connect(SocketAddr::new(hub.addr, hub.port), Duration::from_secs(5)).unwrap();
    let peer = pair_limited(
        &mut t,
        code,
        "handheld",
        &mut PairingLimiter::default(),
        &SystemClock,
    )
    .unwrap();
    format!("{peer:?}")
}

#[test]
fn a_device_finds_a_fake_hub_on_loopback_and_pairs_with_it() {
    let net = MemoryMdns::new();
    let hub = fake_hub(&net, "482913");
    let hubs = discover_on(&net, 10).unwrap();
    assert!(hubs.iter().all(|f| f.addr.is_loopback()));
    let device = find_and_pair(&hubs, "482913");
    assert!(device.contains("strix-halo"));
    assert!(hub.join().unwrap().contains("handheld"));
    withdraw(&net, Hub::StrixHalo).unwrap();
    assert!(discover_on(&net, 1).unwrap().is_empty());
}

// Real multicast on the loopback interface. Only Linux CI sends it: Windows and macOS would ask
// for firewall or Local Network permission on a desktop.
#[cfg(target_os = "linux")]
#[test]
fn real_mdns_finds_a_fake_hub_on_loopback() {
    use uncharted_core::hub::{is_home_network, MdnsDaemon};

    let net = MdnsDaemon::loopback_only().unwrap();
    let hub = fake_hub(&net, "604117");
    let hubs = discover(5000).unwrap();
    assert!(hubs.iter().all(|f| is_home_network(f.addr)));
    let device = find_and_pair(&hubs, "604117");
    assert!(device.contains("strix-halo"));
    assert!(hub.join().unwrap().contains("handheld"));
    withdraw(&net, Hub::StrixHalo).unwrap();
}
