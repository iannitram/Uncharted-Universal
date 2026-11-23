//! Stage 7.04 (`#525`): pure-Rust resumable per-game asset downloader.
//!
//! Downloads a game's prepared files from the owner's Dropbox App folder through
//! `save-relay` (`iannitram/save-relay#305`) using [`RangeDownloadTransport`].
//! Transfers stage into `.uu-download/part/<rel>.part` with a durable
//! `.uu-download/checkpoint.json`, resume from exact byte offsets via `Range`,
//! verify size + SHA-256 before atomic rename, and recover automatically from
//! truncated or corrupt `.part` files.
//!
//! Never bundles game files in the APK, repo, CI, or logs (SPEC.md M9).

use crate::error::{Error, Result};
use crate::games::Game;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const STAGING_DIR: &str = ".uu-download";
const PART_SUBDIR: &str = "part";
const CHECKPOINT_FILE: &str = "checkpoint.json";
const CHECKPOINT_TMP: &str = "checkpoint.json.tmp";
const HASH_BUF_LEN: usize = 64 * 1024;
const DEFAULT_CHUNK_SIZE: usize = 256 * 1024;
const MANIFEST_VERSION: u64 = 1;

/// One prepared game asset in a per-game download manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameAssetEntry {
    /// Relative path using `/` separators (e.g. `"eboot.bin"` or `"USRDIR/data.psarc"`).
    pub path: String,
    /// Exact expected byte length.
    pub size: u64,
    /// Expected lowercase hex SHA-256 digest (64 hex chars).
    pub sha256: String,
}

/// Per-game asset manifest (`u1`, `u2`, or `u3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameAssetManifest {
    pub game: Game,
    pub files: Vec<GameAssetEntry>,
}

impl GameAssetManifest {
    /// Constructs and validates a manifest, rejecting unsafe paths, invalid hashes, and duplicates.
    pub fn new(game: Game, files: Vec<GameAssetEntry>) -> Result<Self> {
        let manifest = Self { game, files };
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validates all entries in the manifest.
    pub fn validate(&self) -> Result<()> {
        let mut seen = HashSet::with_capacity(self.files.len());
        for entry in &self.files {
            if !is_safe_relative_path(&entry.path) {
                return Err(Error::Launch(format!(
                    "unsafe relative asset path in manifest: {}",
                    entry.path
                )));
            }
            if !is_sha256_hex(&entry.sha256) {
                return Err(Error::Launch(format!(
                    "invalid SHA-256 hex digest for {}",
                    entry.path
                )));
            }
            if !seen.insert(entry.path.as_str()) {
                return Err(Error::Launch(format!(
                    "duplicate asset path in manifest: {}",
                    entry.path
                )));
            }
        }
        Ok(())
    }

    /// Total expected byte size across all files in the manifest.
    pub fn total_bytes(&self) -> u64 {
        self.files
            .iter()
            .fold(0u64, |acc, f| acc.saturating_add(f.size))
    }

    /// Serializes the manifest to canonical JSON.
    pub fn to_json(&self) -> String {
        let files: Vec<Value> = self
            .files
            .iter()
            .map(|f| {
                json!({
                    "path": f.path,
                    "size": f.size,
                    "sha256": f.sha256,
                })
            })
            .collect();
        json!({
            "version": MANIFEST_VERSION,
            "game": self.game.key(),
            "files": files,
        })
        .to_string()
    }

    /// Parses and validates a manifest from JSON.
    pub fn from_json(raw: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(raw)
            .map_err(|e| Error::Launch(format!("invalid game asset manifest JSON: {e}")))?;
        let obj = value
            .as_object()
            .ok_or_else(|| Error::Launch("manifest JSON must be an object".into()))?;
        let version = obj
            .get("version")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::Launch("manifest JSON missing integer version".into()))?;
        if version != MANIFEST_VERSION {
            return Err(Error::Launch(format!(
                "unsupported game asset manifest version: {version}"
            )));
        }
        let game_key = obj
            .get("game")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Launch("manifest JSON missing game key".into()))?;
        let game = Game::from_key(game_key)
            .ok_or_else(|| Error::Launch(format!("unknown game key in manifest: {game_key}")))?;
        let files_arr = obj
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::Launch("manifest JSON missing files array".into()))?;

        let mut files = Vec::with_capacity(files_arr.len());
        for item in files_arr {
            let entry = item
                .as_object()
                .ok_or_else(|| Error::Launch("manifest file entry must be an object".into()))?;
            let path = entry
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Launch("manifest file entry missing path".into()))?
                .to_owned();
            let size = entry
                .get("size")
                .and_then(Value::as_u64)
                .ok_or_else(|| Error::Launch("manifest file entry missing size".into()))?;
            let sha256 = entry
                .get("sha256")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Launch("manifest file entry missing sha256".into()))?
                .to_owned();
            files.push(GameAssetEntry { path, size, sha256 });
        }

        Self::new(game, files)
    }

    /// Lowercase hex SHA-256 of the canonical JSON representation.
    pub fn canonical_sha256(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.to_json().as_bytes());
        hasher.finalize_hex()
    }
}

/// Byte-range transport implemented by `save-relay#305` (Dropbox App folder) or test fakes.
pub trait RangeDownloadTransport {
    /// Fetches up to `max_bytes` starting at `byte_offset` for `remote_path`.
    ///
    /// Returning fewer than `max_bytes` bytes is valid on short reads or EOF. Returning an empty
    /// slice when more bytes are still expected signals an interrupted stream so the caller
    /// preserves the `.part` file for resume on the next attempt.
    fn fetch_range(
        &mut self,
        remote_path: &str,
        byte_offset: u64,
        max_bytes: usize,
    ) -> Result<Vec<u8>>;
}

/// Configuration for [`download_resumable`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadConfig {
    /// Maximum bytes requested per [`RangeDownloadTransport::fetch_range`] call.
    pub chunk_size: usize,
    /// Optional remote folder prefix prepended before `<game>/<rel_path>`.
    pub remote_prefix: String,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            remote_prefix: String::new(),
        }
    }
}

impl DownloadConfig {
    /// Builds the remote path passed to [`RangeDownloadTransport::fetch_range`].
    pub fn remote_path(&self, game: Game, rel_path: &str) -> String {
        let prefix = self.remote_prefix.trim_matches('/');
        if prefix.is_empty() {
            format!("{}/{}", game.key(), rel_path)
        } else {
            format!("{}/{}/{}", prefix, game.key(), rel_path)
        }
    }
}

/// Live progress snapshot emitted during [`download_resumable`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadProgress {
    pub game: Game,
    pub completed_files: usize,
    pub total_files: usize,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub current_file: Option<String>,
}

impl DownloadProgress {
    /// Whole-percent completion in `0..=100`.
    pub fn percent(&self) -> u8 {
        match self
            .downloaded_bytes
            .saturating_mul(100)
            .checked_div(self.total_bytes)
        {
            Some(pct) => pct.min(100) as u8,
            None => u8::from(self.completed_files >= self.total_files) * 100,
        }
    }
}

/// Control signal returned by the progress callback to continue, pause, or cancel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadControl {
    Continue,
    Pause,
    Cancel,
}

/// Final outcome of a [`download_resumable`] invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadOutcome {
    /// All manifest files are verified and atomically in place.
    Completed(DownloadProgress),
    /// Caller requested pause; `.part` files and checkpoint are preserved for resume.
    Paused(DownloadProgress),
    /// Caller requested cancel; `.part` staging and checkpoint were cleaned up.
    Cancelled(DownloadProgress),
}

/// Validates that `rel_path` is a clean relative path without traversal or staging collisions.
pub fn is_safe_relative_path(rel_path: &str) -> bool {
    if rel_path.is_empty()
        || rel_path.starts_with('/')
        || rel_path.starts_with('\\')
        || rel_path.contains('\\')
        || rel_path.contains(':')
        || rel_path.chars().any(char::is_control)
    {
        return false;
    }
    let components: Vec<&str> = rel_path.split('/').collect();
    if components
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return false;
    }
    if components.contains(&STAGING_DIR) {
        return false;
    }
    if rel_path.ends_with(".part") {
        return false;
    }
    true
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VerifiedRecord {
    size: u64,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Checkpoint {
    game: Game,
    manifest_sha256: String,
    verified: BTreeMap<String, VerifiedRecord>,
    active_path: Option<String>,
    active_bytes: u64,
}

impl Checkpoint {
    fn new(game: Game, manifest_sha256: String) -> Self {
        Self {
            game,
            manifest_sha256,
            verified: BTreeMap::new(),
            active_path: None,
            active_bytes: 0,
        }
    }

    fn load(path: &Path, game: Game, manifest_sha256: &str) -> Option<Self> {
        let raw = fs::read_to_string(path).ok()?;
        let val: Value = serde_json::from_str(&raw).ok()?;
        let obj = val.as_object()?;
        if obj.get("version").and_then(Value::as_u64) != Some(MANIFEST_VERSION) {
            return None;
        }
        let cp_game = obj
            .get("game")
            .and_then(Value::as_str)
            .and_then(Game::from_key)?;
        let cp_hash = obj.get("manifest_sha256").and_then(Value::as_str)?;
        if cp_game != game || cp_hash != manifest_sha256 {
            return None;
        }
        let mut verified = BTreeMap::new();
        if let Some(map) = obj.get("verified").and_then(Value::as_object) {
            for (k, v) in map {
                if let (Some(size), Some(sha256)) = (
                    v.get("size").and_then(Value::as_u64),
                    v.get("sha256").and_then(Value::as_str),
                ) {
                    verified.insert(
                        k.clone(),
                        VerifiedRecord {
                            size,
                            sha256: sha256.to_owned(),
                        },
                    );
                }
            }
        }
        let active_path = obj
            .get("active_path")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let active_bytes = obj.get("active_bytes").and_then(Value::as_u64).unwrap_or(0);
        Some(Self {
            game,
            manifest_sha256: cp_hash.to_owned(),
            verified,
            active_path,
            active_bytes,
        })
    }

    fn save(&self, staging_dir: &Path) -> Result<()> {
        let verified_json: serde_json::Map<String, Value> = self
            .verified
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    json!({
                        "size": v.size,
                        "sha256": v.sha256,
                    }),
                )
            })
            .collect();
        let payload = json!({
            "version": MANIFEST_VERSION,
            "game": self.game.key(),
            "manifest_sha256": self.manifest_sha256,
            "verified": verified_json,
            "active_path": self.active_path,
            "active_bytes": self.active_bytes,
        })
        .to_string();

        let tmp = staging_dir.join(CHECKPOINT_TMP);
        let dst = staging_dir.join(CHECKPOINT_FILE);
        let mut file = File::create(&tmp)?;
        file.write_all(payload.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, &dst)?;
        Ok(())
    }
}

fn staging_paths(dest_dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let staging = dest_dir.join(STAGING_DIR);
    let part_root = staging.join(PART_SUBDIR);
    let checkpoint = staging.join(CHECKPOINT_FILE);
    (staging, part_root, checkpoint)
}

fn part_file_path(part_root: &Path, rel_path: &str) -> PathBuf {
    let mut p = part_root.to_path_buf();
    let mut parts = rel_path.split('/').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            p.push(format!("{part}.part"));
        } else {
            p.push(part);
        }
    }
    p
}

fn final_file_path(dest_dir: &Path, rel_path: &str) -> PathBuf {
    let mut p = dest_dir.to_path_buf();
    for part in rel_path.split('/') {
        p.push(part);
    }
    p
}

fn is_regular_file_with_size_and_hash(
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() != expected_size {
        return false;
    }
    matches!(
        sha256_file(path),
        Ok((size, digest)) if size == expected_size && digest == expected_sha256
    )
}

fn promote_part_to_final(part_path: &Path, final_path: &Path) -> Result<()> {
    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(part_path, final_path)?;
    Ok(())
}

/// Downloads or resumes all files in `manifest` into `dest_dir` using `transport`.
///
/// - Skips files already present at `dest_dir` with matching size and SHA-256.
/// - Resumes partial `.part` files from their exact byte offset on disk.
/// - Recovers automatically from truncated, oversized, or corrupt `.part` files.
/// - Verifies SHA-256 and exact byte size before atomically renaming into `dest_dir`.
/// - Reports live [`DownloadProgress`] snapshots and honors [`DownloadControl::Pause`] /
///   [`DownloadControl::Cancel`].
pub fn download_resumable<T, F>(
    manifest: &GameAssetManifest,
    dest_dir: &Path,
    transport: &mut T,
    config: &DownloadConfig,
    mut on_progress: F,
) -> Result<DownloadOutcome>
where
    T: RangeDownloadTransport + ?Sized,
    F: FnMut(&DownloadProgress) -> DownloadControl,
{
    manifest.validate()?;
    if config.chunk_size == 0 {
        return Err(Error::Launch("download chunk_size must be > 0".into()));
    }

    let (staging_dir, part_root, checkpoint_path) = staging_paths(dest_dir);
    fs::create_dir_all(&part_root)?;

    let manifest_hash = manifest.canonical_sha256();
    let mut checkpoint = match Checkpoint::load(&checkpoint_path, manifest.game, &manifest_hash) {
        Some(cp) => cp,
        None => {
            // If an old checkpoint existed for a different manifest, clear stale .part files.
            if checkpoint_path.exists() {
                let _ = fs::remove_dir_all(&part_root);
                fs::create_dir_all(&part_root)?;
            }
            Checkpoint::new(manifest.game, manifest_hash)
        }
    };

    let total_files = manifest.files.len();
    let total_bytes = manifest.total_bytes();
    let mut completed_files = 0usize;
    let mut downloaded_bytes = 0u64;
    let mut pending: Vec<(&GameAssetEntry, u64)> = Vec::new();

    // Reconcile existing final files and staged .part files.
    for entry in &manifest.files {
        let final_path = final_file_path(dest_dir, &entry.path);
        let part_path = part_file_path(&part_root, &entry.path);

        if is_regular_file_with_size_and_hash(&final_path, entry.size, &entry.sha256) {
            let _ = fs::remove_file(&part_path);
            checkpoint.verified.insert(
                entry.path.clone(),
                VerifiedRecord {
                    size: entry.size,
                    sha256: entry.sha256.clone(),
                },
            );
            completed_files += 1;
            downloaded_bytes = downloaded_bytes.saturating_add(entry.size);
            continue;
        }

        let mut staged_offset = 0u64;
        if let Ok(meta) = fs::symlink_metadata(&part_path) {
            if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > entry.size {
                let _ = fs::remove_file(&part_path);
            } else if meta.len() == entry.size {
                if is_regular_file_with_size_and_hash(&part_path, entry.size, &entry.sha256) {
                    promote_part_to_final(&part_path, &final_path)?;
                    checkpoint.verified.insert(
                        entry.path.clone(),
                        VerifiedRecord {
                            size: entry.size,
                            sha256: entry.sha256.clone(),
                        },
                    );
                    completed_files += 1;
                    downloaded_bytes = downloaded_bytes.saturating_add(entry.size);
                    continue;
                } else {
                    // Full-length .part failed SHA-256: discard corrupt staging.
                    let _ = fs::remove_file(&part_path);
                }
            } else {
                staged_offset = meta.len();
            }
        }

        downloaded_bytes = downloaded_bytes.saturating_add(staged_offset);
        pending.push((entry, staged_offset));
    }

    checkpoint.save(&staging_dir)?;

    let mut progress = DownloadProgress {
        game: manifest.game,
        completed_files,
        total_files,
        downloaded_bytes,
        total_bytes,
        current_file: pending.first().map(|(e, _)| e.path.clone()),
    };

    match on_progress(&progress) {
        DownloadControl::Pause => {
            checkpoint.save(&staging_dir)?;
            return Ok(DownloadOutcome::Paused(progress));
        }
        DownloadControl::Cancel => {
            let _ = fs::remove_dir_all(&staging_dir);
            return Ok(DownloadOutcome::Cancelled(progress));
        }
        DownloadControl::Continue => {}
    }

    for (entry, initial_offset) in pending {
        let final_path = final_file_path(dest_dir, &entry.path);
        let part_path = part_file_path(&part_root, &entry.path);
        let remote_path = config.remote_path(manifest.game, &entry.path);

        let mut offset = initial_offset;
        let mut retried_corrupt_resume = false;

        'file_attempt: loop {
            if let Some(parent) = part_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut file = if offset == 0 {
                OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&part_path)?
            } else {
                let mut f = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(false)
                    .open(&part_path)?;
                f.seek(SeekFrom::Start(offset))?;
                f
            };

            progress.current_file = Some(entry.path.clone());

            while offset < entry.size {
                let remaining = entry.size - offset;
                let max_bytes = remaining.min(config.chunk_size as u64) as usize;
                let chunk = match transport.fetch_range(&remote_path, offset, max_bytes) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        let _ = file.sync_data();
                        checkpoint.active_path = Some(entry.path.clone());
                        checkpoint.active_bytes = offset;
                        let _ = checkpoint.save(&staging_dir);
                        return Err(err);
                    }
                };

                if chunk.is_empty() {
                    let _ = file.sync_data();
                    checkpoint.active_path = Some(entry.path.clone());
                    checkpoint.active_bytes = offset;
                    checkpoint.save(&staging_dir)?;
                    return Err(Error::Io(format!(
                        "unexpected EOF downloading {} at byte offset {}",
                        entry.path, offset
                    )));
                }
                if chunk.len() > max_bytes {
                    let _ = file.sync_data();
                    checkpoint.active_path = Some(entry.path.clone());
                    checkpoint.active_bytes = offset;
                    let _ = checkpoint.save(&staging_dir);
                    return Err(Error::Io(format!(
                        "transport returned {} bytes for {} when at most {} were requested",
                        chunk.len(),
                        entry.path,
                        max_bytes
                    )));
                }

                file.write_all(&chunk)?;
                file.sync_data()?;
                offset = offset.saturating_add(chunk.len() as u64);
                progress.downloaded_bytes =
                    progress.downloaded_bytes.saturating_add(chunk.len() as u64);

                checkpoint.active_path = Some(entry.path.clone());
                checkpoint.active_bytes = offset;
                checkpoint.save(&staging_dir)?;

                match on_progress(&progress) {
                    DownloadControl::Pause => {
                        return Ok(DownloadOutcome::Paused(progress));
                    }
                    DownloadControl::Cancel => {
                        drop(file);
                        let _ = fs::remove_dir_all(&staging_dir);
                        return Ok(DownloadOutcome::Cancelled(progress));
                    }
                    DownloadControl::Continue => {}
                }
            }

            file.sync_all()?;
            drop(file);

            if is_regular_file_with_size_and_hash(&part_path, entry.size, &entry.sha256) {
                promote_part_to_final(&part_path, &final_path)?;
                progress.completed_files += 1;
                checkpoint.verified.insert(
                    entry.path.clone(),
                    VerifiedRecord {
                        size: entry.size,
                        sha256: entry.sha256.clone(),
                    },
                );
                checkpoint.active_path = None;
                checkpoint.active_bytes = 0;
                checkpoint.save(&staging_dir)?;
                break 'file_attempt;
            }

            // Corrupt .part (e.g. corrupt prefix prior to resume): discard and retry once from 0.
            let _ = fs::remove_file(&part_path);
            progress.downloaded_bytes = progress.downloaded_bytes.saturating_sub(entry.size);
            checkpoint.active_path = None;
            checkpoint.active_bytes = 0;
            checkpoint.save(&staging_dir)?;

            if initial_offset > 0 && !retried_corrupt_resume {
                retried_corrupt_resume = true;
                offset = 0;
                continue 'file_attempt;
            }

            return Err(Error::Io(format!(
                "SHA-256 verification failed for {} after download",
                entry.path
            )));
        }
    }

    progress.current_file = None;
    let _ = fs::remove_dir_all(&staging_dir);
    let _ = on_progress(&progress);
    Ok(DownloadOutcome::Completed(progress))
}

/// Computes `(byte_len, lowercase_sha256_hex)` for a regular file on disk, refusing symlinks.
pub fn sha256_file(path: &Path) -> Result<(u64, String)> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(Error::Io(format!(
            "refusing non-regular file {}",
            path.display()
        )));
    }
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; HASH_BUF_LEN];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        total = total.saturating_add(read as u64);
    }
    Ok((total, hasher.finalize_hex()))
}

/// Streaming pure-Rust SHA-256 implementation (`#![forbid(unsafe_code)]`).
pub(crate) struct Sha256 {
    state: [u32; 8],
    bit_len: u64,
    buf: [u8; 64],
    buf_len: usize,
}

impl Sha256 {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    pub(crate) fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            bit_len: 0,
            buf: [0u8; 64],
            buf_len: 0,
        }
    }

    pub(crate) fn update(&mut self, mut data: &[u8]) {
        self.bit_len = self
            .bit_len
            .wrapping_add((data.len() as u64).wrapping_mul(8));
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                let block = self.buf;
                Self::compress(&mut self.state, &block);
                self.buf_len = 0;
            }
        }
        while data.len() >= 64 {
            Self::compress(&mut self.state, &data[..64]);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    pub(crate) fn finalize_hex(mut self) -> String {
        let total_bits = self.bit_len;
        self.buf[self.buf_len] = 0x80;
        self.buf_len += 1;
        if self.buf_len > 56 {
            self.buf[self.buf_len..64].fill(0);
            let block = self.buf;
            Self::compress(&mut self.state, &block);
            self.buf_len = 0;
        }
        self.buf[self.buf_len..56].fill(0);
        self.buf[56..64].copy_from_slice(&total_bits.to_be_bytes());
        let block = self.buf;
        Self::compress(&mut self.state, &block);

        let mut out = String::with_capacity(64);
        for word in self.state {
            use std::fmt::Write as _;
            let _ = write!(out, "{word:08x}");
        }
        out
    }

    fn compress(state: &mut [u32; 8], block: &[u8]) {
        let mut w = [0u32; 64];
        for (slot, chunk) in w.iter_mut().zip(block.as_chunks::<4>().0.iter()) {
            *slot = u32::from_be_bytes(*chunk);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for (k, word) in Self::K.iter().zip(w.iter()) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(*k)
                .wrapping_add(*word);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_DIR_SEQ: AtomicU64 = AtomicU64::new(1);

    fn temp_dir(label: &str) -> PathBuf {
        let id = TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "uu_game_dl_{}_{}_{}",
            std::process::id(),
            label,
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn hash_bytes(bytes: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(bytes);
        h.finalize_hex()
    }

    struct MockTransport {
        files: HashMap<String, Vec<u8>>,
        /// Optional byte budget before simulating battery death / stream drop.
        fail_after_bytes: Option<usize>,
        /// Total bytes served across all calls (to verify zero re-downloaded bytes).
        bytes_served: usize,
        /// Observed `(remote_path, byte_offset, max_bytes)` calls.
        requests: Vec<(String, u64, usize)>,
    }

    impl MockTransport {
        fn new(files: HashMap<String, Vec<u8>>) -> Self {
            Self {
                files,
                fail_after_bytes: None,
                bytes_served: 0,
                requests: Vec::new(),
            }
        }
    }

    impl RangeDownloadTransport for MockTransport {
        fn fetch_range(
            &mut self,
            remote_path: &str,
            byte_offset: u64,
            max_bytes: usize,
        ) -> Result<Vec<u8>> {
            self.requests
                .push((remote_path.to_owned(), byte_offset, max_bytes));
            let data = self
                .files
                .get(remote_path)
                .ok_or_else(|| Error::Io(format!("remote file not found: {remote_path}")))?;
            let start = byte_offset as usize;
            if start >= data.len() {
                return Ok(Vec::new());
            }
            let mut end = (start + max_bytes).min(data.len());
            if let Some(budget) = self.fail_after_bytes.as_mut() {
                if *budget == 0 {
                    return Err(Error::Io("simulated power loss / stream drop".into()));
                }
                let allowed = (*budget).min(end - start);
                end = start + allowed;
                *budget -= allowed;
            }
            let slice = data[start..end].to_vec();
            self.bytes_served += slice.len();
            Ok(slice)
        }
    }

    #[test]
    fn streaming_sha256_matches_known_vectors() {
        assert_eq!(
            hash_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hash_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let multi = vec![b'z'; 200];
        let mut inc = Sha256::new();
        inc.update(&multi[..17]);
        inc.update(&multi[17..99]);
        inc.update(&multi[99..]);
        assert_eq!(inc.finalize_hex(), hash_bytes(&multi));
    }

    #[test]
    fn manifest_rejects_path_traversal_and_invalid_fields() {
        for bad in [
            "",
            "/etc/passwd",
            "../secret",
            "a/../b",
            "a/./b",
            "a//b",
            "a\\b",
            "C:foo",
            ".uu-download/checkpoint.json",
            "eboot.bin.part",
            "bad\npath",
        ] {
            assert!(!is_safe_relative_path(bad), "should reject {bad:?}");
            let res = GameAssetManifest::new(
                Game::U1,
                vec![GameAssetEntry {
                    path: bad.to_owned(),
                    size: 1,
                    sha256: hash_bytes(b"a"),
                }],
            );
            assert!(res.is_err(), "manifest should reject {bad:?}");
        }

        let dup = GameAssetManifest::new(
            Game::U2,
            vec![
                GameAssetEntry {
                    path: "eboot.bin".into(),
                    size: 1,
                    sha256: hash_bytes(b"a"),
                },
                GameAssetEntry {
                    path: "eboot.bin".into(),
                    size: 1,
                    sha256: hash_bytes(b"a"),
                },
            ],
        );
        assert!(dup.is_err());

        let json_traversal = r#"{
            "version": 1,
            "game": "u3",
            "files": [{"path": "../../escape.bin", "size": 3, "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"}]
        }"#;
        assert!(GameAssetManifest::from_json(json_traversal).is_err());
    }

    #[test]
    fn resume_after_mid_file_power_loss_re_downloads_zero_bytes() {
        let dir = temp_dir("resume_zero_redownload");
        let file1 = vec![0x41u8; 500];
        let mut file2 = Vec::with_capacity(1200);
        for i in 0..1200usize {
            file2.push((i % 251) as u8);
        }

        let manifest = GameAssetManifest::new(
            Game::U2,
            vec![
                GameAssetEntry {
                    path: "big2-ps4_Shipping.elf".into(),
                    size: file1.len() as u64,
                    sha256: hash_bytes(&file1),
                },
                GameAssetEntry {
                    path: "USRDIR/pak01.psarc".into(),
                    size: file2.len() as u64,
                    sha256: hash_bytes(&file2),
                },
            ],
        )
        .unwrap();

        let mut remote = HashMap::new();
        remote.insert("u2/big2-ps4_Shipping.elf".into(), file1.clone());
        remote.insert("u2/USRDIR/pak01.psarc".into(), file2.clone());

        let config = DownloadConfig {
            chunk_size: 256,
            remote_prefix: String::new(),
        };

        // First boot: die after 500 + 377 = 877 bytes (mid-way through file 2).
        let mut transport1 = MockTransport::new(remote.clone());
        transport1.fail_after_bytes = Some(877);
        let err = download_resumable(&manifest, &dir, &mut transport1, &config, |_| {
            DownloadControl::Continue
        })
        .unwrap_err();
        assert!(err.to_string().contains("simulated power loss"));
        assert_eq!(transport1.bytes_served, 877);

        // File 1 must already be promoted; file 2 must be staged at 377 bytes.
        assert_eq!(fs::read(dir.join("big2-ps4_Shipping.elf")).unwrap(), file1);
        assert!(!dir.join("USRDIR/pak01.psarc").exists());
        let part2 = dir.join(".uu-download/part/USRDIR/pak01.psarc.part");
        assert_eq!(fs::metadata(&part2).unwrap().len(), 377);

        // Second boot: resume from exact byte offset 377; zero bytes re-downloaded.
        let mut transport2 = MockTransport::new(remote);
        let mut snapshots = Vec::new();
        let outcome = download_resumable(&manifest, &dir, &mut transport2, &config, |p| {
            snapshots.push(p.clone());
            DownloadControl::Continue
        })
        .unwrap();

        assert_eq!(transport2.bytes_served, (file1.len() + file2.len()) - 877);
        assert_eq!(
            transport2.requests.first().unwrap(),
            &("u2/USRDIR/pak01.psarc".to_owned(), 377, 256)
        );
        assert!(matches!(
            outcome,
            DownloadOutcome::Completed(ref p)
                if p.completed_files == 2 && p.downloaded_bytes == 1700 && p.total_bytes == 1700
        ));
        assert_eq!(fs::read(dir.join("USRDIR/pak01.psarc")).unwrap(), file2);
        assert!(!dir.join(".uu-download").exists());
        assert_eq!(snapshots.first().unwrap().downloaded_bytes, 877);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovers_from_corrupt_partial_and_oversized_part_files() {
        let dir = temp_dir("corrupt_part");
        let payload = b"clean-uncharted-asset-payload-0123456789".to_vec();
        let manifest = GameAssetManifest::new(
            Game::U1,
            vec![GameAssetEntry {
                path: "eboot.bin".into(),
                size: payload.len() as u64,
                sha256: hash_bytes(&payload),
            }],
        )
        .unwrap();

        let part_dir = dir.join(".uu-download/part");
        fs::create_dir_all(&part_dir).unwrap();
        let part_path = part_dir.join("eboot.bin.part");

        // Case 1: corrupt partial prefix (< size) -> resumed bytes fail SHA-256 -> auto-retry from 0.
        fs::write(&part_path, b"CORRUPT!").unwrap();
        let mut remote = HashMap::new();
        remote.insert("u1/eboot.bin".into(), payload.clone());
        let mut transport = MockTransport::new(remote.clone());
        let config = DownloadConfig {
            chunk_size: 16,
            remote_prefix: String::new(),
        };
        let outcome = download_resumable(&manifest, &dir, &mut transport, &config, |_| {
            DownloadControl::Continue
        })
        .unwrap();
        assert!(matches!(outcome, DownloadOutcome::Completed(_)));
        assert_eq!(fs::read(dir.join("eboot.bin")).unwrap(), payload);

        // Case 2: oversized corrupt .part (> size) when final file is missing.
        fs::remove_file(dir.join("eboot.bin")).unwrap();
        fs::create_dir_all(&part_dir).unwrap();
        fs::write(&part_path, vec![0x99u8; payload.len() + 25]).unwrap();
        let mut transport2 = MockTransport::new(remote);
        let outcome2 = download_resumable(&manifest, &dir, &mut transport2, &config, |_| {
            DownloadControl::Continue
        })
        .unwrap();
        assert!(matches!(outcome2, DownloadOutcome::Completed(_)));
        assert_eq!(fs::read(dir.join("eboot.bin")).unwrap(), payload);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pause_preserves_part_and_cancel_cleans_staging() {
        let dir = temp_dir("pause_cancel");
        let payload = vec![0x5Au8; 600];
        let manifest = GameAssetManifest::new(
            Game::U3,
            vec![GameAssetEntry {
                path: "big3-ps4_Shipping.elf".into(),
                size: payload.len() as u64,
                sha256: hash_bytes(&payload),
            }],
        )
        .unwrap();

        let mut remote = HashMap::new();
        remote.insert(
            "dropbox-root/u3/big3-ps4_Shipping.elf".into(),
            payload.clone(),
        );
        let config = DownloadConfig {
            chunk_size: 200,
            remote_prefix: "dropbox-root".into(),
        };

        // Pause after first 200-byte chunk.
        let mut transport = MockTransport::new(remote.clone());
        let paused = download_resumable(&manifest, &dir, &mut transport, &config, |p| {
            if p.downloaded_bytes >= 200 {
                DownloadControl::Pause
            } else {
                DownloadControl::Continue
            }
        })
        .unwrap();
        assert!(matches!(
            paused,
            DownloadOutcome::Paused(ref p) if p.downloaded_bytes == 200 && p.percent() == 33
        ));
        assert!(dir
            .join(".uu-download/part/big3-ps4_Shipping.elf.part")
            .exists());
        assert!(dir.join(".uu-download/checkpoint.json").exists());

        // Cancel on next chunk cleans up staging.
        let cancelled = download_resumable(&manifest, &dir, &mut transport, &config, |p| {
            if p.downloaded_bytes >= 400 {
                DownloadControl::Cancel
            } else {
                DownloadControl::Continue
            }
        })
        .unwrap();
        assert!(matches!(cancelled, DownloadOutcome::Cancelled(_)));
        assert!(!dir.join(".uu-download").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_already_verified_files_and_round_trips_manifest_json() {
        let dir = temp_dir("skip_verified");
        let payload = b"already-installed-asset".to_vec();
        let manifest = GameAssetManifest::new(
            Game::U1,
            vec![GameAssetEntry {
                path: "eboot.bin".into(),
                size: payload.len() as u64,
                sha256: hash_bytes(&payload),
            }],
        )
        .unwrap();

        let parsed = GameAssetManifest::from_json(&manifest.to_json()).unwrap();
        assert_eq!(parsed, manifest);

        fs::write(dir.join("eboot.bin"), &payload).unwrap();
        let mut transport = MockTransport::new(HashMap::new());
        let outcome = download_resumable(
            &manifest,
            &dir,
            &mut transport,
            &DownloadConfig::default(),
            |_| DownloadControl::Continue,
        )
        .unwrap();
        assert!(matches!(
            outcome,
            DownloadOutcome::Completed(ref p)
                if p.completed_files == 1 && p.downloaded_bytes == payload.len() as u64
        ));
        assert_eq!(transport.bytes_served, 0);
        assert!(transport.requests.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
