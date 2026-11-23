//! Stage 7.03 Android APK scaffold for the Slint launcher:
//! - [`driver`]: explicit fail-closed Turnip / system Vulkan driver selection mirroring
//!   `android/driver_loader.h`.
//! - [`gamepad`]: Android `KeyEvent` / `MotionEvent` controller-to-Slint input code adapter.
//! - [`game`]: per-game thin APK identity (U1, U2, U3), M9 no-bundled-assets guard, and
//!   Stage 7.04 first-boot download prompt hook.
//! - [`entry`]: `NativeActivity` entry stub and host-testable startup bootstrapper.

pub mod driver;
pub mod entry;
pub mod game;
pub mod gamepad;
