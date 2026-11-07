//! The build hub on the home network (SPEC.md M11, M12; ADR 0004): find a hub, pair with it once,
//! serve and pull bundles. Behind the feature `hub`, so the core builds and tests with no network.
//! Nothing here reads the dump, keys or a save; it moves only what `bundle` lists. Home network
//! only: an address outside the private ranges is refused. The shape is copied from
//! iannitram/totk-native's `hub.rs` (MIT); SPAKE2 and RustCrypto AEAD are used for pairing and
//! game-data transfer. Peer persistence, pairing rate limits, mDNS and bundle serving still need
//! integration before this is a complete hub service.

use crate::bundle::Bundle;
use crate::error::{not_built, Error, Result};
use crate::platform::Os;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, io};

/// The mDNS service a hub announces.
pub const SERVICE: &str = "_uncharted-universal._tcp.local.";
const MAX_CONTROL_FRAME: usize = 1024 * 1024;
const DATA_CHUNK: usize = 64 * 1024;
const MAX_GAME_FILES: usize = 10_000;
const MAX_GAME_MANIFEST: usize = 1024 * 1024;
const MAX_GAME_FILE: u64 = 32 * 1024 * 1024 * 1024;
const MAX_GAME_TOTAL: u64 = 100 * 1024 * 1024 * 1024;
const PAIRING_ID: &[u8] = b"uncharted-universal-hub-pairing-v1";
const TRANSFER_DOMAIN: &[u8] = b"uncharted-universal-game-transfer-v1";

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
    pub device: String,
    /// Hex of the key both sides derived at pairing.
    pub key_hex: String,
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

/// Pairs over `t` with the six-digit `code` shown on both screens (SPAKE2), confirms possession
/// on both sides, then returns an ephemeral peer record with the same key on both devices.
/// Persistent protected storage and failed-attempt throttling are not implemented yet.
pub fn pair(t: &mut dyn Transport, code: &str, device: &str) -> Result<Peer> {
    if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::Hub(
            "the pairing code must contain six digits".into(),
        ));
    }
    if device.is_empty() || device.len() > 128 || device.chars().any(char::is_control) {
        return Err(Error::Hub("the device name is invalid".into()));
    }

    let (state, outbound) = Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(code.as_bytes()),
        &Identity::new(PAIRING_ID),
    );
    write_packet(t, &outbound, 256)?;
    let inbound = read_packet(t, 256)?;
    let shared = state
        .finish(&inbound)
        .map_err(|_| Error::Hub("the pairing code did not match".into()))?;
    let mut key_hash = Sha256::new();
    key_hash.update(b"uncharted-universal peer key v1\0");
    key_hash.update(&shared);
    let key: [u8; 32] = key_hash.finalize().into();

    // Confirm possession in both directions with independent random challenges. This prevents
    // a reflection from making one side accept its own confirmation as the peer's response.
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
    let own_tag = pairing_confirmation(&key, &peer_challenge)?;
    write_packet(t, &own_tag, 64)?;
    let peer_tag = read_packet(t, 64)?;
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&key)
        .map_err(|_| Error::Hub("could not initialize pairing confirmation".into()))?;
    mac.update(b"peer confirmation v1\0");
    mac.update(&challenge);
    mac.verify_slice(&peer_tag)
        .map_err(|_| Error::Hub("the pairing code did not match".into()))?;

    Ok(Peer {
        device: device.to_string(),
        key_hex: hex(&key),
    })
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
    let (session, request) = read_sealed(t, &key, None)?;
    let expected = format!("PULLGAME1 {}", target.key());
    if request != expected.as_bytes() {
        return Err(Error::Hub(
            "the game-data request was not authorized".into(),
        ));
    }

    let files = enumerate_game_files(prepared_data)?;
    let manifest = format_game_manifest(target, &files)?;
    write_sealed(t, &key, &session, &manifest)?;

    let mut transferred = 0u64;
    for (index, file) in files.iter().enumerate() {
        let command = read_sealed(t, &key, Some(&session))?.1;
        let expected_prefix = format!("NEED {index}");
        if command == format!("SKIP {index}").as_bytes() {
            continue;
        }
        if command != expected_prefix.as_bytes() {
            return Err(Error::Hub(
                "the game-data transfer request was invalid".into(),
            ));
        }
        let path = prepared_data.join(&file.path);
        regular_file_in_tree(prepared_data, &file.path)?;
        if hash_file(&path)? != file.sha256 || fs::metadata(&path)?.len() != file.size {
            return Err(Error::Hub(
                "prepared game data changed during transfer".into(),
            ));
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
            write_sealed(t, &key, &session, &packet)?;
            transferred = transferred
                .checked_add(read as u64)
                .ok_or_else(|| Error::Hub("game-data byte count overflowed".into()))?;
        }
        write_sealed(t, &key, &session, format!("END {index}").as_bytes())?;
    }
    write_sealed(t, &key, &session, b"DONE")?;
    Ok(transferred)
}

/// Pulls prepared game data into `staged`. Only authenticated, encrypted metadata is accepted;
/// paths are relative and symlink-free, each file is capped at 32 GiB and the aggregate at 100 GiB.
/// Existing regular files with the expected size/hash are skipped to resume completed files.
pub fn pull_game(t: &mut dyn Transport, peer: &Peer, target: Os, staged: &Path) -> Result<u64> {
    let key = peer_key(peer)?;
    let mut session = [0u8; 16];
    getrandom::getrandom(&mut session)
        .map_err(|_| Error::Hub("the operating system could not provide random bytes".into()))?;
    let request = format!("PULLGAME1 {}", target.key());
    write_sealed(t, &key, &session, request.as_bytes())?;
    let (_, manifest) = read_sealed(t, &key, Some(&session))?;
    let files = parse_game_manifest(&manifest, target)?;
    prepare_staging_root(staged)?;
    let mut received = 0u64;

    for (index, file) in files.iter().enumerate() {
        let destination = safe_destination(staged, &file.path, true)?;
        if file_matches(&destination, file)? {
            write_sealed(t, &key, &session, format!("SKIP {index}").as_bytes())?;
            continue;
        }
        write_sealed(t, &key, &session, format!("NEED {index}").as_bytes())?;
        let temp = temporary_sibling(&destination)?;
        let result = receive_game_file(t, &key, &session, index, file, &temp);
        if let Err(error) = result {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        replace_verified(&temp, &destination)?;
        received = received
            .checked_add(file.size)
            .ok_or_else(|| Error::Hub("game-data byte count overflowed".into()))?;
    }
    let (_, done) = read_sealed(t, &key, Some(&session))?;
    if done != b"DONE" {
        return Err(Error::Hub(
            "the game-data transfer ended unexpectedly".into(),
        ));
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

fn pairing_confirmation(key: &[u8; 32], challenge: &[u8]) -> Result<Vec<u8>> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key)
        .map_err(|_| Error::Hub("could not initialize pairing confirmation".into()))?;
    mac.update(b"peer confirmation v1\0");
    mac.update(challenge);
    Ok(mac.finalize().into_bytes().to_vec())
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
) -> Result<([u8; 16], Vec<u8>)> {
    let mut session = [0u8; 16];
    t.read_exact(&mut session)?;
    if expected_session.is_some_and(|expected| expected != &session) {
        return Err(Error::Hub("transfer session did not match".into()));
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

fn enumerate_game_files(root: &Path) -> Result<Vec<GameFile>> {
    if !root.is_dir() || fs::symlink_metadata(root)?.file_type().is_symlink() {
        return Err(Error::Hub(
            "prepared game-data folder is not a regular directory".into(),
        ));
    }
    let mut result = Vec::new();
    collect_game_files(root, root, &mut result)?;
    result.sort_by(|a, b| a.path.cmp(&b.path));
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
            .is_some_and(|first| ["mods", "user"].iter().any(|owned| first == *owned))
        || [".keys", ".rif", ".pkg"]
            .iter()
            .any(|suffix| lowered.ends_with(suffix))
}

fn safe_transfer_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
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

fn temporary_sibling(destination: &Path) -> Result<PathBuf> {
    let name = destination
        .file_name()
        .ok_or_else(|| Error::Hub("invalid game-data destination".into()))?;
    let mut random = [0u8; 8];
    getrandom::getrandom(&mut random)
        .map_err(|_| Error::Hub("the operating system could not provide random bytes".into()))?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(name);
    temp_name.push(format!(".uu-{}.tmp", hex(&random)));
    Ok(destination.with_file_name(temp_name))
}

fn receive_game_file(
    t: &mut dyn Transport,
    key: &[u8; 32],
    session: &[u8; 16],
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
        let (_, packet) = read_sealed(t, key, Some(session))?;
        if packet == format!("END {index}").as_bytes() {
            break;
        }
        let newline = packet
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| Error::Hub("hub game-data chunk is malformed".into()))?;
        let header = std::str::from_utf8(&packet[..newline])
            .map_err(|_| Error::Hub("hub game-data chunk is malformed".into()))?;
        let mut fields = header.split(' ');
        if fields.next() != Some("DATA")
            || fields.next().and_then(|s| s.parse::<usize>().ok()) != Some(index)
        {
            return Err(Error::Hub("hub game-data chunk was out of order".into()));
        }
        let chunk_size = fields
            .next()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| *n > 0 && *n <= DATA_CHUNK)
            .ok_or_else(|| Error::Hub("hub game-data chunk was out of bounds".into()))?;
        if fields.next().is_some() || packet.len() - newline - 1 != chunk_size {
            return Err(Error::Hub("hub game-data chunk was malformed".into()));
        }
        written = written
            .checked_add(chunk_size as u64)
            .ok_or_else(|| Error::Hub("game-data file size overflowed".into()))?;
        if written > file.size {
            return Err(Error::Hub("hub sent too much game data".into()));
        }
        let bytes = &packet[newline + 1..];
        io::Write::write_all(&mut output, bytes)?;
        hash.update(bytes);
    }
    io::Write::flush(&mut output)?;
    output.sync_all()?;
    if written != file.size || hex(&hash.finalize()) != file.sha256 {
        return Err(Error::Hub(
            "received game data failed its integrity check".into(),
        ));
    }
    Ok(())
}

fn replace_verified(temp: &Path, destination: &Path) -> Result<()> {
    let backup = temporary_sibling(destination)?;
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
        let left = std::thread::spawn(move || pair(&mut a, "483921", "hub"));
        let right = std::thread::spawn(move || pair(&mut b, "483921", "handheld"));
        let left = left.join().unwrap().unwrap();
        let right = right.join().unwrap().unwrap();
        assert_ne!(left.device, right.device);
        assert_eq!(left.key_hex, right.key_hex);

        let (mut a, mut b) = MemoryTransport::pair();
        let left = std::thread::spawn(move || pair(&mut a, "483921", "hub"));
        let right = std::thread::spawn(move || pair(&mut b, "111111", "handheld"));
        assert!(left.join().unwrap().is_err());
        assert!(right.join().unwrap().is_err());
    }

    #[test]
    fn paired_loopback_game_data_transfer_is_sealed_bounded_and_resumable() {
        let (mut a, mut b) = MemoryTransport::pair();
        let hub_peer = std::thread::spawn(move || pair(&mut a, "483921", "hub"));
        let device_peer = std::thread::spawn(move || pair(&mut b, "483921", "handheld"));
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
        write_sealed(&mut buffer, &key, &session, b"synthetic manifest").unwrap();
        buffer.outgoing[48] ^= 0x80;
        buffer.incoming = io::Cursor::new(buffer.outgoing.clone());
        assert!(read_sealed(&mut buffer, &key, None).is_err());
    }

    #[test]
    fn manifest_rejects_traversal_sensitive_paths_and_oversize_files() {
        for path in [
            "../outside",
            "content/../../outside",
            "saves/slot",
            "user/mod",
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
    fn invalid_file_hash_is_never_committed_to_staging() {
        let (mut receiver, mut sender) = MemoryTransport::pair();
        let key = [3u8; 32];
        let session = [5u8; 16];
        let session_copy = session;
        let sender_thread = std::thread::spawn(move || {
            write_sealed(&mut sender, &key, &session_copy, b"DATA 0 3\nxyz").unwrap();
            write_sealed(&mut sender, &key, &session_copy, b"END 0").unwrap();
        });
        let path = std::env::temp_dir().join(format!("uu-corrupt-{}.tmp", std::process::id()));
        let _ = fs::remove_file(&path);
        let file = GameFile {
            path: "file.bin".into(),
            size: 3,
            sha256: hex(&Sha256::digest(b"abc")),
        };
        assert!(receive_game_file(&mut receiver, &key, &session, 0, &file, &path).is_err());
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
}
