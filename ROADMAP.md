# Roadmap

Updated: 2026-11-12

features:
- campaign: Uncharted 1, 2 and 3 play start to finish on the AMD handheld (M1-M3): the playthroughs, progress blockers, crashes, physics, input and audio in today's runtime
- gpu: The graphics library draws each game correctly (M8): play-breaking renderer bugs, shadPS4 fixes, upscalers and GPU vendor workarounds
- perf: Power and frame rate (M5): render size, run settings, the benchmark and auto-tune
- crash-help: Crash help: the crash line, the known-issues list, the playthrough log and the after-crash screen
- prep: Stage 1, the prep tools in Rust: dump check, revision gate, package, patches, link, prepare, mods, and no game files (M9, M11)
- launcher: Stage 2 and stage 10, the faces: the CLI, the Slint app, the Continue screen, first run, Saves and Settings, About and languages, the design pass (M7, M10)
- runtime: Stage 3, the runtime to Rust, one module per piece, then the C build path deleted (M20)
- hubs: Stage 4, build hubs: discovery, pairing, serve and pull, fleet records (M12)
- steamos: Stage 5, SteamOS handhelds: Steam shortcut, artwork, handheld profile, Game Mode, the device hours (M13)
- windows: Stage 6, the Windows desktop: the build, the Windows shims, an hour per game (M14)
- android: Stage 7, Android on Adreno through FEXCore and Turnip (M17) (1.x)
- apple: Stage 8, Mac, iPhone and iPad through a recompiled library and Metal (M15, M16) (1.x)
- saves: Saves work, one save everywhere through save-relay, and updates keep saves (M4, M18, M19, stage 9)
- builds: Stage 0 and every build verified: housekeeping, CI, release builds, upstream syncs, project rules (M6)
- later: After 1.0 only, the new features among the Won'ts: in-game graphics menu, Game Mode menu, Xbox prompts, launcher trophies, AI beta testers, shader preload, FSR 4, stereo, plain ELF input, Mali GPUs
- save-relay: Save Relay integration: this port adopts the shared save-sync core (https://github.com/iannitram/save-relay, 1.0 reached 2026-11-11, release tag to follow) so its saves follow me between machines

From `SPEC.md` (approved 2026-11-01) to 1.0: Uncharted 1, 2 and 3 on the AMD handheld, the Strix Halo tablet and the RTX 3090 Ti desktop, the same save on each, each game start to finish on the AMD handheld, and our code in Rust. Mac, iPhone/iPad and Android are 1.x (scope ruling 2026-11-12, #268). The code map is `docs/code-map.md`; the decisions are `docs/adr/` (0001 workspace, 0002 runtime to Rust in strangler order, 0003 Slint launcher, 0004 build hubs, 0005 Android through FEXCore, 0006 Apple through a recompiled library and Metal). Every piece is an issue titled `<id>: ...`; `Blocked by` in each issue sets the order. The plan of 2026-10-24 (`docs/review-2026-10-24/ROADMAP-proposed.md`) is history now; its open work is carried into the stages below.

## Game files

On the RTX 3090 Ti desktop (`desktop-3090ti`), recorded 2026-11-12. These files never go into a repo or a bundle (M9).

| | Path |
|---|---|
| Raw package (US, v1.02, CUSA02320) | `L:\Dropbox\Software\Emulation\roms\PS4\Uncharted The Nathan Drake Collection - [US] [EN] [1.02].pkg` (read only; never install or extract into Dropbox) |
| Installed copy (55.7 GB: 15,195 files in 336 folders, 59,807,175,456 bytes, counted 2026-11-12) | `B:\games-donotdelete\Uncharted-Universal\CUSA02320\` (`eboot.bin`, `big2-ps4_Shipping.elf`, `big3-ps4_Shipping.elf`, `sce_discmap.plt`, `sce_module`, `sce_sys`, `shareddata`, `u1data`, `u2data`, `u3data`) |

How it was made: the shadPS4 fork's `shadps4.exe --extract-pkg <pkg> --extract-dir <new empty folder> --extract-threads 8`. shadPS4's `config.toml` `installDirs` points at `B:/games-donotdelete/Uncharted-Universal`.

## Features, by Must

| Must | Pieces |
|---|---|
| M1-M3 playthroughs on the AMD handheld | the correctness and campaign work already open (#31, #35, #37, #60, #62-#64, #109, #111-#113, #164 and the bug issues), crash help (#165: #168-#170, #175; the after-crash screen is 2.07, which replaced closed #176), playthroughs #76-#78; 5.02 |
| M4 saves work | 3.06 (savedata in Rust), 1.17, 9.02 |
| M5 power and frame rate | 1.14, 1.15, 1.16, 1.27, auto-tune (#166: #171, #173 (the `BB_BENCHMARK` runtime benchmark that the M5 soak reads), #174 and #178 are 1.27, and #172 is 1.05 `dump`; the display cards are 2.06, which replaced closed #177), #22; 5.02 |
| M6 every build verified | 0.02, 2.10, 3.01, 6.01; 1.x: 7.02, 8.02 |
| M7 the launcher starts each game | 1.04-1.15, 1.25, 2.02-2.05 |
| M8 play-breaking bugs | #31, #35, #37, all in `gpu/` (and #164 first) |
| M9 no game files | 1.23 (bundle never holds the dump), 1.26 (redact), PR review |
| M10 Continue screen | 1.18, 1.25, 2.01, 2.04, 2.05, 2.06, 2.07, 10.01 |
| M11 prepare on a hub, revision gate | 1.02-1.10, 1.24, 2.05 |
| M12 pull from a hub, fleet | 1.23, 4.01-4.05, 2.08 |
| M13 Steam shortcut, artwork, handheld profile | 1.16, 1.21, 1.22, 5.01, 5.02 |
| M14 Windows desktop | 3.15, 6.01, 6.02 |
| M15 Mac and the iOS install | 1.x: 8.01-8.05, 8.08 |
| M16 iPhone/iPad with a controller | 1.x: 8.05, 8.06, 8.09 |
| M17 Android | 1.x: 7.01-7.06 |
| M18 save-relay everywhere | 1.17, 1.25, 9.01 (#163), 9.02; 1.x: 7.05, 8.07 |
| M19 updates keep saves | 1.19, 1.20 |
| M20 our code in Rust | stage 1 (tools), stage 2 (launcher), stage 3 (runtime), 2.10 (the C in `packaging/`), 2.11, 3.17 |
| M21 resolution independence | 1.14 `render_size` (already done, PR #294), 1.28 `aspect` (pure Rust aspect-math module in `crates/core`: display or window size in, render size, HUD safe area, FOV mode and fit rectangle out, with unit tests), M21a (render size chosen from the display or window), M21b (HUD/menu anchoring in `gpu/`), M21c (FOV rule), M21d (video letter/pillarbox), M21e (runtime resize, presenter-only rescale plus restart prompt for a full re-render) |

Game-side pieces for M21 resolution independence (#297, #14):
- M21a: render size chosen from the display or window within the memory ceiling, via `render_size` (1.14).
- M21b: HUD, menus, pause and map anchoring in `gpu/` to a safe area, not stretched.
- M21c: camera FOV rule: Hor+ for wider screens, Vert+ or kept horizontal FOV for taller screens.
- M21d: pre-rendered video and cutscenes letterboxed or pillarboxed to their own aspect, with pillarbox only as a fallback.
- M21e: runtime resize and fold/unfold via presenter-only rescale plus a restart prompt for a full re-render.

## Stages

Each stage names its platforms (R04). Stages overlap: anything not blocked can be taken. The campaign work on today's C runtime (M1-M3, M8, #164 first) continues alongside; a bug fixed in C before its module moves is ported with the module.

0. **Housekeeping** (all). 0.01 the rename inside the repo: `uncharted_pc` and the `linux` branch in our docs, workflows, launcher update check and bug-report links. 0.02 the Rust workspace check (`check.yml`) on Linux, Windows, macOS, beside `build.yml`.
1. **Engine core: the prep tools in Rust** (Linux, Windows, macOS in CI). Each Python script becomes a `crates/core` module; its Python tests are the expected results. 1.01 error and platform; 1.02 `sfo`; 1.03 `games` and the revision gate; 1.04 `pkg`; 1.05 `dump` check (#172 folds in); 1.06 `content_profile`; 1.07 `patches`; 1.08 `link` (modules); 1.09 `link` (libc); 1.10 `prepare`; 1.11 `mods`; 1.12 `cheats`; 1.13 `trophies`; 1.14 `render_size`; 1.15 `run_env`; 1.16 `profile`; 1.17 `saves` and the save-relay manifest; 1.18 `savecard`; 1.19 `backup`; 1.20 `update`; 1.21 `vdf` and `steam` (from totk-native); 1.22 `artwork`; 1.23 `bundle` and `fleet` records; 1.24 `recipe` (targets, hubs); 1.25 `launch`; 1.26 `logs` (redact, crash line, known issues, playthrough log: #169 and #170 move here; #168 writes the line in the runtime, 3.06 and 3.16, and 1.26 reads it); 1.27 `tune` (the hardware report read from `uncharted-run --probe`, the choices it hides, the three cards, the chapter list of `recipe`: #171, #174, #178; reads #173's benchmark report); 1.28 `aspect`.
2. **Faces** (Linux, Windows; macOS builds in CI, the Mac app is 1.x). 2.01 the [Continue screen brief](docs/continue-screen.md), from totk-native's, for three games (design first: the Continue screen is the product; written, waiting for the owner's approval on #288); 2.02 the `uncharted-universal` CLI; 2.03 the Slint app scaffold with gilrs input, built in CI; 2.04 the Continue screen (built, waiting for the real-pad check, #293); 2.05 the guided first run (hub: dump, check, prepare; client: pair, pull; then Add to Steam); 2.06 Saves and Settings (the display cards, #177 folds in); 2.07 the after-crash screen (#176 folds in); 2.08 Fleet; 2.09 About (patches, licences, Slint notice) and the seven languages; 2.10 release builds (AppImage, Windows zip, Mac app); the Rust app takes over `packaging/bbport-entry.c` (it clears Steam's `LD_PRELOAD` itself) and `uncharted-universal play` takes over `packaging/windows/play.c` (built, waiting for the Steam Deck and Windows checks, #356); 2.11 retire `launcher/`, `scripts/`, `run.py` and the PyInstaller packaging.
3. **Runtime to Rust** (Linux first in CI, Windows with 3.15; macOS: CI build check only). 3.01 `crates/runtime` scaffold: static library linked into the loader, `cbindgen` header, `gpu/` built by its build script, the boot smoke test in CI, `abi` (the NID table and `runtime_resolve`, the first module), and `src/vulkan_smoke.c` as a test of the `gpu` build; then one module per piece, each exporting the C symbols it replaces: 3.02 `time` and `rtc`; 3.03 `sync`; 3.04 `content` and `display`; 3.05 `trophy`; 3.06 `savedata` (and `where.json`, #168); 3.07 `audio`; 3.08 `ajm`; 3.09 `file` (VFS, mods); 3.10 `pad` (SDL3, gyro, rumble; the scripted camera path of `BB_BENCHMARK`, #173); 3.11 `services` and `kernel`; 3.12 `memory`; 3.13 `fiber` and `thread`; 3.14 `cpu_patch` and `red_zone`; 3.15 `os` (Windows shims); 3.16 `loader` and the `uncharted-run` binary (with the `Crash:` line, #168, and `--probe`, which takes over `tools/gpu_capabilities.c`); 3.17 delete `src/` and the C build path. M21 in the runtime (#297): 3.18 M21a render size from the display or window, through `render_size` (1.14) and `aspect` (1.28); 3.19 M21b HUD and menu anchoring to the safe area in `gpu/`; 3.20 M21c the FOV rule; 3.21 M21d video and cutscene letterbox or pillarbox; 3.22 M21e live resize and fold/unfold, presenter rescale plus a restart prompt. A device check on the AMD handheld follows each of 3.02-3.16 (SPEC.md); ADR 0002 names six of them (savedata, audio, AJM, memory, threads, the loader).
4. **Build hubs** (Linux, Windows; the Mac hub and the Android client are 1.x). 4.01 discovery; 4.02 pairing; 4.03 serve and pull with the shader translations; 4.04 fleet records and drift; 4.05 `hub` and `fleet` commands. CI runs a fake hub on loopback.
5. **SteamOS handhelds** (Linux). 5.01 the Strix Halo tablet as hub: prepare all three games on itself, play an hour each from Game Mode; 5.02 the AMD handheld: install pulled from the tablet, Steam artwork, an hour each, the M5 soak; then the playthroughs #76-#78.
6. **Windows desktop.** 6.01 the Rust workspace and the GPU library on `windows-latest` (CLANG64, Rust target chosen here); 6.02 an hour per game with a controller.
7. **Android** (Adreno) (1.x, after 1.0). 7.01 adapt selected reviewed Android dependency/build, FEX bridge and Turnip/libadrenotools sources from revvv991 and zenithblue with credit through #207-#210, after the runtime ABI/thread/memory prerequisites; do not graft the whole peer project. 7.02 the runtime cross-builds for `aarch64-linux-android` in CI; 7.03 the Slint launcher's Android APK build with gamepad keys; 7.04 the hub serves the APK and install, the launcher installs them; 7.05 save-relay in the Android launcher; 7.06 a session per game with a controller; 7.07 the M21 check on the Pixel Fold: unfolded (about 6:5), the cover screen, and a fold/unfold mid-game.
8. **Mac, iPhone and iPad** (1.x, after 1.0). 8.01 study bloodborne-native's recompiler, Metal renderer and iOS shell: licence files, what is Bloodborne-specific, a written plan (docs); 8.02 the runtime builds for `aarch64-apple-darwin` and `aarch64-apple-ios` in CI; 8.03 bring in the recompiler, run on the Mac from my dump to a library per game; 8.04 the Metal renderer for the collection; 8.05 the Mac app from the launcher, then the iOS app built on the Mac and installed by cable; 8.06 iOS controller mapping, touch hidden with a controller; 8.07 save-relay in the iOS app (its C interface); 8.08 the Mac: an hour per game; 8.09 the iPhone or iPad: a session per game with a controller; 8.10 the M21 check on the iPad mini (about 3:2) and the Mac window resized.
9. **One save everywhere.** External dependency: [iannitram/save-relay](https://github.com/iannitram/save-relay) for Linux, Windows, macOS, iOS/iPadOS and Android. Save Relay reached 1.0 on 2026-11-11, so it is now adopted, not just referenced: every build carries it (tracker #249). Desktop integration (9.01, #163) waits on its rc.1 release cut (save-relay#205); Android and iOS (7.05, 8.07) wait on its C-interface libraries (save-relay#49); they are 1.x and no longer block 1.0. The Mac waits on a supported macOS build (8.05, 1.x). Save slots move the save folders, so the save-relay manifest and adapter follow the selected slot. 9.01 attaches the adapter in the launcher (#163 reworked), with 7.05 and 8.07 (1.x) covering the mobile integration; 9.02 verifies the round trip across the three 1.0 devices (the AMD handheld, the Strix Halo tablet and the RTX 3090 Ti desktop): the 1.0 sentence. This project does not build a separate sync service.
10. **Design pass** (R04), last before 1.0 (Linux/SteamOS and Windows; macOS: CI build check only). 10.01 the built screens against the Continue screen brief and the shared design system, including the Steam artwork and Game Mode; findings become `design-pass` issues.

## Platform readiness evidence

The [2026-11-06 code audit](docs/audits/2026-11-06-universal-platforms.md) maps current blockers to these existing stages. The [platform peer survey](PEERS.md) supplies frozen source and file-level license links; the [issue-disposition ledger](docs/audits/2026-11-06-issue-dispositions.md) records the full open-issue review. Prioritize the dump/revision/recipe contracts and a wired preparation flow before advertising target builds. Stage 8.01 is a hard source/synthetic feasibility gate (#211): it does not inspect or retain game dumps. It must assess the bloodborne-recomp, bbcpu and Metal route before downstream Apple implementation; the direct Uncharted Mac launcher is a Rosetta-based comparison, and the Uncharted iOS peer is a scaffold. No stage or acceptance gate is marked complete by the survey.

### Current implementation checkpoint (2026-11-06)

The foundations of #188 (PR #228), #189 (PR #231), #206 (PR #232), #207 (PR #229) and #211 (PR #230) are merged into `main`; the code map lists what each still owes. In particular, #189 still needs listener/discovery/CLI wiring and device acceptance; #207 has not been built with the Android SDK/NDK; and #211 is plan-only because CPU/Metal probes were not run on Apple hardware. No platform application or gameplay milestone is complete from this work. #188 is closed, and #211 and #212 are now 1.x work (scope ruling 2026-11-12, #268).

The next implementation wave (status 2026-11-12): #37 shader barrier is closed (PR #263, with a `wbinvl1` test; credits shadPS4 6752d8b1). #175 known-issue data is done on `main` (19 verified entries, each linked to an issue, loaded by a test that reads the shipped file). #190 Rust artwork generator: PR #223 is merged and emits the original images from Rust; #190 is reopened to delete `launcher/uncharted_art.py` and its test once nothing calls them. #31 ExpandImage stale-cache fix (PR #226) and #35 equeue lost-request fix (PR #225) are fixed on `main` since 2026-11-07; both issues stay open until the handheld check is recorded. Also merged 2026-11-12: #168 the Rust `crash::report` and its `--force-fault` test hook (PR #267; the C runtime that ships today still prints no `Crash:` line, so #168 stays open), #171 `uncharted-run --probe` hardware report (PR #260), #173 benchmark report and pad-path contracts only (PR #236; nothing reads `BB_BENCHMARK` yet, wiring is #242), #147 tune DLSS/FSR rules (PR #257), #138 RAM/VRAM line on the FPS counter (PR #264), #246 save slots in the Linux launcher (PR #254), #247 Turnip KGSL/FEX patches vendored (PR #255), CI on GitHub-hosted runners for Linux, Windows and macOS (PR #243), and an upstream sync of Supermedo/uncharted_pc `main` on 2026-11-11 (PR #239). Device and visual acceptance remain required; none of this marks its containing roadmap stage complete.


### Issue-backed peer import sequence

These issues refine work inside the existing platform stages; they do not close
or replace stage commitments. Issue #207 builds pinned Android dependencies and
a synthetic native artifact only, not the full game APK/engine. #208 depends on
#207 and adapts the Linux ARM64/glibc donor smoke harness to Android NDK/bionic
and Android execution. #209 depends on #208 plus the runtime C ABI scaffold
(3.01), memory (3.12), and thread/fiber (3.13); the Windows OS shims in 3.15
are not an Android prerequisite. #210 is a standalone native Turnip/libadrenotools
bootstrap after #207, before Slint app wiring: only explicitly selected,
validated drivers may start; invalid custom selection must fail closed and may
not silently fall back to an unvalidated stock Adreno driver. App wiring remains
inside 7.03. The exact source boundaries are in [PEERS.md](PEERS.md#issue-backed-platform-survey-2026-11-06).

For Apple, #211 gates implementation on source/synthetic coverage and a
file-level license/dependency map; it creates no game-dump inspection. #212 is
synthetic no-JIT CPU coverage after #211. Existing #31 and #37 remain the
small, issue-scoped shadPS4 import candidates; link to their pinned upstream changes in
PEERS.md rather than creating duplicates. The upstream `main` was merged into production `main` on 2026-11-11 (PR #239), so no sync batch is planned. #206 carries the
separate Rust SELF/plain-ELF loader-parity obligation from the closure of #66.
Save Relay reached 1.0 on 2026-11-11 and is folded into every build; tracker #249
holds one row per build. This project still builds no sync service of its own.

## Open-source pieces and where they fit

| Project | Licence evidence | Where | How |
|---|---|---|---|
| shadPS4 | GPL-2.0 | `gpu/` | kept in our fork; read-only source research only; no reports or PRs to shadPS4 (anti-AI) |
| Supermedo/uncharted_pc, spacepepperoni/uncharted_Linux | GPL-2.0 roots | current C/C++ lineage and Linux base | upstream `main` merged into production `main` on 2026-11-11 (PR #239); no sync batch now. Port behavior into Rust modules as separate issues. |
| zorde23/Uncharted_Collection_PC and older community forks | GPL-2.0 roots | Windows/Linux comparisons | targeted fixes only when an open bug has the same cause; no broad engine graft |
| revvv991/uncharted-android | GPL-2.0-or-later root; [`android/LICENSES.md`](https://github.com/revvv991/uncharted-android/blob/847faf5a802253cf90664144765f0e5d12eb410e/android/LICENSES.md#L1-L30) inventories dependencies | Android, #207-#210 | selected build/bridge references; exclude recipes, byte patches, generated game output and assets |
| zenithblue-oss/shadps4-arm64 | per-file SPDX MIT for selected FEX source; transitive `hle_trace.h` is GPL-2.0-or-later | Android FEX, #208/#209 | experimental synthetic harness/context/HLE reference, not a drop-in engine; preserve exact file and dependency notices |
| FEX-Emu/FEX | MIT root; bundled components have individual notices | Android, #207-#209 | FEXCore dependency through a pinned build recipe |
| bylaws/libadrenotools | BSD-2-Clause root and loader files | Android, #210 | verified original API/implementation; use explicit choice and verify loaded driver before Vulkan startup |
| JICA98/libadrenotools | BSD-2-Clause | Android dependency comparison | peer's exact pinned variant; prefer bylaws source unless its fork-specific patch is required |
| SDL3 (`sdl3` crate) | zlib | runtime input/window/audio | existing dependency; launcher UI remains Slint |
| eladharels/uncharted_android | GPL-2.0 root | Android device notes | learn; incomplete coverage and no additional import identified |
| bloodborne-native/bloodborne-recomp, bbcpu, bloodborne-runtime, -mac, -ios | mostly GPL-2.0 roots; exact files/dependencies vary; [`bbnet20` is GPL-3.0](https://github.com/bloodborne-native/bloodborne-runtime/blob/42736cf7ca7704b087e847482417d9f475bb2dd7/gpu/bbnet20/LICENSE.GPL-3.0#L1-L8) | Apple, #211/#212 before 8.03-8.06 | hard feasibility/license gate; no blanket adoption, no Bloodborne annotations/generated output, exclude GPL-3.0 component |
| Ryxm-a/uncharted-ios | GPL-2.0 root | iOS scaffold | learn only; default lite build does not run engine; conflicts with no-JIT route |
| bmy/bbport-mac | GPL-2.0 root | Apple comparison | Rosetta guest is outside approved no-Rosetta route |
| truwa/nathan-drake-collection-macos | no root license/API identifier established | Mac packaging comparison | learn only; Rosetta and different engine; no source copy until terms established |
| iannitram/totk-native | MIT | `vdf`, `steam`, `platform`, `bundle`, `hub`, `fleet`, `savecard`, Continue brief | copy with credit as already planned |
| iannitram/save-relay | MIT | `relay` and cross-platform saves | adopted at 1.0 (2026-11-11) in every build, tracker #249; #163 is the desktop adapter; no separate sync service |
| slint-ui/slint | royalty-free (attribution) | `app/` | dependency; About carries notice |
| gilrs, mdns-sd, spake2, chacha20poly1305, sha2, cbindgen, cmake, roxmltree, serde_json, sys-locale | MIT or Apache-2.0 | `app/`, `hub`, `patches`, `language`, `runtime` | dependencies; the ones the app and core link are credited on About (`app/src/vm/about.rs`, tested against this table) |
| Revivedx/SavageWindRecomp | MIT | packaging | learn, for 2.10 |
| bloodborne-native/bloodborne-launcher | GPL-2.0 | launcher profiles and settings | learn, for 1.16 and 2.06 |
| GNSB/UFC-3-PS4-PORT-LINUX-WINDOWS | GPL-2.0 | sibling PS4 port | watch |

## Save Relay integration

I'm adopting [Save Relay](https://github.com/iannitram/save-relay) for this port's saves. It shares game saves between machines through my own Nextcloud, pulling at launch and pushing at exit, as one small Rust core reused by every game port. Save Relay reached 1.0 on 2026-11-11 (release tag to follow). This port will adopt it for saves, so a save made on one of my machines is there on the next.

## After 1.0

The Won'ts in `SPEC.md`: the in-game graphics menu and the Game Mode menu (#10, #167), Xbox prompts (#11), trophies in the launcher (#12), ultrawide beyond the target aspects (#158), AI beta testers (#15), shader preload (#16), the U1 1080p power study (#17), FSR 4 (#18), stereo (#19), plain ELF input (#66), the save edge cases (#38, #48-#53, #57), runner caching (#58), Mali GPUs.
