# Android Per-Game Thin APKs (Stage 7.03, `#524`)

This directory holds the Stage 7.03 Android thin-APK shell, split into three separate per-game
apps (Uncharted 1, Uncharted 2, Uncharted 3) with **no bundled game assets (M9)**:

- `manifests/u1/AndroidManifest.xml`: `org.uncharted.universal.u1` (`Uncharted 1`, `game_id=u1`).
- `manifests/u2/AndroidManifest.xml`: `org.uncharted.universal.u2` (`Uncharted 2`, `game_id=u2`).
- `manifests/u3/AndroidManifest.xml`: `org.uncharted.universal.u3` (`Uncharted 3`, `game_id=u3`).
- `AndroidManifest.xml`: shared base manifest template targeting Android API 33–35 (`arm64-v8a`), requiring Vulkan 1.1+, declaring optional `android.hardware.gamepad`, handling foldable/orientation config changes (`configChanges`), and setting `android:extractNativeLibs="true"` (`useLegacyPackaging=true`) as required by `android/driver_loader.cpp`.
- `src/game.rs`: `AndroidGame` (`U1`, `U2`, `U3`), M9 no-bundled-assets enforcement, and the Stage 7.04 first-boot prompt state machine (`"Would you like to download the rest of the game files?"`) with gamepad navigation.
- `src/driver.rs`: Rust-side explicit fail-closed Turnip / system Vulkan driver selection and identity validation mirroring `android/driver_loader.h` (`BbAndroidDriverMode`, `BbAndroidDriverConfig`). Never silently falls back to stock Adreno when a custom or bundled Turnip driver is selected.
- `src/gamepad.rs`: Android `KeyEvent` (`AKEYCODE_*`) and `MotionEvent` (`AMOTION_EVENT_AXIS_*`) adapter mapping controller D-pad, left stick (with Android's inverted Y axis), face buttons, and L1/R1 bumpers into the Slint launcher's `Input::from_code` codes (`0..=10`) and prompt styles (`Xbox`, `PlayStation`, `Nintendo`).
- `src/entry.rs`: Android entry scaffold (`android_main` under `#[cfg(target_os = "android")]`) and host-testable startup bootstrapper that validates per-game package identity, M9 no-bundled-assets, and Turnip driver identity before Slint/Vulkan initialization and Stage 7.04 handoff.

## Host checks & Android cross-check

```bash
cargo test --manifest-path android/apk/Cargo.toml
cargo check --manifest-path android/apk/Cargo.toml --features game-u1
cargo check --manifest-path android/apk/Cargo.toml --features game-u2
cargo check --manifest-path android/apk/Cargo.toml --features game-u3
cargo check --manifest-path android/apk/Cargo.toml --target aarch64-linux-android
cargo build --manifest-path android/apk/Cargo.toml --lib --target aarch64-linux-android
```

Full NDK/SDK APK packaging (`android-activity`, `slint::android::init`, `libadrenotools` JNI/C++ link, and resumable Dropbox game-file download via `save-relay#305`) arrives in Stages 7.04–7.06.
