# Android Dependencies & Synthetic Verification

This directory holds the pinned Android dependency lock, patch series,
dependency build orchestration, and synthetic smoke harnesses for Uncharted
Universal (issues `#207`, `#208`, `#209`, `#210`, `#247`).

## Contents

- `components.lock.json` — Pinned revisions for `fex` (FEXCore),
  `mesa-turnip` (Mesa freedreno Vulkan KGSL), `libadrenotools`, and `sdl3`,
  along with target host/NDK/SDK requirements (`linux-x86_64`, NDK `r30`,
  SDK platform `35`, build-tools `35.0.1`, Android API `33`, ABI `arm64-v8a`).
- `patches/` — Ordered patch series for FEX (`patches/fex/`, 3 patches) and
  Mesa Turnip KGSL (`patches/mesa/`, 23 patches). See `patches/README.md`.
- `LICENSES.md` — Upstream license notices and donor-reference boundaries.
- `scripts/build_dependencies.sh` — Preflight, lock/patch verification, and
  full NDK r30 dependency + synthetic smoke build orchestration.
- `scripts/setup_ndk_sdk.sh` — Idempotent Linux `x86_64` bootstrap helper for
  Android NDK `r30`, `platforms;android-35`, and `build-tools;35.0.1`.
- `scripts/verify_fex_smoke.sh` — Builds and runs the C++20 synthetic FEX
  guest smoke harness on host, cross-builds for Android `arm64-v8a` when `NDK`
  is configured, and executes on-device when `adb` has a connected target.
- `scripts/test_preflight_and_setup.sh` — Offline unit/integration tests for
  `build_dependencies.sh` (`--check-lock`, `--preflight`) and
  `setup_ndk_sdk.sh` (`--dry-run`, idempotency).
- `synthetic/` — Authored synthetic smoke targets (`smoke.c`, `fex_smoke.*`,
  `fexcore_smoke_entry.cpp`).

## 1. Lockfile & Patch Verification (`--check-lock`)

Validate `components.lock.json` schema/pins, `LICENSES.md` coverage, and all
patch files under `android/patches/` (non-empty, valid UTF-8, LF line endings,
strictly ordered numbering, and parsed by `git apply --numstat`) without
requiring NDK/SDK or `WORK`:

```bash
bash android/scripts/build_dependencies.sh --check-lock
```

## 2. Environment Preflight (`--preflight`)

Run lock/patch validation plus host build-tool checks (`git`, `cmake`, `ninja`,
`python3` with `venv`/`ensurepip`, `curl`, `pkg-config`, `bison`, `flex`,
`file`, `readelf`) and verify Android NDK `r30` + SDK `platforms;android-35` +
`build-tools;35.0.1`:

```bash
bash android/scripts/build_dependencies.sh --preflight
```

If host C/C++ build tools such as `bison` and `flex` live inside the
`craft-build` container, run preflight inside it:

```bash
distrobox enter craft-build -- bash -c "cd '$PWD' && bash android/scripts/build_dependencies.sh --preflight"
```

Exit codes: `0` when ready to build, `2` when host tools or NDK/SDK components
are missing, `3` when `components.lock.json` or `android/patches/` fail
validation.

## 3. Bootstrap Android NDK r30 & SDK 35 (`setup_ndk_sdk.sh`)

Preview the exact download/install plan without touching disk or network:

```bash
bash android/scripts/setup_ndk_sdk.sh --dry-run
```

Install idempotently into `~/Android` (requires `curl`, `unzip`, and `java`
JDK/JRE 17+ for `sdkmanager`):

```bash
bash android/scripts/setup_ndk_sdk.sh --target "$HOME/Android" --accept-licenses
export NDK="$HOME/Android/android-ndk-r30"
export SDK="$HOME/Android/sdk"
bash android/scripts/build_dependencies.sh --preflight
```

## 4. Full Dependency & Smoke Build

Once preflight passes, build all pinned dependencies and link the synthetic
Android binaries into an external `WORK` directory (nothing is built inside the
repository):

```bash
WORK=/tmp/uncharted-android-work \
NDK="$HOME/Android/android-ndk-r30" \
SDK="$HOME/Android/sdk" \
bash android/scripts/build_dependencies.sh
```

If an Android device is attached over `adb`, `build_dependencies.sh` also pushes
and runs the synthetic smoke binaries under `/data/local/tmp/uncharted-android-smoke`.

## 5. Synthetic FEX Smoke & Script Self-Tests

Run the synthetic FEX smoke verification (host C++20 unconditionally; Android
`arm64-v8a` cross-build + `adb` execution when `NDK` / device are present):

```bash
bash android/scripts/verify_fex_smoke.sh
```

Inside `craft-build`:

```bash
distrobox enter craft-build -- bash -c "cd '$PWD' && bash android/scripts/verify_fex_smoke.sh"
```

Run the offline script self-tests (lock/patch negative cases, preflight
diagnostics, `--dry-run`, and idempotency):

```bash
bash android/scripts/test_preflight_and_setup.sh
```
