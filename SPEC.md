# Uncharted Universal: spec for 1.0

**Approval:** 2026-11-01, in the Project Manager session's question tool: "Approve (Recommended)" (rewritten 2026-11-01 to the totk-native plan: one Rust engine, Continue screen, home build hubs, six device families, same save everywhere). Rulings behind this draft, 2026-11-01, in the Project Manager session's question tool: Rust scope "Ours to Rust, GPU stays (Recommended)"; 1.0 devices "Full TotK parity"; branches "Ours becomes main (Recommended)"; repo renamed from `uncharted_pc` to `Uncharted-Universal` at my word ("rename the Uncharted repo to Uncharted-Universal"). Previous approval: 2026-10-26, "Approve, to Dev Loop (Recommended)" (1.0 = plays start to finish on the AMD handheld). A changed Must or Won't sets this back to "not yet".

## 1.0 in one sentence

I play Uncharted 1, 2 and 3 from my own dump of the PS4 collection on my AMD handheld and my Strix Halo tablet (SteamOS, from Game Mode), my RTX 3090 Ti desktop (Windows), my Mac, my iPhone or iPad with a controller, and an Adreno Android device with a controller, each game plays start to finish on the AMD handheld, and the same save loads on every device through save-relay.

## A fork, built our way

This repo is a fork of [Supermedo/uncharted_pc](https://github.com/Supermedo/uncharted_pc) (upstream plan: its [README](https://github.com/Supermedo/uncharted_pc/blob/main/README.md) and [docs/ROADMAP.md](https://github.com/Supermedo/uncharted_pc/blob/main/docs/ROADMAP.md)), carrying the spacepepperoni Linux fork. Our delta (R18) is now most of the project:

| Delta | Upstream? |
|---|---|
| Our code in Rust: launcher, prep tools and the host runtime (`src/`); shadPS4 (`gpu/`) stays C++ | keep on our fork |
| Continue screen launcher (Rust + Slint), home build hubs, fleet view, Steam artwork | keep on our fork |
| Android, Mac, iPhone/iPad builds | keep on our fork |
| Shared saves through save-relay | keep on our fork |
| Linux build, SteamOS packaging, handheld profiles (20 W, 30/60 Hz) | keep on our fork |
| Correctness fixes (compute fences, small copies, the review's play-breaking bugs) | send upstream as bug reports (code PRs need my word) |

Upstream syncs (R21) keep coming for `gpu/`, assets and patches. Once a module of ours is Rust, upstream's C or Python change to it becomes an `upstream-sync` issue that ports the change by hand.

## Musts

Device checks are `needs-owner-device`: they need my machines and my dump. IDs M1-M9 keep their old meaning so the issues that cite them stay right.

| # | Must | Done looks like | Check |
|---|---|---|---|
| M1 | Uncharted 1 plays start to finish on the AMD handheld | Every chapter completes, the credits roll, at 30 or 60 Hz with no crash or hang | a playthrough log per chapter (`playthrough.jsonl`, #170) |
| M2 | Uncharted 2 plays start to finish on the AMD handheld | Same; chapter 21 progresses and the streaming assertion does not stop play | same |
| M3 | Uncharted 3 plays start to finish on the AMD handheld | Same; no view-blocking corruption, videos in step with audio | same |
| M4 | Saves work | A save made in each game loads after a restart and after switching games, on every device | save, quit, restart, load, per game |
| M5 | Power and frame rate on the handhelds | 20 W holds; 30 Hz is a steady 30 FPS and 60 Hz holds 57-60 FPS on the AMD handheld | frame stats over a 30-minute soak per game |
| M6 | Every build is verified | `cargo test` and the GPU tests pass in CI on Linux, Windows and macOS for each merge to `main`; the Android and Apple targets cross-build in CI | CI green |
| M7 | The launcher starts each game | On every device with our launcher, U1, U2 and U3 start from it, with third-party patches on | start each game from the launcher |
| M8 | The review's play-breaking bugs are fixed | ExpandImage, flexible-mapping split, equeue wait, BUFFER_WBINVL1, AJM, the CE/DE counter waits | each issue's own check |
| M9 | No game files, ever | Nothing from a dump, firmware, keys or game-binary analysis is in the repo, a release, a log or an issue | review of every PR; the release file list |
| M10 | WHEN I open the launcher on any device, IT SHALL show one controller-first Continue screen (full screen in Game Mode, a window on desktop, mouse works too): the latest save of the game I last played, the device it was played on, when, and its play time, with one button to sync and play; the three games one press away; first run is a guided setup driven by the controller; Saves, Settings, Fleet and About one button away; a sync conflict shows both saves side by side and I pick one | From Game Mode, A starts the game in one press; a forced conflict shows two saves and keeps the one I pick | view-model tests with fake saves; on each handheld |
| M11 | WHEN I point the launcher on a build hub at my dump, IT SHALL prepare the game for every device that hub serves: the Strix Halo tablet for both SteamOS handhelds and Android, the RTX 3090 Ti desktop for Windows, the Mac for Mac, iPhone and iPad. It SHALL refuse an executable revision the port does not support with a plain message, SHALL keep the dump on the three hubs only, and SHALL never copy, upload or log game files beyond the home network | A working install per device from its hub; a wrong revision is refused; the AMD handheld, the phone and the Android device hold only what a hub sent | core tests with fake dumps; one real prepare per hub |
| M12 | WHEN a device without a dump starts its launcher at home, IT SHALL find a hub on the home network, pair once with a code shown on both screens, and pull its install, patch level and prepared shader cache over the home network only; a fleet view SHALL show each paired device's build, patch level, last sync and free space, and SHALL warn before I play on a device whose build differs from the one that made the latest save | The AMD handheld installs from the Strix Halo tablet; a device one patch behind shows the warning | tests with a fake hub on loopback; on the devices |
| M13 | ON the AMD handheld and the Strix Halo tablet, THE LAUNCHER SHALL add itself to Steam with each game's artwork (grid, hero, logo, icon, from the dump's own images) so Game Mode opens the Continue screen, and the game SHALL play on the built-in controls with that device's profile (resolution, frame cap, power) | Steam's library shows the artwork; an hour from Game Mode with no crash on each | on each handheld |
| M14 | ON the RTX 3090 Ti desktop, EACH GAME SHALL play from the launcher with a controller | An hour of play per game, no crash | on the device |
| M15 | ON my Mac, THE LAUNCHER SHALL prepare and run the Mac build natively on Apple silicon (no Rosetta), and SHALL build the iPhone/iPad app and install it over a cable | Each game plays on the Mac; the iPhone or iPad app installs and starts | on the devices |
| M16 | ON the iPhone or iPad, EACH GAME SHALL play with an Xbox, PlayStation or MFi controller, every button mapped, touch controls hidden while a controller is connected | A session per game on a controller | on the device |
| M17 | ON an Android device with an Adreno GPU, THE LAUNCHER (the Android build of the same app) SHALL install the build pulled from the Strix Halo tablet, and each game SHALL play with a controller | A session per game, no crash | on the device |
| M18 | ON EVERY DEVICE, THE GAME SHALL pull my saves through save-relay before it starts and push them after it quits, syncing only the save data and settings; the same save SHALL load on all six | A save made on one device loads on each of the others | save-relay manifest tests; the round trip on all devices |
| M19 | WHEN I update the launcher or the port, THE LAUNCHER SHALL back up my saves first, rebuild, and keep them | Saves intact after an update | core test with a fake build; on one device |
| M20 | OUR CODE SHALL be Rust: the launcher, the prep tools and the host runtime ship as Rust, and no Python or C of ours ships in 1.0; shadPS4 and the Android and Apple code we adopt stay in their own languages, behind a C ABI. The art generator `packaging/windows/make_uncharted_art.py` (upstream's, kept by the 0.3.11 sync) ports to Rust in #190, before 1.0 | `git ls-files src launcher scripts` holds no shipped source; the game builds and boots at every step of the move | CI; the boot smoke test per step |

## Won'ts (1.0)

- Any game data, keys or firmware in this repo, its releases or its logs, ever. The bundle holds no game data either (ruling (d), 2026-11-02).
- A `probe` (the hardware report, `uncharted-run --probe`) on Android or iOS at 1.0. Only desktops and handhelds get one (owner ruling, 2026-11-01).
- Executable revisions the port does not support.
- Steam Frame or other devices; Android GPUs other than Adreno (Turnip is the driver; Mali is after 1.0); Android kernels with 16 KB pages (FEXCore needs 4 KB).
- Rewriting shadPS4 in Rust.
- New mod or cheat features (the existing mod folder, patches and cheats are ported, not extended).
- Online play (bbnet stays as upstream has it, untested). Builds and caches move between my devices over the home network only, never through a server or save-relay's storage.
- Above 60 FPS.
- After 1.0: in-game graphics menu and the Game Mode menu (#10, #167), Xbox prompts (#11), trophies to the launcher (#12), 16:10 / ultrawide (#14, #158), AI beta testers (#15), shader preload (#16), the U1 1080p power study (#17), FSR 4 (#18), U3 stereo (#19), plain ELF input (#66), the save edge cases from the review (#38, #48-#53, #57), runner caching (#58).

## Prior art

Notes and the survey's findings: [PEERS.md](PEERS.md). The original peers were recorded during the 2026-10-31 / 2026-11-01 planning. The 2026-11-06 platform survey adds the separate Apple recompiler/CPU repositories and a direct Uncharted Mac launcher as research candidates; it changes no Must or approved platform route. Listing a peer does not approve importing its entire tree.

peer: Supermedo/uncharted_pc | GPL-2.0 | upstream: Linux, handheld, resolution, frame rate and upscaler issues and PRs
peer: spacepepperoni/uncharted_Linux | GPL-2.0 | the Linux fork we sync from: launcher, Steam Deck mode, releases
peer: drakolordx7/uncharted_pc | GPL-2.0 | community build: VRAM budget, FSR fixes, 45 FPS limit
peer: Pepin208/uncharted_pc_linux | GPL-2.0 | Linux AppImage, shadPS4 texture-cache back-ports
peer: AdilDevStuff/uncharted_pc_linux | GPL-2.0 | raw mouse, CI packages, aim blur and exposure fixes
peer: Mevlock/uncharted_pc-dev | GPL-2.0 | performance builds: pools, buffer arenas, overlay stats
peer: revvv991/uncharted-android | GPL-2.0 | the Android base: guest x86-64 through FEXCore, the rest native ARM64, Turnip on Adreno; all three games playable on a Retroid Pocket G2
peer: shadps4-emu/shadPS4 | GPL-2.0 | texture and buffer cache fixes to back-port into gpu/shadps4
peer: bloodborne-native/bloodborne-runtime | GPL-2.0 | the shim's ancestor; its CPU recompiler and Metal renderer are the Apple route
peer: bloodborne-native/bloodborne-recomp | GPL-2.0 root licence | separate AOT generator for the Apple route; Bloodborne-specific executable assumptions need an Uncharted feasibility check
peer: bloodborne-native/bbcpu | GPL-2.0 root licence | CPU support for the Apple route; iOS disables JIT and needs coverage or interpreter fallback
peer: bloodborne-native/bloodborne-launcher | GPL-2.0 | launcher profiles and settings
peer: eladharels/uncharted_android | GPL-2.0 | a second Android port (FEX JIT + Turnip): device notes
peer: bloodborne-native/bloodborne-mac | GPL-2.0 | native Apple Silicon port of the same bbport family: the Mac base
peer: bloodborne-native/bloodborne-ios | GPL-2.0 | Bloodborne on iPhone with no JIT (recompiled library + Metal, sideloaded): the iOS base and its 2 GiB limit
peer: Ryxm-a/uncharted-ios | GPL-2.0 | an Uncharted iOS scaffold (FEX with sideload JIT); watched
peer: zorde23/Uncharted_Collection_PC | GPL-2.0 | independent Windows port of the collection: crash and rendering fixes to port with credit
peer: iannitram/totk-native | MIT | our sibling: the same launcher, hubs, fleet, Steam and save-card modules in Rust, copied with credit
peer: iannitram/save-relay | MIT | all save sync
peer: bmy/bbport-mac | GPL-2.0 root licence | second macOS bbport: Rosetta guest and native renderer process; comparison, not our no-Rosetta route
peer: truwa/nathan-drake-collection-macos | root licence not established; learn only | direct Uncharted Mac launcher using Rosetta and KosmicKrisp; packaging comparison
peer: FEX-Emu/FEX | MIT | x86-64 on ARM64 (Android)
peer: K11MCH1/libadrenotools | BSD-2-Clause | loads Turnip on Android
peer: slint-ui/slint | royalty-free licence (attribution) | the launcher UI
peer: gilrs (crate) | MIT or Apache-2.0 | gamepad input on desktop
peer: Revivedx/SavageWindRecomp | MIT | AppImage and Steam Deck packaging to learn from
peer: GNSB/UFC-3-PS4-PORT-LINUX-WINDOWS | GPL-2.0 | a sibling PS4 port with Linux and Windows builds

## Problem

I want to pick up any of my devices and keep playing the collection, but today the port runs well only on Linux PCs, its launcher is Python and Tk, every device prepares its own copy from the dump, and saves stay where they were made.

## Solution

One Rust engine with a Slint launcher on every device, opening on the Continue screen. Three home build hubs hold the dump and prepare each device's install; the other devices pull it over the home network. save-relay wraps every start. The host runtime moves from C to Rust module by module, so the game keeps building and booting at every step. Android adopts the FEXCore route, Apple the recompiler and Metal route; shadPS4 stays the renderer.

## User stories

1. As the owner, I want to pick up any of my devices and continue the same save, so that I never copy saves by hand.
2. As the owner, I want to start each game from Game Mode on my handhelds, so that I never need the desktop.
3. As the owner, I want to play on my iPhone, iPad or Android device with a controller, so that I can play away from home.
4. As the owner, I want one button on any device to put me back in my latest save.
5. As the owner, I want my dump on as few machines as possible.
6. As a contributor, I want one language and one build (`cargo`) for everything of ours, so that the code is easy to change and test on every OS.

## Implementation decisions

- **Workspace** (ADR 0001): `crates/core` (`uncharted_core`: dump and revision checks, prepare, patches, mods, cheats, trophies, render size, recipes, saves, profiles, Steam, artwork, save cards, bundles, hubs, fleet, backup, update), `crates/runtime` (`uncharted_runtime`: the host runtime that replaces `src/`), `crates/cli` (`uncharted-universal`), `app/` (`uncharted-app`, Rust + Slint). `gpu/` stays C++ and CMake, built from `crates/runtime`'s build script, behind the existing `bbgpu.h` C ABI.
- **Moving the runtime** (ADR 0002): strangler order. `crates/runtime` builds as a static library linked with the remaining C; each `runtime_*.c` is replaced by a Rust module exporting the same symbols, its C test becoming a Rust test; the loader (`probe.c`) moves last. A boot smoke test (the game reaches its first frame on a fake image, plus a device check per step on the AMD handheld) gates each step.
- **Launcher** (ADR 0003): all Rust with a Slint UI, as totk-native ADR 0004: view models in plain Rust, `.slint` only draws; gamepad through gilrs on desktop and key events on Android. The Python launcher stays until the Slint app has parity, then goes. The generic modules (vdf, Steam shortcuts, platform folders, hub, bundle, fleet, save card shape) are copied from totk-native (MIT) with credit.
- **Build hubs** (ADR 0004): the Strix Halo tablet (Linux handhelds, Android), the RTX 3090 Ti desktop (Windows), the Mac (Mac, iPhone, iPad). mDNS discovery, one-time pairing code (SPAKE2), the stream sealed, home network only. The hub prepares the install (boot image, linked modules, patches) and the GPU-independent shader translations, and the player's prepared game data, which it copies to the player's own devices over the home network and never puts in a bundle; a driver pipeline cache ships only between devices with the same GPU and driver.
- **Android** (ADR 0005): adopt revvv991/uncharted-android's route and code (GPL-2.0, with credit): the game's x86-64 code through FEXCore, our runtime and shadPS4 native ARM64, Turnip through libadrenotools. Our Rust runtime cross-builds for `aarch64-linux-android`. The Slint launcher pulls the APK and install from the hub.
- **Apple** (ADR 0006): adopt bloodborne-native's route (GPL-2.0, with credit, licence checked at the issue): a recompiled library built on the Mac from my dump (no JIT, so it also serves iOS), and a Metal renderer. Their code is vendored and stays in its language, like shadPS4. The Mac launcher builds and installs the iOS app over a cable with Xcode signing. Risk: a free Apple ID gives about 2 GiB of address space and re-signs every 7 days; the paid Apple Developer Program lifts both.
- **Saves:** a save-relay manifest for the collection (#163) naming each game's savedata folder per device; the same savedata layout on every device, so a save moves unchanged.
- **Windows:** the existing CLANG64 toolchain for the C++ parts; Rust target chosen at the issue (`x86_64-pc-windows-gnullvm` to match, or MSVC).

## Testing decisions

- Core logic (dump and revision checks, prepare steps, patches, mods, recipes, saves, Steam files) is tested with fake dumps in `cargo test` on Linux, Windows and macOS. The 34 Python test files are the expected results for their Rust ports.
- Each runtime module's Rust tests replace its C test; the boot smoke test runs in CI on Linux.
- Launcher pages are tested through their view models (button presses in, page state out), and once headless through Slint's testing backend.
- Hub pairing, transfer and the fleet view are tested with a fake hub on loopback.
- Everything that runs the game on hardware is a device check (`needs-owner-device`).

## Platforms

| | Linux / SteamOS | Windows | macOS | iOS / iPadOS | Android |
|---|---|---|---|---|---|
| 1.0 device | AMD handheld, Strix Halo tablet | RTX 3090 Ti desktop | a Mac with Apple silicon | iPhone or iPad + controller | an Adreno device + controller |
| Game code runs | natively (x86-64) | natively (x86-64) | recompiled library | recompiled library | FEXCore |
| Renderer | shadPS4 on Vulkan | shadPS4 on Vulkan | Metal | Metal | shadPS4 on Vulkan (Turnip) |
| Hub | the Strix Halo tablet | the desktop | the Mac | the Mac | the Strix Halo tablet |
| Launcher | Slint app (AppImage) | Slint app | Slint app | none: the game app | Slint app |
