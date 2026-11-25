//! Pixel Fold 1 inside/outside screen detection and full M21 visual adaptation (Stage 7.07).
//!
//! Classifies the active Android display (`WindowMetrics` / `Display` / `Configuration`):
//! - Inside unfolded (`2208x1840` landscape 6:5, `1840x2208` portrait 5:6),
//! - Outside cover (`2092x1080` landscape ~17.4:9, `1080x2092` portrait ~9:17.4),
//! - Generic Android display.
//!
//! Computes the full M21 adaptation state on startup and across live fold/unfold transitions:
//! - Target render size (`M21a`),
//! - Camera FOV mode (`M21c`: `VertPlus` on 6:5 unfolded and tall cover; `HorPlus` on ~17.4:9
//!   landscape cover; `Native` within 1% of 16:9),
//! - 16:9 HUD/menu/pause/map safe area (`M21b`, never stretched),
//! - Pre-rendered video / cutscene fit rect (`M21d`, letterbox/pillarbox),
//! - Live fold/unfold presenter rescale rect + debounced restart-needed flag (`M21e`),
//! - Responsive first-boot prompt and touch-overlay bounds (hiding touch controls when a
//!   controller is connected).

use std::fmt;

/// Native PS4 game aspect ratio (width, height).
pub const NATIVE_ASPECT: (u32, u32) = (16, 9);

/// Aspect tolerance (in percent) within which a screen counts as native 16:9.
pub const NATIVE_TOLERANCE_PERCENT: u64 = 1;

/// Minimum and maximum supported render sizes, matching `uncharted_core::render_size`.
pub const MIN_RENDER_SIZE: (u32, u32) = (640, 360);
pub const MAX_RENDER_SIZE: (u32, u32) = (7680, 4320);

/// Default target vertical render height on Android.
pub const DEFAULT_RENDER_HEIGHT: u32 = 1080;

/// Pixel Fold 1 inside unfolded display dimensions (landscape 6:5).
pub const PIXEL_FOLD_INSIDE_LANDSCAPE: (u32, u32) = (2208, 1840);
/// Pixel Fold 1 inside unfolded display dimensions (portrait 5:6).
pub const PIXEL_FOLD_INSIDE_PORTRAIT: (u32, u32) = (1840, 2208);
/// Pixel Fold 1 outside cover display dimensions (landscape ~17.4:9).
pub const PIXEL_FOLD_COVER_LANDSCAPE: (u32, u32) = (2092, 1080);
/// Pixel Fold 1 outside cover display dimensions (portrait ~9:17.4).
pub const PIXEL_FOLD_COVER_PORTRAIT: (u32, u32) = (1080, 2092);
/// Pixel Fold 1 default display density (420 dpi / 2.625x), observed via `dumpsys display`.
pub const PIXEL_FOLD_DENSITY_DPI: u32 = 420;
/// Pixel Fold 1 outside cover display camera cutout height (133 px), observed via `dumpsys display`.
pub const PIXEL_FOLD_COVER_CUTOUT_PX: u32 = 133;

/// Aspect factor bounds (aspect / (16/9)), matching ACNH-Universal `nvn_aspect.h`:
/// - `0.45` allows Pixel Fold upright inner screen (5:6 -> k ≈ 0.469),
/// - `3.0` matches `android:max_aspect="3.0"`.
pub const MIN_ASPECT_FACTOR: f64 = 0.45;
pub const MAX_ASPECT_FACTOR: f64 = 3.0;
pub const LARGE_SCREEN_MIN_DP: i32 = 600;

/// Android `ActivityInfo` orientation constant (`android.content.pm.ActivityInfo`),
/// matching ACNH-Universal `apply_orientation`:
/// - `SensorLandscape (6)` on cover/phone screens (`smallestScreenWidthDp < 600`),
/// - `FullUser (13)` on unfolded Pixel Fold / tablets (`smallestScreenWidthDp >= 600`).
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestedOrientation {
    SensorLandscape = 6,
    FullUser = 13,
}

pub const fn requested_orientation_for_smallest_width_dp(
    smallest_width_dp: i32,
) -> RequestedOrientation {
    if smallest_width_dp >= LARGE_SCREEN_MIN_DP {
        RequestedOrientation::FullUser
    } else {
        RequestedOrientation::SensorLandscape
    }
}

pub fn clamped_aspect_factor(width: u32, height: u32) -> Result<f64, DisplayError> {
    if width == 0 || height == 0 {
        return Err(DisplayError::ZeroSize {
            what: "Display",
            width,
            height,
        });
    }
    let ratio = f64::from(width) / f64::from(height);
    let base = f64::from(NATIVE_ASPECT.0) / f64::from(NATIVE_ASPECT.1);
    Ok((ratio / base).clamp(MIN_ASPECT_FACTOR, MAX_ASPECT_FACTOR))
}

/// Dimension delta (in pixels) above which a live fold/unfold or resize offers a restart prompt
/// for a full re-render (`M21e`, `#436`).
pub const RESIZE_RESTART_THRESHOLD_PX: u32 = 16;

/// Default debounce window (in milliseconds) before surfacing the mid-game restart prompt on
/// fold/unfold transitions, matching `crates/runtime` and `bbgpu_notify_resize` (`250 ms`).
pub const RESIZE_DEBOUNCE_MS: u64 = 250;

/// Errors produced by invalid display or aspect dimensions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisplayError {
    ZeroSize {
        what: &'static str,
        width: u32,
        height: u32,
    },
    ZeroRenderHeight,
}

impl fmt::Display for DisplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroSize {
                what,
                width,
                height,
            } => write!(f, "{what}: size {width}x{height} must not be zero"),
            Self::ZeroRenderHeight => write!(f, "Render height must not be zero"),
        }
    }
}

impl std::error::Error for DisplayError {}

/// Active screen classification on Pixel Fold 1 vs. generic Android devices.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldScreen {
    /// Any other Android display.
    Generic = 0,
    /// Pixel Fold 1 inside unfolded panel (`2208x1840` / `1840x2208`).
    PixelFoldInside = 1,
    /// Pixel Fold 1 outside cover panel (`2092x1080` / `1080x2092`).
    PixelFoldCover = 2,
}

/// Physical hinge / fold posture reported by Android `WindowLayoutInfo` / `Display` state.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldPosture {
    Unknown = 0,
    Folded = 1,
    HalfOpened = 2,
    Flat = 3,
}

/// System-bar and display-cutout safe insets (in pixels) from Android `WindowMetrics`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DisplayInsets {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl DisplayInsets {
    pub const fn new(left: u32, top: u32, right: u32, bottom: u32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub const fn total_horizontal(self) -> u32 {
        self.left.saturating_add(self.right)
    }

    pub const fn total_vertical(self) -> u32 {
        self.top.saturating_add(self.bottom)
    }
}

/// Active display orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayOrientation {
    Landscape,
    Portrait,
}

/// How the 3D camera FOV follows the screen aspect ratio (`M21c`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FovMode {
    Native,
    HorPlus,
    VertPlus,
}

/// Camera FOV mode and tangent scale factors (`M21c`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fov {
    pub mode: FovMode,
    pub horizontal: f64,
    pub vertical: f64,
}

/// Integer pixel rectangle (`x`, `y` is top-left offset inside the container).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// Returns `true` if `self` lies completely within `(container_w, container_h)`.
    pub const fn fits_within(self, container_w: u32, container_h: u32) -> bool {
        self.x.saturating_add(self.width) <= container_w
            && self.y.saturating_add(self.height) <= container_h
    }

    /// Returns `true` if `self` and `other` have non-zero overlapping area.
    pub const fn overlaps(self, other: Self) -> bool {
        self.x < other.x.saturating_add(other.width)
            && other.x < self.x.saturating_add(self.width)
            && self.y < other.y.saturating_add(other.height)
            && other.y < self.y.saturating_add(self.height)
    }
}

/// Raw display metrics supplied by Android `WindowMetrics` / `Display` / `Configuration`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayMetrics {
    pub width: u32,
    pub height: u32,
}

impl DisplayMetrics {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub const fn as_tuple(self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub const fn orientation(self) -> DisplayOrientation {
        if self.width >= self.height {
            DisplayOrientation::Landscape
        } else {
            DisplayOrientation::Portrait
        }
    }

    /// Classifies whether these dimensions correspond to the Pixel Fold 1 inside screen,
    /// outside cover screen, or a generic Android display.
    ///
    /// Allows up to [`RESIZE_RESTART_THRESHOLD_PX`] (16 px) of system-bar variation on both panels,
    /// and up to [`PIXEL_FOLD_COVER_CUTOUT_PX`] (133 px) of camera-cutout inset along the long edge
    /// of the outside cover panel (e.g. `1080x1959` / `1959x1080`), without misclassifying
    /// `1920x1080` or `2400x1080` generic displays.
    pub const fn classify_fold_screen(self) -> FoldScreen {
        if matches_panel(self.width, self.height, PIXEL_FOLD_INSIDE_LANDSCAPE)
            || matches_panel(self.width, self.height, PIXEL_FOLD_INSIDE_PORTRAIT)
        {
            FoldScreen::PixelFoldInside
        } else if matches_panel(self.width, self.height, PIXEL_FOLD_COVER_LANDSCAPE)
            || matches_panel(self.width, self.height, PIXEL_FOLD_COVER_PORTRAIT)
            || matches_cover_with_cutout(self.width, self.height)
        {
            FoldScreen::PixelFoldCover
        } else {
            FoldScreen::Generic
        }
    }

    /// Infers the default [`FoldPosture`] from [`Self::classify_fold_screen`].
    pub const fn infer_posture(self) -> FoldPosture {
        match self.classify_fold_screen() {
            FoldScreen::PixelFoldInside => FoldPosture::Flat,
            FoldScreen::PixelFoldCover => FoldPosture::Folded,
            FoldScreen::Generic => FoldPosture::Unknown,
        }
    }
}

impl Default for DisplayMetrics {
    fn default() -> Self {
        Self::new(1920, 1080)
    }
}

/// Full Android `WindowMetrics` / `Display` / `Configuration` snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AndroidWindowMetrics {
    pub bounds: DisplayMetrics,
    pub insets: DisplayInsets,
    pub density_dpi: u32,
    pub display_id: u32,
    pub posture: FoldPosture,
}

impl AndroidWindowMetrics {
    pub const fn new(
        width: u32,
        height: u32,
        insets: DisplayInsets,
        density_dpi: u32,
        display_id: u32,
        posture: FoldPosture,
    ) -> Self {
        Self {
            bounds: DisplayMetrics::new(width, height),
            insets,
            density_dpi,
            display_id,
            posture,
        }
    }

    /// Reconstructs the full physical panel dimensions by adding cutout/system-bar insets.
    pub const fn physical_metrics(self) -> DisplayMetrics {
        DisplayMetrics::new(
            self.bounds
                .width
                .saturating_add(self.insets.total_horizontal()),
            self.bounds
                .height
                .saturating_add(self.insets.total_vertical()),
        )
    }

    /// Classifies the active panel using physical dimensions (with insets restored) first,
    /// falling back to raw window bounds.
    pub const fn classify_fold_screen(self) -> FoldScreen {
        match self.physical_metrics().classify_fold_screen() {
            FoldScreen::Generic => self.bounds.classify_fold_screen(),
            panel => panel,
        }
    }
}

const fn matches_panel(w: u32, h: u32, target: (u32, u32)) -> bool {
    w.abs_diff(target.0) <= RESIZE_RESTART_THRESHOLD_PX
        && h.abs_diff(target.1) <= RESIZE_RESTART_THRESHOLD_PX
}

const fn matches_cover_with_cutout(w: u32, h: u32) -> bool {
    let (short, long) = if w <= h { (w, h) } else { (h, w) };
    let min_long =
        PIXEL_FOLD_COVER_PORTRAIT.1 - PIXEL_FOLD_COVER_CUTOUT_PX - RESIZE_RESTART_THRESHOLD_PX;
    let max_long = PIXEL_FOLD_COVER_PORTRAIT.1 + RESIZE_RESTART_THRESHOLD_PX;
    short.abs_diff(PIXEL_FOLD_COVER_PORTRAIT.0) <= RESIZE_RESTART_THRESHOLD_PX
        && long >= min_long
        && long <= max_long
}

fn check_size(size: (u32, u32), what: &'static str) -> Result<(), DisplayError> {
    if size.0 == 0 || size.1 == 0 {
        return Err(DisplayError::ZeroSize {
            what,
            width: size.0,
            height: size.1,
        });
    }
    Ok(())
}

/// Computes the render size for a chosen vertical height on `screen`, matching
/// `uncharted_core::aspect::render_size`.
pub fn render_size(screen: (u32, u32), height: u32) -> Result<(u32, u32), DisplayError> {
    check_size(screen, "Screen")?;
    if height == 0 {
        return Err(DisplayError::ZeroRenderHeight);
    }
    let height = height.clamp(MIN_RENDER_SIZE.1, MAX_RENDER_SIZE.1);
    let (sw, sh) = (u64::from(screen.0), u64::from(screen.1));
    let pairs = (u64::from(height) * sw + sh) / (2 * sh);
    let width = u32::try_from(pairs * 2).unwrap_or(u32::MAX);
    Ok((width.clamp(MIN_RENDER_SIZE.0, MAX_RENDER_SIZE.0), height))
}

/// Computes the largest centered rectangle of `aspect` fitting inside `container`, matching
/// `uncharted_core::aspect::fit_rect` and `bbgpu_movie_fit_rect`.
pub fn fit_rect(container: (u32, u32), aspect: (u32, u32)) -> Result<Rect, DisplayError> {
    check_size(container, "Container")?;
    check_size(aspect, "Aspect")?;
    let (cw, ch) = (u64::from(container.0), u64::from(container.1));
    let (aw, ah) = (u64::from(aspect.0), u64::from(aspect.1));
    let (width, height) = if cw * ah >= ch * aw {
        (ch * aw / ah, ch)
    } else {
        (cw, cw * ah / aw)
    };
    let (width, height) = (width as u32, height as u32);
    Ok(Rect {
        x: (container.0 - width) / 2,
        y: (container.1 - height) / 2,
        width,
        height,
    })
}

/// Computes the centered 16:9 HUD/menu/pause/map safe area (`M21b`, `bbgpu_safe_area`).
pub fn safe_area(screen: (u32, u32)) -> Result<Rect, DisplayError> {
    fit_rect(screen, NATIVE_ASPECT)
}

/// Computes the centered pre-rendered video / cutscene rectangle (`M21d`, `bbgpu_movie_fit_rect`).
pub fn movie_fit_rect(screen: (u32, u32), movie_aspect: (u32, u32)) -> Result<Rect, DisplayError> {
    fit_rect(screen, movie_aspect)
}

/// Computes the camera FOV mode and tangent scale factors (`M21c`), matching
/// `uncharted_core::aspect::fov`.
pub fn fov(screen: (u32, u32)) -> Result<Fov, DisplayError> {
    check_size(screen, "Screen")?;
    let wide = u64::from(screen.0) * u64::from(NATIVE_ASPECT.1);
    let tall = u64::from(screen.1) * u64::from(NATIVE_ASPECT.0);
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

/// Returns `true` when `current` diverges from `startup` by more than
/// [`RESIZE_RESTART_THRESHOLD_PX`] in width or height (`M21e`).
pub const fn resize_needs_restart(startup: (u32, u32), current: (u32, u32)) -> bool {
    startup.0 > 0
        && startup.1 > 0
        && current.0 > 0
        && current.1 > 0
        && (startup.0.abs_diff(current.0) > RESIZE_RESTART_THRESHOLD_PX
            || startup.1.abs_diff(current.1) > RESIZE_RESTART_THRESHOLD_PX)
}

/// Button arrangement for the responsive first-boot / restart prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptButtonArrangement {
    Horizontal,
    Vertical,
}

/// Responsive layout bounds for the Stage 7.04 first-boot download prompt and restart banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromptLayout {
    pub bounds: Rect,
    pub button_arrangement: PromptButtonArrangement,
}

/// Responsive on-screen touch-overlay layout bounds.
///
/// Hidden (`visible == false`) whenever a hardware or virtual gamepad is connected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchOverlayLayout {
    pub visible: bool,
    pub left_cluster: Rect,
    pub right_cluster: Rect,
    pub pause_button: Rect,
}

/// Full M21 visual adaptation snapshot for the current Android display state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayAdaptation {
    pub metrics: DisplayMetrics,
    pub insets: DisplayInsets,
    pub screen: FoldScreen,
    pub posture: FoldPosture,
    pub orientation: DisplayOrientation,
    pub aspect_ratio: f64,
    pub target_render_size: (u32, u32),
    pub presenter_rect: Rect,
    pub safe_area: Rect,
    pub movie_fit_rect: Rect,
    pub fov: Fov,
    pub needs_restart: bool,
    pub prompt_layout: PromptLayout,
    pub touch_overlay: TouchOverlayLayout,
}

impl DisplayAdaptation {
    /// Formats `BB_GAME_RES` (`<w>x<h>`) for M21a render-size patching.
    pub fn bb_game_res_env(&self) -> String {
        format!(
            "{}x{}",
            self.target_render_size.0, self.target_render_size.1
        )
    }

    /// Formats `BB_MOVIE_SIZE` (`<w>x<h>`) for M21d pre-rendered video canvas scaling.
    pub fn bb_movie_size_env(&self) -> String {
        format!("{}x{}", self.metrics.width, self.metrics.height)
    }
}

fn compute_prompt_layout(metrics: DisplayMetrics, screen: FoldScreen, safe: Rect) -> PromptLayout {
    let (sw, sh) = metrics.as_tuple();
    // Anchor prompt inside the safe area when it has enough room, otherwise use the full screen
    // with a modest margin so narrow portrait cover screens stay readable.
    let (max_w, max_h, arrangement) = match (screen, metrics.orientation()) {
        (FoldScreen::PixelFoldCover, DisplayOrientation::Portrait) => (
            (sw * 88) / 100,
            (sh * 28) / 100,
            PromptButtonArrangement::Vertical,
        ),
        (FoldScreen::PixelFoldInside, _) => (
            (safe.width * 62) / 100,
            (safe.height * 42) / 100,
            PromptButtonArrangement::Horizontal,
        ),
        (_, DisplayOrientation::Portrait) => (
            (sw * 88) / 100,
            (sh * 30) / 100,
            PromptButtonArrangement::Vertical,
        ),
        (_, DisplayOrientation::Landscape) => (
            (safe.width * 58) / 100,
            (safe.height * 46) / 100,
            PromptButtonArrangement::Horizontal,
        ),
    };

    let width = max_w.clamp(1, sw);
    let height = max_h.clamp(1, sh);
    let bounds = Rect {
        x: (sw - width) / 2,
        y: (sh - height) / 2,
        width,
        height,
    };
    PromptLayout {
        bounds,
        button_arrangement: arrangement,
    }
}

fn compute_touch_overlay(
    metrics: DisplayMetrics,
    insets: DisplayInsets,
    screen: FoldScreen,
    controller_connected: bool,
) -> TouchOverlayLayout {
    let (sw, sh) = metrics.as_tuple();
    let visible = !controller_connected;

    // Size clusters proportionally to the shorter screen edge while keeping them non-overlapping
    // even on narrow portrait cover screens, and clear of display cutouts / system-bar insets.
    let short = sw.min(sh);
    let margin = (short / 24).max(16).min(sw / 12).min(sh / 12);
    let left_margin = margin.max(insets.left).min(sw.saturating_sub(2) / 3);
    let right_margin = margin.max(insets.right).min(sw.saturating_sub(2) / 3);
    let top_margin = margin.max(insets.top).min(sh.saturating_sub(2) / 3);
    let bottom_margin = margin.max(insets.bottom).min(sh.saturating_sub(2) / 3);
    let max_cluster_w = sw.saturating_sub(left_margin + right_margin + margin) / 2;
    let max_cluster_h = sh.saturating_sub(top_margin + bottom_margin);

    let cluster_size = match (screen, metrics.orientation()) {
        (FoldScreen::PixelFoldCover, DisplayOrientation::Portrait) => (short * 32) / 100,
        (FoldScreen::PixelFoldInside, _) => (short * 22) / 100,
        _ => (short * 26) / 100,
    }
    .clamp(1, max_cluster_w.max(1))
    .clamp(1, max_cluster_h.max(1));

    let left_cluster = Rect {
        x: left_margin,
        y: sh.saturating_sub(bottom_margin + cluster_size),
        width: cluster_size,
        height: cluster_size,
    };
    let right_cluster = Rect {
        x: sw.saturating_sub(right_margin + cluster_size),
        y: sh.saturating_sub(bottom_margin + cluster_size),
        width: cluster_size,
        height: cluster_size,
    };

    let pause_w = (short / 10).clamp(1, sw.saturating_sub(left_margin + right_margin).max(1));
    let pause_h = (short / 14).clamp(1, sh.saturating_sub(top_margin + bottom_margin).max(1));
    let pause_button = Rect {
        x: sw.saturating_sub(right_margin + pause_w),
        y: top_margin,
        width: pause_w,
        height: pause_h,
    };

    TouchOverlayLayout {
        visible,
        left_cluster,
        right_cluster,
        pause_button,
    }
}

/// Stateful M21 display & fold/unfold adaptation manager for an Android APK session.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayState {
    startup_metrics: DisplayMetrics,
    startup_render: (u32, u32),
    target_height: u32,
    debounce_ms: u64,
    pending_size: Option<(u32, u32)>,
    pending_restart_since_ms: Option<u64>,
    controller_connected: bool,
    current: DisplayAdaptation,
}

impl DisplayState {
    /// Initializes M21 adaptation state from the startup display metrics.
    pub fn new(
        startup_metrics: DisplayMetrics,
        controller_connected: bool,
    ) -> Result<Self, DisplayError> {
        Self::with_options(
            startup_metrics,
            DEFAULT_RENDER_HEIGHT,
            RESIZE_DEBOUNCE_MS,
            controller_connected,
        )
    }

    /// Initializes M21 adaptation state from a full Android `WindowMetrics` snapshot.
    pub fn from_window_metrics(
        wm: AndroidWindowMetrics,
        controller_connected: bool,
    ) -> Result<Self, DisplayError> {
        let startup_render = render_size(wm.bounds.as_tuple(), DEFAULT_RENDER_HEIGHT)?;
        let screen = wm.classify_fold_screen();
        let posture = match wm.posture {
            FoldPosture::Unknown => wm.bounds.infer_posture(),
            other => other,
        };
        let current = build_adaptation(
            wm.bounds,
            wm.insets,
            screen,
            posture,
            startup_render,
            DEFAULT_RENDER_HEIGHT,
            false,
            controller_connected,
        )?;
        Ok(Self {
            startup_metrics: wm.bounds,
            startup_render,
            target_height: DEFAULT_RENDER_HEIGHT,
            debounce_ms: RESIZE_DEBOUNCE_MS,
            pending_size: None,
            pending_restart_since_ms: None,
            controller_connected,
            current,
        })
    }

    /// Initializes M21 adaptation state with explicit target render height and debounce window.
    pub fn with_options(
        startup_metrics: DisplayMetrics,
        target_height: u32,
        debounce_ms: u64,
        controller_connected: bool,
    ) -> Result<Self, DisplayError> {
        let startup_render = render_size(startup_metrics.as_tuple(), target_height)?;
        let current = build_adaptation(
            startup_metrics,
            DisplayInsets::default(),
            startup_metrics.classify_fold_screen(),
            startup_metrics.infer_posture(),
            startup_render,
            target_height,
            false,
            controller_connected,
        )?;
        Ok(Self {
            startup_metrics,
            startup_render,
            target_height,
            debounce_ms,
            pending_size: None,
            pending_restart_since_ms: None,
            controller_connected,
            current,
        })
    }

    /// Returns the startup render resolution locked for the active game process.
    pub const fn startup_render(&self) -> (u32, u32) {
        self.startup_render
    }

    /// Returns the latest computed M21 visual adaptation snapshot.
    pub const fn current(&self) -> &DisplayAdaptation {
        &self.current
    }

    /// Updates whether a hardware/virtual controller is connected, immediately hiding or showing
    /// on-screen touch controls.
    pub fn set_controller_connected(&mut self, connected: bool) -> &DisplayAdaptation {
        if self.controller_connected != connected {
            self.controller_connected = connected;
            self.current.touch_overlay = compute_touch_overlay(
                self.current.metrics,
                self.current.insets,
                self.current.screen,
                connected,
            );
        }
        &self.current
    }

    /// Applies a live display / fold-unfold configuration change at `timestamp_ms`.
    ///
    /// Presenter rescale, FOV, 16:9 safe area, movie fit rect, prompt bounds, and touch overlay
    /// update immediately; `needs_restart` is debounced over [`Self::debounce_ms`] so transient
    /// hinge/inset events do not flash a spurious restart prompt.
    pub fn on_display_change(
        &mut self,
        metrics: DisplayMetrics,
        timestamp_ms: u64,
    ) -> Result<&DisplayAdaptation, DisplayError> {
        self.apply_change(
            metrics,
            DisplayInsets::default(),
            metrics.classify_fold_screen(),
            metrics.infer_posture(),
            timestamp_ms,
        )
    }

    /// Applies a live Android `WindowMetrics` / `Configuration` fold-unfold change at `timestamp_ms`.
    pub fn on_window_metrics_change(
        &mut self,
        wm: AndroidWindowMetrics,
        timestamp_ms: u64,
    ) -> Result<&DisplayAdaptation, DisplayError> {
        let screen = wm.classify_fold_screen();
        let posture = match wm.posture {
            FoldPosture::Unknown => match screen {
                FoldScreen::PixelFoldInside => FoldPosture::Flat,
                FoldScreen::PixelFoldCover => FoldPosture::Folded,
                FoldScreen::Generic => FoldPosture::Unknown,
            },
            other => other,
        };
        self.apply_change(wm.bounds, wm.insets, screen, posture, timestamp_ms)
    }

    fn apply_change(
        &mut self,
        metrics: DisplayMetrics,
        insets: DisplayInsets,
        screen: FoldScreen,
        posture: FoldPosture,
        timestamp_ms: u64,
    ) -> Result<&DisplayAdaptation, DisplayError> {
        let raw_needs_restart =
            resize_needs_restart(self.startup_metrics.as_tuple(), metrics.as_tuple());
        let debounced_restart = if !raw_needs_restart {
            self.pending_size = None;
            self.pending_restart_since_ms = None;
            false
        } else if self.debounce_ms == 0 {
            self.pending_size = Some(metrics.as_tuple());
            self.pending_restart_since_ms = Some(timestamp_ms);
            true
        } else {
            self.pending_size = Some(metrics.as_tuple());
            let since = *self.pending_restart_since_ms.get_or_insert(timestamp_ms);
            timestamp_ms.saturating_sub(since) >= self.debounce_ms
        };

        self.current = build_adaptation(
            metrics,
            insets,
            screen,
            posture,
            self.startup_render,
            self.target_height,
            debounced_restart,
            self.controller_connected,
        )?;
        Ok(&self.current)
    }

    /// Advances the debounce clock without a dimension change and returns the updated snapshot.
    pub fn tick(&mut self, now_ms: u64) -> &DisplayAdaptation {
        if let Some(since) = self.pending_restart_since_ms {
            if now_ms.saturating_sub(since) >= self.debounce_ms
                && resize_needs_restart(
                    self.startup_metrics.as_tuple(),
                    self.current.metrics.as_tuple(),
                )
            {
                self.current.needs_restart = true;
            }
        }
        &self.current
    }
}

#[allow(clippy::too_many_arguments)]
fn build_adaptation(
    metrics: DisplayMetrics,
    insets: DisplayInsets,
    screen: FoldScreen,
    posture: FoldPosture,
    startup_render: (u32, u32),
    target_height: u32,
    needs_restart: bool,
    controller_connected: bool,
) -> Result<DisplayAdaptation, DisplayError> {
    let screen_tuple = metrics.as_tuple();
    let orientation = metrics.orientation();
    let aspect_ratio = f64::from(metrics.width) / f64::from(metrics.height.max(1));
    let target_render_size = render_size(screen_tuple, target_height)?;
    let presenter_rect = fit_rect(screen_tuple, startup_render)?;
    let safe = safe_area(screen_tuple)?;
    let movie = movie_fit_rect(screen_tuple, (1920, 1080))?;
    let computed_fov = fov(screen_tuple)?;
    let prompt_layout = compute_prompt_layout(metrics, screen, safe);
    let touch_overlay = compute_touch_overlay(metrics, insets, screen, controller_connected);

    Ok(DisplayAdaptation {
        metrics,
        insets,
        screen,
        posture,
        orientation,
        aspect_ratio,
        target_render_size,
        presenter_rect,
        safe_area: safe,
        movie_fit_rect: movie,
        fov: computed_fov,
        needs_restart,
        prompt_layout,
        touch_overlay,
    })
}

/// C-ABI rectangle matching `BbMovieRect` / `RuntimeRect`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AndroidRectC {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl From<Rect> for AndroidRectC {
    fn from(r: Rect) -> Self {
        Self {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

/// C-ABI FOV mode and factors matching `BbFovScale` / `RuntimeDisplayFov`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AndroidFovC {
    pub mode: u32,
    pub horizontal: f32,
    pub vertical: f32,
}

/// C-ABI M21 display adaptation snapshot for Android `NativeActivity` / JNI callers.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AndroidDisplayAdaptationC {
    pub screen: u32,
    pub posture: u32,
    pub render_width: u32,
    pub render_height: u32,
    pub aspect_ratio: f32,
    pub presenter_rect: AndroidRectC,
    pub safe_area: AndroidRectC,
    pub movie_fit_rect: AndroidRectC,
    pub fov: AndroidFovC,
    pub needs_restart: bool,
    pub touch_overlay_visible: bool,
}

/// Classifies an Android display into `0` (Generic), `1` (PixelFoldInside), or `2` (PixelFoldCover).
#[no_mangle]
pub extern "C" fn uncharted_android_classify_fold_screen(width: u32, height: u32) -> u32 {
    DisplayMetrics::new(width, height).classify_fold_screen() as u32
}

/// Computes the M21a render size for `screen_w x screen_h` at `target_height`.
///
/// # Safety
/// `out_w` and `out_h` must be valid, non-null, writable pointers.
#[no_mangle]
pub unsafe extern "C" fn uncharted_android_render_size(
    screen_w: u32,
    screen_h: u32,
    target_height: u32,
    out_w: *mut u32,
    out_h: *mut u32,
) -> bool {
    if out_w.is_null() || out_h.is_null() {
        return false;
    }
    let Ok((w, h)) = render_size((screen_w, screen_h), target_height) else {
        return false;
    };
    *out_w = w;
    *out_h = h;
    true
}

/// Computes the M21c camera FOV mode (`0` Native, `1` HorPlus, `2` VertPlus) and scale factors.
#[no_mangle]
pub extern "C" fn uncharted_android_fov(screen_w: u32, screen_h: u32) -> AndroidFovC {
    match fov((screen_w, screen_h)) {
        Ok(f) => AndroidFovC {
            mode: match f.mode {
                FovMode::Native => 0,
                FovMode::HorPlus => 1,
                FovMode::VertPlus => 2,
            },
            horizontal: f.horizontal as f32,
            vertical: f.vertical as f32,
        },
        Err(_) => AndroidFovC {
            mode: 0,
            horizontal: 1.0,
            vertical: 1.0,
        },
    }
}

/// Computes the M21b 16:9 HUD/menu/pause/map safe area (`bbgpu_safe_area`).
#[no_mangle]
pub extern "C" fn uncharted_android_safe_area(screen_w: u32, screen_h: u32) -> AndroidRectC {
    safe_area((screen_w, screen_h))
        .map(AndroidRectC::from)
        .unwrap_or_default()
}

/// Computes the M21d centered pre-rendered video fit rect (`bbgpu_movie_fit_rect`).
#[no_mangle]
pub extern "C" fn uncharted_android_movie_fit_rect(
    screen_w: u32,
    screen_h: u32,
    movie_w: u32,
    movie_h: u32,
) -> AndroidRectC {
    movie_fit_rect((screen_w, screen_h), (movie_w, movie_h))
        .map(AndroidRectC::from)
        .unwrap_or_default()
}

/// Returns `true` when `cur_w x cur_h` diverges from `start_w x start_h` by >16 px (M21e).
#[no_mangle]
pub extern "C" fn uncharted_android_resize_needs_restart(
    start_w: u32,
    start_h: u32,
    cur_w: u32,
    cur_h: u32,
) -> bool {
    resize_needs_restart((start_w, start_h), (cur_w, cur_h))
}

/// Computes a full M21 display adaptation snapshot from startup and current dimensions.
///
/// # Safety
/// `out` must be a valid, non-null, writable pointer.
#[no_mangle]
pub unsafe extern "C" fn uncharted_android_compute_display_adaptation(
    startup_w: u32,
    startup_h: u32,
    cur_w: u32,
    cur_h: u32,
    controller_connected: bool,
    out: *mut AndroidDisplayAdaptationC,
) -> bool {
    if out.is_null() {
        return false;
    }
    let Ok(mut state) = DisplayState::with_options(
        DisplayMetrics::new(startup_w, startup_h),
        DEFAULT_RENDER_HEIGHT,
        0,
        controller_connected,
    ) else {
        return false;
    };
    let Ok(cur) = state.on_display_change(DisplayMetrics::new(cur_w, cur_h), 0) else {
        return false;
    };
    *out = AndroidDisplayAdaptationC {
        screen: cur.screen as u32,
        posture: cur.posture as u32,
        render_width: cur.target_render_size.0,
        render_height: cur.target_render_size.1,
        aspect_ratio: cur.aspect_ratio as f32,
        presenter_rect: cur.presenter_rect.into(),
        safe_area: cur.safe_area.into(),
        movie_fit_rect: cur.movie_fit_rect.into(),
        fov: AndroidFovC {
            mode: match cur.fov.mode {
                FovMode::Native => 0,
                FovMode::HorPlus => 1,
                FovMode::VertPlus => 2,
            },
            horizontal: cur.fov.horizontal as f32,
            vertical: cur.fov.vertical as f32,
        },
        needs_restart: cur.needs_restart,
        touch_overlay_visible: cur.touch_overlay.visible,
    };
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn classifies_pixel_fold_inside_cover_and_generic_displays() {
        let inside_land = DisplayMetrics::new(2208, 1840);
        assert_eq!(
            inside_land.classify_fold_screen(),
            FoldScreen::PixelFoldInside
        );
        assert_eq!(inside_land.orientation(), DisplayOrientation::Landscape);

        let inside_port = DisplayMetrics::new(1840, 2208);
        assert_eq!(
            inside_port.classify_fold_screen(),
            FoldScreen::PixelFoldInside
        );
        assert_eq!(inside_port.orientation(), DisplayOrientation::Portrait);

        let cover_land = DisplayMetrics::new(2092, 1080);
        assert_eq!(
            cover_land.classify_fold_screen(),
            FoldScreen::PixelFoldCover
        );
        assert_eq!(cover_land.orientation(), DisplayOrientation::Landscape);

        let cover_port = DisplayMetrics::new(1080, 2092);
        assert_eq!(
            cover_port.classify_fold_screen(),
            FoldScreen::PixelFoldCover
        );
        assert_eq!(cover_port.orientation(), DisplayOrientation::Portrait);

        // Small system-bar inset within 16 px still classifies as Pixel Fold.
        assert_eq!(
            DisplayMetrics::new(2208, 1824).classify_fold_screen(),
            FoldScreen::PixelFoldInside
        );
        assert_eq!(
            DisplayMetrics::new(2092, 1064).classify_fold_screen(),
            FoldScreen::PixelFoldCover
        );

        // Generic phones/tablets must not be misclassified as Pixel Fold 1.
        assert_eq!(
            DisplayMetrics::new(1920, 1080).classify_fold_screen(),
            FoldScreen::Generic
        );
        assert_eq!(
            DisplayMetrics::new(2400, 1080).classify_fold_screen(),
            FoldScreen::Generic
        );
        assert_eq!(
            DisplayMetrics::new(2560, 1440).classify_fold_screen(),
            FoldScreen::Generic
        );
    }

    #[test]
    fn cover_portrait_to_cover_landscape_to_inside_unfolded_to_cover_transitions() {
        // 1. Start on Cover Portrait (1080x2092): VertPlus, letterboxed 16:9 safe area & video.
        let mut state =
            DisplayState::new(DisplayMetrics::new(1080, 2092), false).expect("valid startup");
        assert_eq!(state.startup_render(), (640, 1080));
        {
            let cur = state.current();
            assert_eq!(cur.screen, FoldScreen::PixelFoldCover);
            assert_eq!(cur.orientation, DisplayOrientation::Portrait);
            assert_eq!(cur.fov.mode, FovMode::VertPlus);
            assert_eq!(cur.fov.horizontal, 1.0);
            assert!(near(cur.fov.vertical, (2092.0 * 16.0) / (1080.0 * 9.0)));
            assert_eq!(
                cur.safe_area,
                Rect {
                    x: 0,
                    y: 742,
                    width: 1080,
                    height: 607,
                }
            );
            assert_eq!(cur.movie_fit_rect, cur.safe_area);
            assert!(!cur.needs_restart);
            assert_eq!(
                cur.prompt_layout.button_arrangement,
                PromptButtonArrangement::Vertical
            );
            assert!(cur.prompt_layout.bounds.fits_within(1080, 2092));
            assert!(cur.touch_overlay.visible);
            assert!(cur.touch_overlay.left_cluster.fits_within(1080, 2092));
            assert!(cur.touch_overlay.right_cluster.fits_within(1080, 2092));
            assert!(cur.touch_overlay.pause_button.fits_within(1080, 2092));
            assert!(!cur
                .touch_overlay
                .left_cluster
                .overlaps(cur.touch_overlay.right_cluster));
        }

        // 2. Rotate to Cover Landscape (2092x1080) at t=100ms: visual layout updates
        // immediately, restart prompt is debounced until t=250ms.
        let cur = state
            .on_display_change(DisplayMetrics::new(2092, 1080), 100)
            .unwrap();
        assert_eq!(cur.screen, FoldScreen::PixelFoldCover);
        assert_eq!(cur.orientation, DisplayOrientation::Landscape);
        assert_eq!(cur.fov.mode, FovMode::HorPlus);
        assert!(near(cur.fov.horizontal, 523.0 / 480.0));
        assert_eq!(cur.fov.vertical, 1.0);
        assert_eq!(
            cur.safe_area,
            Rect {
                x: 86,
                y: 0,
                width: 1920,
                height: 1080,
            }
        );
        assert_eq!(cur.movie_fit_rect, cur.safe_area);
        assert!(cur.presenter_rect.fits_within(2092, 1080));
        assert!(!cur.needs_restart, "debounced at t=100ms");
        assert_eq!(
            cur.prompt_layout.button_arrangement,
            PromptButtonArrangement::Horizontal
        );

        let cur = state.tick(100 + RESIZE_DEBOUNCE_MS);
        assert!(cur.needs_restart, "restart prompt surfaces after debounce");

        // 3. Unfold to Inside Unfolded (2208x1840, 6:5) after debounce: VertPlus + letterboxed safe area.
        let cur = state
            .on_display_change(
                DisplayMetrics::new(2208, 1840),
                100 + RESIZE_DEBOUNCE_MS + 50,
            )
            .unwrap();
        assert_eq!(cur.screen, FoldScreen::PixelFoldInside);
        assert_eq!(cur.orientation, DisplayOrientation::Landscape);
        assert_eq!(cur.target_render_size, (1296, 1080));
        assert_eq!(cur.fov.mode, FovMode::VertPlus);
        assert_eq!(cur.fov.horizontal, 1.0);
        assert!(near(cur.fov.vertical, 40.0 / 27.0));
        assert_eq!(
            cur.safe_area,
            Rect {
                x: 0,
                y: 299,
                width: 2208,
                height: 1242,
            }
        );
        assert_eq!(cur.movie_fit_rect, cur.safe_area);
        assert!(cur.presenter_rect.fits_within(2208, 1840));
        assert!(cur.needs_restart);
        assert!(cur.prompt_layout.bounds.fits_within(2208, 1840));
        assert!(cur.touch_overlay.left_cluster.fits_within(2208, 1840));
        assert!(cur.touch_overlay.right_cluster.fits_within(2208, 1840));
        assert!(!cur
            .touch_overlay
            .left_cluster
            .overlaps(cur.touch_overlay.right_cluster));

        // 4. Fold back to Cover Portrait (1080x2092) at t=500ms: clears restart flag immediately.
        let cur = state
            .on_display_change(DisplayMetrics::new(1080, 2092), 500)
            .unwrap();
        assert_eq!(cur.screen, FoldScreen::PixelFoldCover);
        assert_eq!(cur.orientation, DisplayOrientation::Portrait);
        assert_eq!(cur.fov.mode, FovMode::VertPlus);
        assert_eq!(cur.safe_area.width, 1080);
        assert_eq!(cur.safe_area.height, 607);
        assert!(!cur.needs_restart);
    }

    #[test]
    fn transient_fold_bounce_within_debounce_window_does_not_prompt_restart() {
        let mut state = DisplayState::new(DisplayMetrics::new(2208, 1840), false).unwrap();
        assert!(!state.current().needs_restart);

        // Transiently report cover dimensions at t=1000ms, then settle back to unfolded at t=1050ms.
        let cur = state
            .on_display_change(DisplayMetrics::new(2092, 1080), 1000)
            .unwrap();
        assert!(!cur.needs_restart);

        let cur = state
            .on_display_change(DisplayMetrics::new(2208, 1840), 1050)
            .unwrap();
        assert!(!cur.needs_restart);

        let cur = state.tick(1500);
        assert!(!cur.needs_restart);
    }

    #[test]
    fn controller_connection_hides_and_restores_touch_overlay() {
        let mut state = DisplayState::new(DisplayMetrics::new(2208, 1840), false).unwrap();
        assert!(state.current().touch_overlay.visible);

        state.set_controller_connected(true);
        assert!(!state.current().touch_overlay.visible);

        // Remains hidden across fold/unfold transitions while controller is connected.
        let cur = state
            .on_display_change(DisplayMetrics::new(1080, 2092), 200)
            .unwrap();
        assert!(!cur.touch_overlay.visible);

        // Disconnecting controller restores touch overlay.
        state.set_controller_connected(false);
        assert!(state.current().touch_overlay.visible);
    }

    #[test]
    fn zero_dimensions_fail_closed() {
        assert_eq!(
            DisplayState::new(DisplayMetrics::new(0, 1080), false).err(),
            Some(DisplayError::ZeroSize {
                what: "Screen",
                width: 0,
                height: 1080,
            })
        );
        assert_eq!(
            render_size((1920, 1080), 0).err(),
            Some(DisplayError::ZeroRenderHeight)
        );
    }

    #[test]
    fn real_pixel_fold_1_cutout_and_window_metrics_classification_and_insets() {
        // Real Pixel Fold 1 Outer Display observed via `adb dumpsys display`:
        // 1080x2092 @ 420dpi with 133px top camera cutout.
        let cover_cutout = DisplayInsets::new(0, PIXEL_FOLD_COVER_CUTOUT_PX, 0, 0);
        let wm_cover = AndroidWindowMetrics::new(
            1080,
            2092 - PIXEL_FOLD_COVER_CUTOUT_PX,
            cover_cutout,
            PIXEL_FOLD_DENSITY_DPI,
            0,
            FoldPosture::Folded,
        );
        assert_eq!(wm_cover.physical_metrics(), DisplayMetrics::new(1080, 2092));
        assert_eq!(wm_cover.classify_fold_screen(), FoldScreen::PixelFoldCover);
        assert_eq!(
            DisplayMetrics::new(1080, 1959).classify_fold_screen(),
            FoldScreen::PixelFoldCover
        );
        assert_eq!(
            DisplayMetrics::new(1959, 1080).classify_fold_screen(),
            FoldScreen::PixelFoldCover
        );

        let mut state = DisplayState::from_window_metrics(wm_cover, false).unwrap();
        assert_eq!(state.current().screen, FoldScreen::PixelFoldCover);
        assert_eq!(state.current().posture, FoldPosture::Folded);
        assert!(
            state.current().touch_overlay.pause_button.y >= PIXEL_FOLD_COVER_CUTOUT_PX,
            "pause button must clear the 133px Pixel Fold 1 cover cutout"
        );

        // Unfold to real Inner Display (2208x1840 @ 420dpi, displayId 1).
        let wm_inside = AndroidWindowMetrics::new(
            2208,
            1840,
            DisplayInsets::default(),
            PIXEL_FOLD_DENSITY_DPI,
            1,
            FoldPosture::Flat,
        );
        let cur = state.on_window_metrics_change(wm_inside, 100).unwrap();
        assert_eq!(cur.screen, FoldScreen::PixelFoldInside);
        assert_eq!(cur.posture, FoldPosture::Flat);
        assert_eq!(cur.target_render_size, (1296, 1080));
        assert_eq!(cur.bb_game_res_env(), "1296x1080");
        assert_eq!(cur.bb_movie_size_env(), "2208x1840");
        assert!(!cur.needs_restart);
        assert!(state.tick(100 + RESIZE_DEBOUNCE_MS).needs_restart);
    }

    #[test]
    fn landscape_cover_to_unfolded_to_inside_portrait_to_cover_cycle() {
        let mut state =
            DisplayState::new(DisplayMetrics::new(2092, 1080), false).expect("valid startup");
        assert_eq!(state.startup_render(), (2092, 1080));
        assert_eq!(state.current().screen, FoldScreen::PixelFoldCover);
        assert_eq!(state.current().fov.mode, FovMode::HorPlus);
        assert!(near(state.current().aspect_ratio, 2092.0 / 1080.0));

        // Unfold to 2208x1840 (6:5) at t=100ms, then settle at t=350ms.
        let cur = state
            .on_display_change(DisplayMetrics::new(2208, 1840), 100)
            .unwrap();
        assert_eq!(cur.screen, FoldScreen::PixelFoldInside);
        assert_eq!(cur.fov.mode, FovMode::VertPlus);
        assert!(!cur.needs_restart);
        let cur = state.tick(100 + RESIZE_DEBOUNCE_MS);
        assert!(cur.needs_restart);

        // Rotate unfolded inside display to portrait (1840x2208, 5:6).
        let cur = state
            .on_display_change(DisplayMetrics::new(1840, 2208), 500)
            .unwrap();
        assert_eq!(cur.screen, FoldScreen::PixelFoldInside);
        assert_eq!(cur.orientation, DisplayOrientation::Portrait);
        assert_eq!(cur.fov.mode, FovMode::VertPlus);
        assert!(cur.safe_area.fits_within(1840, 2208));
        assert!(cur.presenter_rect.fits_within(1840, 2208));
        assert!(cur.needs_restart);

        // Fold back to cover landscape (2092x1080) -> clears restart prompt.
        let cur = state
            .on_display_change(DisplayMetrics::new(2092, 1080), 700)
            .unwrap();
        assert_eq!(cur.screen, FoldScreen::PixelFoldCover);
        assert!(!cur.needs_restart);
    }

    #[test]
    fn intermediate_fold_animation_frames_debounce_from_first_divergent_frame() {
        let mut state = DisplayState::new(DisplayMetrics::new(1080, 2092), false).unwrap();
        assert!(
            !state
                .on_display_change(DisplayMetrics::new(1600, 1900), 100)
                .unwrap()
                .needs_restart
        );
        assert!(
            !state
                .on_display_change(DisplayMetrics::new(2208, 1840), 250)
                .unwrap()
                .needs_restart
        );
        assert!(state.tick(100 + RESIZE_DEBOUNCE_MS).needs_restart);
    }

    #[test]
    fn c_abi_exports_match_rust_adaptation_and_fail_closed_on_null() {
        assert_eq!(
            uncharted_android_classify_fold_screen(2208, 1840),
            FoldScreen::PixelFoldInside as u32
        );
        assert_eq!(
            uncharted_android_classify_fold_screen(1080, 2092),
            FoldScreen::PixelFoldCover as u32
        );
        assert_eq!(
            uncharted_android_classify_fold_screen(1920, 1080),
            FoldScreen::Generic as u32
        );

        let mut rw = 0u32;
        let mut rh = 0u32;
        unsafe {
            assert!(!uncharted_android_render_size(
                2208,
                1840,
                1080,
                core::ptr::null_mut(),
                &mut rh
            ));
            assert!(uncharted_android_render_size(
                2208, 1840, 1080, &mut rw, &mut rh
            ));
        }
        assert_eq!((rw, rh), (1296, 1080));

        let fov_c = uncharted_android_fov(2208, 1840);
        assert_eq!(fov_c.mode, 2);
        assert!((fov_c.horizontal - 1.0).abs() < 1e-6);

        let safe_c = uncharted_android_safe_area(2208, 1840);
        assert_eq!(
            safe_c,
            AndroidRectC {
                x: 0,
                y: 299,
                width: 2208,
                height: 1242,
            }
        );
        assert_eq!(
            uncharted_android_movie_fit_rect(2208, 1840, 1920, 1080),
            safe_c
        );
        assert!(uncharted_android_resize_needs_restart(
            1080, 2092, 2208, 1840
        ));

        let mut out = AndroidDisplayAdaptationC {
            screen: 0,
            posture: 0,
            render_width: 0,
            render_height: 0,
            aspect_ratio: 0.0,
            presenter_rect: AndroidRectC::default(),
            safe_area: AndroidRectC::default(),
            movie_fit_rect: AndroidRectC::default(),
            fov: AndroidFovC {
                mode: 0,
                horizontal: 0.0,
                vertical: 0.0,
            },
            needs_restart: false,
            touch_overlay_visible: false,
        };
        unsafe {
            assert!(!uncharted_android_compute_display_adaptation(
                1080,
                2092,
                2208,
                1840,
                false,
                core::ptr::null_mut()
            ));
            assert!(uncharted_android_compute_display_adaptation(
                1080, 2092, 2208, 1840, true, &mut out
            ));
        }
        assert_eq!(out.screen, FoldScreen::PixelFoldInside as u32);
        assert_eq!(out.posture, FoldPosture::Flat as u32);
        assert_eq!((out.render_width, out.render_height), (1296, 1080));
        assert!(out.needs_restart);
        assert!(!out.touch_overlay_visible);
    }

    #[test]
    fn acnh_orientation_and_aspect_factor_cover_pixel_fold_inner_and_cover() {
        assert_eq!(
            requested_orientation_for_smallest_width_dp(411),
            RequestedOrientation::SensorLandscape
        );
        assert_eq!(
            requested_orientation_for_smallest_width_dp(700),
            RequestedOrientation::FullUser
        );
        let upright_k = clamped_aspect_factor(1840, 2208).unwrap();
        assert!((upright_k - 0.46875).abs() < 0.001);
        assert!(upright_k >= MIN_ASPECT_FACTOR);
        let wide_k = clamped_aspect_factor(2092, 1080).unwrap();
        assert!(wide_k > 1.0 && wide_k <= MAX_ASPECT_FACTOR);
    }
}
