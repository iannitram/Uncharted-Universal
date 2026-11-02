//! Render-size patches by byte pattern: a render size other than the PS4's 1920x1080 (540p up,
//! 4K and beyond) for all three games, and the 720p Deck mode. Everything is found by byte
//! pattern, so 1.00 and 1.02 of every region are covered. Replaces `scripts/render_res.py`
//! (ported from BilalReloaded's port, GPL-2.0-or-later, credited there) and
//! `scripts/render_720p.py`; their Python tests are the expected results.

use crate::error::{not_built, Result};
use crate::games::Game;
use crate::patches::ByteWrite;
use std::collections::BTreeMap;

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

/// Extra render-target memory a game needs per extra 1080p worth of pixels, in MiB (`RT_MIB_PER_1080P`,
/// `RT2_MIB_PER_1080P` in `run.py`; docs/resolution.md).
pub const RT_MIB_PER_1080P: u32 = 300;
pub const RT2_MIB_PER_1080P: u32 = 150;

/// The variables for the game's own render size (`BB_GAME_RES`, `game_res` in `run.py`, #139).
/// Reads `BB_GAME_RES` ("WxH", case-insensitive, `x` or the multiplication sign) from
/// `player_env`. Returns nothing when it is missing, not a size, outside `MIN_SIZE` ..
/// `MAX_SIZE`, or 1920x1080. Otherwise returns `BB_GAME_RES` as "<w>x<h>" and the heap
/// variables `BB_GAME_RT_MB`, `BB_GAME_RT2_MB`, `BB_GAME_FREE_MB`, `BB_GAME_MOVIE_MB` and, when
/// the pool needs it, `BB_DMEM_MB`. A heap variable the player set to digits keeps its value.
pub fn game_res_env(player_env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    // Get BB_GAME_RES from player_env, lowercase it and replace multiplication sign with 'x'
    let res_text = match player_env.get("BB_GAME_RES") {
        Some(text) => text.to_lowercase().replace('\u{d7}', "x"),
        None => return BTreeMap::new(),
    };

    // Split into width and height
    let parts: Vec<&str> = res_text.split('x').collect();
    if parts.len() != 2 {
        return BTreeMap::new();
    }

    // Parse width and height as integers
    let width = match parts[0].parse::<u32>() {
        Ok(w) => w,
        Err(_) => return BTreeMap::new(),
    };
    let height = match parts[1].parse::<u32>() {
        Ok(h) => h,
        Err(_) => return BTreeMap::new(),
    };

    // Check if the size is within bounds
    if width < MIN_SIZE.0 || width > MAX_SIZE.0 || height < MIN_SIZE.1 || height > MAX_SIZE.1 {
        return BTreeMap::new();
    }

    // Return empty map if it's exactly 1920x1080
    if width == 1920 && height == 1080 {
        return BTreeMap::new();
    }

    // Calculate scale factor
    let scale = (width as f64 * height as f64) / (1920.0 * 1080.0) - 1.0;

    // Helper function to check if a string is all digits (optionally with leading '-')
    let is_all_digits = |s: &str| -> bool {
        let digits = s.strip_prefix('-').unwrap_or(s);
        !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
    };

    // Helper function to compute heap sizes with rounding to nearest multiple of 64
    let compute_heap_size = |per_1080p: u32| -> u32 {
        if scale <= 0.0 {
            0
        } else {
            let value = (scale * per_1080p as f64).trunc() / 64.0;
            (value.ceil() * 64.0) as u32
        }
    };

    // Helper function to compute movie MB
    let compute_movie_mb = |width: u32, height: u32| -> u32 {
        // frame_width = ceil(width / 16) * 16
        let frame_width = width.div_ceil(16) * 16;
        // frame_height = ceil(height / 64) * 64
        let frame_height = height.div_ceil(64) * 64;
        // frame_bytes = frame_width * frame_height * 3 / 2 + 65536
        let frame_bytes = (frame_width as u64 * frame_height as u64 * 3 / 2) + 65536;
        // ceil(frame_bytes / 2097152) * 2, where 2097152 = 2^21
        let mb = frame_bytes.div_ceil(2097152); // Use div_ceil for ceiling division
        (mb * 2).max(4) as u32
    };

    // Build the result map
    let mut result = BTreeMap::new();

    // Always set BB_GAME_RES
    result.insert("BB_GAME_RES".to_string(), format!("{}x{}", width, height));

    // Handle BB_GAME_RT_MB
    if let Some(rt_mb) = player_env.get("BB_GAME_RT_MB") {
        if is_all_digits(rt_mb) {
            result.insert("BB_GAME_RT_MB".to_string(), rt_mb.clone());
        } else {
            result.insert(
                "BB_GAME_RT_MB".to_string(),
                compute_heap_size(RT_MIB_PER_1080P).to_string(),
            );
        }
    } else {
        result.insert(
            "BB_GAME_RT_MB".to_string(),
            compute_heap_size(RT_MIB_PER_1080P).to_string(),
        );
    }

    // Handle BB_GAME_RT2_MB
    if let Some(rt2_mb) = player_env.get("BB_GAME_RT2_MB") {
        if is_all_digits(rt2_mb) {
            result.insert("BB_GAME_RT2_MB".to_string(), rt2_mb.clone());
        } else {
            result.insert(
                "BB_GAME_RT2_MB".to_string(),
                compute_heap_size(RT2_MIB_PER_1080P).to_string(),
            );
        }
    } else {
        result.insert(
            "BB_GAME_RT2_MB".to_string(),
            compute_heap_size(RT2_MIB_PER_1080P).to_string(),
        );
    }

    // Handle BB_GAME_FREE_MB
    if let Some(free_mb) = player_env.get("BB_GAME_FREE_MB") {
        if is_all_digits(free_mb) {
            result.insert("BB_GAME_FREE_MB".to_string(), free_mb.clone());
        } else {
            result.insert("BB_GAME_FREE_MB".to_string(), "0".to_string());
        }
    } else {
        result.insert("BB_GAME_FREE_MB".to_string(), "0".to_string());
    }

    // Handle BB_GAME_MOVIE_MB
    if let Some(movie_mb) = player_env.get("BB_GAME_MOVIE_MB") {
        if is_all_digits(movie_mb) {
            result.insert("BB_GAME_MOVIE_MB".to_string(), movie_mb.clone());
        } else {
            result.insert(
                "BB_GAME_MOVIE_MB".to_string(),
                compute_movie_mb(width, height).to_string(),
            );
        }
    } else {
        result.insert(
            "BB_GAME_MOVIE_MB".to_string(),
            compute_movie_mb(width, height).to_string(),
        );
    }

    // Calculate pool size and BB_DMEM_MB
    let rt_mb = result
        .get("BB_GAME_RT_MB")
        .unwrap_or(&"0".to_string())
        .parse::<i64>()
        .unwrap_or(0);
    let rt2_mb = result
        .get("BB_GAME_RT2_MB")
        .unwrap_or(&"0".to_string())
        .parse::<i64>()
        .unwrap_or(0);
    let free_mb = result
        .get("BB_GAME_FREE_MB")
        .unwrap_or(&"0".to_string())
        .parse::<i64>()
        .unwrap_or(0);

    let pool = rt_mb + rt2_mb - free_mb;

    if let Some(dmem_mb) = player_env.get("BB_DMEM_MB") {
        if !dmem_mb.is_empty() {
            result.insert("BB_DMEM_MB".to_string(), dmem_mb.clone());
        } else if pool > 0 {
            let dmem_val = 5056 + pool;
            result.insert("BB_DMEM_MB".to_string(), dmem_val.min(16384).to_string());
        }
    } else if pool > 0 {
        let dmem_val = 5056 + pool;
        result.insert("BB_DMEM_MB".to_string(), dmem_val.min(16384).to_string());
    }

    result
}
