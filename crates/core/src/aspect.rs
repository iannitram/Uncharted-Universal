//! Aspect math for resolution independence (M21): the render size that fills a screen, the
//! 16:9 HUD safe area, the field-of-view mode and scale factors, and the letterbox or pillarbox
//! rectangle of a fixed-aspect image such as a video. Pure arithmetic: no I/O, no game patches;
//! the patch pieces (M21a-M21e) call it.
//!
//! Pixel sizes and rectangles are integers, computed in `u64` so no `u32` input overflows.
//! Every division rounds as its doc comment says. Only the FOV scale factors are `f64`.
//! A zero size or zero aspect is `Error::RenderSize(..)`.

use crate::error::{Error, Result};
use crate::render_size::{MAX_SIZE, MIN_SIZE};

/// The game's native aspect, width to height.
pub const NATIVE_ASPECT: (u32, u32) = (16, 9);

/// A screen counts as native 16:9 when its aspect is within this fraction (1%) of 16:9.
pub const NATIVE_TOLERANCE_PERCENT: u64 = 1;

/// How the game's field of view follows the screen shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FovMode {
    /// Within the tolerance of 16:9: the game's own field of view.
    Native,
    /// Wider than 16:9: the vertical field of view is kept, the horizontal one grows.
    HorPlus,
    /// Taller than 16:9: the horizontal field of view is kept, the vertical one grows.
    VertPlus,
}

/// The field-of-view mode and the factors to apply to the game's field of view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fov {
    pub mode: FovMode,
    /// Factor on the tangent of the horizontal half-angle: screen aspect / 16:9 for
    /// `HorPlus`, otherwise 1.0.
    pub horizontal: f64,
    /// Factor on the tangent of the vertical half-angle: 16:9 / screen aspect for
    /// `VertPlus`, otherwise 1.0.
    pub vertical: f64,
}

/// A pixel rectangle: `x`, `y` is the top-left corner (the offset from the container's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

fn refuse(message: String) -> Error {
    Error::RenderSize(message)
}

fn check_size(size: (u32, u32), what: &str) -> Result<()> {
    if size.0 == 0 || size.1 == 0 {
        return Err(refuse(format!(
            "{what}: size {}x{} must not be zero",
            size.0, size.1
        )));
    }
    Ok(())
}

/// The render size for a chosen vertical size on `screen`: the height is clamped to
/// `render_size::MIN_SIZE.1 ..= MAX_SIZE.1`, the width that fills the screen is rounded to the
/// nearest even number (a half rounds up) and clamped to `MIN_SIZE.0 ..= MAX_SIZE.0`.
/// A zero screen or `height` is `Error::RenderSize(..)`.
pub fn render_size(screen: (u32, u32), height: u32) -> Result<(u32, u32)> {
    check_size(screen, "Screen")?;
    if height == 0 {
        return Err(refuse("Render height must not be zero".to_owned()));
    }
    let height = height.clamp(MIN_SIZE.1, MAX_SIZE.1);
    let (sw, sh) = (u64::from(screen.0), u64::from(screen.1));
    // width = 2 * round(height * sw / (2 * sh)), rounding halves up.
    let pairs = (u64::from(height) * sw + sh) / (2 * sh);
    let width = u32::try_from(pairs * 2).unwrap_or(u32::MAX);
    Ok((width.clamp(MIN_SIZE.0, MAX_SIZE.0), height))
}

/// The largest rectangle of aspect `aspect` (width, height) that fits `container`, centred.
/// The shorter side is rounded down, and so is each offset (the odd pixel goes to the far
/// side). A wider container gives a pillarbox (`x` > 0), a taller one a letterbox (`y` > 0).
/// A zero container or aspect is `Error::RenderSize(..)`.
pub fn fit_rect(container: (u32, u32), aspect: (u32, u32)) -> Result<Rect> {
    check_size(container, "Container")?;
    check_size(aspect, "Aspect")?;
    let (cw, ch) = (u64::from(container.0), u64::from(container.1));
    let (aw, ah) = (u64::from(aspect.0), u64::from(aspect.1));
    // Compare cw/ch with aw/ah without dividing.
    let (width, height) = if cw * ah >= ch * aw {
        (ch * aw / ah, ch)
    } else {
        (cw, cw * ah / aw)
    };
    // Each is at most the container's side, so these fit u32.
    let (width, height) = (width as u32, height as u32);
    Ok(Rect {
        x: (container.0 - width) / 2,
        y: (container.1 - height) / 2,
        width,
        height,
    })
}

/// The HUD safe area: the 16:9 rectangle centred in `screen` ([`fit_rect`] with
/// [`NATIVE_ASPECT`], same rounding). A zero screen is `Error::RenderSize(..)`.
pub fn safe_area(screen: (u32, u32)) -> Result<Rect> {
    fit_rect(screen, NATIVE_ASPECT)
}

/// The FOV mode and factors for `screen`. `Native` when the aspect is within
/// [`NATIVE_TOLERANCE_PERCENT`] of 16:9 (both factors exactly 1.0), `HorPlus` when wider,
/// `VertPlus` when taller. The factors are the ratio of the two aspects as one `f64`
/// division of exact integer products, not rounded further. A zero screen is
/// `Error::RenderSize(..)`.
pub fn fov(screen: (u32, u32)) -> Result<Fov> {
    check_size(screen, "Screen")?;
    let wide = u64::from(screen.0) * u64::from(NATIVE_ASPECT.1);
    let tall = u64::from(screen.1) * u64::from(NATIVE_ASPECT.0);
    // Within tolerance: |wide - tall| <= tall * percent / 100.
    if wide.abs_diff(tall) * 100 <= tall * NATIVE_TOLERANCE_PERCENT {
        return Ok(Fov {
            mode: FovMode::Native,
            horizontal: 1.0,
            vertical: 1.0,
        });
    }
    Ok(if wide > tall {
        Fov {
            mode: FovMode::HorPlus,
            horizontal: wide as f64 / tall as f64,
            vertical: 1.0,
        }
    } else {
        Fov {
            mode: FovMode::VertPlus,
            horizontal: 1.0,
            vertical: tall as f64 / wide as f64,
        }
    })
}
