# 0005: Android runs the game's code through FEXCore

**Decided:** 2026-11-01, in the spec approval.

**Amended 2026-11-23** by owner rulings (`SPEC.md`, M17, M18, M21):
- Android ships as **three separate per-game thin APKs** (Uncharted 1, Uncharted 2, Uncharted 3) so install size stays manageable (#524). Each APK installs thin (no bundled game assets), boots and prompts *"Would you like to download the rest of the game files?"*, and pulls that game's files from Dropbox through `save-relay`'s Dropbox app (`iannitram/save-relay#305`, #525) with resumable downloads that pick right back up after battery death or force-close.
- Android supports **both Snapdragon (Adreno + Turnip via `libadrenotools`) and Google Tensor (ARM Mali via validated system Vulkan `libvulkan.so`, e.g., Pixel Fold 1 Tensor G2 / Mali-G710)** (#536), superseding the earlier Adreno-only / Mali-after-1.0 limit.
- Each Android app detects **Pixel Fold 1 inside (`2208x1840`, 6:5) vs. outside (`2092x1080` / `1080x2092`, ~17.4:9) screens** on startup and live fold/unfold so all visual elements (3D FOV, HUD/menus safe area, video fit, runtime resize/restart prompt, touch/overlay controls) respond per M21 (#537).

## Decision

Adopt revvv991/uncharted-android's route and code (GPL-2.0, credited in the files and PRs): the game's x86-64 code runs through FEXCore inside an ordinary Android app (one thin APK per game: U1, U2, U3), our runtime and shadPS4 run as native ARM64, and the Vulkan driver is Turnip loaded with `libadrenotools` on Snapdragon/Adreno or the validated system Mali Vulkan driver on Google Tensor. Its `android/` project comes in under `android/`; our Rust runtime cross-builds for `aarch64-linux-android`. Each per-game Android app downloads its game files on first boot through `save-relay`'s Dropbox app (resumable across battery death or force-close), adapts all visuals to Pixel Fold 1 inside/outside screens (M21), and syncs saves through `save-relay`.

## Limits

Snapdragon (Adreno/Turnip) and Google Tensor (ARM Mali); a kernel with 4 KB pages; 8 GB of RAM.
