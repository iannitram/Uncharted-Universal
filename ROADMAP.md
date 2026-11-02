# Roadmap

From `SPEC.md` (approved 2026-11-01) to 1.0: Uncharted 1, 2 and 3 on the AMD handheld, the Strix Halo tablet, the RTX 3090 Ti desktop, a Mac, an iPhone or iPad, and an Adreno Android device, the same save on each, each game start to finish on the AMD handheld, and our code in Rust. The code map is `docs/code-map.md`; the decisions are `docs/adr/` (0001 workspace, 0002 runtime to Rust in strangler order, 0003 Slint launcher, 0004 build hubs, 0005 Android through FEXCore, 0006 Apple through a recompiled library and Metal). Every piece is an issue titled `<id>: ...`; `Blocked by` in each issue sets the order. The plan of 2026-10-24 (`docs/review-2026-10-24/ROADMAP-proposed.md`) is history now; its open work is carried into the stages below.

## Features, by Must

| Must | Pieces |
|---|---|
| M1-M3 playthroughs on the AMD handheld | the correctness and campaign work already open (#31, #35, #37, #60, #62-#64, #109, #111-#113, #164 and the bug issues), crash help (#165: #168-#170, #175; the after-crash screen is 2.07, which replaced closed #176), playthroughs #76-#78; 5.02 |
| M4 saves work | 3.06 (savedata in Rust), 1.17, 9.02 |
| M5 power and frame rate | 1.14, 1.15, 1.16, 1.27, auto-tune (#166: #171, #173, #174 and #178 are 1.27, and #172 is 1.05 `dump`; the display cards are 2.06, which replaced closed #177), #22; 5.02 |
| M6 every build verified | 0.02, 2.10, 3.01, 6.01, 7.02, 8.02 |
| M7 the launcher starts each game | 1.04-1.15, 1.25, 2.02-2.05 |
| M8 play-breaking bugs | #31, #35, #37, all in `gpu/` (and #164 first) |
| M9 no game files | 1.23 (bundle never holds the dump), 1.26 (redact), PR review |
| M10 Continue screen | 1.18, 1.25, 2.01, 2.04, 2.05, 2.06, 2.07, 10.01 |
| M11 prepare on a hub, revision gate | 1.02-1.10, 1.24, 2.05 |
| M12 pull from a hub, fleet | 1.23, 4.01-4.05, 2.08 |
| M13 Steam shortcut, artwork, handheld profile | 1.16, 1.21, 1.22, 5.01, 5.02 |
| M14 Windows desktop | 3.15, 6.01, 6.02 |
| M15 Mac and the iOS install | 8.01-8.05, 8.08 |
| M16 iPhone/iPad with a controller | 8.05, 8.06, 8.09 |
| M17 Android | 7.01-7.06 |
| M18 save-relay everywhere | 1.17, 1.25, 9.01 (#163), 7.05, 8.07, 9.02 |
| M19 updates keep saves | 1.19, 1.20 |
| M20 our code in Rust | stage 1 (tools), stage 2 (launcher), stage 3 (runtime), 2.10 (the C in `packaging/`), 2.11, 3.17 |

## Stages

Each stage names its platforms (R04). Stages overlap: anything not blocked can be taken. The campaign work on today's C runtime (M1-M3, M8, #164 first) continues alongside; a bug fixed in C before its module moves is ported with the module.

0. **Housekeeping** (all). 0.01 the rename inside the repo: `uncharted_pc` and the `linux` branch in our docs, workflows, launcher update check and bug-report links. 0.02 the Rust workspace check (`check.yml`) on Linux, Windows, macOS, beside `build.yml`.
1. **Engine core: the prep tools in Rust** (Linux, Windows, macOS in CI). Each Python script becomes a `crates/core` module; its Python tests are the expected results. 1.01 error and platform; 1.02 `sfo`; 1.03 `games` and the revision gate; 1.04 `pkg`; 1.05 `dump` check (#172 folds in); 1.06 `content_profile`; 1.07 `patches`; 1.08 `link` (modules); 1.09 `link` (libc); 1.10 `prepare`; 1.11 `mods`; 1.12 `cheats`; 1.13 `trophies`; 1.14 `render_size`; 1.15 `run_env`; 1.16 `profile`; 1.17 `saves` and the save-relay manifest; 1.18 `savecard`; 1.19 `backup`; 1.20 `update`; 1.21 `vdf` and `steam` (from totk-native); 1.22 `artwork`; 1.23 `bundle` and `fleet` records; 1.24 `recipe` (targets, hubs); 1.25 `launch`; 1.26 `logs` (redact, crash line, known issues, playthrough log: #169 and #170 move here; #168 writes the line in the runtime, 3.06 and 3.16, and 1.26 reads it); 1.27 `tune` (the hardware report read from `uncharted-run --probe`, the choices it hides, the three cards, the chapter list of `recipe`: #171, #174, #178; reads #173's benchmark report).
2. **Faces** (Linux, Windows, macOS). 2.01 the Continue screen brief, from totk-native's, for three games (design first: the Continue screen is the product); 2.02 the `uncharted-universal` CLI; 2.03 the Slint app scaffold with gilrs input, built in CI; 2.04 the Continue screen; 2.05 the guided first run (hub: dump, check, prepare; client: pair, pull; then Add to Steam); 2.06 Saves and Settings (the display cards, #177 folds in); 2.07 the after-crash screen (#176 folds in); 2.08 Fleet; 2.09 About (patches, licences, Slint notice) and the seven languages; 2.10 release builds (AppImage, Windows zip, Mac app); the Rust app takes over `packaging/bbport-entry.c` (it clears Steam's `LD_PRELOAD` itself) and `uncharted-universal play` takes over `packaging/windows/play.c`; 2.11 retire `launcher/`, `scripts/`, `run.py` and the PyInstaller packaging.
3. **Runtime to Rust** (Linux first in CI, Windows with 3.15). 3.01 `crates/runtime` scaffold: static library linked into the loader, `cbindgen` header, `gpu/` built by its build script, the boot smoke test in CI, `abi` (the NID table and `runtime_resolve`, the first module), and `src/vulkan_smoke.c` as a test of the `gpu` build; then one module per piece, each exporting the C symbols it replaces: 3.02 `time` and `rtc`; 3.03 `sync`; 3.04 `content` and `display`; 3.05 `trophy`; 3.06 `savedata` (and `where.json`, #168); 3.07 `audio`; 3.08 `ajm`; 3.09 `file` (VFS, mods); 3.10 `pad` (SDL3, gyro, rumble; the scripted camera path of `BB_BENCHMARK`, #173); 3.11 `services` and `kernel`; 3.12 `memory`; 3.13 `fiber` and `thread`; 3.14 `cpu_patch` and `red_zone`; 3.15 `os` (Windows shims); 3.16 `loader` and the `uncharted-run` binary (with the `Crash:` line, #168, and `--probe`, which takes over `tools/gpu_capabilities.c`); 3.17 delete `src/` and the C build path. A device check on the AMD handheld follows each of 3.06, 3.07, 3.08, 3.09, 3.10, 3.12, 3.13 and 3.16 (one device check per step, as SPEC.md has it, applied to these eight: ADR 0002 names savedata, audio, AJM, memory, threads and the loader and predates SPEC M20, so it names six steps; file and pad are the other two; the mismatch is for the owner).
4. **Build hubs** (Linux, Windows, macOS; Android as a client). 4.01 discovery; 4.02 pairing; 4.03 serve and pull with the shader translations; 4.04 fleet records and drift; 4.05 `hub` and `fleet` commands. CI runs a fake hub on loopback.
5. **SteamOS handhelds** (Linux). 5.01 the Strix Halo tablet as hub: prepare all three games on itself, play an hour each from Game Mode; 5.02 the AMD handheld: install pulled from the tablet, Steam artwork, an hour each, the M5 soak; then the playthroughs #76-#78.
6. **Windows desktop.** 6.01 the Rust workspace and the GPU library on `windows-latest` (CLANG64, Rust target chosen here); 6.02 an hour per game with a controller.
7. **Android** (Adreno). 7.01 bring in revvv991/uncharted-android's `android/` (APK, FEXCore, Turnip patches) with credit, rebased on our tree; 7.02 the runtime cross-builds for `aarch64-linux-android` in CI; 7.03 the Slint launcher's Android build with gamepad keys; 7.04 the hub serves the APK and install, the launcher installs them; 7.05 save-relay in the Android launcher; 7.06 a session per game with a controller.
8. **Mac, iPhone and iPad.** 8.01 study bloodborne-native's recompiler, Metal renderer and iOS shell: licence files, what is Bloodborne-specific, a written plan (docs); 8.02 the runtime builds for `aarch64-apple-darwin` and `aarch64-apple-ios` in CI; 8.03 bring in the recompiler, run on the Mac from my dump to a library per game; 8.04 the Metal renderer for the collection; 8.05 the Mac app from the launcher, then the iOS app built on the Mac and installed by cable; 8.06 iOS controller mapping, touch hidden with a controller; 8.07 save-relay in the iOS app (its C interface); 8.08 the Mac: an hour per game; 8.09 the iPhone or iPad: a session per game with a controller.
9. **One save everywhere.** 9.01 the save-relay adapter in the launcher (#163 reworked); 9.02 the round trip across all six devices: the 1.0 sentence.
10. **Design pass** (R04), last before 1.0. 10.01 the built screens against the Continue screen brief and the shared design system, including the Steam artwork and Game Mode; findings become `design-pass` issues.

## Open-source pieces and where they fit

| Project | Licence | Where | How |
|---|---|---|---|
| shadPS4 | GPL-2.0 | `gpu/` | kept, back-ports continue (bug reports only: anti-AI) |
| Supermedo/uncharted_pc, spacepepperoni/uncharted_Linux | GPL-2.0 | upstream | syncs for `gpu/` and assets; `src/` and Python changes ported by hand as `upstream-sync` issues once moved |
| revvv991/uncharted-android | GPL-2.0 | `android/`, 7.01 | adopt with credit |
| eladharels/uncharted_android | GPL-2.0 | Android device notes | learn |
| bloodborne-native/bloodborne-runtime, -mac, -ios | GPL-2.0 (to confirm at 8.01) | `apple/`, 8.03-8.06 | adopt with credit |
| bmy/bbport-mac | not yet read | Apple notes | learn until read |
| iannitram/totk-native | MIT | `vdf`, `steam`, `platform`, `bundle`, `hub`, `fleet`, `savecard`, the Continue brief | copy with credit |
| iannitram/save-relay | MIT | `relay`, Android and iOS | link |
| FEX-Emu/FEX | MIT | Android | through `android/` |
| K11MCH1/libadrenotools | BSD-2-Clause | Android | through `android/` |
| slint-ui/slint | royalty-free (attribution) | `app/` | dependency; About carries the notice |
| gilrs, mdns-sd, spake2, chacha20poly1305, sha2, cbindgen, cmake | MIT or Apache-2.0 | `app/`, `hub`, `runtime` | dependencies |
| SDL3 (`sdl3` crate) | Zlib | `runtime::pad`, window | dependency, as today |
| zorde23/Uncharted_Collection_PC, Mevlock, drakolordx7, Pepin208, AdilDevStuff forks | GPL-2.0 | fixes | adopt with credit through `from-peer` issues |
| Revivedx/SavageWindRecomp | MIT | packaging | learn, for 2.10 |
| bloodborne-native/bloodborne-launcher | GPL-2.0 | launcher profiles and settings | learn, for 1.16 and 2.06 |
| Ryxm-a/uncharted-ios | GPL-2.0 | iOS notes (FEX with sideload JIT) | watch, for 8.01 |
| GNSB/UFC-3-PS4-PORT-LINUX-WINDOWS | GPL-2.0 | a sibling PS4 port | watch |

## After 1.0

The Won'ts in `SPEC.md`: the in-game graphics menu and the Game Mode menu (#10, #167), Xbox prompts (#11), trophies in the launcher (#12), aspect ratios (#14, #158), AI beta testers (#15), shader preload (#16), the U1 1080p power study (#17), FSR 4 (#18), stereo (#19), plain ELF input (#66), the save edge cases (#38, #48-#53, #57), runner caching (#58), Mali GPUs.
