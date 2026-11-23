# Android APK Scaffold (Stage 7.03)

This directory holds the Stage 7.03 Android APK scaffold for the Slint launcher:

- `AndroidManifest.xml`: `org.uncharted.universal` manifest targeting Android API 33–35 (`arm64-v8a`), requiring Vulkan 1.1+, declaring optional `android.hardware.gamepad`, handling foldable/orientation config changes (`configChanges`), and setting `android:extractNativeLibs="true"` (`useLegacyPackaging=true`) as required by `android/driver_loader.cpp`.
- `src/driver.rs`: Rust-side explicit fail-closed Turnip / system Vulkan driver selection and identity validation mirroring `android/driver_loader.h` (`BbAndroidDriverMode`, `BbAndroidDriverConfig`). Never silently falls back to stock Adreno when a custom or bundled Turnip driver is selected.
- `src/gamepad.rs`: Android `KeyEvent` (`AKEYCODE_*`) and `MotionEvent` (`AMOTION_EVENT_AXIS_*`) adapter mapping controller D-pad, left stick (with Android's inverted Y axis), face buttons, and L1/R1 bumpers into the Slint launcher's `Input::from_code` codes (`0..=10`) and prompt styles (`Xbox`, `PlayStation`, `Nintendo`).
- `src/entry.rs`: Android entry scaffold (`android_main` under `#[cfg(target_os = "android")]`) and host-testable startup bootstrapper that validates packaging and Turnip driver identity before Slint/Vulkan initialization.

## Host checks & Android cross-check

```bash
cargo test --manifest-path android/apk/Cargo.toml
cargo check --manifest-path android/apk/Cargo.toml --target aarch64-linux-android
cargo build --manifest-path android/apk/Cargo.toml --lib --target aarch64-linux-android
```

Full NDK/SDK APK packaging (`android-activity`, `slint::android::init`, `libadrenotools` JNI/C++ link, and hub APK serving) arrives in Stages 7.04–7.06.
