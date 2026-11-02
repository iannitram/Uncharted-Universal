//! Build bundles (SPEC.md M9, M11, M12; ADR 0004): a prepared install as a list of files with
//! their hashes, which a hub serves and a client checks before it installs. A bundle never holds
//! the dump folder: `is_forbidden` is checked for every file, and a test enforces it. New
//! module; the text format and the flow are copied from iannitram/totk-native's `bundle.rs` (MIT).
//!
//! OPEN: ADR 0004 says a bundle holds "the game data folder", the code map says "never the dump
//! folder". `is_forbidden`'s rule for game data (what the hub may send a device that has no
//! dump) is decided by piece 1.23; this skeleton fixes only the shape.

use crate::error::{not_built, Result};
use crate::platform::Os;
use std::path::Path;

/// What a bundled file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Part of the prepared install.
    Install,
    /// A GPU-independent shader translation (a driver pipeline cache is never bundled).
    ShaderTranslation,
}

/// One file of a build, its path relative to the build folder (with `/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleFile {
    pub path: String,
    pub size: u64,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
    pub kind: Kind,
}

/// A finished build for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    pub target: Os,
    /// How many of our patches were applied (`patches` plus the render-size patches).
    pub patch_level: u32,
    /// Names this exact set of files.
    pub build_id: String,
    /// Sorted by `path`.
    pub files: Vec<BundleFile>,
}

/// Top-level folders of an install that belong to the device, not the build: never bundled, and
/// kept from the old install by `install`.
pub const DEVICE_OWNED: &[&str] = &["saves", "logs", "mods", "user"];

/// True when a path (relative, with `/`) must never leave a hub: a `.pkg`, anything under
/// `DEVICE_OWNED`, a `param.sfo` or `eboot.bin` copied from a dump, or any other file of the
/// dump folder itself. The final list is piece 1.23's.
pub fn is_forbidden(rel_path: &str) -> bool {
    not_built!("bundle::is_forbidden")
}

/// The bundle as text, one item per line, each ending in `\n`: `uncharted-bundle 1`,
/// `target <os key>`, `patches <patch_level>`, `build <build_id>`, then one
/// `file <kind> <size> <sha256> <path>` line per file in order.
pub fn to_text(b: &Bundle) -> String {
    not_built!("bundle::to_text")
}

/// Reads `to_text`'s format back. Any malformed line is `Error::Hub("bad bundle line <n>: <line>")`;
/// a forbidden path is `Error::Hub("refusing to bundle <path>: game files, keys and saves never
/// leave a hub")`.
pub fn from_text(text: &str) -> Result<Bundle> {
    not_built("bundle::from_text")
}

/// The bundle of an install folder: every file under `dir` (recursively), skipping
/// `DEVICE_OWNED`; a forbidden file is the `from_text` refusal error. `build_id` is the first 16
/// hex digits of the SHA-256 of all the file lines joined in order.
pub fn manifest(dir: &Path, target: Os, patch_level: u32) -> Result<Bundle> {
    not_built("bundle::manifest")
}

/// Checks every file of `b` in `dir` for its size and hash; the first that is missing or differs
/// is `Error::Hub("<path> does not match the bundle")`.
pub fn verify(b: &Bundle, dir: &Path) -> Result<()> {
    not_built("bundle::verify")
}

/// Installs a checked build: `verify(b, staged)` first (nothing changes if it fails); then moves
/// each `DEVICE_OWNED` entry the old `install_dir` has into `staged`, swaps the folders, and
/// deletes the old one. Saves are backed up by `update` before this runs (M19).
pub fn install(b: &Bundle, staged: &Path, install_dir: &Path) -> Result<()> {
    not_built("bundle::install")
}
