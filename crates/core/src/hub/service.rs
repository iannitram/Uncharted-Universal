//! The hub service (roadmap 4.05): one TCP listener that takes both pairing and bundle pulls, so
//! a hub needs one port and one mDNS record. `uncharted-universal hub serve` runs it;
//! `hub pair` and `hub pull` are its clients (`pair_with_service`, `pull_from_service`).
//!
//! Each connection opens with one plain packet from the device:
//!
//! - `UUHUB1 pair`: the connection goes to the session's `PairingServer` (its limiter, lockout
//!   and one-time code, unchanged).
//! - `UUHUB1 pull <target> <device>`: the device names its target (`Os::key`) and the name the
//!   hub stored at pairing. The hub answers `UUHUB1 ok` and then runs the sealed bundle stream
//!   (`serve`/`pull`), or `UUHUB1 unpaired` / `UUHUB1 no-build` and closes.
//!
//! The opening packet holds no secret and grants nothing: the request after `ok` is still sealed
//! with the key from pairing, so a device that names another device gets nothing. Connections
//! are taken one at a time, each with `SERVICE_IO_TIMEOUT` on every read and write.

use super::{
    newest_bundle, pair_client, read_packet, read_peers, serve, valid_device_name, write_packet,
    Clock, PairingServer, Peer, SystemClock, TcpTransport, Transport,
};
use crate::bundle::Bundle;
use crate::error::{Error, Result};
use crate::platform::Os;
use std::net::{IpAddr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The TCP port a hub listens on unless told otherwise.
pub const PORT: u16 = 47_210;
/// How long one connection to the service may stall on a read or write before it fails.
pub const SERVICE_IO_TIMEOUT: Duration = Duration::from_secs(30);
const HELLO_LIMIT: usize = 256;
const REPLY_LIMIT: usize = 64;
const HELLO_PAIR: &[u8] = b"UUHUB1 pair";
const HELLO_PULL: &str = "UUHUB1 pull ";
const REPLY_OK: &[u8] = b"UUHUB1 ok";
const REPLY_UNPAIRED: &[u8] = b"UUHUB1 unpaired";
const REPLY_NO_BUILD: &[u8] = b"UUHUB1 no-build";

/// What one connection to the service did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Served {
    /// A device paired with the session's code (stored in the paired-device file).
    Paired(Peer),
    /// A paired device pulled `bundle`.
    Sent { device: String, bundle: Bundle },
}

/// The hub's side: one pairing session, the paired-device file, the builds folder and the
/// targets this hub serves (`recipe::serves`).
pub struct HubService<C: Clock = SystemClock> {
    pairing: PairingServer<C>,
    peers: PathBuf,
    builds: PathBuf,
    targets: Vec<Os>,
}

impl<C: Clock> HubService<C> {
    /// A service for `targets` that pairs with `pairing`'s code and serves the builds published
    /// under `builds` (`builds/<build name>/bundle.txt`) to the devices stored in `peers`.
    /// `peers` should be the file `pairing` stores into.
    pub fn new(pairing: PairingServer<C>, peers: &Path, builds: &Path, targets: &[Os]) -> Self {
        Self {
            pairing,
            peers: peers.to_path_buf(),
            builds: builds.to_path_buf(),
            targets: targets.to_vec(),
        }
    }

    /// The pairing code for the hub's screen ("483 921").
    pub fn pairing_code(&self) -> String {
        self.pairing.display_code()
    }

    /// True once the code paired a device or ran out of tries; pairing again needs a new service.
    pub fn pairing_spent(&self) -> bool {
        self.pairing.is_spent()
    }

    /// One connection from `remote` over `t`: reads the opening packet and pairs or serves.
    /// A pull from a device that is not paired, or for a target this hub does not serve or has no
    /// build for, is answered with the reason and fails here with it.
    pub fn handle(&mut self, remote: IpAddr, t: &mut dyn Transport) -> Result<Served> {
        let hello = read_packet(t, HELLO_LIMIT)?;
        if hello == HELLO_PAIR {
            return self.pairing.attempt(remote, t).map(Served::Paired);
        }
        let (target, device) = std::str::from_utf8(&hello)
            .ok()
            .and_then(|text| text.strip_prefix(HELLO_PULL))
            .and_then(|rest| rest.split_once(' '))
            .and_then(|(target, device)| Some((Os::from_key(target)?, device)))
            .filter(|(_, device)| valid_device_name(device))
            .ok_or_else(|| Error::Hub("a device sent an unknown request".into()))?;
        let Some(peer) = read_peers(&self.peers)?
            .into_iter()
            .find(|peer| peer.device == device)
        else {
            let _ = write_packet(t, REPLY_UNPAIRED, REPLY_LIMIT);
            return Err(Error::Hub(format!(
                "{device} asked for a build but is not paired with this hub"
            )));
        };
        if !self.targets.contains(&target) {
            let _ = write_packet(t, REPLY_NO_BUILD, REPLY_LIMIT);
            return Err(Error::Hub(format!(
                "{device} asked for {}, which this hub does not serve",
                target.key()
            )));
        }
        if let Err(error) = newest_bundle(&self.builds, target) {
            let _ = write_packet(t, REPLY_NO_BUILD, REPLY_LIMIT);
            return Err(error);
        }
        write_packet(t, REPLY_OK, REPLY_LIMIT)?;
        let bundle = serve(t, &peer, &self.builds, target)?;
        Ok(Served::Sent {
            device: peer.device,
            bundle,
        })
    }

    /// Takes the next connection on `listener` and handles it. A connection from outside the
    /// home network is refused unread (the error says so); the caller decides whether to go on.
    pub fn accept(&mut self, listener: &TcpListener) -> Result<Served> {
        let (stream, remote) = listener.accept()?;
        let mut transport = TcpTransport::from_stream(stream)?;
        transport.set_timeout(SERVICE_IO_TIMEOUT)?;
        self.handle(remote.ip(), &mut transport)
    }
}

fn connect(hub: SocketAddr) -> Result<TcpTransport> {
    let transport = TcpTransport::connect(hub, SERVICE_IO_TIMEOUT)?;
    transport.set_timeout(SERVICE_IO_TIMEOUT)?;
    Ok(transport)
}

/// The device's side of pairing through a hub service at `hub` (home network only): the code
/// the player typed (spaces and dashes allowed) is checked before connecting, and the hub is
/// stored in `peers` under the name it sent. Returns that record.
pub fn pair_with_service(
    hub: SocketAddr,
    code: &str,
    own_name: &str,
    peers: &Path,
) -> Result<Peer> {
    let code = super::parse_pairing_code(code)?;
    if !valid_device_name(own_name) {
        return Err(Error::Hub("the device name is invalid".into()));
    }
    let mut transport = connect(hub)?;
    write_packet(&mut transport, HELLO_PAIR, HELLO_LIMIT)?;
    pair_client(&mut transport, &code, own_name, peers)
}

/// Pulls the newest build for `target` from the hub service at `hub` into `staged` (`pull`:
/// resume, then `bundle::verify`). `hub_peer` is this device's record of the hub (from pairing);
/// `own_name` is the name this device paired under, which the hub looks its key up by.
pub fn pull_from_service(
    hub: SocketAddr,
    hub_peer: &Peer,
    own_name: &str,
    target: Os,
    staged: &Path,
) -> Result<Bundle> {
    if !valid_device_name(own_name) {
        return Err(Error::Hub("the device name is invalid".into()));
    }
    let mut transport = connect(hub)?;
    let hello = format!("{HELLO_PULL}{} {own_name}", target.key());
    write_packet(&mut transport, hello.as_bytes(), HELLO_LIMIT)?;
    let reply = read_packet(&mut transport, REPLY_LIMIT)?;
    if reply == REPLY_OK {
        super::pull(&mut transport, hub_peer, target, staged)
    } else if reply == REPLY_UNPAIRED {
        Err(Error::Hub(format!(
            "{} does not know this device as {own_name}; pair with it again",
            hub_peer.device
        )))
    } else if reply == REPLY_NO_BUILD {
        Err(Error::Hub(format!(
            "{} has no build ready for {}",
            hub_peer.device,
            target.key()
        )))
    } else {
        Err(Error::Hub("the hub sent an unexpected reply".into()))
    }
}
