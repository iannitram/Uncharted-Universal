//! The build hub on the home network (SPEC.md M11, M12; ADR 0004): find a hub, pair with it once,
//! serve and pull bundles. Behind the feature `hub`, so the core builds and tests with no network.
//! Nothing here reads the dump, keys or a save; it moves only what `bundle` lists. Home network
//! only: an address outside the private ranges is refused. New module; the shape is copied from
//! iannitram/totk-native's `hub.rs` (MIT). The crates it will need (`mdns-sd`, RustCrypto
//! `spake2` and `chacha20poly1305`, `sha2`) are added by the hub issues, not here.

use crate::bundle::Bundle;
use crate::error::{not_built, Result};
use crate::platform::Os;
use std::net::IpAddr;
use std::path::Path;

/// The mDNS service a hub announces.
pub const SERVICE: &str = "_uncharted-universal._tcp.local.";

/// A hub found on the home network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// `recipe::Hub::key()`.
    pub hub: String,
    pub addr: IpAddr,
    pub port: u16,
    pub targets: Vec<Os>,
}

/// A device paired with this one, as kept in `<config>/uncharted-universal/peers.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub device: String,
    /// Hex of the key both sides derived at pairing.
    pub key_hex: String,
}

/// A byte stream to a peer: TCP on the home network, or an in-memory pair in tests.
pub trait Transport: std::io::Read + std::io::Write {}

/// True for loopback, 10/8, 172.16/12, 192.168/16, 169.254/16, IPv6 loopback, fc00::/7 and
/// fe80::/10; false for anything else. Every connection checks it first.
pub fn is_home_network(addr: IpAddr) -> bool {
    not_built!("hub::is_home_network")
}

/// Hubs announcing `SERVICE`, waited for up to `wait_ms`.
pub fn discover(wait_ms: u64) -> Result<Vec<Found>> {
    not_built("hub::discover")
}

/// Pairs over `t` with the six-digit `code` shown on both screens (SPAKE2); both sides derive
/// the same key, or it is `Error::Hub("the pairing code did not match")`.
pub fn pair(t: &mut dyn Transport, code: &str, device: &str) -> Result<Peer> {
    not_built("hub::pair")
}

/// Sends the newest bundle for `target` and its files from `builds` to a paired peer, sealed.
pub fn serve(t: &mut dyn Transport, peer: &Peer, builds: &Path, target: Os) -> Result<Bundle> {
    not_built("hub::serve")
}

/// Pulls the newest bundle for `target` from a paired hub into `staged`, resuming files already
/// there, and checks it (`bundle::verify`) before returning.
pub fn pull(t: &mut dyn Transport, peer: &Peer, target: Os, staged: &Path) -> Result<Bundle> {
    not_built("hub::pull")
}
