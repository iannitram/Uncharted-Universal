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
//! - `UUHUB1 pull-game <target> <device>`: same pairing and target check, then answers `UUHUB1 ok`
//!   and runs the sealed prepared game-data stream (`serve_game`/`pull_game`), or
//!   `UUHUB1 unpaired` / `UUHUB1 no-game` and closes.
//!
//! The opening packet holds no secret and grants nothing: the request after `ok` is still sealed
//! with the key from pairing, so a device that names another device gets nothing. Connections
//! are taken one at a time, each with `SERVICE_IO_TIMEOUT` on every read and write.

use super::{
    newest_bundle, pair_client, pull_game, read_packet, read_peers, serve, serve_game,
    valid_device_name, write_packet, Clock, PairingServer, Peer, SystemClock, TcpTransport,
    Transport,
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
const HELLO_PULL_GAME: &str = "UUHUB1 pull-game ";
const REPLY_OK: &[u8] = b"UUHUB1 ok";
const REPLY_UNPAIRED: &[u8] = b"UUHUB1 unpaired";
const REPLY_NO_BUILD: &[u8] = b"UUHUB1 no-build";
const REPLY_NO_GAME: &[u8] = b"UUHUB1 no-game";

/// What one connection to the service did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Served {
    /// A device paired with the session's code (stored in the paired-device file).
    Paired(Peer),
    /// A paired device pulled `bundle`.
    Sent { device: String, bundle: Bundle },
    /// A paired device pulled prepared game data (`bytes` transferred).
    SentGame {
        device: String,
        target: Os,
        bytes: u64,
    },
}

/// The hub's side: one pairing session, the paired-device file, the builds folder, an optional
/// prepared game-data folder and the targets this hub serves (`recipe::serves`).
pub struct HubService<C: Clock = SystemClock> {
    pairing: PairingServer<C>,
    peers: PathBuf,
    builds: PathBuf,
    game_data: Option<PathBuf>,
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
            game_data: None,
            targets: targets.to_vec(),
        }
    }

    /// Also serve prepared game data from `game_data` (`serve_game`) for `UUHUB1 pull-game`.
    pub fn with_game_data(mut self, game_data: &Path) -> Self {
        self.game_data = Some(game_data.to_path_buf());
        self
    }

    /// The pairing code for the hub's screen ("483 921").
    pub fn pairing_code(&self) -> String {
        self.pairing.display_code()
    }

    /// True once the code paired a device or ran out of tries; pairing again needs a new service.
    pub fn pairing_spent(&self) -> bool {
        self.pairing.is_spent()
    }

    fn authorize_pull(
        &self,
        t: &mut dyn Transport,
        target: Os,
        device: &str,
        no_target_reply: &[u8],
    ) -> Result<Peer> {
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
            let _ = write_packet(t, no_target_reply, REPLY_LIMIT);
            return Err(Error::Hub(format!(
                "{device} asked for {}, which this hub does not serve",
                target.key()
            )));
        }
        Ok(peer)
    }

    /// One connection from `remote` over `t`: reads the opening packet and pairs or serves.
    /// A pull from a device that is not paired, or for a target this hub does not serve or has no
    /// build/game data for, is answered with the reason and fails here with it.
    pub fn handle(&mut self, remote: IpAddr, t: &mut dyn Transport) -> Result<Served> {
        let hello = read_packet(t, HELLO_LIMIT)?;
        if hello == HELLO_PAIR {
            return self.pairing.attempt(remote, t).map(Served::Paired);
        }
        if let Some((target, device)) = parse_pull_hello(HELLO_PULL_GAME, &hello) {
            let peer = self.authorize_pull(t, target, device, REPLY_NO_GAME)?;
            let Some(game_data) = self.game_data.as_deref().filter(|p| p.is_dir()) else {
                let _ = write_packet(t, REPLY_NO_GAME, REPLY_LIMIT);
                return Err(Error::Hub(format!(
                    "{} has no prepared game data ready for {}",
                    peer.device,
                    target.key()
                )));
            };
            write_packet(t, REPLY_OK, REPLY_LIMIT)?;
            let bytes = serve_game(t, &peer, game_data, target)?;
            return Ok(Served::SentGame {
                device: peer.device,
                target,
                bytes,
            });
        }
        let (target, device) = parse_pull_hello(HELLO_PULL, &hello)
            .ok_or_else(|| Error::Hub("a device sent an unknown request".into()))?;
        let peer = self.authorize_pull(t, target, device, REPLY_NO_BUILD)?;
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

fn parse_pull_hello<'a>(prefix: &str, hello: &'a [u8]) -> Option<(Os, &'a str)> {
    std::str::from_utf8(hello)
        .ok()
        .and_then(|text| text.strip_prefix(prefix))
        .and_then(|rest| rest.split_once(' '))
        .and_then(|(target, device)| Some((Os::from_key(target)?, device)))
        .filter(|(_, device)| valid_device_name(device))
}

fn connect(hub: SocketAddr) -> Result<TcpTransport> {
    let transport = TcpTransport::connect(hub, SERVICE_IO_TIMEOUT)?;
    transport.set_timeout(SERVICE_IO_TIMEOUT)?;
    Ok(transport)
}

fn open_pull_stream(
    hub: SocketAddr,
    hub_peer: &Peer,
    own_name: &str,
    prefix: &str,
    target: Os,
) -> Result<TcpTransport> {
    if !valid_device_name(own_name) {
        return Err(Error::Hub("the device name is invalid".into()));
    }
    let mut transport = connect(hub)?;
    let hello = format!("{prefix}{} {own_name}", target.key());
    write_packet(&mut transport, hello.as_bytes(), HELLO_LIMIT)?;
    let reply = read_packet(&mut transport, REPLY_LIMIT)?;
    if reply == REPLY_OK {
        Ok(transport)
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
    } else if reply == REPLY_NO_GAME {
        Err(Error::Hub(format!(
            "{} has no prepared game data ready for {}",
            hub_peer.device,
            target.key()
        )))
    } else {
        Err(Error::Hub("the hub sent an unexpected reply".into()))
    }
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
    let mut transport = open_pull_stream(hub, hub_peer, own_name, HELLO_PULL, target)?;
    super::pull(&mut transport, hub_peer, target, staged)
}

/// Pulls prepared game data for `target` from the hub service at `hub` into `staged`
/// (`pull_game`: resume, hash verification, atomic staging). `hub_peer` is this device's record
/// of the hub (from pairing); `own_name` is the name this device paired under.
pub fn pull_game_from_service(
    hub: SocketAddr,
    hub_peer: &Peer,
    own_name: &str,
    target: Os,
    staged: &Path,
) -> Result<u64> {
    let mut transport = open_pull_stream(hub, hub_peer, own_name, HELLO_PULL_GAME, target)?;
    pull_game(&mut transport, hub_peer, target, staged)
}
