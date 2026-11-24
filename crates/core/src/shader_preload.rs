//! Shader preload and cache-warming manifest (Issue #16).
//!
//! Records shader hashes and pipeline keys encountered during a route through the game,
//! validates cache-key compatibility (game revision, translated shader version, GPU/driver
//! identity, and shader feature flags), and drives cancellable, idempotent pre-warming at launch.
//!
//! Never stores or ships game data, shader dumps, or driver-private blobs.

use crate::error::{Error, Result};
use crate::games::Game;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Current on-disk manifest format version.
pub const MANIFEST_VERSION: u32 = 1;

/// Magic header for serialized shader preload manifests.
pub const MANIFEST_MAGIC: &str = "uncharted-shader-preload";

/// Environment variable that disables shader preload when set to `"0"`.
pub const ENV_WARM_MODE: &str = "BB_WARM_MODE";

/// Feature flag bitmask values that affect shader translation or pipeline state.
pub mod feature_flags {
    pub const SAFE_SHADERS: u64 = 1 << 0;
    pub const NAN_GUARD: u64 = 1 << 1;
    pub const EFFECT_LOOP_CLAMP: u64 = 1 << 2;
    pub const INTERP_INT_FIX: u64 = 1 << 3;
    pub const MULTIWAVE_LDS_BARRIERS: u64 = 1 << 4;
    pub const SCOPE_SPLIT_BARRIER: u64 = 1 << 5;
}

/// Identity of the target environment and shader translation configuration.
///
/// Cache entries are only valid when every field in `CacheKey` matches the current runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheKey {
    pub game: Game,
    /// Game revision (e.g. `APP_VER` from `param.sfo`, such as `"01.02"`).
    pub game_revision: String,
    /// Combined translated shader / pipeline serialization version.
    pub shader_version: u32,
    pub gpu_vendor_id: u32,
    pub gpu_device_id: u32,
    pub driver_id: u32,
    pub driver_version: u32,
    /// Hex-encoded Vulkan pipeline cache UUID (or platform driver UUID).
    pub driver_uuid: String,
    /// Bitmask of [`feature_flags`] that alter translated SPIR-V or pipeline state.
    pub feature_flags: u64,
}

/// Reason a cached manifest or pipeline entry is incompatible with the current runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MismatchReason {
    GameMismatch {
        expected: Game,
        found: Game,
    },
    GameRevisionMismatch {
        expected: String,
        found: String,
    },
    ShaderVersionMismatch {
        expected: u32,
        found: u32,
    },
    GpuMismatch {
        expected: (u32, u32),
        found: (u32, u32),
    },
    DriverMismatch {
        expected: (u32, u32, String),
        found: (u32, u32, String),
    },
    FeatureFlagsMismatch {
        expected: u64,
        found: u64,
    },
}

impl CacheKey {
    /// Checks whether `self` (the cached key) is compatible with `current` (the live runtime).
    pub fn check_compatible(&self, current: &CacheKey) -> std::result::Result<(), MismatchReason> {
        if self.game != current.game {
            return Err(MismatchReason::GameMismatch {
                expected: current.game,
                found: self.game,
            });
        }
        if self.game_revision != current.game_revision {
            return Err(MismatchReason::GameRevisionMismatch {
                expected: current.game_revision.clone(),
                found: self.game_revision.clone(),
            });
        }
        if self.shader_version != current.shader_version {
            return Err(MismatchReason::ShaderVersionMismatch {
                expected: current.shader_version,
                found: self.shader_version,
            });
        }
        if (self.gpu_vendor_id, self.gpu_device_id)
            != (current.gpu_vendor_id, current.gpu_device_id)
        {
            return Err(MismatchReason::GpuMismatch {
                expected: (current.gpu_vendor_id, current.gpu_device_id),
                found: (self.gpu_vendor_id, self.gpu_device_id),
            });
        }
        if (self.driver_id, self.driver_version, &self.driver_uuid)
            != (
                current.driver_id,
                current.driver_version,
                &current.driver_uuid,
            )
        {
            return Err(MismatchReason::DriverMismatch {
                expected: (
                    current.driver_id,
                    current.driver_version,
                    current.driver_uuid.clone(),
                ),
                found: (
                    self.driver_id,
                    self.driver_version,
                    self.driver_uuid.clone(),
                ),
            });
        }
        if self.feature_flags != current.feature_flags {
            return Err(MismatchReason::FeatureFlagsMismatch {
                expected: current.feature_flags,
                found: self.feature_flags,
            });
        }
        Ok(())
    }
}

/// Hardware shader stage for a recorded entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Compute,
    Geometry,
    TessellationControl,
    TessellationEval,
}

impl ShaderStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vertex => "vs",
            Self::Fragment => "fs",
            Self::Compute => "cs",
            Self::Geometry => "gs",
            Self::TessellationControl => "hs",
            Self::TessellationEval => "ds",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "vs" => Some(Self::Vertex),
            "fs" => Some(Self::Fragment),
            "cs" => Some(Self::Compute),
            "gs" => Some(Self::Geometry),
            "hs" => Some(Self::TessellationControl),
            "ds" => Some(Self::TessellationEval),
            _ => None,
        }
    }
}

/// One recorded shader/pipeline entry in a preload route manifest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShaderEntry {
    pub pgm_hash: u64,
    pub perm_idx: u32,
    pub stage: ShaderStage,
    pub pipeline_key_hash: Option<u64>,
    pub route_tag: Option<String>,
}

/// A versioned shader preload manifest recorded during a route or session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreloadManifest {
    pub version: u32,
    pub cache_key: CacheKey,
    pub entries: Vec<ShaderEntry>,
}

impl PreloadManifest {
    /// Creates an empty manifest for `cache_key`.
    pub fn new(cache_key: CacheKey) -> Self {
        Self {
            version: MANIFEST_VERSION,
            cache_key,
            entries: Vec::new(),
        }
    }

    /// Records `entry` if an identical `(pgm_hash, perm_idx, stage, pipeline_key_hash)` is not
    /// already present. Returns `true` if the entry was newly added.
    pub fn record_entry(&mut self, entry: ShaderEntry) -> bool {
        let exists = self.entries.iter().any(|existing| {
            existing.pgm_hash == entry.pgm_hash
                && existing.perm_idx == entry.perm_idx
                && existing.stage == entry.stage
                && existing.pipeline_key_hash == entry.pipeline_key_hash
        });
        if exists {
            return false;
        }
        self.entries.push(entry);
        true
    }

    /// Returns a deterministically sorted, deduplicated copy of the entries.
    pub fn canonical_entries(&self) -> Vec<ShaderEntry> {
        let unique: BTreeSet<ShaderEntry> = self.entries.iter().cloned().collect();
        unique.into_iter().collect()
    }

    /// Serializes the manifest to JSON.
    pub fn to_json_string(&self) -> Result<String> {
        let entries_json: Vec<serde_json::Value> = self
            .canonical_entries()
            .into_iter()
            .map(|e| {
                serde_json::json!({
                    "pgm_hash": format!("{:#018x}", e.pgm_hash),
                    "perm_idx": e.perm_idx,
                    "stage": e.stage.as_str(),
                    "pipeline_key_hash": e.pipeline_key_hash.map(|h| format!("{h:#018x}")),
                    "route_tag": e.route_tag,
                })
            })
            .collect();

        let root = serde_json::json!({
            "magic": MANIFEST_MAGIC,
            "version": self.version,
            "cache_key": {
                "game": self.cache_key.game.key(),
                "game_revision": self.cache_key.game_revision,
                "shader_version": self.cache_key.shader_version,
                "gpu_vendor_id": format!("{:#06x}", self.cache_key.gpu_vendor_id),
                "gpu_device_id": format!("{:#06x}", self.cache_key.gpu_device_id),
                "driver_id": self.cache_key.driver_id,
                "driver_version": self.cache_key.driver_version,
                "driver_uuid": self.cache_key.driver_uuid,
                "feature_flags": format!("{:#018x}", self.cache_key.feature_flags),
            },
            "entries": entries_json,
        });

        serde_json::to_string_pretty(&root).map_err(|err| {
            Error::Env(format!(
                "failed to serialize shader preload manifest: {err}"
            ))
        })
    }

    /// Parses a manifest from JSON, validating the magic, version, and required fields.
    ///
    /// If `skip_corrupt_entries` is `true`, malformed individual entries are skipped rather than
    /// failing the entire manifest load, and the count of skipped entries is returned alongside.
    pub fn from_json_str(text: &str, skip_corrupt_entries: bool) -> Result<(Self, u32)> {
        let root: serde_json::Value = serde_json::from_str(text)
            .map_err(|err| Error::Env(format!("corrupt shader preload manifest JSON: {err}")))?;

        let magic = root
            .get("magic")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Error::Env("shader preload manifest missing magic".into()))?;
        if magic != MANIFEST_MAGIC {
            return Err(Error::Env(format!(
                "invalid shader preload manifest magic: {magic}"
            )));
        }

        let version = root
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| Error::Env("shader preload manifest missing version".into()))?;
        if version != MANIFEST_VERSION {
            return Err(Error::Env(format!(
                "unsupported shader preload manifest version: {version}"
            )));
        }

        let key_obj = root
            .get("cache_key")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| Error::Env("shader preload manifest missing cache_key".into()))?;

        let game_str = key_obj
            .get("game")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Error::Env("cache_key missing game".into()))?;
        let game = Game::from_key(game_str)
            .ok_or_else(|| Error::Env(format!("unknown game key in cache_key: {game_str}")))?;

        let game_revision = key_obj
            .get("game_revision")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Error::Env("cache_key missing game_revision".into()))?
            .to_owned();

        let shader_version = key_obj
            .get("shader_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| Error::Env("cache_key missing shader_version".into()))?;

        let gpu_vendor_id = parse_hex_u32(key_obj.get("gpu_vendor_id"))
            .ok_or_else(|| Error::Env("cache_key invalid gpu_vendor_id".into()))?;
        let gpu_device_id = parse_hex_u32(key_obj.get("gpu_device_id"))
            .ok_or_else(|| Error::Env("cache_key invalid gpu_device_id".into()))?;

        let driver_id = key_obj
            .get("driver_id")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| Error::Env("cache_key missing driver_id".into()))?;
        let driver_version = key_obj
            .get("driver_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| Error::Env("cache_key missing driver_version".into()))?;
        let driver_uuid = key_obj
            .get("driver_uuid")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Error::Env("cache_key missing driver_uuid".into()))?
            .to_owned();
        let feature_flags = parse_hex_u64(key_obj.get("feature_flags"))
            .ok_or_else(|| Error::Env("cache_key invalid feature_flags".into()))?;

        let raw_entries = root
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| Error::Env("shader preload manifest missing entries".into()))?;

        let mut entries = Vec::with_capacity(raw_entries.len());
        let mut skipped_corrupt = 0u32;

        for (idx, item) in raw_entries.iter().enumerate() {
            match parse_entry(item) {
                Some(entry) => {
                    entries.push(entry);
                }
                None if skip_corrupt_entries => {
                    skipped_corrupt += 1;
                }
                None => {
                    return Err(Error::Env(format!("corrupt shader entry at index {idx}")));
                }
            }
        }

        let mut manifest = Self::new(CacheKey {
            game,
            game_revision,
            shader_version,
            gpu_vendor_id,
            gpu_device_id,
            driver_id,
            driver_version,
            driver_uuid,
            feature_flags,
        });
        for entry in entries {
            manifest.record_entry(entry);
        }
        Ok((manifest, skipped_corrupt))
    }
}

fn parse_hex_u64(val: Option<&serde_json::Value>) -> Option<u64> {
    let s = val?.as_str()?;
    let trimmed = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    if trimmed.is_empty() {
        return None;
    }
    u64::from_str_radix(trimmed, 16).ok()
}

fn parse_hex_u32(val: Option<&serde_json::Value>) -> Option<u32> {
    u32::try_from(parse_hex_u64(val)?).ok()
}

fn parse_entry(item: &serde_json::Value) -> Option<ShaderEntry> {
    let obj = item.as_object()?;
    let pgm_hash = parse_hex_u64(obj.get("pgm_hash"))?;
    let perm_idx = u32::try_from(obj.get("perm_idx")?.as_u64()?).ok()?;
    let stage = ShaderStage::parse(obj.get("stage")?.as_str()?)?;
    let pipeline_key_hash = match obj.get("pipeline_key_hash") {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => Some(parse_hex_u64(Some(v))?),
    };
    let route_tag = match obj.get("route_tag") {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => Some(v.as_str()?.to_owned()),
    };
    Some(ShaderEntry {
        pgm_hash,
        perm_idx,
        stage,
        pipeline_key_hash,
        route_tag,
    })
}

/// Writes `manifest` atomically (write temp + rename) to `path`.
pub fn write_manifest(path: &Path, manifest: &PreloadManifest) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = manifest.to_json_string()?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Reads and validates a manifest from `path`.
pub fn read_manifest(path: &Path, skip_corrupt_entries: bool) -> Result<(PreloadManifest, u32)> {
    let text = fs::read_to_string(path)?;
    PreloadManifest::from_json_str(&text, skip_corrupt_entries)
}

/// Outcome of attempting to warm one shader entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarmOutcome {
    /// Compiled and stored in the cache.
    Warmed,
    /// Already present in the cache (idempotent skip).
    AlreadyCached,
}

/// Backend that checks and compiles shader entries during preload.
pub trait ShaderCompiler {
    fn is_cached(&self, entry: &ShaderEntry) -> bool;
    fn warm_entry(&mut self, entry: &ShaderEntry) -> std::result::Result<WarmOutcome, String>;
}

/// Progress snapshot reported during preload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreloadProgress {
    pub total: usize,
    pub completed: usize,
    pub warmed: usize,
    pub already_cached: usize,
    pub failed: usize,
    pub cancelled: bool,
}

/// Final report returned by [`warm_manifest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreloadReport {
    pub skipped_disabled: bool,
    pub invalidated: Option<MismatchReason>,
    pub total: usize,
    pub warmed: usize,
    pub already_cached: usize,
    pub failed: usize,
    pub cancelled: bool,
}

/// Checks whether `BB_WARM_MODE` disables shader preload (`"0"`).
pub fn is_warm_enabled(env_lookup: impl Fn(&str) -> Option<String>) -> bool {
    !matches!(env_lookup(ENV_WARM_MODE).as_deref(), Some("0"))
}

/// Pre-warms validated entries from `manifest` if `manifest.cache_key` matches `current_key`.
///
/// - If `enabled` is `false` (e.g. `BB_WARM_MODE=0`), returns immediately with `skipped_disabled: true`.
/// - If `manifest.cache_key` is incompatible with `current_key`, does not warm any entries and
///   returns `invalidated: Some(reason)`.
/// - Checks `cancel` before each entry; if set, stops cleanly and returns `cancelled: true`.
/// - Calls `on_progress` after each entry with the updated [`PreloadProgress`].
pub fn warm_manifest(
    manifest: &PreloadManifest,
    current_key: &CacheKey,
    compiler: &mut impl ShaderCompiler,
    enabled: bool,
    cancel: &Arc<AtomicBool>,
    mut on_progress: impl FnMut(&PreloadProgress),
) -> PreloadReport {
    if !enabled {
        return PreloadReport {
            skipped_disabled: true,
            invalidated: None,
            total: 0,
            warmed: 0,
            already_cached: 0,
            failed: 0,
            cancelled: false,
        };
    }

    if let Err(reason) = manifest.cache_key.check_compatible(current_key) {
        return PreloadReport {
            skipped_disabled: false,
            invalidated: Some(reason),
            total: 0,
            warmed: 0,
            already_cached: 0,
            failed: 0,
            cancelled: false,
        };
    }

    let entries = manifest.canonical_entries();
    let total = entries.len();
    let mut progress = PreloadProgress {
        total,
        completed: 0,
        warmed: 0,
        already_cached: 0,
        failed: 0,
        cancelled: false,
    };

    for entry in &entries {
        if cancel.load(Ordering::Acquire) {
            progress.cancelled = true;
            on_progress(&progress);
            break;
        }
        if compiler.is_cached(entry) {
            progress.already_cached += 1;
        } else {
            match compiler.warm_entry(entry) {
                Ok(WarmOutcome::Warmed) => progress.warmed += 1,
                Ok(WarmOutcome::AlreadyCached) => progress.already_cached += 1,
                Err(_) => progress.failed += 1,
            }
        }
        progress.completed += 1;
        on_progress(&progress);
    }

    PreloadReport {
        skipped_disabled: false,
        invalidated: None,
        total,
        warmed: progress.warmed,
        already_cached: progress.already_cached,
        failed: progress.failed,
        cancelled: progress.cancelled,
    }
}

#[cfg(test)]
mod tests;
