# 0005: Android runs the game's code through FEXCore

**Decided:** 2026-11-01, in the spec approval.

## Decision

Adopt revvv991/uncharted-android's route and code (GPL-2.0, credited in the files and PRs): the game's x86-64 code runs through FEXCore inside an ordinary Android app, our runtime and shadPS4 run as native ARM64, and the Vulkan driver is Turnip loaded with libadrenotools. Its `android/` project comes in under `android/`; our Rust runtime cross-builds for `aarch64-linux-android`. The Slint launcher is the app's front, pulls its install from the Strix Halo tablet, and runs save-relay.

## Limits

Adreno only (Turnip); a kernel with 4 KB pages; 8 GB of RAM. Mali is after 1.0.
