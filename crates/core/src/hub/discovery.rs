//! Hub discovery on the home network (roadmap 4.01; ADR 0004; SPEC.md M12): a hub announces
//! `SERVICE` over mDNS with its port and a TXT record, and a device browses for it. The browse
//! and announce calls go through the `Mdns` seam: `MdnsDaemon` is the real network (`mdns-sd`),
//! `MemoryMdns` is an in-process fake so CI runs a fake hub on loopback without multicast.
//! Shape lifted from iannitram/totk-native's `discover` (MIT, PR #88); its Switch-specific TXT
//! parts stay out.
//!
//! TXT schema (ADR 0004): `hub=<recipe::Hub::key()>` and `targets=<Os::key()>,<Os::key()>...`.
//! Keys match case-insensitively and only the first copy of a key counts (RFC 6763 6.4); other
//! keys are ignored so a later schema can add some. A record with an unknown hub, an empty,
//! unknown or repeated target, or port 0 is dropped, and so is every address that fails
//! `is_home_network`.

use super::{is_home_network, Found, SERVICE};
use crate::error::{Error, Result};
use crate::platform::Os;
use crate::recipe::Hub;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The TXT key that names the hub (`recipe::Hub::key()`).
pub const TXT_HUB: &str = "hub";
/// The TXT key that lists the targets the hub prepares for, comma-separated `Os::key()` values.
pub const TXT_TARGETS: &str = "targets";

/// What a hub announces: who it is, what it prepares for, and where to reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    pub hub: Hub,
    pub targets: Vec<Os>,
    /// The TCP port the hub listens on.
    pub port: u16,
    /// The addresses to announce; empty means this machine's own addresses.
    pub addrs: Vec<IpAddr>,
}

/// One service record as the network carried it, before any check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub txt: Vec<(String, String)>,
    pub addrs: Vec<IpAddr>,
    pub port: u16,
}

/// The seam between discovery and the network: register and browse `SERVICE` records.
pub trait Mdns {
    /// Publish `record` under `SERVICE` as `instance`, replacing an earlier one of that name.
    /// Empty `record.addrs` means this machine's own addresses.
    fn register(&self, instance: &str, record: &Record) -> Result<()>;
    /// Stop publishing `instance`. Withdrawing a name that is not published is not an error.
    fn withdraw(&self, instance: &str) -> Result<()>;
    /// Every record seen for `SERVICE` within `wait`.
    fn browse(&self, wait: Duration) -> Result<Vec<Record>>;
}

/// The TXT record for `hub` serving `targets`.
pub fn txt(hub: Hub, targets: &[Os]) -> Vec<(String, String)> {
    let targets: Vec<&str> = targets.iter().map(|os| os.key()).collect();
    vec![
        (TXT_HUB.to_string(), hub.key().to_string()),
        (TXT_TARGETS.to_string(), targets.join(",")),
    ]
}

/// The hub and targets in a TXT record, or `None` when the record is malformed.
pub fn parse_txt(txt: &[(String, String)]) -> Option<(Hub, Vec<Os>)> {
    let value = |key: &str| {
        txt.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    };
    let hub = Hub::from_key(value(TXT_HUB)?).ok()?;
    let mut targets = Vec::new();
    for key in value(TXT_TARGETS)?.split(',') {
        let os = Os::from_key(key)?;
        if targets.contains(&os) {
            return None;
        }
        targets.push(os);
    }
    Some((hub, targets))
}

/// The hubs in `records`: malformed records and non-home addresses dropped, one entry per
/// hub, address and port (the first record wins), sorted by hub, address and port.
pub fn found(records: impl IntoIterator<Item = Record>) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    for record in records {
        if record.port == 0 {
            continue;
        }
        let Some((hub, targets)) = parse_txt(&record.txt) else {
            continue;
        };
        for addr in record.addrs {
            if !is_home_network(addr) {
                continue;
            }
            let duplicate = found
                .iter()
                .any(|f| f.hub == hub.key() && f.addr == addr && f.port == record.port);
            if !duplicate {
                found.push(Found {
                    hub: hub.key().to_string(),
                    addr,
                    port: record.port,
                    targets: targets.clone(),
                });
            }
        }
    }
    found.sort_by(|a, b| (&a.hub, a.addr, a.port).cmp(&(&b.hub, b.addr, b.port)));
    found
}

/// Hubs announcing `SERVICE` on the home network, waited for up to `wait_ms`. A zero wait
/// returns nothing without starting mDNS.
pub fn discover(wait_ms: u64) -> Result<Vec<Found>> {
    if wait_ms == 0 {
        return Ok(Vec::new());
    }
    discover_on(&MdnsDaemon::new()?, wait_ms)
}

/// `discover` through `net`.
pub fn discover_on(net: &dyn Mdns, wait_ms: u64) -> Result<Vec<Found>> {
    if wait_ms == 0 {
        return Ok(Vec::new());
    }
    Ok(found(net.browse(Duration::from_millis(wait_ms))?))
}

/// Announce this hub on `net` until `withdraw` (or until `net` is dropped, for `MdnsDaemon`).
/// Refuses an announcement no device would accept: no targets, a repeated target, port 0, or
/// an address outside the home network.
pub fn announce(net: &dyn Mdns, a: &Announcement) -> Result<()> {
    if a.targets.is_empty() {
        return Err(Error::Hub("a hub must announce at least one target".into()));
    }
    if a.targets
        .iter()
        .enumerate()
        .any(|(i, os)| a.targets[..i].contains(os))
    {
        return Err(Error::Hub("a hub announced the same target twice".into()));
    }
    if a.port == 0 {
        return Err(Error::Hub("a hub must announce its port".into()));
    }
    if let Some(addr) = a.addrs.iter().find(|addr| !is_home_network(**addr)) {
        return Err(Error::Hub(format!(
            "refusing to announce {addr}, which is outside the home network"
        )));
    }
    net.register(
        a.hub.key(),
        &Record {
            txt: txt(a.hub, &a.targets),
            addrs: a.addrs.clone(),
            port: a.port,
        },
    )
}

/// Stop announcing `hub` on `net`.
pub fn withdraw(net: &dyn Mdns, hub: Hub) -> Result<()> {
    net.withdraw(hub.key())
}

/// An in-process `Mdns` for tests and CI: every clone shares one registry, a record registered
/// with no addresses is reachable at 127.0.0.1, and `browse` waits the full window like the
/// real network before returning what is registered.
#[derive(Debug, Clone, Default)]
pub struct MemoryMdns {
    records: Arc<Mutex<Vec<(String, Record)>>>,
}

impl MemoryMdns {
    pub fn new() -> Self {
        Self::default()
    }

    fn records(&self) -> std::sync::MutexGuard<'_, Vec<(String, Record)>> {
        self.records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Mdns for MemoryMdns {
    fn register(&self, instance: &str, record: &Record) -> Result<()> {
        let mut record = record.clone();
        if record.addrs.is_empty() {
            record.addrs.push(IpAddr::V4(Ipv4Addr::LOCALHOST));
        }
        let mut records = self.records();
        records.retain(|(name, _)| name != instance);
        records.push((instance.to_string(), record));
        Ok(())
    }

    fn withdraw(&self, instance: &str) -> Result<()> {
        self.records().retain(|(name, _)| name != instance);
        Ok(())
    }

    fn browse(&self, wait: Duration) -> Result<Vec<Record>> {
        std::thread::sleep(wait);
        Ok(self.records().iter().map(|(_, r)| r.clone()).collect())
    }
}

/// The real network: an `mdns-sd` daemon. Dropping it withdraws what it registered and stops
/// the daemon.
pub struct MdnsDaemon {
    daemon: mdns_sd::ServiceDaemon,
    registered: Mutex<Vec<String>>,
}

impl MdnsDaemon {
    /// A daemon on every interface.
    pub fn new() -> Result<Self> {
        let daemon = mdns_sd::ServiceDaemon::new().map_err(mdns_error)?;
        // `uncharted-universal` is 19 bytes, past RFC 6763's 15; under mdns-sd's default limit
        // `register` succeeds but the daemon drops the service with only a log line.
        let name_len = SERVICE.len() - "._tcp.local.".len() - 1;
        daemon
            .set_service_name_len_max(name_len as u8)
            .map_err(mdns_error)?;
        Ok(Self {
            daemon,
            registered: Mutex::new(Vec::new()),
        })
    }

    /// A daemon on the IPv4 loopback interface only, for a fake hub in CI.
    pub fn loopback_only() -> Result<Self> {
        let this = Self::new()?;
        this.daemon
            .disable_interface(mdns_sd::IfKind::All)
            .map_err(mdns_error)?;
        this.daemon
            .enable_interface(mdns_sd::IfKind::LoopbackV4)
            .map_err(mdns_error)?;
        Ok(this)
    }

    fn registered(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        self.registered
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Mdns for MdnsDaemon {
    fn register(&self, instance: &str, record: &Record) -> Result<()> {
        let host = format!("uncharted-universal-{instance}.local.");
        let txt: Vec<(&str, &str)> = record
            .txt
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let info = mdns_sd::ServiceInfo::new(
            SERVICE,
            instance,
            &host,
            record.addrs.as_slice(),
            record.port,
            txt.as_slice(),
        )
        .map_err(mdns_error)?;
        let info = if record.addrs.is_empty() {
            info.enable_addr_auto()
        } else {
            info
        };
        let fullname = info.get_fullname().to_string();
        self.daemon.register(info).map_err(mdns_error)?;
        let mut registered = self.registered();
        if !registered.contains(&fullname) {
            registered.push(fullname);
        }
        Ok(())
    }

    fn withdraw(&self, instance: &str) -> Result<()> {
        let fullname = format!("{instance}.{SERVICE}");
        let mut registered = self.registered();
        let Some(i) = registered.iter().position(|name| *name == fullname) else {
            return Ok(());
        };
        registered.remove(i);
        self.daemon.unregister(&fullname).map_err(mdns_error)?;
        Ok(())
    }

    fn browse(&self, wait: Duration) -> Result<Vec<Record>> {
        let deadline = Instant::now()
            .checked_add(wait)
            .ok_or_else(|| Error::Hub("discovery wait is too long".into()))?;
        let events = self.daemon.browse(SERVICE).map_err(mdns_error)?;
        let mut records = Vec::new();
        let result = loop {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break Ok(());
            };
            match events.recv_timeout(remaining) {
                Ok(mdns_sd::ServiceEvent::ServiceResolved(service)) => {
                    records.push(record_from(&service));
                }
                Ok(_) => {}
                Err(_) if events.is_disconnected() => {
                    break Err(Error::Hub("the mDNS daemon stopped".into()));
                }
                Err(_) => break Ok(()),
            }
        };
        let _ = self.daemon.stop_browse(SERVICE);
        result.map(|()| records)
    }
}

impl Drop for MdnsDaemon {
    fn drop(&mut self) {
        for fullname in self.registered().drain(..) {
            let _ = self.daemon.unregister(&fullname);
        }
        let _ = self.daemon.shutdown();
    }
}

fn record_from(service: &mdns_sd::ResolvedService) -> Record {
    let mut addrs: Vec<IpAddr> = service
        .get_addresses()
        .iter()
        .map(|addr| addr.to_ip_addr())
        .collect();
    addrs.sort();
    Record {
        txt: service
            .get_properties()
            .iter()
            .map(|p| (p.key().to_string(), p.val_str().to_string()))
            .collect(),
        addrs,
        port: service.get_port(),
    }
}

fn mdns_error(error: mdns_sd::Error) -> Error {
    Error::Hub(format!("mDNS: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(txt: &[(&str, &str)]) -> Vec<(String, String)> {
        txt.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_resolved_mdns_service_becomes_a_record() {
        let info = mdns_sd::ServiceInfo::new(
            SERVICE,
            "strix-halo",
            "uncharted-universal-strix-halo.local.",
            "192.168.1.20,8.8.8.8,::1",
            4710,
            [("hub", "strix-halo"), ("targets", "linux,android")].as_slice(),
        )
        .unwrap();
        let record = record_from(&info.as_resolved_service());
        assert_eq!(record.port, 4710);
        assert_eq!(
            parse_txt(&record.txt),
            Some((Hub::StrixHalo, vec![Os::Linux, Os::Android]))
        );
        let hubs = found([record]);
        let addrs: Vec<String> = hubs.iter().map(|f| f.addr.to_string()).collect();
        assert_eq!(addrs, ["192.168.1.20", "::1"]);
    }

    #[test]
    fn txt_keys_match_without_case_and_the_first_copy_wins() {
        let txt = pairs(&[
            ("HUB", "desktop"),
            ("hub", "mac"),
            ("Targets", "windows"),
            ("later", "ignored"),
        ]);
        assert_eq!(parse_txt(&txt), Some((Hub::Desktop, vec![Os::Windows])));
    }

    #[test]
    fn memory_mdns_replaces_and_withdraws_by_instance() {
        let net = MemoryMdns::new();
        let mut a = Announcement {
            hub: Hub::Desktop,
            targets: vec![Os::Windows],
            port: 4710,
            addrs: Vec::new(),
        };
        announce(&net, &a).unwrap();
        a.port = 4711;
        announce(&net, &a).unwrap();
        let hubs = discover_on(&net, 1).unwrap();
        assert_eq!(hubs.len(), 1);
        assert_eq!(hubs[0].port, 4711);
        assert_eq!(hubs[0].addr, IpAddr::V4(Ipv4Addr::LOCALHOST));
        withdraw(&net, Hub::Desktop).unwrap();
        withdraw(&net, Hub::Desktop).unwrap();
        assert!(discover_on(&net, 1).unwrap().is_empty());
    }
}
