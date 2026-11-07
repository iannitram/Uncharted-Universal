# Peers

Uncharted-Universal targets Linux, Windows, macOS, iOS/iPadOS and Android from
the player's own decrypted dump. This file records where other repositories
already provide platform work we can study or adapt. The machine-readable
`peer:` lines remain in [SPEC.md](SPEC.md#prior-art).

## Platform survey — checked 2026-11-06

Repository heads, READMEs, trees and the linked implementation files were checked
live on GitHub. Status claims below are source/maintainer reports, not game runs
measured by us. The [local code audit](docs/audits/2026-11-06-universal-platforms.md)
separates this fork's implementation from its planned platform integrations.
“Candidate” means study and adaptation, not imported code or a verified port.

| Target | Primary head start | What to inspect / reuse boundary |
|---|---|---|
| Linux / SteamOS | [Supermedo/uncharted_pc](https://github.com/Supermedo/uncharted_pc); [spacepepperoni/uncharted_Linux](https://github.com/spacepepperoni/uncharted_Linux) | Keep the upstream runtime/renderer lineage and Linux packaging. The Linux fork is our existing integration base, not a new port. |
| Windows | [Supermedo/uncharted_pc](https://github.com/Supermedo/uncharted_pc); [zorde23/Uncharted_Collection_PC](https://github.com/zorde23/Uncharted_Collection_PC) | Compare CLANG64 packaging, exception/ABI handling and game-specific fixes. The independent zorde23 engine has a different lineage; port selected fixes with provenance. |
| Android | [revvv991/uncharted-android](https://github.com/revvv991/uncharted-android); [eladharels/uncharted_android](https://github.com/eladharels/uncharted_android) | Direct Uncharted ports: FEXCore guest execution, native ARM64 host/renderer, Turnip and APK tooling. revvv991 reports testing one handheld; eladharels reports a different phone and incomplete game coverage. Neither establishes support for every Android GPU or dump revision. |
| macOS | [bloodborne-native/bloodborne-mac](https://github.com/bloodborne-native/bloodborne-mac); [truwa/nathan-drake-collection-macos](https://github.com/truwa/nathan-drake-collection-macos) | The Bloodborne family is the candidate for the approved no-Rosetta route. truwa is a direct Uncharted launcher with Rosetta 2 and KosmicKrisp: useful packaging/compatibility comparison, not the native-CPU milestone. |
| iOS / iPadOS | [bloodborne-native/bloodborne-ios](https://github.com/bloodborne-native/bloodborne-ios); [Ryxm-a/uncharted-ios](https://github.com/Ryxm-a/uncharted-ios) | Bloodborne is a no-JIT proof of concept with an AOT library and interpreter fallback, device reports and memory limits. Ryxm-a is an Uncharted FEX scaffold: its default lite build does not run the engine. Treat it as shell/bridge research, not a playable iOS port. |

### Source entry points

The Android guest bridge and the Apple CPU/recompiler are separate from the app
shells. Inspect these boundaries before copying platform-specific code:

- **Android build and dependency pins:** [revvv991/uncharted-android: android/scripts/build_all.sh](https://github.com/revvv991/uncharted-android/blob/847faf5a802253cf90664144765f0e5d12eb410e/android/scripts/build_all.sh) — FEX, Mesa/Turnip, NDK dependencies and APK orchestration.
- **Android guest CPU:** [revvv991/uncharted-android: src/guest_cpu_fex.cpp](https://github.com/revvv991/uncharted-android/blob/847faf5a802253cf90664144765f0e5d12eb410e/src/guest_cpu_fex.cpp) — guest calls and FEX contexts; compare with our x86 TLS, signals and fibers.
- **Second Android integration:** [eladharels/uncharted_android: docs/ANDROID_PORT.md](https://github.com/eladharels/uncharted_android/blob/d77e6c190489373b8c0b566f03c8060cd701a67a/docs/ANDROID_PORT.md) — different host/runtime lineage and guest-fiber handling.
- **Apple AOT generator:** [bloodborne-native/bloodborne-recomp: tools/recomp/recomp.sh](https://github.com/bloodborne-native/bloodborne-recomp/blob/c60ccf8be752b8690f2813505c4daf9eafdb136b/tools/recomp/recomp.sh) — game functions become C, then a host library; Bloodborne annotations and executable assumptions need replacement for all three Uncharted games.
- **Apple CPU layer:** [bloodborne-native/bbcpu: src/cpu/jit_arm64.c](https://github.com/bloodborne-native/bbcpu/blob/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca/src/cpu/jit_arm64.c) — interpreter/translator support; the iOS conditional disables JIT, so uncovered AOT code still needs a safe execution path.
- **Apple Metal backend:** [bloodborne-native/bloodborne-runtime: gpu/shadps4/video_core/renderer_metal/mtl_device.mm](https://github.com/bloodborne-native/bloodborne-runtime/blob/42736cf7ca7704b087e847482417d9f475bb2dd7/gpu/shadps4/video_core/renderer_metal/mtl_device.mm) — real renderer implementation; audit dependencies and Bloodborne-specific hooks before adoption.
- **Apple iOS library build:** [bloodborne-native/bloodborne-runtime: scripts/ios/recomp.sh](https://github.com/bloodborne-native/bloodborne-runtime/blob/42736cf7ca7704b087e847482417d9f475bb2dd7/scripts/ios/recomp.sh) — builds the generated library with the iPhone SDK; generated game code remains private.
- **iOS app entry:** [bloodborne-native/bloodborne-ios: app/app_main.c](https://github.com/bloodborne-native/bloodborne-ios/blob/8f08be42c881c85051f2076c3721dbb2e273b025/app/app_main.c) — in-process startup, recompiled-library loading, input/lifecycle and memory handling.
- **Uncharted iOS FEX scaffold:** [Ryxm-a/uncharted-ios: ios/core/fex/FexLoader.cpp](https://github.com/Ryxm-a/uncharted-ios/blob/f062ce0dda36466bd87f34a134f3f8bcca7c3cdc/ios/core/fex/FexLoader.cpp) — inspect bring-up gaps against the README; a bridge file and green lite build are not engine execution.
- **Direct Uncharted Mac packaging:** [truwa/nathan-drake-collection-macos: macos/bundle_native.py](https://github.com/truwa/nathan-drake-collection-macos/blob/aa0a3835428b2a80269316f1daf5e5f4bfaa6bd7/macos/bundle_native.py) — bundles a different Uncharted engine, KosmicKrisp and libraries; learning reference until reuse terms are established.
- **Alternative Mac implementation:** [bmy/bbport-mac: tools/macos/README-macos.md](https://github.com/bmy/bbport-mac/blob/a69f56af2069df45357e86c27396baa945b53231/tools/macos/README-macos.md) — Bloodborne: Rosetta guest plus native renderer process; does not satisfy our approved no-Rosetta route.
- **Windows build:** [Supermedo/uncharted_pc: build.sh](https://github.com/Supermedo/uncharted_pc/blob/8b66edc0192ea246ca63b360857402a0aa55626d/build.sh) — existing Windows baseline to retain while the Rust integration progresses.
- **Linux packaging:** [spacepepperoni/uncharted_Linux: packaging/linux/package.sh](https://github.com/spacepepperoni/uncharted_Linux/blob/cf8d0b7c9f70b03e9ca06e1204d8c63f91311c33/packaging/linux/package.sh) — existing AppImage work rather than inventing another packaging route.

### Snapshot and reuse checks

These are survey anchors, not imported dependency pins. Root GPL texts and the
GitHub license identifier do not audit every file. Before importing code, read
its file headers, dependency notices and the exact source license, retain
credits and record the source commit. The fork's standing GPL-2.0 rule remains.

In particular, the runtime tree contains
[an explicitly GPL-3.0 network component](https://github.com/bloodborne-native/bloodborne-runtime/blob/42736cf7ca7704b087e847482417d9f475bb2dd7/gpu/bbnet20/LICENSE.GPL-3.0); do not blanket-import that tree under its root label.
The checked Metal device file carries `GPL-2.0-or-later`. truwa's
[NOTICES.txt](https://github.com/truwa/nathan-drake-collection-macos/blob/aa0a3835428b2a80269316f1daf5e5f4bfaa6bd7/NOTICES.txt)
credits dependencies, but its root has no LICENSE file and the API has no
license identifier; establish permission for launcher code before copying it.

| Repository | Default branch at survey | Head | Observed license record |
|---|---|---|---|
| [Supermedo/uncharted_pc](https://github.com/Supermedo/uncharted_pc) | `main` | [`8b66edc0192e`](https://github.com/Supermedo/uncharted_pc/commit/8b66edc0192ea246ca63b360857402a0aa55626d) | GPL-2.0 API identifier; root LICENSE present |
| [spacepepperoni/uncharted_Linux](https://github.com/spacepepperoni/uncharted_Linux) | `main` | [`cf8d0b7c9f70`](https://github.com/spacepepperoni/uncharted_Linux/commit/cf8d0b7c9f70b03e9ca06e1204d8c63f91311c33) | GPL-2.0 API identifier; root LICENSE present |
| [zorde23/Uncharted_Collection_PC](https://github.com/zorde23/Uncharted_Collection_PC) | `uc3` | [`4fd0c045919c`](https://github.com/zorde23/Uncharted_Collection_PC/commit/4fd0c045919cca9665fc16248ab973f9a54215f8) | GPL-2.0 API identifier; root LICENSE present |
| [revvv991/uncharted-android](https://github.com/revvv991/uncharted-android) | `main` | [`847faf5a8022`](https://github.com/revvv991/uncharted-android/commit/847faf5a802253cf90664144765f0e5d12eb410e) | GPL-2.0 API identifier; root LICENSE present |
| [eladharels/uncharted_android](https://github.com/eladharels/uncharted_android) | `main` | [`d77e6c190489`](https://github.com/eladharels/uncharted_android/commit/d77e6c190489373b8c0b566f03c8060cd701a67a) | GPL-2.0 API identifier; root LICENSE present |
| [bloodborne-native/bloodborne-mac](https://github.com/bloodborne-native/bloodborne-mac) | `main` | [`5e6e8e714550`](https://github.com/bloodborne-native/bloodborne-mac/commit/5e6e8e714550ae37527c1876009a1ef1f5f3933a) | GPL-2.0 API identifier; root LICENSE present |
| [bloodborne-native/bloodborne-runtime](https://github.com/bloodborne-native/bloodborne-runtime) | `main` | [`42736cf7ca77`](https://github.com/bloodborne-native/bloodborne-runtime/commit/42736cf7ca7704b087e847482417d9f475bb2dd7) | GPL-2.0 API identifier; root LICENSE present |
| [bloodborne-native/bloodborne-recomp](https://github.com/bloodborne-native/bloodborne-recomp) | `main` | [`c60ccf8be752`](https://github.com/bloodborne-native/bloodborne-recomp/commit/c60ccf8be752b8690f2813505c4daf9eafdb136b) | GPL-2.0 API identifier; root LICENSE present |
| [bloodborne-native/bbcpu](https://github.com/bloodborne-native/bbcpu) | `main` | [`3f9b6c1a11d5`](https://github.com/bloodborne-native/bbcpu/commit/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca) | GPL-2.0 API identifier; root LICENSE present |
| [bloodborne-native/bloodborne-ios](https://github.com/bloodborne-native/bloodborne-ios) | `main` | [`8f08be42c881`](https://github.com/bloodborne-native/bloodborne-ios/commit/8f08be42c881c85051f2076c3721dbb2e273b025) | GPL-2.0 API identifier; root LICENSE present |
| [Ryxm-a/uncharted-ios](https://github.com/Ryxm-a/uncharted-ios) | `main` | [`f062ce0dda36`](https://github.com/Ryxm-a/uncharted-ios/commit/f062ce0dda36466bd87f34a134f3f8bcca7c3cdc) | GPL-2.0 API identifier; root LICENSE present |
| [bmy/bbport-mac](https://github.com/bmy/bbport-mac) | `macos-native-0.6` | [`a69f56af2069`](https://github.com/bmy/bbport-mac/commit/a69f56af2069df45357e86c27396baa945b53231) | GPL-2.0 API identifier; root LICENSE present |
| [truwa/nathan-drake-collection-macos](https://github.com/truwa/nathan-drake-collection-macos) | `main` | [`aa0a3835428b`](https://github.com/truwa/nathan-drake-collection-macos/commit/aa0a3835428b2a80269316f1daf5e5f4bfaa6bd7) | No root LICENSE / API identifier; learn only |

`Alyx-ML/uncharted_macos` was also checked after appearing in search results,
but GitHub returned 404 on 2026-11-06. It is not a verified source or adoption
candidate in this survey. A missing repository is not evidence that its work
never existed.

The approved Android route remains ADR 0005 (FEXCore/Turnip); the Apple route
remains ADR 0006 (recompiled library/Metal). This survey adds candidates and
implementation detail without changing the spec's Musts. Apple adaptation
feasibility is ROADMAP 8.01; do not assume another game's recompiler is portable
without executable coverage, ABI and renderer checks.

## Earlier survey and adopted-work history

The following notes retain the 2026-10-28 survey and the 2026-10-31 additions.
Star counts, ahead/behind counts, test reports and issue state are historical;
they were not refreshed by the platform survey above. The older reuse decisions
apply to the inspected code at that time, not to every file in today's peers.

`bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port` is not in that last group (checked 2026-10-31). GitHub shows no licence for it because the licence file sits in `gow3_pc/LICENSE` (GPL-2.0 text, present since the first commit, 2026-10-14), not at the repo root; the README's "Credits and license" section says "Licensed under the GNU GPL v2 or later" and invites reuse under the GPL with credit and a link back. Its source files carry no per-file SPDX headers. So `scripts/render_res.py` and `gpu/shim/bbport_movie.*` may stay (GPL-2.0-or-later into ours); keep the credit and link in `README.md` and the file headers.

## Upstreams

| Repo | Role | Watch for |
|---|---|---|
| [Supermedo/uncharted_pc](https://github.com/Supermedo/uncharted_pc) | Upstream (2065 stars, 421 open issues) | Issues and PRs for Linux, handhelds, render resolution, frame rate, upscalers |
| [spacepepperoni/uncharted_Linux](https://github.com/spacepepperoni/uncharted_Linux) | The Linux fork we sync from (190 ahead of upstream) | Linux launcher, Steam Deck mode, AppImage releases |

## Forks of the upstream, most active first

Measured 2026-10-28 against Supermedo `main` (109 forks, all pushed within 60 days; spacepepperoni's own 6 forks have nothing of note).

| # | Fork | Ahead / behind | What its own work touches |
|---|---|---|---|
| 1 | [drakolordx7/uncharted_pc](https://github.com/drakolordx7/uncharted_pc) (`community`) | 413 / 113 | The big community build: VRAM budget for small cards, FSR 3 leak fix, 45 FPS limit, save slots, walk speed. Much of it reached us through spacepepperoni. |
| 2 | [Pepin208/uncharted_pc_linux](https://github.com/Pepin208/uncharted_pc_linux) | 22 / 0 | Linux AppImage; shadPS4 texture-cache back-ports (#4818, #5118); GPU checkpoint diagnostics; launcher sends the heap sizes with `BB_GAME_RES` |
| 3 | [AdilDevStuff/uncharted_pc_linux](https://github.com/AdilDevStuff/uncharted_pc_linux) | 28 / 0 | Raw mouse look; CI AppImage/deb/rpm; aim-blur and auto-exposure dark-screen fix |
| 4 | [revvv991/uncharted-android](https://github.com/revvv991/uncharted-android) | 10 / 0 | Android on Adreno: ideas for low-power GPUs |
| 5 | [Mevlock/uncharted_pc-dev](https://github.com/Mevlock/uncharted_pc-dev) (`dev`) | 406 / 70 | Performance builds: move-action pool 64 → 256, buffer-arena probation, FPS counter RAM/VRAM line, jitter fix for U3 blue dots |
| 6 | [Cosmo/uncharted_xbox](https://github.com/Cosmo/uncharted_xbox) | 22 / 70 | Xbox/UWP port; nothing to take |
| 7 | [antikmewzik/uncharted_pc_controls_qol](https://github.com/antikmewzik/uncharted_pc_controls_qol) | 9 / 70 | U1 keyboard and mouse, per-controller rumble |
| 8 | [currantsforeyes/uncharted_pc](https://github.com/currantsforeyes/uncharted_pc) | 61 / 70 | 30 FPS option, thread priorities, off-thread pipeline compile, per-GPU pipeline cache |

## Sibling projects

| Repo | Licence | Watch for |
|---|---|---|
| [shadps4-emu/shadPS4](https://github.com/shadps4-emu/shadPS4) | GPL-2.0 | The emulator our `gpu/shadps4/` comes from: back-port its texture and buffer cache fixes |
| [bloodborne-native/bloodborne-runtime](https://github.com/bloodborne-native/bloodborne-runtime) | GPL-2.0 | The Bloodborne loader and Vulkan layer our shim descends from |
| [bloodborne-native/bloodborne-launcher](https://github.com/bloodborne-native/bloodborne-launcher) | GPL-2.0 | A launcher with profiles and settings: a model for #10 |
| [GNSB/UFC-3-PS4-PORT-LINUX-WINDOWS](https://github.com/GNSB/UFC-3-PS4-PORT-LINUX-WINDOWS) | GPL-2.0 | A sibling port with Linux and Windows builds |
| [AbdelmoumenDeghbouche/wipeout_pc](https://github.com/AbdelmoumenDeghbouche/wipeout_pc) | GPL-2.0 | A sibling PS4 port |
| [zorde23/Uncharted_Collection_PC](https://github.com/zorde23/Uncharted_Collection_PC) | GPL-2.0 | An independent Windows port of the same collection, from the God of War III port line (FelipePrado0/SpartanPort): U3 start to end at 60 FPS in 1080p. Watch its releases for crash and rendering fixes to port, with credit (added 2026-10-31) |

## Findings from the 2026-10-28 survey

Each is an issue labelled `from-peer` (adopt: port their code; learn: their idea, written fresh; drop-ours: they already did one of ours).

| Kind | Issue | Source |
|---|---|---|
| adopt | #136 shadPS4 #4818 texture-cache `CanContain` back-port (U1 blue textures, #128) | Pepin208 1d027419b |
| adopt | #137 move-action pool 64 → 256 for high frame rates | Mevlock d82693625, 51e993ee4 |
| adopt | #138 FPS counter line with RAM and VRAM | Mevlock cf02c6cff |
| adopt (or drop-ours) | #139 launcher sends heap sizes with `BB_GAME_RES` | Pepin208 eb4d69899 |
| learn | #140 VRAM budget and buffer-arena probation for small cards | drakolordx7 0cdde58e4, Mevlock 2d0b81552 |
| learn | #141 U3 blue-dot jitter fix, aim-blur dark-screen fix | Mevlock b2d69bafa, AdilDevStuff ae027aa4b |
| learn | 45 FPS through `BB_FPS_LIMIT` (noted on #124) | drakolordx7 44a0556c8 |
| drop-ours (theirs) | FSR 3.1 protected-memory patch: we already carry it (#114) | drakolordx7 6bc6ea38e |

Upstream issues brought in with an `Upstream:` line: #142 (DLSS probe on GPUs without DLSS), #143 (Intel HD 630 extension), #144 (RDNA 4 black weapons), #145 (Steam Input on non-Steam shortcuts), #146 (zh-CN). Upstream links added to our existing #10, #41, #114, #115, #116, #117, #118, #124 and #126.

## Supporting crates

| Crate | Role | Version and licence |
|---|---|---|
| [RazrFalcon/roxmltree](https://github.com/RazrFalcon/roxmltree) | Read-only parser for third-party patch XML; DTD expansion and external entity resolution are disabled | Pin 0.21.1; [crate metadata](https://docs.rs/crate/roxmltree/0.21.1/source/Cargo.toml.orig) declares `MIT OR Apache-2.0`; use the [MIT option](https://github.com/RazrFalcon/roxmltree/blob/master/LICENSE-MIT). The adapter validates numeric references because the parser maps invalid Unicode scalar references (such as surrogates) to U+FFFD. |
