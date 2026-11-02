//! The `.pkg` extractor: entry table, bounded copies, overlap refusal. Unpacks the container
//! only; a package from PSN or a disc is encrypted (drm type 0x0F), and decrypting it needs
//! console keys, which are out of scope and never in this crate (SPEC.md M9). Encrypted entries
//! are counted and skipped; delta patches are reported unsupported, never half-applied.
//! Replaces `scripts/pkg.py`; its `tests/test_pkg.py` are the expected results. Everything in
//! a `.pkg` is big-endian.

use crate::error::{not_built, Result};
use std::path::Path;

/// The first four bytes: `7F 43 4E 54`.
pub const MAGIC: [u8; 4] = *b"\x7fCNT";
/// The header is this many bytes.
pub const HEADER_SIZE: u64 = 0x1000;
/// Each entry-table record.
pub const ENTRY_SIZE: u64 = 0x20;
/// The entry id of the names blob.
pub const ENTRY_NAMES_ID: u32 = 0x200;
/// Entry ids below this are the package's own metadata, not game files.
pub const STRUCTURAL_ID_MAX: u32 = 0x1000;
/// Entry bodies are copied in pieces of this size, so one entry is never in memory whole.
pub const CHUNK_SIZE: usize = 1 << 20;
/// Entry flag: the body is encrypted.
pub const FLAG_ENCRYPTED: u32 = 0x8000_0000;
/// Content type of a delta patch, which is refused.
pub const CONTENT_TYPE_DELTA_PATCH: u32 = 0x1E;

/// Random access to a package, by seeking: a retail package is tens of gigabytes.
pub trait Source {
    /// Total length in bytes.
    fn size(&self) -> u64;
    /// Fill `buf` from `offset`; reading past the end is `Error::Pkg("package is cut short")`.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()>;
}

/// The header fields the port uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// "EP9000-CUSA02344_00-UNCHARTEDTRILOGY"-style content id.
    pub content_id: String,
    pub content_type: u32,
    pub content_flags: u32,
    pub drm_type: u32,
    pub entry_count: u32,
}

/// One record of the entry table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u32,
    /// The path inside the package (with `/`) when the names blob gives one.
    pub name: Option<String>,
    pub flags: u32,
    pub offset: u64,
    pub size: u64,
}

/// What `analyse` found, without writing anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub header: Header,
    pub entries: Vec<Entry>,
}

/// What `extract` did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    pub extracted: u32,
    /// Entries skipped because they are encrypted.
    pub skipped_encrypted: u32,
}

/// Read the header and entry table. Wrong magic, an entry outside the package, or two entries
/// that overlap is `Error::Pkg(..)`; a delta patch (`CONTENT_TYPE_DELTA_PATCH`) is
/// `Error::Pkg("delta patch packages are not supported")`.
pub fn analyse(source: &dyn Source) -> Result<Info> {
    not_built("pkg::analyse")
}

/// A package path made safe to join under an output folder: relative, `/`-separated, with no
/// `..`, no drive letter and no empty part; anything else is `Error::Pkg(..)`.
pub fn safe_relative(name: &str) -> Result<String> {
    not_built("pkg::safe_relative")
}

/// Extract every unencrypted game file under `out` (extracting several packages into one
/// folder, in order, gives the newest file per path).
pub fn extract(source: &dyn Source, out: &Path) -> Result<Report> {
    not_built("pkg::extract")
}
