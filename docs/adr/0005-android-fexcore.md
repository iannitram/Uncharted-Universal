# 0005: Android runs the game's code through FEXCore

**Decided:** 2026-11-01, in the spec approval.

**Amended 2026-11-23** by owner ruling (`SPEC.md`, M17, M18): Android ships as **three separate per-game APKs** (Uncharted 1, Uncharted 2, Uncharted 3) so install size stays manageable. Each APK installs thin (no bundled game assets), boots and prompts *"Would you like to download the rest of the game files?"*, and pulls that game's files from Dropbox through `save-relay`'s Dropbox app (`iannitram/save-relay#305`) with resumable downloads that pick right back up after battery death or force-close.

## Decision

Adopt revvv991/uncharted-android's route and code (GPL-2.0, credited in the files and PRs): the game's x86-64 code runs through FEXCore inside an ordinary Android app (one thin APK per game: U1, U2, U3), our runtime and shadPS4 run as native ARM64, and the Vulkan driver is Turnip loaded with libadrenotools. Its `android/` project comes in under `android/`; our Rust runtime cross-builds for `aarch64-linux-android`. Each per-game Android app downloads its game files on first boot through `save-relay`'s Dropbox app (resumable across battery death or force-close) and syncs saves through `save-relay`.

## Limits

Adreno only (Turnip); a kernel with 4 KB pages; 8 GB of RAM. Mali is after 1.0.
