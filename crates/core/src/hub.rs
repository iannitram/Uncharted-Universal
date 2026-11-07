//! The build hub on the home network (SPEC.md M11, M12; ADR 0004): find a hub, pair with it once,
//! serve and pull bundles. Behind the feature `hub`, so the core builds and tests with no network.
//! Nothing here reads the dump, keys or a save; it moves only what `bundle` lists. Home network
//! only: an address outside the private ranges is refused. New module; the shape is copied from
//! iannitram/totk-native's `hub.rs` (MIT). The crates it will need (`mdns-sd`, RustCrypto
//! `spake2` and `chacha20poly1305`, `sha2`) are added by the hub issues, not here.

use crate::bundle::Bundle;
use crate::error::{not_built, Result};
use crate::platform::Os;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

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

/// A TCP stream which has passed the address allowlist before application bytes are read/written.
pub struct TcpTransport(TcpStream);

impl TcpTransport {
    /// Connect only to a loopback, RFC1918, IPv6 ULA, or link-local address.
    pub fn connect(addr: SocketAddr, timeout: Duration) -> Result<Self> {
        if !is_home_network(addr.ip()) {
            return Err(crate::error::Error::Hub(
                "refusing a connection outside the home network".into(),
            ));
        }
        Ok(Self(TcpStream::connect_timeout(&addr, timeout)?))
    }

    /// Wrap an accepted socket only after checking its remote address. Call this before handing
    /// the stream to any protocol parser so a public peer cannot send application data.
    pub fn from_stream(stream: TcpStream) -> Result<Self> {
        let addr = stream.peer_addr()?;
        if !is_home_network(addr.ip()) {
            return Err(crate::error::Error::Hub(
                "refusing a connection outside the home network".into(),
            ));
        }
        Ok(Self(stream))
    }
}

impl std::io::Read for TcpTransport {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.0, buf)
    }
}

impl std::io::Write for TcpTransport {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        std::io::Write::write(&mut self.0, buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::Write::flush(&mut self.0)
    }
}

impl Transport for TcpTransport {}

/// True for loopback, 10/8, 172.16/12, 192.168/16, 169.254/16, IPv6 loopback, fc00::/7 and
/// fe80::/10; false for anything else. Every connection checks it first.
pub fn is_home_network(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(ip) => {
            let [a, b, _, _] = ip.octets();
            ip.is_loopback()
                || ip.is_link_local()
                || a == 10
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return is_home_network(IpAddr::V4(v4));
            }
            let s = ip.segments();
            ip.is_loopback()
                || (s[0] & 0xfe00) == 0xfc00 // fc00::/7
                || (s[0] & 0xffc0) == 0xfe80 // fe80::/10
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr, TcpListener};

    #[test]
    fn private_home_address_ranges_are_classified() {
        for address in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::new(10, 2, 3, 4)),
            IpAddr::V4(Ipv4Addr::new(172, 16, 9, 1)),
            IpAddr::V4(Ipv4Addr::new(172, 31, 9, 1)),
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
            IpAddr::V4(Ipv4Addr::new(169, 254, 1, 2)),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            "fc00::1".parse().unwrap(),
            "fe80::1".parse().unwrap(),
        ] {
            assert!(is_home_network(address), "{address}");
        }
        for address in [
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            IpAddr::V4(Ipv4Addr::new(172, 15, 9, 1)),
            IpAddr::V4(Ipv4Addr::new(172, 32, 9, 1)),
            IpAddr::V4(Ipv4Addr::new(192, 167, 1, 2)),
            IpAddr::V6("2001:4860:4860::8888".parse().unwrap()),
        ] {
            assert!(!is_home_network(address), "{address}");
        }
    }

    #[test]
    fn loopback_transport_checks_before_accepting_protocol_io() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let client =
            std::thread::spawn(move || TcpTransport::connect(addr, Duration::from_secs(2)));
        let (accepted, _) = listener.accept().unwrap();
        let mut server = TcpTransport::from_stream(accepted).unwrap();
        let mut client = client.join().unwrap().unwrap();
        std::io::Write::write_all(&mut client, b"loopback").unwrap();
        let mut received = [0; 8];
        std::io::Read::read_exact(&mut server, &mut received).unwrap();
        assert_eq!(&received, b"loopback");
    }
}
