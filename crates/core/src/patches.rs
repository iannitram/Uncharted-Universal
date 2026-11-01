//! Third-party patches (shadPS4/GoldHEN XML): parse, pick the ones for this executable and
//! revision, apply, list for About. Replaces `scripts/patches.py` (the external-patch half; its
//! Bloodborne-only built-in database `patches/Bloodborne.xml`, FPS presets and effect switches
//! are not carried over). Patches are byte writes at the addresses of ONE build, so they are
//! keyed by title id, executable (`AppElf`) and `APP_VER`, and never applied to another build.
//!
//! Only literal writes are supported (`bytes`, `bytes16/32/64`, `float32/64`, `utf8`,
//! `utf16`); a pattern ("mask") patch is skipped whole, never half-applied. The XML reader is
//! hand-written in std (no parser dependency yet).

use crate::error::{not_built, Result};
use crate::games::Game;

/// Patch addresses are PS4 virtual addresses with the executable at this base.
pub const EBOOT_BASE: u64 = 0x40_0000;
/// The first eight bytes of `patches.bin`.
pub const PATCHES_MAGIC: [u8; 8] = *b"BBPATCH2";

/// One byte write, at an offset into the executable's memory image (`address - EBOOT_BASE`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteWrite {
    pub offset: u64,
    pub bytes: Vec<u8>,
}

/// What a patch is for. A patch applies only when all three match the dump.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Target {
    /// `param.sfo` `TITLE_ID` ("CUSA02320"); a patch file may list several.
    pub title_id: String,
    /// `AppElf` ("eboot.bin" when the file does not say): `Game::executable`.
    pub executable: String,
    /// `AppVer` ("01.02"): `param.sfo` `APP_VER`, falling back to `VERSION` only when missing.
    pub app_ver: String,
}

/// One patch of a patch file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    /// "<file name>/<patch name>": the launcher's selection key (`patches.json`).
    pub key: String,
    pub name: String,
    pub author: String,
    pub note: String,
    /// The title ids the file lists; empty when it lists none.
    pub title_ids: Vec<String>,
    pub executable: String,
    pub app_ver: String,
    /// `isEnabled` in the file.
    pub enabled_by_default: bool,
    pub writes: Vec<ByteWrite>,
}

/// The player's choice (`patches.json`): `enabled` and `disabled` override `isEnabled` per key.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selection {
    pub enabled: Vec<String>,
    pub disabled: Vec<String>,
}

/// Parse one patch XML file (`file_name` forms the keys). Bad XML is `Error::Patch(..)`; a
/// patch with an unsupported line type is kept with `writes` empty and a note, so About can
/// list it as unavailable.
pub fn parse(file_name: &str, xml: &str) -> Result<Vec<Patch>> {
    not_built("patches::parse")
}

/// The patches that may be written to this dump: the file lists no title ids or lists
/// `target.title_id`, and `executable` and `app_ver` match. With an unknown title id
/// (`fail_closed`), a file that lists title ids is skipped, never applied on a guess.
pub fn applicable<'a>(patches: &'a [Patch], target: &Target, fail_closed: bool) -> Vec<&'a Patch> {
    not_built!("patches::applicable")
}

/// The selected patches among `found`: `Selection` first, else `enabled_by_default`.
pub fn select<'a>(found: &[&'a Patch], selection: &Selection) -> Vec<&'a Patch> {
    not_built!("patches::select")
}

/// The writes of `selected`, checked to lie inside the executable's loadable `segments`
/// (`(start, end)` offsets from [`eboot_segments`]); a patch with an address outside is
/// skipped whole and named in the returned notes.
pub fn compile(
    selected: &[&Patch],
    segments: &[(u64, u64)],
) -> Result<(Vec<ByteWrite>, Vec<String>)> {
    not_built("patches::compile")
}

/// The loadable segments of an ELF image as `(vaddr, vaddr + memsz)`.
pub fn eboot_segments(elf: &[u8]) -> Result<Vec<(u64, u64)>> {
    not_built("patches::eboot_segments")
}

/// `patches.bin` for the loader: `PATCHES_MAGIC`, `EBOOT_BASE` and the count (little-endian
/// u64), then per write its offset, length and bytes. Empty `writes` is a valid file.
pub fn to_patches_bin(writes: &[ByteWrite]) -> Vec<u8> {
    not_built!("patches::to_patches_bin")
}

/// The game a patch's `AppElf` belongs to, if any.
pub fn game_of(patch: &Patch) -> Option<Game> {
    Game::from_executable(&patch.executable)
}
