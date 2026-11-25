//! Stage 7.03–7.07 Android APK scaffold for the Slint launcher:
//! - [`display`]: Pixel Fold 1 inside/outside screen detection and full M21 visual adaptation.
//! - [`driver`]: explicit fail-closed Turnip / system Vulkan driver selection mirroring
//!   `android/driver_loader.h`.
//! - [`gamepad`]: Android `KeyEvent` / `MotionEvent` controller-to-Slint input code adapter.
//! - [`game`]: per-game thin APK identity (U1, U2, U3), M9 no-bundled-assets guard, and
//!   Stage 7.04 first-boot download prompt hook.
//! - [`save_relay`]: Save Relay C-ABI/JNI bridge, slot-aware paths/manifests, setup/OAuth flow,
//!   and side-by-side conflict UI state machine (`#250`).
//! - [`entry`]: `NativeActivity` entry stub and host-testable startup bootstrapper.

pub mod display;
pub mod driver;
pub mod entry;
pub mod game;
pub mod gamepad;
pub mod save_relay;
