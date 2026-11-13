//! The build hub on the home network (SPEC.md M11, M12; ADR 0004): find a hub, pair with it once,
//! serve and pull bundles. Behind the feature `hub`, so the core builds and tests with no network.
//! Nothing here reads the dump, keys or a save; it moves only what `bundle` lists. Home network
//! only: an address outside the private ranges is refused. The shape is copied from
//! iannitram/totk-native's `hub.rs` (MIT); SPAKE2 and RustCrypto AEAD are used for pairing and
//! game-data transfer.
//!
//! Pairing (ROADMAP 4.02): the hub makes a random six-digit code (`new_pairing_code`) and shows
//! it (`format_pairing_code`); the player types it on the client (`parse_pairing_code`). The hub
//! runs a `PairingServer`, whose accept loop keeps one `PairingLimiter` per remote address and
//! stores the new `Peer` in `peers.json`; the client runs `pair_with_hub`, which stores the hub.
//! `remove_peer` forgets a device. mDNS discovery and the announce side live in `hub/discovery.rs`
//! (roadmap 4.01; `mdns-sd`).
//!
//! Bundles (ROADMAP 4.03): a hub publishes each build as `builds/<build name>/bundle.txt` beside
//! its files; `serve` sends the newest bundle for a target and the files it lists, shader
//! translations included, over the same sealed channel and per-file exchange as the game-data
//! copy (`serve_game`/`pull_game`, #189). `pull` stages them with resume and runs
//! `bundle::verify` before returning.
//!
//! Service (ROADMAP 4.05): `HubService` in `hub/service.rs` takes pairing and bundle pulls on
//! one TCP port behind a plain opening packet; `pair_with_service` and `pull_from_service` are
//! the device's side. `uncharted-universal hub serve|pair|pull` runs them.

use crate::bundle::{self, Bundle};
use crate::error::{Error, Result};
use crate::platform::Os;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, io};

mod discovery;
pub use discovery::{
    announce, discover, discover_on, found, parse_txt, txt, withdraw, Announcement, Mdns,
    MdnsDaemon, MemoryMdns, Record, TXT_HUB, TXT_TARGETS,
};
mod service;
pub use service::{
    pair_with_service, pull_from_service, HubService, Served, PORT, SERVICE_IO_TIMEOUT,
};

/// The mDNS service a hub announces.
pub const SERVICE: &str = "_uncharted-universal._tcp.local.";
const MAX_CONTROL_FRAME: usize = 1024 * 1024;
const DATA_CHUNK: usize = 64 * 1024;
const MAX_GAME_FILES: usize = 10_000;
const MAX_GAME_MANIFEST: usize = 1024 * 1024;
const MAX_GAME_FILE: u64 = 32 * 1024 * 1024 * 1024;
const MAX_GAME_TOTAL: u64 = 100 * 1024 * 1024 * 1024;
/// The bundle text of a published build: `builds/<build name>/bundle.txt`.
pub const BUNDLE_TEXT: &str = "bundle.txt";
const STAGING_MARKER: &str = ".uu-transfer-incomplete";
const STAGING_LOCK: &str = ".uu-transfer.lock";
const TRANSFER_WORK_DIR: &str = ".uu-transfer-work";
const TRANSFER_WORK_OWNER: &str = ".owner";
const TRANSFER_WORK_OWNER_CONTENT: &[u8] = b"uncharted-universal-transfer-work-v1\n";
const PAIRING_ID: &[u8] = b"uncharted-universal-hub-pairing-v2";
const PAIRING_MISMATCH: &str = "the pairing code did not match";
const PAIRING_LOCKED: &str = "too many pairing attempts; try again in five minutes";
const PAIRING_SPENT: &str = "this pairing code is used up; start pairing again with a new code";
const PAIRING_READY: &[u8] = b"UUPAIR2 ready";
const PAIRING_REFUSED_LOCKED: &[u8] = b"UUPAIR2 locked";
const PAIRING_REFUSED_SPENT: &[u8] = b"UUPAIR2 spent";
/// Wrong codes from one remote address before it is locked out.
pub const MAX_FAILED_ATTEMPTS: u8 = 3;
/// How long a remote address stays locked out after `MAX_FAILED_ATTEMPTS` wrong codes.
pub const PAIRING_LOCKOUT: Duration = Duration::from_secs(5 * 60);
/// Wrong codes from all remote addresses together before a `PairingServer`'s code is spent. It
/// bounds a guesser that changes address (IPv6 privacy addresses) to ten tries in a million.
pub const MAX_TOTAL_FAILED_ATTEMPTS: u32 = 10;
/// How long one pairing exchange may stall on a read or write before it fails (and counts).
pub const PAIRING_IO_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_DEVICE_NAME: usize = 128;
const TRANSFER_DOMAIN: &[u8] = b"uncharted-universal-game-transfer-v1";
const CLIENT_TO_SERVER: u8 = 1;
const SERVER_TO_CLIENT: u8 = 2;

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
#[derive(Clone, PartialEq, Eq)]
pub struct Peer {
    /// The other device's name, as it sent it at pairing.
    pub device: String,
    /// Hex of the key both sides derived at pairing.
    key_hex: String,
}

/// Clock seam for deterministic pairing-rate-limit tests and app integrations.
pub trait Clock {
    fn now(&self) -> std::time::Duration;
}

/// Wall clock implementation for pairing attempts.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> std::time::Duration {
        static PROCESS_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        PROCESS_START.get_or_init(std::time::Instant::now).elapsed()
    }
}

/// Per-remote-device pairing attempt budget. Keep one limiter for each remote address
/// (`PairingServer` does); after `MAX_FAILED_ATTEMPTS` wrong codes that remote is blocked for
/// `PAIRING_LOCKOUT`.
#[derive(Debug, Clone, Default)]
pub struct PairingLimiter {
    failures: u8,
    locked_until: Option<std::time::Duration>,
}

impl PairingLimiter {
    pub fn check(&mut self, now: std::time::Duration) -> Result<()> {
        if let Some(until) = self.locked_until {
            if now < until {
                return Err(Error::Hub(PAIRING_LOCKED.into()));
            }
            self.failures = 0;
            self.locked_until = None;
        }
        Ok(())
    }

    fn failed(&mut self, now: std::time::Duration) {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= MAX_FAILED_ATTEMPTS {
            self.locked_until = Some(now + PAIRING_LOCKOUT);
        }
    }

    fn reset(&mut self) {
        self.failures = 0;
        self.locked_until = None;
    }
}

impl std::fmt::Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Peer")
            .field("device", &self.device)
            .field("key_hex", &"[REDACTED]")
            .finish()
    }
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

    /// Bound every read and write on this connection, so a stalled peer cannot hold it open.
    pub fn set_timeout(&self, timeout: Duration) -> Result<()> {
        self.0.set_read_timeout(Some(timeout))?;
        self.0.set_write_timeout(Some(timeout))?;
        Ok(())
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

/// A random six-digit pairing code from the operating system's random source, uniform over
/// 000000-999999. The hub makes a new one for each pairing session and shows it on its screen.
pub fn new_pairing_code() -> Result<String> {
    loop {
        let mut word = [0u8; 4];
        getrandom::getrandom(&mut word).map_err(|_| {
            Error::Hub("the operating system could not provide random bytes".into())
        })?;
        if let Some(code) = pairing_code_from(u32::from_be_bytes(word)) {
            return Ok(code);
        }
    }
}

/// One random word as a code, or `None` for the top sliver of words that would favour low codes.
fn pairing_code_from(word: u32) -> Option<String> {
    // The largest multiple of a million that fits in a u32.
    const LIMIT: u32 = 4_294_000_000;
    (word < LIMIT).then(|| format!("{:06}", word % 1_000_000))
}

/// The code as a screen shows it, in two groups of three: `"483 921"`.
pub fn format_pairing_code(code: &str) -> Result<String> {
    let code = parse_pairing_code(code)?;
    Ok(format!("{} {}", &code[..3], &code[3..]))
}

/// The code as the player typed it on the client: six digits, with any spaces or dashes dropped.
pub fn parse_pairing_code(input: &str) -> Result<String> {
    let code: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::Hub(
            "the pairing code must contain six digits".into(),
        ));
    }
    Ok(code)
}

fn valid_device_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_DEVICE_NAME && !name.chars().any(char::is_control)
}

fn check_pairing_input(code: &str, own_name: &str) -> Result<()> {
    if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::Hub(
            "the pairing code must contain six digits".into(),
        ));
    }
    if !valid_device_name(own_name) {
        return Err(Error::Hub("the device name is invalid".into()));
    }
    Ok(())
}

fn pairing_mismatch() -> Error {
    Error::Hub(PAIRING_MISMATCH.into())
}

/// Pairs over `t` with the six-digit `code` shown on both screens (SPAKE2). Each side sends its
/// own name; both confirm the key and the sender's name in both directions, then acknowledge the
/// other's confirmation, so a damaged confirmation leaves neither side remembering the other.
/// Returns the remote device, under the name it sent, with the key both sides derived.
/// Internal; callers go through `pair_limited`, `PairingServer` or `pair_client`.
fn pair_inner(t: &mut dyn Transport, code: &str, own_name: &str) -> Result<Peer> {
    check_pairing_input(code, own_name)?;

    let (state, outbound) = Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(code.as_bytes()),
        &Identity::new(PAIRING_ID),
    );
    write_packet(t, &outbound, 256)?;
    let inbound = read_packet(t, 256)?;
    // A reflected message would make this side pair with itself.
    if inbound == outbound {
        return Err(pairing_mismatch());
    }
    let shared = state.finish(&inbound).map_err(|_| pairing_mismatch())?;
    let mut key_hash = Sha256::new();
    key_hash.update(b"uncharted-universal peer key v2\0");
    key_hash.update(&shared);
    let key: [u8; 32] = key_hash.finalize().into();

    // Confirm possession in both directions with independent random challenges; a reflected
    // challenge is refused so one side cannot accept its own confirmation as the peer's.
    let mut challenge = [0u8; 32];
    getrandom::getrandom(&mut challenge)
        .map_err(|_| Error::Hub("the operating system could not provide random bytes".into()))?;
    write_packet(t, &challenge, 64)?;
    let peer_challenge = read_packet(t, 64)?;
    if peer_challenge.len() != 32 {
        return Err(Error::Hub(
            "the peer sent an invalid pairing challenge".into(),
        ));
    }
    if peer_challenge == challenge {
        return Err(pairing_mismatch());
    }

    write_packet(t, own_name.as_bytes(), MAX_DEVICE_NAME)?;
    let peer_name = String::from_utf8(read_packet(t, MAX_DEVICE_NAME)?)
        .ok()
        .filter(|name| valid_device_name(name))
        .ok_or_else(|| Error::Hub("the peer sent an invalid device name".into()))?;

    let own_tag = pairing_mac(&key, b"peer confirmation v2\0", &peer_challenge, own_name)?
        .finalize()
        .into_bytes();
    write_packet(t, &own_tag, 64)?;
    let peer_tag = read_packet(t, 64)?;
    let confirmed = pairing_mac(&key, b"peer confirmation v2\0", &challenge, &peer_name)?
        .verify_slice(&peer_tag)
        .is_ok();

    // Acknowledge the peer's confirmation; a failed check sends no valid acknowledgement.
    let own_ack: Vec<u8> = if confirmed {
        pairing_mac(&key, b"peer accepted v2\0", &peer_challenge, own_name)?
            .finalize()
            .into_bytes()
            .to_vec()
    } else {
        vec![0; 32]
    };
    write_packet(t, &own_ack, 64)?;
    let peer_ack = read_packet(t, 64)?;
    let acknowledged = pairing_mac(&key, b"peer accepted v2\0", &challenge, &peer_name)?
        .verify_slice(&peer_ack)
        .is_ok();
    if !confirmed || !acknowledged {
        return Err(pairing_mismatch());
    }

    Ok(Peer {
        device: peer_name,
        key_hex: hex(&key),
    })
}

/// Rate-limited pairing exchange. `own_name` is this device's name, sent to the peer; the returned
/// `Peer` carries the name the peer sent. The caller keeps `limiter` attached to the remote
/// address for the full lockout window (`PairingServer` does). Every failed exchange counts:
/// a wrong code, a dropped connection, a timeout or a malformed packet, because a guesser could
/// otherwise check its guess against this side's confirmation and hang up unseen.
pub fn pair_limited(
    t: &mut dyn Transport,
    code: &str,
    own_name: &str,
    limiter: &mut PairingLimiter,
    clock: &dyn Clock,
) -> Result<Peer> {
    check_pairing_input(code, own_name)?;
    let now = clock.now();
    limiter.check(now)?;
    let result = pair_inner(t, code, own_name);
    match &result {
        Ok(_) => limiter.reset(),
        Err(_) => limiter.failed(now),
    }
    result
}

/// The hub's side of pairing: one code for the hub's screen and the accept loop that guards it.
/// It keeps one `PairingLimiter` per remote address for the whole session, so a remote that sent
/// `MAX_FAILED_ATTEMPTS` wrong codes is refused for `PAIRING_LOCKOUT` before any key exchange
/// and is told why. The code is spent by one pairing, or by `MAX_TOTAL_FAILED_ATTEMPTS` wrong codes
/// from all addresses together; a new session needs a new code. The new peer is stored in the
/// paired-device file, normally `<AppDirs.config>/peers.json`.
pub struct PairingServer<C: Clock = SystemClock> {
    code: String,
    own_name: String,
    peers: PathBuf,
    clock: C,
    limiters: HashMap<IpAddr, PairingLimiter>,
    failures: u32,
    spent: bool,
}

impl PairingServer<SystemClock> {
    /// A session with a new random code (`new_pairing_code`), timed by the system clock.
    pub fn new(own_name: &str, peers: &Path) -> Result<Self> {
        Self::with_code(&new_pairing_code()?, own_name, peers, SystemClock)
    }
}

impl<C: Clock> PairingServer<C> {
    /// A session for `code`, timed by `clock` (tests inject their own).
    pub fn with_code(code: &str, own_name: &str, peers: &Path, clock: C) -> Result<Self> {
        let code = parse_pairing_code(code)?;
        check_pairing_input(&code, own_name)?;
        Ok(Self {
            code,
            own_name: own_name.to_string(),
            peers: peers.to_path_buf(),
            clock,
            limiters: HashMap::new(),
            failures: 0,
            spent: false,
        })
    }

    /// The code for the hub's screen, as `format_pairing_code` shows it.
    pub fn display_code(&self) -> String {
        format!("{} {}", &self.code[..3], &self.code[3..])
    }

    /// True once the code paired a device or ran out of tries.
    pub fn is_spent(&self) -> bool {
        self.spent
    }

    /// One pairing attempt from `remote` over `t`. A locked-out remote, or any remote once the
    /// code is spent, is told so and refused before any key exchange. On success the peer is
    /// stored in the paired-device file and the code is spent.
    pub fn attempt(&mut self, remote: IpAddr, t: &mut dyn Transport) -> Result<Peer> {
        let remote = canonical_ip(remote);
        if self.spent {
            let _ = write_packet(t, PAIRING_REFUSED_SPENT, 64);
            return Err(Error::Hub(PAIRING_SPENT.into()));
        }
        let now = self.clock.now();
        if let Some(limiter) = self.limiters.get_mut(&remote) {
            if let Err(error) = limiter.check(now) {
                let _ = write_packet(t, PAIRING_REFUSED_LOCKED, 64);
                return Err(error);
            }
        }
        write_packet(t, PAIRING_READY, 64)?;
        let mut limiter = self.limiters.remove(&remote).unwrap_or_default();
        match pair_limited(t, &self.code, &self.own_name, &mut limiter, &self.clock) {
            Ok(peer) => {
                self.spent = true;
                remember_peer(&self.peers, &peer)?;
                Ok(peer)
            }
            Err(error) => {
                self.limiters.insert(remote, limiter);
                self.failures = self.failures.saturating_add(1);
                if self.failures >= MAX_TOTAL_FAILED_ATTEMPTS {
                    self.spent = true;
                }
                Err(error)
            }
        }
    }

    /// The accept loop: takes connections on `listener` until one pairs (returning that peer) or
    /// the code is spent (returning why). A connection from outside the home network is dropped
    /// unread; each exchange gets `PAIRING_IO_TIMEOUT` per read or write. Failed attempts are
    /// counted and the loop goes on.
    pub fn serve(&mut self, listener: &TcpListener) -> Result<Peer> {
        loop {
            let (stream, remote) = listener.accept()?;
            let Ok(mut transport) = TcpTransport::from_stream(stream) else {
                continue;
            };
            if transport.set_timeout(PAIRING_IO_TIMEOUT).is_err() {
                continue;
            }
            match self.attempt(remote.ip(), &mut transport) {
                Ok(peer) => return Ok(peer),
                Err(error) if self.spent => return Err(error),
                Err(_) => continue,
            }
        }
    }
}

/// An IPv4-mapped IPv6 address as plain IPv4, so a dual-stack listener keeps one limiter per device.
fn canonical_ip(addr: IpAddr) -> IpAddr {
    match addr {
        IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or(addr, IpAddr::V4),
        IpAddr::V4(_) => addr,
    }
}

/// The client's side of pairing: connects to the hub at `hub` (home network only), pairs with the
/// code the player typed (spaces and dashes allowed), and stores the hub in the paired-device file
/// `peers`, normally `<AppDirs.config>/peers.json`. Returns the hub under the name it sent.
pub fn pair_with_hub(hub: SocketAddr, code: &str, own_name: &str, peers: &Path) -> Result<Peer> {
    let mut transport = TcpTransport::connect(hub, PAIRING_IO_TIMEOUT)?;
    transport.set_timeout(PAIRING_IO_TIMEOUT)?;
    pair_client(&mut transport, code, own_name, peers)
}

/// `pair_with_hub` over a transport the caller opened. A hub that has locked this device out, or
/// whose code is spent, says so before any key exchange, and nothing is stored.
pub fn pair_client(
    t: &mut dyn Transport,
    code: &str,
    own_name: &str,
    peers: &Path,
) -> Result<Peer> {
    let code = parse_pairing_code(code)?;
    check_pairing_input(&code, own_name)?;
    let hello = read_packet(t, 64)?;
    if hello == PAIRING_REFUSED_LOCKED {
        return Err(Error::Hub(PAIRING_LOCKED.into()));
    }
    if hello == PAIRING_REFUSED_SPENT {
        return Err(Error::Hub(PAIRING_SPENT.into()));
    }
    if hello != PAIRING_READY {
        return Err(Error::Hub(
            "the hub sent an unexpected pairing reply".into(),
        ));
    }
    let peer = pair_inner(t, &code, own_name)?;
    remember_peer(peers, &peer)?;
    Ok(peer)
}

/// Serializes read-modify-write of the paired-device file within this process.
static PEERS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Adds `peer` to the paired-device file at `path`, replacing an older record of the same device
/// (pairing again makes a new key). Fails closed where protected storage is unavailable.
pub fn remember_peer(path: &Path, peer: &Peer) -> Result<()> {
    let _guard = PEERS_LOCK
        .lock()
        .map_err(|_| Error::Hub("paired-device storage is busy".into()))?;
    let mut peers = read_peers(path)?;
    peers.retain(|old| old.device != peer.device);
    peers.push(peer.clone());
    write_peers(path, &peers)
}

/// Forgets (unpairs) `device` in the paired-device file at `path`, returning whether it was
/// remembered. The file is left untouched when it was not.
pub fn remove_peer(path: &Path, device: &str) -> Result<bool> {
    let _guard = PEERS_LOCK
        .lock()
        .map_err(|_| Error::Hub("paired-device storage is busy".into()))?;
    let mut peers = read_peers(path)?;
    let before = peers.len();
    peers.retain(|peer| peer.device != device);
    if peers.len() == before {
        return Ok(false);
    }
    write_peers(path, &peers)?;
    Ok(true)
}

/// Load paired devices from a protected `peers.json` file. Callers should normally pass
/// `<AppDirs.config>/peers.json`; the format contains secrets and is never suitable for logs.
pub fn read_peers(path: &Path) -> Result<Vec<Peer>> {
    #[cfg(windows)]
    {
        let _ = path;
        Err(Error::Hub(
            "protected paired-device storage is not implemented on Windows".into(),
        ))
    }
    #[cfg(not(windows))]
    read_peers_impl(path)
}

#[cfg(not(windows))]
fn read_peers_impl(path: &Path) -> Result<Vec<Peer>> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(Error::Hub(
            "paired-device storage is not a regular file".into(),
        ));
    }
    if !crate::platform::private_file_permissions(path)? {
        return Err(Error::Hub("paired-device storage is not private".into()));
    }
    let bytes = fs::read(path)?;
    if bytes.len() > 1024 * 1024 {
        return Err(Error::Hub("paired-device storage exceeds its limit".into()));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Hub("paired-device storage is malformed".into()))?;
    if value.get("version").and_then(|v| v.as_u64()) != Some(1) {
        return Err(Error::Hub(
            "paired-device storage has an unsupported version".into(),
        ));
    }
    let entries = value
        .get("peers")
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::Hub("paired-device storage is malformed".into()))?;
    if entries.len() > 256 {
        return Err(Error::Hub(
            "paired-device storage has too many peers".into(),
        ));
    }
    let mut peers = Vec::with_capacity(entries.len());
    for entry in entries {
        let device = entry
            .get("device")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Hub("paired-device storage is malformed".into()))?;
        let key_hex = entry
            .get("key_hex")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Hub("paired-device storage is malformed".into()))?;
        if device.is_empty()
            || device.len() > 128
            || device.chars().any(char::is_control)
            || key_hex.len() != 64
            || peer_key(&Peer {
                device: device.to_string(),
                key_hex: key_hex.to_string(),
            })
            .is_err()
            || peers.iter().any(|peer: &Peer| peer.device == device)
        {
            return Err(Error::Hub("paired-device storage is malformed".into()));
        }
        peers.push(Peer {
            device: device.to_string(),
            key_hex: key_hex.to_ascii_lowercase(),
        });
    }
    Ok(peers)
}

/// Atomically replace the per-user paired-device file. On Unix the new file is created mode
/// 0600 before rename. On Windows the destination ACL is inherited from the app's protected
/// configuration directory; replacement of an existing file may be unavailable in this build.
pub fn write_peers(path: &Path, peers: &[Peer]) -> Result<()> {
    #[cfg(windows)]
    {
        let _ = (path, peers);
        Err(Error::Hub(
            "protected paired-device storage is not implemented on Windows".into(),
        ))
    }
    #[cfg(not(windows))]
    write_peers_impl(path, peers)
}

#[cfg(not(windows))]
fn write_peers_impl(path: &Path, peers: &[Peer]) -> Result<()> {
    if peers.len() > 256 {
        return Err(Error::Hub(
            "paired-device storage has too many peers".into(),
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut entries = Vec::with_capacity(peers.len());
    for peer in peers {
        if peer.device.is_empty()
            || peer.device.len() > 128
            || peer.device.chars().any(char::is_control)
            || peer_key(peer).is_err()
            || !seen.insert(peer.device.clone())
        {
            return Err(Error::Hub("paired-device record is invalid".into()));
        }
        entries.push(serde_json::json!({
            "device": peer.device,
            "key_hex": peer.key_hex.to_ascii_lowercase(),
        }));
    }
    let bytes = serde_json::to_vec(&serde_json::json!({ "version": 1, "peers": entries }))
        .map_err(|_| Error::Hub("could not encode paired-device storage".into()))?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::Hub("paired-device path has no parent directory".into()))?;
    fs::create_dir_all(parent)?;
    let parent_meta = fs::symlink_metadata(parent)?;
    if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
        return Err(Error::Hub(
            "paired-device directory must be a real directory".into(),
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink()
                || !meta.is_file()
                || !crate::platform::private_file_permissions(path)?
            {
                return Err(Error::Hub("paired-device storage is not private".into()));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut random = [0u8; 8];
    getrandom::getrandom(&mut random)
        .map_err(|_| Error::Hub("the operating system could not provide random bytes".into()))?;
    let temp = parent.join(format!(".peers-{}.tmp", hex(&random)));
    let mut output = crate::platform::create_private_file(&temp)?;
    if let Err(error) = io::Write::write_all(&mut output, &bytes).and_then(|_| output.sync_all()) {
        drop(output);
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    drop(output);
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
}

/// Sends the newest bundle for `target` and its files from `builds` to a paired peer, sealed.
///
/// Publish each build as `builds/<build name>/bundle.txt` (`bundle::to_text`) with its files
/// beside it; the newest `bundle.txt` for `target` by modification time wins, then the folder
/// name. Only the files the bundle lists are read, so shader translations travel and a driver
/// pipeline cache (refused by `bundle::is_forbidden`) never does. `peer` is the record stored
/// at pairing: the request must be sealed with its key, which is checked before `builds` is
/// read. Every file is checked against the bundle before the manifest goes out and again,
/// by hash, just before it is sent.
pub fn serve(t: &mut dyn Transport, peer: &Peer, builds: &Path, target: Os) -> Result<Bundle> {
    let key = peer_key(peer)?;
    let (session, request) = read_sealed(t, &key, None, CLIENT_TO_SERVER, 0)?;
    let mut channel = SealedChannel::server(t, key, session);
    if request != bundle_request(target).as_bytes() {
        return Err(Error::Hub("the bundle request was not authorized".into()));
    }

    let (bundle, root) = newest_bundle(builds, target)?;
    let files = bundle_transfer_files(&bundle)?;
    for file in &files {
        regular_file_in_tree(&root, &file.path)?;
        if fs::metadata(root.join(&file.path))?.len() != file.size {
            return Err(Error::Hub(format!(
                "{} does not match the bundle",
                file.path
            )));
        }
    }
    channel.send(bundle::to_text(&bundle).as_bytes())?;
    send_files(&mut channel, &root, &files)?;
    channel.send(b"DONE")?;
    Ok(bundle)
}

/// Pulls the newest bundle for `target` from a paired hub into `staged`, resuming files already
/// there, and checks it (`bundle::verify`) before returning.
///
/// The manifest must parse as a bundle for `target` with only allowed paths. A file already
/// staged with the listed size and hash is skipped; any other is received into the reserved work
/// directory, hash-checked and only then moved into place. `.uu-transfer-incomplete` stays in
/// `staged` until every file has arrived and `bundle::verify` has passed, so `bundle::install`
/// refuses a partial pull. Files in `staged` that the bundle does not list are left alone: use
/// one staging folder per pull.
pub fn pull(t: &mut dyn Transport, peer: &Peer, target: Os, staged: &Path) -> Result<Bundle> {
    let key = peer_key(peer)?;
    prepare_staging_root(staged)?;
    let _lock = acquire_staging_lock(staged)?;
    let marker = mark_staging_incomplete(staged)?;
    let mut session = [0u8; 16];
    getrandom::getrandom(&mut session)
        .map_err(|_| Error::Hub("the operating system could not provide random bytes".into()))?;
    let mut channel = SealedChannel::client(t, key, session);
    channel.send(bundle_request(target).as_bytes())?;
    let manifest = channel.receive()?;
    if manifest.len() > MAX_GAME_MANIFEST {
        return Err(Error::Hub("hub bundle manifest exceeds its limit".into()));
    }
    let text = std::str::from_utf8(&manifest)
        .map_err(|_| Error::Hub("hub bundle manifest is not valid text".into()))?;
    let bundle = bundle::from_text(text)?;
    if bundle.target != target {
        return Err(Error::Hub(
            "hub bundle manifest did not match the request".into(),
        ));
    }
    let files = bundle_transfer_files(&bundle)?;
    let work_dir = prepare_transfer_work_dir(staged)?;
    cleanup_transfer_temps(&work_dir)?;
    receive_files(&mut channel, staged, &work_dir, &files)?;
    if channel.receive()? != b"DONE" {
        return Err(Error::Hub("the bundle transfer ended unexpectedly".into()));
    }
    finish_transfer_work_dir(&work_dir)?;
    bundle::verify(&bundle, staged)?;
    fs::remove_file(marker)?;
    Ok(bundle)
}

fn bundle_request(target: Os) -> String {
    format!("PULLBUNDLE1 {}", target.key())
}

/// The newest `builds/<build name>/bundle.txt` for `target`, parsed, and its build folder.
/// A symlinked bundle text is refused; so is a bundle that does not parse, since serving an
/// older build in its place would hide the broken one.
fn newest_bundle(builds: &Path, target: Os) -> Result<(Bundle, PathBuf)> {
    let meta = fs::symlink_metadata(builds)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(Error::Hub(
            "the builds folder is not a real directory".into(),
        ));
    }
    let mut candidates = Vec::new();
    for entry in fs::read_dir(builds)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let text = entry.path().join(BUNDLE_TEXT);
        match fs::symlink_metadata(&text) {
            Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
                candidates.push((meta.modified()?, entry.file_name(), entry.path()));
            }
            Ok(_) => {
                return Err(Error::Hub(format!(
                    "{} is not a regular file",
                    text.display()
                )))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    candidates.sort();
    for (_, _, root) in candidates.into_iter().rev() {
        let path = root.join(BUNDLE_TEXT);
        let mut text = String::new();
        io::Read::read_to_string(
            &mut io::Read::take(fs::File::open(&path)?, MAX_GAME_MANIFEST as u64 + 1),
            &mut text,
        )?;
        if text.len() > MAX_GAME_MANIFEST {
            return Err(Error::Hub(format!(
                "{} exceeds the bundle size limit",
                path.display()
            )));
        }
        let bundle = bundle::from_text(&text)?;
        if bundle.target == target {
            return Ok((bundle, root));
        }
    }
    Err(Error::Hub(format!(
        "no build is ready for {}",
        target.key()
    )))
}

/// A bundle's files as transfer entries, held to the same path, size and count rules as the
/// game-data copy (portable ASCII paths, no case-fold collisions, nothing sensitive).
fn bundle_transfer_files(b: &Bundle) -> Result<Vec<GameFile>> {
    if b.files.len() > MAX_GAME_FILES {
        return Err(Error::Hub("the bundle has too many files".into()));
    }
    let mut total = 0u64;
    let mut files = Vec::with_capacity(b.files.len());
    for file in &b.files {
        if bundle::is_forbidden(&file.path)
            || sensitive_game_path(&file.path)
            || !safe_transfer_path(&file.path)
            || !is_sha256(&file.sha256)
        {
            return Err(Error::Hub(format!(
                "refusing to transfer {}: game files, keys, saves and driver caches never leave a hub",
                file.path
            )));
        }
        total = total
            .checked_add(file.size)
            .ok_or_else(|| Error::Hub("bundle size overflowed".into()))?;
        if file.size > MAX_GAME_FILE || total > MAX_GAME_TOTAL {
            return Err(Error::Hub("the bundle exceeds the transfer limit".into()));
        }
        files.push(GameFile {
            path: file.path.clone(),
            size: file.size,
            sha256: file.sha256.clone(),
        });
    }
    validate_game_paths(&files)?;
    Ok(files)
}

/// Sends the prepared game-data tree to a client that proves possession of this paired peer key.
/// The caller must pass the peer record stored after `pair`; unpaired requests fail before files
/// are enumerated. Each encrypted frame has a random XChaCha nonce and a per-transfer session id.
/// Completed files can be resumed by hash; interrupted files are replaced only after verification.
pub fn serve_game(
    t: &mut dyn Transport,
    peer: &Peer,
    prepared_data: &Path,
    target: Os,
) -> Result<u64> {
    let key = peer_key(peer)?;
    let (session, request) = read_sealed(t, &key, None, CLIENT_TO_SERVER, 0)?;
    let mut channel = SealedChannel::server(t, key, session);
    let expected = format!("PULLGAME1 {}", target.key());
    if request != expected.as_bytes() {
        return Err(Error::Hub(
            "the game-data request was not authorized".into(),
        ));
    }

    let files = enumerate_game_files(prepared_data)?;
    let manifest = format_game_manifest(target, &files)?;
    channel.send(&manifest)?;
    let transferred = send_files(&mut channel, prepared_data, &files)?;
    channel.send(b"DONE")?;
    Ok(transferred)
}

/// Pulls prepared game data into a disposable `staged` directory. Only authenticated, encrypted metadata is accepted;
/// paths are ASCII-only, relative and symlink-free to avoid cross-platform Unicode normalization collisions,
/// each file is capped at 32 GiB and the aggregate at 100 GiB.
/// Existing regular files with the expected size/hash are skipped to resume completed files.
/// A marker remains until the entire transfer verifies; callers must not expose or install this
/// directory while `.uu-transfer-incomplete` exists.
pub fn pull_game(t: &mut dyn Transport, peer: &Peer, target: Os, staged: &Path) -> Result<u64> {
    let key = peer_key(peer)?;
    prepare_staging_root(staged)?;
    let _lock = acquire_staging_lock(staged)?;
    let marker = mark_staging_incomplete(staged)?;
    let mut session = [0u8; 16];
    getrandom::getrandom(&mut session)
        .map_err(|_| Error::Hub("the operating system could not provide random bytes".into()))?;
    let request = format!("PULLGAME1 {}", target.key());
    let mut channel = SealedChannel::client(t, key, session);
    channel.send(request.as_bytes())?;
    let manifest = channel.receive()?;
    let files = parse_game_manifest(&manifest, target)?;
    let work_dir = prepare_transfer_work_dir(staged)?;
    cleanup_transfer_temps(&work_dir)?;
    let received = receive_files(&mut channel, staged, &work_dir, &files)?;
    let done = channel.receive()?;
    if done != b"DONE" {
        return Err(Error::Hub(
            "the game-data transfer ended unexpectedly".into(),
        ));
    }
    finish_transfer_work_dir(&work_dir)?;
    fs::remove_file(marker)?;
    Ok(received)
}

/// The server half of the per-file exchange shared by the game-data copy and the bundle stream:
/// for each listed file the client answers `SKIP <i>` (already staged) or `NEED <i>`, and a
/// needed file is re-hashed, then sent as `DATA` chunks and `END <i>`. Returns the bytes sent.
fn send_files(channel: &mut SealedChannel<'_>, root: &Path, files: &[GameFile]) -> Result<u64> {
    let mut transferred = 0u64;
    for (index, file) in files.iter().enumerate() {
        let command = channel.receive()?;
        if command == format!("SKIP {index}").as_bytes() {
            continue;
        }
        if command != format!("NEED {index}").as_bytes() {
            return Err(Error::Hub("the transfer request was invalid".into()));
        }
        let path = root.join(&file.path);
        regular_file_in_tree(root, &file.path)?;
        if hash_file(&path)? != file.sha256 || fs::metadata(&path)?.len() != file.size {
            return Err(Error::Hub(format!(
                "{} changed since it was listed for transfer",
                file.path
            )));
        }
        let mut input = fs::File::open(path)?;
        let mut buf = vec![0u8; DATA_CHUNK];
        loop {
            let read = io::Read::read(&mut input, &mut buf)?;
            if read == 0 {
                break;
            }
            let mut packet = format!("DATA {index} {read}\n").into_bytes();
            packet.extend_from_slice(&buf[..read]);
            channel.send(&packet)?;
            transferred = transferred
                .checked_add(read as u64)
                .ok_or_else(|| Error::Hub("transfer byte count overflowed".into()))?;
        }
        channel.send(format!("END {index}").as_bytes())?;
    }
    Ok(transferred)
}

/// The client half of `send_files`: skips files already in `staged` with the listed size and
/// hash, receives the rest into `work_dir` and moves each into place only once it verifies.
/// Returns the bytes received.
fn receive_files(
    channel: &mut SealedChannel<'_>,
    staged: &Path,
    work_dir: &Path,
    files: &[GameFile],
) -> Result<u64> {
    let mut received = 0u64;
    for (index, file) in files.iter().enumerate() {
        let destination = safe_destination(staged, &file.path, true)?;
        if file_matches(&destination, file)? {
            channel.send(format!("SKIP {index}").as_bytes())?;
            continue;
        }
        channel.send(format!("NEED {index}").as_bytes())?;
        let temp = temporary_work_file(work_dir)?;
        if let Err(error) = receive_file(channel, index, file, &temp) {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        replace_verified(&temp, &destination)?;
        received = received
            .checked_add(file.size)
            .ok_or_else(|| Error::Hub("transfer byte count overflowed".into()))?;
    }
    Ok(received)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GameFile {
    path: String,
    size: u64,
    sha256: String,
}

fn peer_key(peer: &Peer) -> Result<[u8; 32]> {
    if peer.key_hex.len() != 64 {
        return Err(Error::Hub("this device has not paired with the hub".into()));
    }
    let mut key = [0u8; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        let pair = &peer.key_hex[index * 2..index * 2 + 2];
        *byte = u8::from_str_radix(pair, 16)
            .map_err(|_| Error::Hub("this device has not paired with the hub".into()))?;
    }
    Ok(key)
}

/// HMAC over `label`, the receiver's `challenge` and the sender's `name` (length-prefixed).
fn pairing_mac(key: &[u8; 32], label: &[u8], challenge: &[u8], name: &str) -> Result<Hmac<Sha256>> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key)
        .map_err(|_| Error::Hub("could not initialize pairing confirmation".into()))?;
    mac.update(label);
    mac.update(challenge);
    mac.update(&(name.len() as u16).to_be_bytes());
    mac.update(name.as_bytes());
    Ok(mac)
}

fn write_packet(t: &mut dyn Transport, packet: &[u8], limit: usize) -> Result<()> {
    if packet.len() > limit {
        return Err(Error::Hub("hub protocol packet exceeds its limit".into()));
    }
    t.write_all(&(packet.len() as u32).to_be_bytes())?;
    t.write_all(packet)?;
    t.flush()?;
    Ok(())
}

fn read_packet(t: &mut dyn Transport, limit: usize) -> Result<Vec<u8>> {
    let mut size = [0u8; 4];
    t.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if size > limit {
        return Err(Error::Hub("hub protocol packet exceeds its limit".into()));
    }
    let mut packet = vec![0; size];
    t.read_exact(&mut packet)?;
    Ok(packet)
}

fn write_sealed(
    t: &mut dyn Transport,
    key: &[u8; 32],
    session: &[u8; 16],
    direction: u8,
    sequence: u64,
    plaintext: &[u8],
) -> Result<()> {
    if plaintext.len() > MAX_CONTROL_FRAME {
        return Err(Error::Hub("hub transfer frame exceeds its limit".into()));
    }
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| Error::Hub("could not initialize transfer encryption".into()))?;
    let mut nonce = [0u8; 24];
    getrandom::getrandom(&mut nonce)
        .map_err(|_| Error::Hub("the operating system could not provide random bytes".into()))?;
    let mut aad = TRANSFER_DOMAIN.to_vec();
    aad.extend_from_slice(session);
    aad.push(direction);
    aad.extend_from_slice(&sequence.to_be_bytes());
    let sealed = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            chacha20poly1305::aead::Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| Error::Hub("could not seal a transfer frame".into()))?;
    let size = nonce
        .len()
        .checked_add(sealed.len())
        .ok_or_else(|| Error::Hub("transfer frame size overflowed".into()))?;
    t.write_all(session)?;
    t.write_all(&sequence.to_be_bytes())?;
    t.write_all(&(size as u32).to_be_bytes())?;
    t.write_all(&nonce)?;
    t.write_all(&sealed)?;
    t.flush()?;
    Ok(())
}

fn read_sealed(
    t: &mut dyn Transport,
    key: &[u8; 32],
    expected_session: Option<&[u8; 16]>,
    direction: u8,
    expected_sequence: u64,
) -> Result<([u8; 16], Vec<u8>)> {
    let mut session = [0u8; 16];
    t.read_exact(&mut session)?;
    if expected_session.is_some_and(|expected| expected != &session) {
        return Err(Error::Hub("transfer session did not match".into()));
    }
    let mut sequence = [0u8; 8];
    t.read_exact(&mut sequence)?;
    let sequence = u64::from_be_bytes(sequence);
    if sequence != expected_sequence {
        return Err(Error::Hub("transfer frame was out of order".into()));
    }
    let mut size = [0u8; 4];
    t.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if !(24 + 16..=MAX_CONTROL_FRAME + 24 + 16).contains(&size) {
        return Err(Error::Hub("hub transfer frame exceeds its limit".into()));
    }
    let mut nonce = [0u8; 24];
    t.read_exact(&mut nonce)?;
    let mut sealed = vec![0u8; size - nonce.len()];
    t.read_exact(&mut sealed)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| Error::Hub("could not initialize transfer encryption".into()))?;
    let mut aad = TRANSFER_DOMAIN.to_vec();
    aad.extend_from_slice(&session);
    aad.push(direction);
    aad.extend_from_slice(&sequence.to_be_bytes());
    let plain = cipher
        .decrypt(
            XNonce::from_slice(&nonce),
            chacha20poly1305::aead::Payload {
                msg: &sealed,
                aad: &aad,
            },
        )
        .map_err(|_| Error::Hub("the peer is not paired with this device".into()))?;
    Ok((session, plain))
}

struct SealedChannel<'a> {
    transport: &'a mut dyn Transport,
    key: [u8; 32],
    session: [u8; 16],
    send_direction: u8,
    receive_direction: u8,
    send_sequence: u64,
    receive_sequence: u64,
}

impl<'a> SealedChannel<'a> {
    fn client(transport: &'a mut dyn Transport, key: [u8; 32], session: [u8; 16]) -> Self {
        Self {
            transport,
            key,
            session,
            send_direction: CLIENT_TO_SERVER,
            receive_direction: SERVER_TO_CLIENT,
            send_sequence: 0,
            receive_sequence: 0,
        }
    }

    fn server(transport: &'a mut dyn Transport, key: [u8; 32], session: [u8; 16]) -> Self {
        Self {
            transport,
            key,
            session,
            send_direction: SERVER_TO_CLIENT,
            receive_direction: CLIENT_TO_SERVER,
            send_sequence: 0,
            receive_sequence: 1, // Request sequence zero established the session.
        }
    }

    fn send(&mut self, plaintext: &[u8]) -> Result<()> {
        write_sealed(
            self.transport,
            &self.key,
            &self.session,
            self.send_direction,
            self.send_sequence,
            plaintext,
        )?;
        self.send_sequence = self
            .send_sequence
            .checked_add(1)
            .ok_or_else(|| Error::Hub("transfer sequence overflowed".into()))?;
        Ok(())
    }

    fn receive(&mut self) -> Result<Vec<u8>> {
        let (_, plaintext) = read_sealed(
            self.transport,
            &self.key,
            Some(&self.session),
            self.receive_direction,
            self.receive_sequence,
        )?;
        self.receive_sequence = self
            .receive_sequence
            .checked_add(1)
            .ok_or_else(|| Error::Hub("transfer sequence overflowed".into()))?;
        Ok(plaintext)
    }
}

fn enumerate_game_files(root: &Path) -> Result<Vec<GameFile>> {
    if !root.is_dir() || fs::symlink_metadata(root)?.file_type().is_symlink() {
        return Err(Error::Hub(
            "prepared game-data folder is not a regular directory".into(),
        ));
    }
    let mut result = Vec::new();
    collect_game_files(root, root, &mut result)?;
    result.sort_by(|a, b| a.path.cmp(&b.path));
    validate_game_paths(&result)?;
    let mut total = 0u64;
    for file in &result {
        total = total
            .checked_add(file.size)
            .ok_or_else(|| Error::Hub("prepared game-data size overflowed".into()))?;
        if file.size > MAX_GAME_FILE || total > MAX_GAME_TOTAL {
            return Err(Error::Hub(
                "prepared game data exceeds the transfer limit".into(),
            ));
        }
    }
    Ok(result)
}

fn collect_game_files(root: &Path, current: &Path, out: &mut Vec<GameFile>) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| Error::Hub("prepared game-data path escaped its root".into()))?
            .to_string_lossy()
            .replace('\\', "/");
        if ty.is_symlink() {
            return Err(Error::Hub("prepared game data contains a symlink".into()));
        }
        if relative.split('/').next().is_some_and(|part| {
            ["saves", "logs", "mods", "user"]
                .iter()
                .any(|owned| owned.eq_ignore_ascii_case(part))
        }) {
            continue;
        }
        if sensitive_game_path(&relative) {
            return Err(Error::Hub(
                "refusing to transfer keys, firmware or saves".into(),
            ));
        }
        if ty.is_dir() {
            collect_game_files(root, &path, out)?;
        } else if ty.is_file() {
            if out.len() >= MAX_GAME_FILES {
                return Err(Error::Hub("prepared game data has too many files".into()));
            }
            if !safe_transfer_path(&relative) {
                return Err(Error::Hub("prepared game data has an unsafe path".into()));
            }
            out.push(GameFile {
                path: relative,
                size: fs::metadata(&path)?.len(),
                sha256: hash_file(&path)?,
            });
        } else {
            return Err(Error::Hub(
                "prepared game data contains a special file".into(),
            ));
        }
    }
    Ok(())
}

fn sensitive_game_path(path: &str) -> bool {
    let lowered = path.to_ascii_lowercase();
    lowered
        .split('/')
        .any(|part| part == "keys" || part == "firmware" || part == "saves" || part == "logs")
        || lowered
            .split('/')
            .next()
            .is_some_and(|first| ["mods", "user"].contains(&first))
        || [".keys", ".rif", ".pkg"]
            .iter()
            .any(|suffix| lowered.ends_with(suffix))
}

fn safe_transfer_path(path: &str) -> bool {
    !path.is_empty()
        && path.is_ascii()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && !path.chars().any(char::is_control)
        && path.split('/').all(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || ["COM", "LPT"].iter().any(|prefix| {
                    stem.strip_prefix(prefix)
                        .and_then(|suffix| suffix.parse::<u8>().ok())
                        .is_some_and(|number| (1..=9).contains(&number))
                });
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with('.')
                && !part.ends_with(' ')
                && !reserved
                && !part.eq_ignore_ascii_case(STAGING_MARKER)
                && !part.eq_ignore_ascii_case(STAGING_LOCK)
                && !part.eq_ignore_ascii_case(TRANSFER_WORK_DIR)
        })
}

fn validate_game_paths(files: &[GameFile]) -> Result<()> {
    let mut paths = std::collections::BTreeSet::new();
    for file in files {
        if !safe_transfer_path(&file.path) {
            return Err(Error::Hub(
                "game-data manifest contains an unsafe path".into(),
            ));
        }
        let canonical = file.path.to_lowercase();
        if !paths.insert(canonical.clone()) {
            return Err(Error::Hub(
                "game-data manifest contains case-folded path collisions".into(),
            ));
        }
        let components: Vec<&str> = canonical.split('/').collect();
        let mut prefix = String::new();
        for component in &components[..components.len() - 1] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            if paths.contains(&prefix) {
                return Err(Error::Hub(
                    "game-data manifest has a file/directory path conflict".into(),
                ));
            }
        }
        let descendant_prefix = format!("{canonical}/");
        if paths
            .range(descendant_prefix.clone()..)
            .next()
            .is_some_and(|existing| existing.starts_with(&descendant_prefix))
        {
            return Err(Error::Hub(
                "game-data manifest has a file/directory path conflict".into(),
            ));
        }
    }
    Ok(())
}

fn format_game_manifest(target: Os, files: &[GameFile]) -> Result<Vec<u8>> {
    if files.len() > MAX_GAME_FILES {
        return Err(Error::Hub("prepared game data has too many files".into()));
    }
    let mut text = format!("uncharted-game-data 1\ntarget {}\n", target.key());
    for file in files {
        if !safe_transfer_path(&file.path) || !is_sha256(&file.sha256) {
            return Err(Error::Hub("prepared game data has invalid metadata".into()));
        }
        text.push_str(&format!(
            "file {} {} {}\n",
            file.size, file.sha256, file.path
        ));
        if text.len() > MAX_GAME_MANIFEST {
            return Err(Error::Hub(
                "prepared game-data manifest exceeds its limit".into(),
            ));
        }
    }
    Ok(text.into_bytes())
}

fn parse_game_manifest(bytes: &[u8], target: Os) -> Result<Vec<GameFile>> {
    if bytes.len() > MAX_GAME_MANIFEST {
        return Err(Error::Hub(
            "hub game-data manifest exceeds its limit".into(),
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::Hub("hub game-data manifest is not valid text".into()))?;
    let mut lines = text.lines();
    if lines.next() != Some("uncharted-game-data 1")
        || lines.next() != Some(format!("target {}", target.key()).as_str())
    {
        return Err(Error::Hub(
            "hub game-data manifest did not match the request".into(),
        ));
    }
    let mut files = Vec::new();
    let mut total = 0u64;
    for line in lines {
        let mut fields = line.splitn(4, ' ');
        if fields.next() != Some("file") {
            return Err(Error::Hub("hub game-data manifest is malformed".into()));
        }
        let size = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| Error::Hub("hub game-data manifest is malformed".into()))?;
        let sha256 = fields
            .next()
            .ok_or_else(|| Error::Hub("hub game-data manifest is malformed".into()))?;
        let path = fields
            .next()
            .ok_or_else(|| Error::Hub("hub game-data manifest is malformed".into()))?;
        if size > MAX_GAME_FILE
            || !is_sha256(sha256)
            || !safe_transfer_path(path)
            || sensitive_game_path(path)
        {
            return Err(Error::Hub(
                "hub game-data manifest contains unsafe metadata".into(),
            ));
        }
        if files.iter().any(|file: &GameFile| file.path == path) {
            return Err(Error::Hub(
                "hub game-data manifest contains duplicate paths".into(),
            ));
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| Error::Hub("hub game-data size overflowed".into()))?;
        if total > MAX_GAME_TOTAL || files.len() >= MAX_GAME_FILES {
            return Err(Error::Hub(
                "hub game-data manifest exceeds its limit".into(),
            ));
        }
        files.push(GameFile {
            path: path.to_string(),
            size,
            sha256: sha256.to_string(),
        });
    }
    validate_game_paths(&files)?;
    Ok(files)
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; DATA_CHUNK];
    loop {
        let read = io::Read::read(&mut file, &mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    let digest = hash.finalize();
    Ok(hex(&digest))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 0x0f) as usize] as char);
    }
    text
}

fn prepare_staging_root(root: &Path) -> Result<()> {
    fs::create_dir_all(root)?;
    let meta = fs::symlink_metadata(root)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(Error::Hub(
            "game-data staging root must be a real directory".into(),
        ));
    }
    Ok(())
}

/// Keep a persistent sibling lock inode so concurrent processes cannot unlink each other's lock.
/// The advisory lock is held for the whole pull and released automatically if the process exits.
fn acquire_staging_lock(root: &Path) -> Result<fs::File> {
    let parent = root
        .parent()
        .ok_or_else(|| Error::Hub("game-data staging root has no parent".into()))?;
    let parent_meta = fs::symlink_metadata(parent)?;
    if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
        return Err(Error::Hub(
            "game-data staging parent must be a real directory".into(),
        ));
    }
    let path = parent.join(STAGING_LOCK);
    let file_meta = match fs::symlink_metadata(&path) {
        Ok(meta) => Some(meta),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if file_meta
        .as_ref()
        .is_some_and(|meta| meta.file_type().is_symlink() || !meta.is_file())
    {
        return Err(Error::Hub("game-data staging lock is unsafe".into()));
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::Hub("game-data staging lock is unsafe".into()));
    }
    fs4::FileExt::lock(&file)?;
    Ok(file)
}

fn mark_staging_incomplete(root: &Path) -> Result<PathBuf> {
    let marker = root.join(STAGING_MARKER);
    match fs::symlink_metadata(&marker) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err(Error::Hub("game-data staging marker is unsafe".into()))
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(marker)
}

fn safe_destination(root: &Path, relative: &str, make_parents: bool) -> Result<PathBuf> {
    if !safe_transfer_path(relative) {
        return Err(Error::Hub(
            "hub game-data manifest contains an unsafe path".into(),
        ));
    }
    let parts: Vec<&str> = relative.split('/').collect();
    let mut current = root.to_path_buf();
    for part in &parts[..parts.len() - 1] {
        current.push(part);
        if !current.exists() && make_parents {
            fs::create_dir(&current)?;
        }
        if current.exists() {
            let meta = fs::symlink_metadata(&current)?;
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(Error::Hub(
                    "game-data staging path contains a symlink".into(),
                ));
            }
        }
    }
    current.push(parts[parts.len() - 1]);
    if current.exists() && fs::symlink_metadata(&current)?.file_type().is_symlink() {
        return Err(Error::Hub(
            "game-data staging path contains a symlink".into(),
        ));
    }
    Ok(current)
}

fn file_matches(path: &Path, expected: &GameFile) -> Result<bool> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() != expected.size {
        return Ok(false);
    }
    Ok(hash_file(path)? == expected.sha256)
}

fn temporary_sibling(destination: &Path, suffix: &str) -> Result<PathBuf> {
    let name = destination
        .file_name()
        .ok_or_else(|| Error::Hub("invalid game-data destination".into()))?;
    let mut random = [0u8; 8];
    getrandom::getrandom(&mut random)
        .map_err(|_| Error::Hub("the operating system could not provide random bytes".into()))?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(name);
    temp_name.push(format!(".uu-{}.{}", hex(&random), suffix));
    Ok(destination.with_file_name(temp_name))
}

fn prepare_transfer_work_dir(root: &Path) -> Result<PathBuf> {
    let work_dir = root.join(TRANSFER_WORK_DIR);
    match fs::symlink_metadata(&work_dir) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            return Err(Error::Hub("game-data transfer work path is unsafe".into()))
        }
        Ok(_) => ensure_transfer_work_owner(&work_dir)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&work_dir)?;
            ensure_transfer_work_owner(&work_dir)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(work_dir)
}

/// Recover a crash between creating/removing the reserved directory and writing/removing its
/// owner marker. An unowned directory is claimed only when it is empty.
fn ensure_transfer_work_owner(work_dir: &Path) -> Result<()> {
    let owner = work_dir.join(TRANSFER_WORK_OWNER);
    match fs::symlink_metadata(&owner) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Err(Error::Hub("game-data transfer work owner is unsafe".into()));
            }
            if meta.len() > TRANSFER_WORK_OWNER_CONTENT.len() as u64 {
                return Err(Error::Hub(
                    "game-data transfer work owner is invalid".into(),
                ));
            }
            let contents = fs::read(&owner)?;
            if contents == TRANSFER_WORK_OWNER_CONTENT {
                return verify_transfer_work_owner(work_dir);
            }
            if !TRANSFER_WORK_OWNER_CONTENT.starts_with(&contents)
                || !transfer_work_dir_contains_only_owner(work_dir)?
            {
                return Err(Error::Hub(
                    "game-data transfer work owner is invalid".into(),
                ));
            }
            // Creation and repair are restartable: a crash during this write leaves only a prefix
            // of the expected marker, which the next pull can safely rewrite under the lock.
            write_transfer_work_owner(&owner)?;
            verify_transfer_work_owner(work_dir)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if fs::read_dir(work_dir)?.next().is_some() {
                return Err(Error::Hub(
                    "unowned game-data transfer work directory is not empty".into(),
                ));
            }
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&owner)?;
            write_transfer_work_owner(&owner)?;
            verify_transfer_work_owner(work_dir)
        }
        Err(error) => Err(error.into()),
    }
}

fn transfer_work_dir_contains_only_owner(work_dir: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(work_dir)?;
    let Some(entry) = entries.next() else {
        return Ok(false);
    };
    Ok(entry?.file_name() == TRANSFER_WORK_OWNER && entries.next().is_none())
}

fn write_transfer_work_owner(owner: &Path) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(owner)?;
    io::Write::write_all(&mut file, TRANSFER_WORK_OWNER_CONTENT)?;
    file.sync_all()?;
    Ok(())
}

fn verify_transfer_work_owner(work_dir: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(work_dir)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(Error::Hub("game-data transfer work path is unsafe".into()));
    }
    let owner = work_dir.join(TRANSFER_WORK_OWNER);
    let meta = fs::symlink_metadata(&owner)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(Error::Hub("game-data transfer work owner is unsafe".into()));
    }
    if fs::read(owner)? != TRANSFER_WORK_OWNER_CONTENT {
        return Err(Error::Hub(
            "game-data transfer work owner is invalid".into(),
        ));
    }
    Ok(())
}

/// Remove only generated regular temp files from the ownership-marked work directory. Call while
/// holding the staging lock; arbitrary staged paths cannot collide with this namespace.
fn cleanup_transfer_temps(work_dir: &Path) -> Result<()> {
    verify_transfer_work_owner(work_dir)?;
    for entry in fs::read_dir(work_dir)? {
        let entry = entry?;
        let Some(candidate) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(token) = candidate
            .strip_prefix(".uu-")
            .and_then(|rest| rest.strip_suffix(".tmp"))
        else {
            continue;
        };
        if token.len() != 16
            || !token
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_file() && !metadata.file_type().is_symlink() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn temporary_work_file(work_dir: &Path) -> Result<PathBuf> {
    let mut random = [0u8; 8];
    getrandom::getrandom(&mut random)
        .map_err(|_| Error::Hub("the operating system could not provide random bytes".into()))?;
    Ok(work_dir.join(format!(".uu-{}.tmp", hex(&random))))
}

fn finish_transfer_work_dir(work_dir: &Path) -> Result<()> {
    cleanup_transfer_temps(work_dir)?;
    let mut entries = fs::read_dir(work_dir)?;
    let Some(owner) = entries.next() else {
        return Err(Error::Hub(
            "game-data transfer work owner is missing".into(),
        ));
    };
    if owner?.file_name() != TRANSFER_WORK_OWNER || entries.next().is_some() {
        return Err(Error::Hub(
            "game-data transfer work directory contains unexpected files".into(),
        ));
    }
    fs::remove_file(work_dir.join(TRANSFER_WORK_OWNER))?;
    fs::remove_dir(work_dir)?;
    Ok(())
}

fn receive_file(
    channel: &mut SealedChannel<'_>,
    index: usize,
    file: &GameFile,
    temp: &Path,
) -> Result<()> {
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)?;
    let mut written = 0u64;
    let mut hash = Sha256::new();
    loop {
        let packet = channel.receive()?;
        if packet == format!("END {index}").as_bytes() {
            break;
        }
        let newline = packet
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| Error::Hub("hub transfer chunk is malformed".into()))?;
        let header = std::str::from_utf8(&packet[..newline])
            .map_err(|_| Error::Hub("hub transfer chunk is malformed".into()))?;
        let mut fields = header.split(' ');
        if fields.next() != Some("DATA")
            || fields.next().and_then(|s| s.parse::<usize>().ok()) != Some(index)
        {
            return Err(Error::Hub("hub transfer chunk was out of order".into()));
        }
        let chunk_size = fields
            .next()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| *n > 0 && *n <= DATA_CHUNK)
            .ok_or_else(|| Error::Hub("hub transfer chunk was out of bounds".into()))?;
        if fields.next().is_some() || packet.len() - newline - 1 != chunk_size {
            return Err(Error::Hub("hub transfer chunk was malformed".into()));
        }
        written = written
            .checked_add(chunk_size as u64)
            .ok_or_else(|| Error::Hub("transfer file size overflowed".into()))?;
        if written > file.size {
            return Err(Error::Hub("hub sent more data than the file holds".into()));
        }
        let bytes = &packet[newline + 1..];
        io::Write::write_all(&mut output, bytes)?;
        hash.update(bytes);
    }
    io::Write::flush(&mut output)?;
    output.sync_all()?;
    if written != file.size || hex(&hash.finalize()) != file.sha256 {
        return Err(Error::Hub(
            "a received file failed its integrity check".into(),
        ));
    }
    Ok(())
}

fn replace_verified(temp: &Path, destination: &Path) -> Result<()> {
    let backup = temporary_sibling(destination, "bak")?;
    let had_destination = destination.exists();
    if had_destination {
        fs::rename(destination, &backup)?;
    }
    if let Err(error) = fs::rename(temp, destination) {
        if had_destination {
            let _ = fs::rename(&backup, destination);
        }
        return Err(error.into());
    }
    if had_destination {
        fs::remove_file(backup)?;
    }
    Ok(())
}

fn regular_file_in_tree(root: &Path, relative: &str) -> Result<()> {
    if !safe_transfer_path(relative) {
        return Err(Error::Hub("prepared game data has an unsafe path".into()));
    }
    let parts: Vec<&str> = relative.split('/').collect();
    let mut current = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        current.push(part);
        let meta = fs::symlink_metadata(&current)?;
        if meta.file_type().is_symlink()
            || (index + 1 == parts.len() && !meta.is_file())
            || (index + 1 != parts.len() && !meta.is_dir())
        {
            return Err(Error::Hub(
                "prepared game data contains an unsafe path".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr, TcpListener};
    use std::sync::mpsc::{self, Receiver, Sender};

    struct MemoryTransport {
        tx: Sender<Vec<u8>>,
        rx: Receiver<Vec<u8>>,
        pending: std::collections::VecDeque<u8>,
    }

    impl MemoryTransport {
        fn pair() -> (Self, Self) {
            let (a_tx, a_rx) = mpsc::channel();
            let (b_tx, b_rx) = mpsc::channel();
            (
                Self {
                    tx: a_tx,
                    rx: b_rx,
                    pending: Default::default(),
                },
                Self {
                    tx: b_tx,
                    rx: a_rx,
                    pending: Default::default(),
                },
            )
        }
    }

    impl io::Read for MemoryTransport {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if buffer.is_empty() {
                return Ok(0);
            }
            while self.pending.is_empty() {
                match self.rx.recv() {
                    Ok(bytes) => self.pending.extend(bytes),
                    Err(_) => return Ok(0),
                }
            }
            let count = buffer.len().min(self.pending.len());
            for byte in &mut buffer[..count] {
                *byte = self
                    .pending
                    .pop_front()
                    .expect("count bounds pending bytes");
            }
            Ok(count)
        }
    }

    impl io::Write for MemoryTransport {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.tx
                .send(buffer.to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "memory peer closed"))?;
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Transport for MemoryTransport {}

    fn pair_test(t: &mut dyn Transport, code: &str, device: &str) -> Result<Peer> {
        pair_limited(
            t,
            code,
            device,
            &mut PairingLimiter::default(),
            &SystemClock,
        )
    }

    #[derive(Default)]
    struct BufferTransport {
        incoming: io::Cursor<Vec<u8>>,
        outgoing: Vec<u8>,
    }

    impl io::Read for BufferTransport {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.incoming.read(buffer)
        }
    }

    impl io::Write for BufferTransport {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.outgoing.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Transport for BufferTransport {}

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

    #[test]
    fn spake2_pairing_confirms_both_peers_and_rejects_wrong_codes() {
        let (mut a, mut b) = MemoryTransport::pair();
        let left = std::thread::spawn(move || pair_test(&mut a, "483921", "hub"));
        let right = std::thread::spawn(move || pair_test(&mut b, "483921", "handheld"));
        let left = left.join().unwrap().unwrap();
        let right = right.join().unwrap().unwrap();
        // Each side records the other under the name the other sent.
        assert_eq!(left.device, "handheld");
        assert_eq!(right.device, "hub");
        assert_eq!(left.key_hex, right.key_hex);

        let (mut a, mut b) = MemoryTransport::pair();
        let left = std::thread::spawn(move || pair_test(&mut a, "483921", "hub"));
        let right = std::thread::spawn(move || pair_test(&mut b, "111111", "handheld"));
        assert!(left.join().unwrap().is_err());
        assert!(right.join().unwrap().is_err());
    }

    #[test]
    fn paired_memory_transport_game_data_transfer_is_sealed_bounded_and_resumable() {
        let (mut a, mut b) = MemoryTransport::pair();
        let hub_peer = std::thread::spawn(move || pair_test(&mut a, "483921", "hub"));
        let device_peer = std::thread::spawn(move || pair_test(&mut b, "483921", "handheld"));
        let hub_peer = hub_peer.join().unwrap().unwrap();
        let device_peer = device_peer.join().unwrap().unwrap();

        let root = std::env::temp_dir().join(format!("uu-hub-test-{}", std::process::id()));
        let source = root.join("source");
        let staged = root.join("staged");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(source.join("content")).unwrap();
        fs::write(
            source.join("content/data.bin"),
            b"synthetic game-data fixture",
        )
        .unwrap();
        fs::write(source.join("content/empty"), b"").unwrap();

        let (mut server, mut client) = MemoryTransport::pair();
        let hub_peer_second = hub_peer.clone();
        let source_second = source.clone();
        let serving = std::thread::spawn(move || {
            serve_game(&mut server, &hub_peer_second, &source_second, Os::Linux)
        });
        let pulled = pull_game(&mut client, &device_peer, Os::Linux, &staged).unwrap();
        assert_eq!(pulled, 27);
        assert_eq!(
            fs::read(staged.join("content/data.bin")).unwrap(),
            b"synthetic game-data fixture"
        );
        assert!(fs::read(staged.join("content/empty")).unwrap().is_empty());
        assert_eq!(serving.join().unwrap().unwrap(), 27);
        assert!(!staged.join(STAGING_MARKER).exists());
        assert!(!staged.join(TRANSFER_WORK_DIR).exists());

        let (mut server, mut client) = MemoryTransport::pair();
        let serving =
            std::thread::spawn(move || serve_game(&mut server, &hub_peer, &source, Os::Linux));
        assert_eq!(
            pull_game(&mut client, &device_peer, Os::Linux, &staged).unwrap(),
            0
        );
        assert_eq!(serving.join().unwrap().unwrap(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn paired_tcp_loopback_transfers_synthetic_game_data() {
        let pairing_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let pairing_addr = pairing_listener.local_addr().unwrap();
        let pairing_server = std::thread::spawn(move || {
            let (stream, _) = pairing_listener.accept().unwrap();
            let mut transport = TcpTransport::from_stream(stream).unwrap();
            pair_limited(
                &mut transport,
                "483921",
                "hub",
                &mut PairingLimiter::default(),
                &SystemClock,
            )
        });
        let mut pairing_client =
            TcpTransport::connect(pairing_addr, Duration::from_secs(2)).unwrap();
        let client_peer = pair_limited(
            &mut pairing_client,
            "483921",
            "handheld",
            &mut PairingLimiter::default(),
            &SystemClock,
        )
        .unwrap();
        let server_peer = pairing_server.join().unwrap().unwrap();

        let root = std::env::temp_dir().join(format!("uu-tcp-transfer-{}", std::process::id()));
        let source = root.join("source");
        let staged = root.join("staged");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("content.bin"), b"synthetic TCP loopback data").unwrap();

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let serving = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut transport = TcpTransport::from_stream(stream).unwrap();
            serve_game(&mut transport, &server_peer, &source, Os::Linux)
        });
        let mut transport = TcpTransport::connect(addr, Duration::from_secs(2)).unwrap();
        assert_eq!(
            pull_game(&mut transport, &client_peer, Os::Linux, &staged).unwrap(),
            27
        );
        assert_eq!(
            fs::read(staged.join("content.bin")).unwrap(),
            b"synthetic TCP loopback data"
        );
        assert!(!staged.join(STAGING_MARKER).exists());
        assert!(!staged.join(TRANSFER_WORK_DIR).exists());
        assert_eq!(serving.join().unwrap().unwrap(), 27);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unpaired_or_wrong_key_cannot_request_game_data() {
        let root = std::env::temp_dir().join(format!("uu-hub-auth-test-{}", std::process::id()));
        let source = root.join("source");
        let staged = root.join("staged");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("data.bin"), b"synthetic").unwrap();
        let known_peer = Peer {
            device: "paired device".into(),
            key_hex: "11".repeat(32),
        };
        let wrong_server_peer = Peer {
            device: "different player".into(),
            key_hex: "22".repeat(32),
        };
        let (mut server, mut client) = MemoryTransport::pair();
        let serving = std::thread::spawn(move || {
            serve_game(&mut server, &wrong_server_peer, &source, Os::Linux)
        });
        assert!(pull_game(&mut client, &known_peer, Os::Linux, &staged).is_err());
        assert!(serving.join().unwrap().is_err());
        assert!(!staged.join("data.bin").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sealed_frames_reject_ciphertext_tampering() {
        let key = [9u8; 32];
        let session = [7u8; 16];
        let mut buffer = BufferTransport::default();
        write_sealed(
            &mut buffer,
            &key,
            &session,
            SERVER_TO_CLIENT,
            0,
            b"synthetic manifest",
        )
        .unwrap();
        buffer.outgoing[48] ^= 0x80;
        buffer.incoming = io::Cursor::new(buffer.outgoing.clone());
        assert!(read_sealed(&mut buffer, &key, None, SERVER_TO_CLIENT, 0).is_err());
    }

    #[test]
    fn manifest_rejects_traversal_sensitive_paths_and_oversize_files() {
        for path in [
            "../outside",
            "content/../../outside",
            "saves/slot",
            "user/mod",
            "CON",
            "NUL.txt",
            "content/trailing.",
            "content/trailing ",
            ".UU-TRANSFER-INCOMPLETE",
            "content/.UU-TRANSFER-INCOMPLETE",
            ".UU-TRANSFER.LOCK",
            "content/.UU-TRANSFER-WORK/data.bin",
            "content/é.bin",
        ] {
            let text = format!(
                "uncharted-game-data 1\ntarget linux\nfile 1 {} {path}\n",
                "0".repeat(64)
            );
            assert!(
                parse_game_manifest(text.as_bytes(), Os::Linux).is_err(),
                "{path}"
            );
        }
        let text = format!(
            "uncharted-game-data 1\ntarget linux\nfile {} {} content/data.bin\n",
            MAX_GAME_FILE + 1,
            "0".repeat(64)
        );
        assert!(parse_game_manifest(text.as_bytes(), Os::Linux).is_err());
    }

    #[test]
    fn manifest_rejects_casefold_collisions_and_file_directory_conflicts() {
        let hash = "0".repeat(64);
        for rows in [
            format!("file 1 {hash} content/Foo\nfile 1 {hash} content/foo\n"),
            format!("file 1 {hash} content\nfile 1 {hash} content/data.bin\n"),
            format!("file 1 {hash} content/data.bin\nfile 1 {hash} CONTENT\n"),
        ] {
            let text = format!("uncharted-game-data 1\ntarget linux\n{rows}");
            assert!(parse_game_manifest(text.as_bytes(), Os::Linux).is_err());
        }
    }

    #[test]
    fn same_staging_directory_pulls_are_serialized() {
        let root = std::env::temp_dir().join(format!("uu-hub-lock-test-{}", std::process::id()));
        let staged = root.join("staged");
        let _ = fs::remove_dir_all(&root);
        prepare_staging_root(&staged).unwrap();
        let first = acquire_staging_lock(&staged).unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let staged_second = staged.clone();
        let second = std::thread::spawn(move || {
            let lock = acquire_staging_lock(&staged_second).unwrap();
            ready_tx.send(()).unwrap();
            drop(lock);
        });
        assert!(matches!(
            ready_rx.recv_timeout(Duration::from_millis(100)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        drop(first);
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        second.join().unwrap();
        assert!(root.join(STAGING_LOCK).is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retry_cleanup_isolated_from_valid_manifest_paths() {
        let root = std::env::temp_dir().join(format!("uu-hub-temp-clean-{}", std::process::id()));
        let staged = root.join("staged");
        let parent = staged.join("content");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&parent).unwrap();
        let work_dir = prepare_transfer_work_dir(&staged).unwrap();
        let abandoned = work_dir.join(".uu-0123456789abcdef.tmp");
        let unrelated = work_dir.join(".other.bin.uu-0123456789abcdef.tmp");
        let malformed = work_dir.join(".uu-not-a-random-token.tmp");
        let backup = work_dir.join(".uu-0123456789abcdef.bak");
        let legitimate = parent.join(".data.bin.uu-0123456789abcdef.tmp");
        fs::write(parent.join("data.bin"), b"synthetic data").unwrap();
        fs::write(&abandoned, b"partial synthetic data").unwrap();
        fs::write(&unrelated, b"keep").unwrap();
        fs::write(&malformed, b"keep").unwrap();
        fs::write(&backup, b"keep").unwrap();
        fs::write(&legitimate, b"legitimate staged data").unwrap();

        let hash = hex(&Sha256::digest(b"legitimate staged data"));
        let manifest = format!(
            "uncharted-game-data 1\ntarget linux\nfile 22 {hash} content/.data.bin.uu-0123456789abcdef.tmp\n"
        );
        assert!(parse_game_manifest(manifest.as_bytes(), Os::Linux).is_ok());

        cleanup_transfer_temps(&work_dir).unwrap();

        assert!(!abandoned.exists());
        assert!(unrelated.exists());
        assert!(malformed.exists());
        assert!(backup.exists());
        assert_eq!(fs::read(&legitimate).unwrap(), b"legitimate staged data");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn transfer_work_dir_recovers_only_empty_unowned_directories() {
        let root = std::env::temp_dir().join(format!("uu-work-owner-{}", std::process::id()));
        let staged = root.join("staged");
        let work_dir = staged.join(TRANSFER_WORK_DIR);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&staged).unwrap();
        fs::create_dir(&work_dir).unwrap();

        assert_eq!(prepare_transfer_work_dir(&staged).unwrap(), work_dir);
        verify_transfer_work_owner(&work_dir).unwrap();

        fs::remove_file(work_dir.join(TRANSFER_WORK_OWNER)).unwrap();
        fs::write(work_dir.join("unexpected"), b"preserve").unwrap();
        assert!(prepare_transfer_work_dir(&staged).is_err());
        assert_eq!(fs::read(work_dir.join("unexpected")).unwrap(), b"preserve");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn transfer_work_dir_recovers_truncated_owner_marker() {
        let root =
            std::env::temp_dir().join(format!("uu-work-owner-partial-{}", std::process::id()));
        let staged = root.join("staged");
        let work_dir = staged.join(TRANSFER_WORK_DIR);
        let owner = work_dir.join(TRANSFER_WORK_OWNER);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&work_dir).unwrap();
        fs::write(&owner, &TRANSFER_WORK_OWNER_CONTENT[..8]).unwrap();

        prepare_transfer_work_dir(&staged).unwrap();

        assert_eq!(fs::read(&owner).unwrap(), TRANSFER_WORK_OWNER_CONTENT);
        verify_transfer_work_owner(&work_dir).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_file_hash_is_never_committed_to_staging() {
        let (mut receiver_transport, mut sender_transport) = MemoryTransport::pair();
        let key = [3u8; 32];
        let session = [5u8; 16];
        let receiver = SealedChannel::client(&mut receiver_transport, key, session);
        let sender_thread = std::thread::spawn(move || {
            let mut sender = SealedChannel::server(&mut sender_transport, key, session);
            sender.send(b"DATA 0 3\nxyz").unwrap();
            sender.send(b"END 0").unwrap();
        });
        let path = std::env::temp_dir().join(format!("uu-corrupt-{}.tmp", std::process::id()));
        let _ = fs::remove_file(&path);
        let file = GameFile {
            path: "file.bin".into(),
            size: 3,
            sha256: hex(&Sha256::digest(b"abc")),
        };
        let mut receiver = receiver;
        assert!(receive_file(&mut receiver, 0, &file, &path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"xyz");
        fs::remove_file(path).unwrap();
        sender_thread.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn staging_parent_symlinks_are_rejected() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("uu-link-{}", std::process::id()));
        let outside = root.with_extension("outside");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, root.join("escape")).unwrap();
        assert!(safe_destination(&root, "escape/file.bin", true).is_err());
        assert!(!outside.join("file.bin").exists());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn pairing_limiter_blocks_three_failures_for_five_minutes() {
        let mut limiter = PairingLimiter::default();
        let start = std::time::Duration::from_secs(10_000);
        limiter.check(start).unwrap();
        limiter.failed(start);
        limiter.failed(start);
        limiter.failed(start);
        assert!(limiter
            .check(start + std::time::Duration::from_secs(299))
            .is_err());
        limiter
            .check(start + std::time::Duration::from_secs(300))
            .unwrap();
        assert_eq!(limiter.failures, 0);
    }

    #[cfg(unix)]
    #[test]
    fn peer_storage_round_trips_with_owner_only_unix_mode_and_redacted_debug() {
        let root = std::env::temp_dir().join(format!("uu-peers-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("peers.json");
        let peer = Peer {
            device: "synthetic handheld".into(),
            key_hex: "a5".repeat(32),
        };
        write_peers(&path, std::slice::from_ref(&peer)).unwrap();
        let peers = read_peers(&path).unwrap();
        assert_eq!(peers, vec![peer.clone()]);
        assert!(!format!("{peer:?}").contains(&peer.key_hex));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn peer_storage_fails_closed_when_protected_storage_is_unavailable() {
        let path = std::env::temp_dir().join(format!("uu-peers-{}", std::process::id()));
        assert!(matches!(
            read_peers(&path),
            Err(Error::Hub(message)) if message.contains("not implemented on Windows")
        ));
        assert!(matches!(
            write_peers(&path, &[]),
            Err(Error::Hub(message)) if message.contains("not implemented on Windows")
        ));
        let peer = Peer {
            device: "synthetic handheld".into(),
            key_hex: "a5".repeat(32),
        };
        assert!(matches!(
            remember_peer(&path, &peer),
            Err(Error::Hub(message)) if message.contains("not implemented on Windows")
        ));
        assert!(matches!(
            remove_peer(&path, "synthetic handheld"),
            Err(Error::Hub(message)) if message.contains("not implemented on Windows")
        ));
        assert!(!path.exists());
    }

    #[test]
    fn peer_storage_rejects_duplicate_and_unprotected_files() {
        let root = std::env::temp_dir().join(format!("uu-peers-invalid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("peers.json");
        let peer = Peer {
            device: "synthetic handheld".into(),
            key_hex: "a5".repeat(32),
        };
        assert!(write_peers(&path, &[peer.clone(), peer]).is_err());
        fs::write(&path, br#"{"version":1,"peers":[]}"#).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(read_peers(&path).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }

    /// A clock the test moves by hand; shared with the hub thread.
    #[derive(Clone)]
    struct TestClock(std::sync::Arc<std::sync::atomic::AtomicU64>);

    impl TestClock {
        fn at(seconds: u64) -> Self {
            Self(std::sync::Arc::new(std::sync::atomic::AtomicU64::new(
                seconds,
            )))
        }

        fn advance(&self, seconds: u64) {
            self.0
                .fetch_add(seconds, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> Duration {
            Duration::from_secs(self.0.load(std::sync::atomic::Ordering::SeqCst))
        }
    }

    /// Writes come back as reads: a peer that reflects everything it is sent.
    #[derive(Default)]
    struct EchoTransport(std::collections::VecDeque<u8>);

    impl io::Read for EchoTransport {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let count = buffer.len().min(self.0.len());
            for byte in &mut buffer[..count] {
                *byte = self.0.pop_front().expect("count bounds queued bytes");
            }
            Ok(count)
        }
    }

    impl io::Write for EchoTransport {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.0.extend(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Transport for EchoTransport {}

    fn fresh_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("uu-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// One attempt on `server` from `remote`, with a client typing `code`, over memory.
    fn memory_attempt<C: Clock + Send>(
        server: &mut PairingServer<C>,
        remote: IpAddr,
        code: &str,
        client_peers: &Path,
    ) -> (Result<Peer>, Result<Peer>) {
        let (mut hub_side, mut client_side) = MemoryTransport::pair();
        std::thread::scope(|scope| {
            let client =
                scope.spawn(move || pair_client(&mut client_side, code, "handheld", client_peers));
            let hub = server.attempt(remote, &mut hub_side);
            drop(hub_side);
            (hub, client.join().unwrap())
        })
    }

    #[test]
    fn pairing_codes_are_six_uniform_digits_and_parse_as_typed() {
        assert_eq!(pairing_code_from(0).as_deref(), Some("000000"));
        assert_eq!(pairing_code_from(1_000_000).as_deref(), Some("000000"));
        assert_eq!(pairing_code_from(4_293_999_999).as_deref(), Some("999999"));
        assert_eq!(pairing_code_from(4_294_000_000), None);
        assert_eq!(pairing_code_from(u32::MAX), None);

        let codes: std::collections::BTreeSet<String> =
            (0..64).map(|_| new_pairing_code().unwrap()).collect();
        assert!(codes
            .iter()
            .all(|code| code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit())));
        assert!(codes.len() > 1, "the generator repeated one code");

        assert_eq!(format_pairing_code("483921").unwrap(), "483 921");
        assert_eq!(format_pairing_code("000042").unwrap(), "000 042");
        for typed in ["483921", "483 921", " 483-921\n", "4 8 3 9 2 1"] {
            assert_eq!(parse_pairing_code(typed).unwrap(), "483921");
        }
        for bad in ["", "48392", "4839210", "48392a", "483_921", "٤٨٣٩٢١"] {
            assert!(parse_pairing_code(bad).is_err(), "{bad:?} parsed");
        }
    }

    #[test]
    fn pairing_refuses_a_reflected_exchange() {
        let mut echo = EchoTransport::default();
        assert_eq!(
            pair_inner(&mut echo, "483921", "hub"),
            Err(Error::Hub(PAIRING_MISMATCH.into()))
        );
    }

    #[test]
    fn a_guesser_that_hangs_up_still_counts_against_the_limiter() {
        let clock = TestClock::at(10_000);
        let mut limiter = PairingLimiter::default();
        for _ in 0..MAX_FAILED_ATTEMPTS {
            // The peer vanishes after the first packet: an I/O error, not a mismatch.
            let mut hung_up = BufferTransport::default();
            assert!(matches!(
                pair_limited(&mut hung_up, "483921", "hub", &mut limiter, &clock),
                Err(Error::Io(_))
            ));
        }
        let mut transport = BufferTransport::default();
        assert_eq!(
            pair_limited(&mut transport, "483921", "hub", &mut limiter, &clock),
            Err(Error::Hub(PAIRING_LOCKED.into()))
        );
        assert!(transport.outgoing.is_empty(), "a locked attempt sent bytes");
    }

    #[test]
    fn pairing_server_keeps_one_limiter_per_remote_and_spends_the_code() {
        let root = fresh_root("pair-limits");
        let clock = TestClock::at(10_000);
        let mut server =
            PairingServer::with_code("483921", "hub", &root.join("hub.json"), clock.clone())
                .unwrap();
        let client_peers = root.join("client.json");
        let a = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20));
        let a_mapped = IpAddr::V6(Ipv4Addr::new(192, 168, 1, 20).to_ipv6_mapped());

        for _ in 0..MAX_FAILED_ATTEMPTS {
            let (hub, client) = memory_attempt(&mut server, a, "000000", &client_peers);
            assert_eq!(hub, Err(Error::Hub(PAIRING_MISMATCH.into())));
            assert_eq!(client, Err(Error::Hub(PAIRING_MISMATCH.into())));
        }
        // The same device, also when it arrives as an IPv4-mapped IPv6 address, is refused
        // before any key exchange, even with the right code.
        for remote in [a, a_mapped] {
            let (hub, client) = memory_attempt(&mut server, remote, "483921", &client_peers);
            assert_eq!(hub, Err(Error::Hub(PAIRING_LOCKED.into())));
            assert_eq!(client, Err(Error::Hub(PAIRING_LOCKED.into())));
        }

        // Another address has its own budget, but every wrong code counts toward the total.
        let mut failures = u32::from(MAX_FAILED_ATTEMPTS);
        let mut host = 30;
        while failures < MAX_TOTAL_FAILED_ATTEMPTS {
            assert!(!server.is_spent());
            let remote = IpAddr::V4(Ipv4Addr::new(192, 168, 1, host));
            let (hub, client) = memory_attempt(&mut server, remote, "111111", &client_peers);
            assert_eq!(hub, Err(Error::Hub(PAIRING_MISMATCH.into())));
            assert_eq!(client, Err(Error::Hub(PAIRING_MISMATCH.into())));
            failures += 1;
            host += 1;
        }
        assert!(server.is_spent());
        let fresh = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 99));
        let (hub, client) = memory_attempt(&mut server, fresh, "483921", &client_peers);
        assert_eq!(hub, Err(Error::Hub(PAIRING_SPENT.into())));
        assert_eq!(client, Err(Error::Hub(PAIRING_SPENT.into())));
        assert!(!client_peers.exists(), "a failed pairing stored a peer");
        fs::remove_dir_all(root).unwrap();
    }

    /// The fake hub on loopback (SPEC.md M12, ROADMAP 4.02): three wrong codes lock the remote
    /// out, the right code is refused until the lockout ends on the fake clock, then pairs, and
    /// both paired-device files hold the same key.
    #[test]
    fn fake_hub_on_loopback_locks_out_then_pairs_and_both_sides_store_the_key() {
        let root = fresh_root("pair-loopback");
        let hub_peers = root.join("hub").join("peers.json");
        let client_peers = root.join("client").join("peers.json");
        let clock = TestClock::at(10_000);
        let mut server =
            PairingServer::with_code("483921", "hub", &hub_peers, clock.clone()).unwrap();
        assert_eq!(server.display_code(), "483 921");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let hub = std::thread::spawn(move || {
            let result = server.serve(&listener);
            (result, server)
        });

        for _ in 0..MAX_FAILED_ATTEMPTS {
            assert_eq!(
                pair_with_hub(addr, "000000", "handheld", &client_peers),
                Err(Error::Hub(PAIRING_MISMATCH.into()))
            );
        }
        assert_eq!(
            pair_with_hub(addr, "483 921", "handheld", &client_peers),
            Err(Error::Hub(PAIRING_LOCKED.into()))
        );
        clock.advance(PAIRING_LOCKOUT.as_secs() - 1);
        assert_eq!(
            pair_with_hub(addr, "483921", "handheld", &client_peers),
            Err(Error::Hub(PAIRING_LOCKED.into()))
        );
        clock.advance(1);
        let client = pair_with_hub(addr, "483-921", "handheld", &client_peers);
        let (hub, server) = hub.join().unwrap();
        assert!(server.is_spent());

        #[cfg(not(windows))]
        {
            let client = client.unwrap();
            let hub = hub.unwrap();
            assert_eq!(client.device, "hub");
            assert_eq!(hub.device, "handheld");
            assert_eq!(client.key_hex, hub.key_hex);
            assert_eq!(read_peers(&client_peers).unwrap(), vec![client.clone()]);
            assert_eq!(read_peers(&hub_peers).unwrap(), vec![hub.clone()]);

            // Pairing again replaces the record; unpairing forgets it.
            let mut again =
                PairingServer::with_code("222333", "hub", &hub_peers, clock.clone()).unwrap();
            let (repaired, _) = memory_attempt(
                &mut again,
                Ipv4Addr::LOCALHOST.into(),
                "222 333",
                &client_peers,
            );
            let repaired = repaired.unwrap();
            assert_ne!(repaired.key_hex, hub.key_hex);
            assert_eq!(read_peers(&hub_peers).unwrap(), vec![repaired]);
            assert_eq!(read_peers(&client_peers).unwrap().len(), 1);
            assert!(remove_peer(&hub_peers, "handheld").unwrap());
            assert!(!remove_peer(&hub_peers, "handheld").unwrap());
            assert!(read_peers(&hub_peers).unwrap().is_empty());
        }
        // Protected peer storage is not built on Windows: the exchange completes, then both
        // sides fail closed instead of storing the key unprotected.
        #[cfg(windows)]
        {
            for result in [client, hub] {
                assert!(matches!(
                    result,
                    Err(Error::Hub(message)) if message.contains("not implemented on Windows")
                ));
            }
            assert!(!hub_peers.exists() && !client_peers.exists());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn remembered_peers_replace_by_device_and_unpair() {
        let root = fresh_root("peers-remember");
        let path = root.join("peers.json");
        let first = Peer {
            device: "handheld".into(),
            key_hex: "a5".repeat(32),
        };
        let other = Peer {
            device: "tablet".into(),
            key_hex: "5a".repeat(32),
        };
        let repaired = Peer {
            device: "handheld".into(),
            key_hex: "c3".repeat(32),
        };
        assert!(!remove_peer(&path, "handheld").unwrap());
        assert!(!path.exists(), "unpairing nothing created the file");
        remember_peer(&path, &first).unwrap();
        remember_peer(&path, &other).unwrap();
        remember_peer(&path, &repaired).unwrap();
        assert_eq!(
            read_peers(&path).unwrap(),
            vec![other.clone(), repaired.clone()]
        );
        assert!(remove_peer(&path, "handheld").unwrap());
        assert_eq!(read_peers(&path).unwrap(), vec![other]);
        fs::remove_dir_all(root).unwrap();
    }
}
