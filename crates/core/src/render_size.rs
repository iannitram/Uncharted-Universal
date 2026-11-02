//! Render-size patches by byte pattern: a render size other than the PS4's 1920x1080 (540p up,
//! 4K and beyond) for all three games, and the 720p Deck mode. Everything is found by byte
//! pattern, so 1.00 and 1.02 of every region are covered. Replaces `scripts/render_res.py`
//! (ported from BilalReloaded's port, GPL-2.0-or-later, credited there) and
//! `scripts/render_720p.py`; their Python tests are the expected results.

use crate::error::{not_built, Result};
use crate::games::Game;
use crate::patches::ByteWrite;

/// The PS4's output size.
pub const OUTPUT_SIZE: (u32, u32) = (1920, 1080);
/// Smallest and largest size `parse_size` accepts.
pub const MIN_SIZE: (u32, u32) = (640, 360);
pub const MAX_SIZE: (u32, u32) = (7680, 4320);

/// The upscaling presets (`UPSCALE_SCALES` in `run.py`): the game draws at the chosen
/// resolution divided by the scale and the upscaler fills the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpscaleQuality {
    Quality,
    Balanced,
    Performance,
    Ultra,
}

impl UpscaleQuality {
    /// 1.5, 1.7, 2.0, 3.0.
    pub fn scale(self) -> f64 {
        match self {
            UpscaleQuality::Quality => 1.5,
            UpscaleQuality::Balanced => 1.7,
            UpscaleQuality::Performance => 2.0,
            UpscaleQuality::Ultra => 3.0,
        }
    }
}

/// What a resolution patch did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patched {
    /// The patched memory image.
    pub image: Vec<u8>,
    /// The size the game renders at: smaller than asked when the game's memory layout cannot
    /// hold it (Drake's Fortune keeps its render targets in the GPU area).
    pub render: (u32, u32),
    /// Direct memory to add, in MiB (`BB_DMEM_MB`).
    pub extra_mib: u32,
}

/// Parse "WxH" (case-insensitive). Width must be a multiple of 8 and height of 4, within
/// `MIN_SIZE` .. `MAX_SIZE`; anything else is `Error::RenderSize("<w>x<h> is not supported ...")`.
pub fn parse_size(text: &str) -> Result<(u32, u32)> {
    not_built("render_size::parse_size")
}

/// Patch the boot image for `game` at `size`, trying smaller sizes in turn until one fits
/// (`patch_image` in `render_res.py`). No size that fits is `Error::RenderSize(..)`.
pub fn patch_image(image: &[u8], game: Game, size: (u32, u32)) -> Result<Patched> {
    not_built("render_size::patch_image")
}

/// The writes that make the executable start in its own 720p mode (Deck mode): the 1280x720
/// setup sites found by pattern. Zero sites, or more than three, is
/// `Error::RenderSize("expected 1 to 3 video setting sites, found <n>")`.
pub fn deck_720p_writes(elf: &[u8]) -> Result<Vec<ByteWrite>> {
    not_built("render_size::deck_720p_writes")
}
