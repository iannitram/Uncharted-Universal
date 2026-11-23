//! Stage 7.03 Android APK scaffold for the Slint launcher:
//! - [`driver`]: explicit fail-closed Turnip / system Vulkan driver selection mirroring
//!   `android/driver_loader.h`.
//! - [`gamepad`]: Android `KeyEvent` / `MotionEvent` controller-to-Slint input code adapter.
//! - [`entry`]: `NativeActivity` entry stub and host-testable startup bootstrapper.

pub mod driver;
pub mod entry;
pub mod gamepad;
