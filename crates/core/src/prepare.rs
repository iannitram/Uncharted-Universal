//! Preparing a game folder into an install: the boot image, linked modules, the content
//! profile, patches and the render size. Replaces `scripts/prepare.py` (SELF/ELF parsing, the
//! memory image, import tables) and the orchestration in `run.py` (`prepare`, `locate`,
//! `patch_files_for`). The install is what `bundle` serves to other devices (SPEC.md M11).

use crate::error::{not_built, Result};
use crate::games::Game;
use std::path::{Path, PathBuf};

/// The first eight bytes of a freshly prepared `boot.bin`.
pub const BOOT_MAGIC: [u8; 8] = *b"BBPROBE2";
/// The memory image of an executable may not exceed this (512 MiB).
pub const MAX_IMAGE_BYTES: u64 = 512 * 1024 * 1024;

/// The NID of a symbol name: the first 8 bytes of SHA-1 over the ASCII name plus the fixed salt
/// (`518d64a635ded8c1e6b039b1c3e55230`), reversed, standard base64 without padding, with `/`
/// written as `-`. Same as `nid()` in `prepare.py`.
pub fn nid(name: &str) -> String {
    not_built!("prepare::nid")
}

/// The loadable view of a SELF or plain ELF: what `parse_self` / `parse_plain_elf` return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// The memory image, `vaddr` 0 first.
    pub bytes: Vec<u8>,
    pub entry: u64,
}

/// Read an executable (SELF or plain ELF) into its memory image. A missing required segment is
/// `Error::Prepare("required segment <n> is unavailable")`; an oversize or unmapped entry is
/// `Error::Prepare("invalid memory image")`.
pub fn load_image(executable: &[u8]) -> Result<Image> {
    not_built("prepare::load_image")
}

/// What the caller asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareOptions {
    /// Third-party patches the player left enabled (`patches::Selection`) apply on top.
    pub apply_patches: bool,
    /// The render size, or `None` for the game's own 1080p (`render_size`).
    pub render_size: Option<(u32, u32)>,
}

/// What an install holds, to be written as `install.json` and hashed into a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallManifest {
    pub game: Game,
    pub title_id: String,
    pub app_ver: String,
    /// Files under the install folder, relative with `/`. Never a file of the dump.
    pub files: Vec<String>,
}

/// Prepare `game` from the dump folder `dump_dir` into `install_dir`: gate on the revision
/// (`games::check_revision`), build and link the boot image, write the content profile,
/// compile patches and the render size. Returns the manifest.
pub fn prepare(
    dump_dir: &Path,
    install_dir: &Path,
    game: Game,
    options: &PrepareOptions,
) -> Result<InstallManifest> {
    not_built("prepare::prepare")
}

/// The folder a game's dump lives in and the executable file to use there: `BB_GAME_DIR_U1` ..
/// `_U3` first, else the shared `BB_GAME_DIR` (`locate` in `run.py`), read through `env` so
/// tests can fake it. A game dumped on its own may hold its executable as that folder's
/// `eboot.bin`, which is then the file returned. Neither variable set is `Error::Env(..)`.
pub fn locate(game: Game, env: impl Fn(&str) -> Option<String>) -> Result<(PathBuf, String)> {
    not_built("prepare::locate")
}
