//! Linking the boot image: `link_modules` resolves the imports of the executable and of the
//! modules it loads (`libc.prx`, `libSceFios2.prx`) into the image; `link_libc` does the same
//! for the older libc-only path. Both read the freshly prepared `boot.bin`
//! (`prepare::BOOT_MAGIC`) and write the linked image next to it. Replace
//! `scripts/link_modules.py` and `scripts/link_libc.py`; their Python tests are the expected
//! results.

use crate::error::{not_built, Result};
use std::path::{Path, PathBuf};

/// The modules linked by default (`DEFAULT_MODULES` in `scripts/link_modules.py`).
pub const DEFAULT_MODULES: [&str; 2] = ["libc.prx", "libSceFios2.prx"];

/// What a link produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    /// The linked boot image written under `out`.
    pub image: PathBuf,
    /// The modules that were linked in, by file name.
    pub modules: Vec<String>,
}

/// Link `modules` (file names under `<game>/sce_module/`) and the executable `exe` into the
/// boot image in `out`. A `boot.bin` that is not `BBPROBE2` is
/// `Error::Prepare("linker expects freshly prepared BBPROBE2")`.
pub fn link_modules(game: &Path, out: &Path, modules: &[&str], exe: &str) -> Result<Linked> {
    not_built("link::link_modules")
}

/// Link `<game>/eboot.bin` against `<game>/sce_module/libc.prx` only.
pub fn link_libc(game: &Path, out: &Path) -> Result<Linked> {
    not_built("link::link_libc")
}
