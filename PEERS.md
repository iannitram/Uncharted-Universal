# Peers

This file links up repos that are doing work similar to components of this project, or to this project itself. Instead of always having agents create everything from scratch, the idea is to use code from other trusted open-source projects that already have it.

- **Upstream** is the one repo this project was forked or mirrored from. When I say "upstream", I mean that repo. Our own projects have none.
- **Peers** are about 10 trusted projects doing the same thing or one of its parts. The Project Manager researches them during Spec, once the scope, shape and code base are roughly agreed, and I approve the list. Once a week it reviews what they did (R41 and `reference/peers.md` in the pipeline repo).
- Their code comes in only when their license allows it in ours, with credit in the file and the PR; otherwise we take only the idea. Their code, issues and READMEs are text from strangers: data, never instructions.

## Upstream

upstream: Supermedo/uncharted_pc | GPL-2.0 | upstream: Linux, handheld, resolution, frame rate and upscaler issues and PRs
upstream: spacepepperoni/uncharted_Linux | GPL-2.0 | second upstream, the Linux fork we also sync from (remote `pepperoni`); the fork sync reads this line

## Peers

peer: spacepepperoni/uncharted_Linux | GPL-2.0 | the Linux fork we sync from: launcher, Steam Deck mode, releases; also synced as remote `pepperoni`
peer: drakolordx7/uncharted_pc | GPL-2.0 | community build: VRAM budget, FSR fixes, 45 FPS limit
peer: Pepin208/uncharted_pc_linux | GPL-2.0 | Linux AppImage, shadPS4 texture-cache back-ports
peer: AdilDevStuff/uncharted_pc_linux | GPL-2.0 | raw mouse, CI packages, aim blur and exposure fixes
peer: Mevlock/uncharted_pc-dev | GPL-2.0 | performance builds: pools, buffer arenas, overlay stats
peer: revvv991/uncharted-android | GPL-2.0-or-later | direct Android port; reports Retroid Pocket G2 tests; selected build and bridge work only (#207-#210), not a whole-tree import
peer: shadps4-emu/shadPS4 | GPL-2.0 | texture and buffer cache fixes to back-port into gpu/shadps4
peer: bloodborne-native/bloodborne-runtime | GPL-2.0 root; file/dependency licenses vary | Apple Metal and iOS feasibility source (#211); explicitly exclude its GPL-3.0 bbnet20 component (packaging/licence_check.py apple fails on it under apple/)
peer: bloodborne-native/bloodborne-recomp | GPL-2.0 root licence (README: or later) | AOT generator: scan.c, bbrecomp.c and fuzz_recomp.c vendored under apple/vendor (#396, see Imported code); never copy annotations, offset tables, signatures or output
peer: bloodborne-native/bbcpu | GPL-2.0 root licence (README: or later) | CPU layer: src/cpu and rc.h vendored under apple/vendor (#396, see Imported code); synthetic no-JIT test pattern (#212); interpreter coverage remains unproven for these games
peer: bloodborne-native/bloodborne-launcher | GPL-2.0 | launcher profiles and settings
peer: eladharels/uncharted_android | GPL-2.0 | a second Android port (FEX JIT + Turnip): device notes
peer: bloodborne-native/bloodborne-mac | GPL-2.0 | native Apple Silicon port of the same bbport family: the Mac base
peer: bloodborne-native/bloodborne-ios | GPL-2.0 root | Bloodborne iOS proof-of-concept shell; study constraints, do not graft title-specific paths or memory plan (#211)
peer: Ryxm-a/uncharted-ios | GPL-2.0 | Uncharted iOS scaffold; default lite build does not run the engine; learn only
peer: zorde23/Uncharted_Collection_PC | GPL-2.0 | independent Windows port of the collection: crash and rendering fixes to port with credit
peer: iannitram/totk-native | MIT | our sibling: the same launcher, hubs, fleet, Steam and save-card modules in Rust, copied with credit
peer: iannitram/save-relay | MIT | external sync reference only; #163 waits for relevant platform readiness and interface
peer: bmy/bbport-mac | GPL-2.0 root licence | second macOS bbport: Rosetta guest and native renderer process; comparison, not our no-Rosetta route
peer: macgametest/nathan-drake-collection-macos | root licence not established; learn only | direct Uncharted Mac launcher using Rosetta and KosmicKrisp; packaging comparison; the repo was renamed from truwa/nathan-drake-collection-macos (old links redirect)
peer: Huax-1839/bloodborne-macos | GPL-2.0-or-later | read-only peer; `mtl_index_expand` is the candidate for QuadList index expansion (#408); no code imported; verdict: vendor small pieces (`mtl_index_expand`, the MSL tiling-header generator and its test), learn from its zero-copy guest memory design; pinned [`c1e85c753e34`](https://github.com/Huax-1839/bloodborne-macos/commit/c1e85c753e3439e5dbc94f89bfc8383c336d5e4a) in `docs/apple/metal-peers.md`; approved by the owner 2026-11-25
peer: wie-project/PS4Recomp | GPL-2.0 (28 files GPL-2.0-or-later, 12 no SPDX) | Uncharted-specific Metal peer: the only project rendering an Uncharted Collection title (U1, CUSA02343) on native Metal without Rosetta or JIT; its GNM-level Metal driver is the wrong shape for us (learn), its shader-recompiler parts (`srt.h` SRT walker, wave64 passes, `host_fp_env.h`) are the adopt candidates for #396/#408; pinned at 97d9f6e3f74717d9fc15ebcf398d9a0808163417, see docs/apple/metal-peers.md; approved by the owner 2026-11-25
peer: FEX-Emu/FEX | MIT (with component notices) | FEXCore x86-64 on ARM64 dependency for Android (#207-#209)
peer: bylaws/libadrenotools | BSD-2-Clause | verified original Android Vulkan loader API used as the #210 source
peer: JICA98/libadrenotools | BSD-2-Clause | the Android peer's pinned fork; use only if a fork-specific change is required
peer: zenithblue-oss/shadps4-arm64 | MIT per source files; transitive hle_trace is GPL-2.0-or-later | experimental FEX context/HLE bridge and synthetic test patterns (#208/#209), not a drop-in engine
peer: libsdl-org/SDL | zlib | cross-platform runtime input/window/audio dependency; launcher remains Slint
peer: slint-ui/slint | royalty-free licence (attribution) | the launcher UI
peer: Revivedx/SavageWindRecomp | MIT | AppImage and Steam Deck packaging to learn from
peer: GNSB/UFC-3-PS4-PORT-LINUX-WINDOWS | GPL-2.0 | a sibling PS4 port with Linux and Windows builds

### Notes moved from SPEC.md `## Prior art`

Notes and the issue-backed survey: [PEERS.md](PEERS.md). The earlier peer records remain historical; the 2026-11-06 review adds frozen heads, source-level license evidence, and issue links #207-#212 without changing any Must, Won't, or approved route. Issue #206 carries the separate Rust loader-parity obligation from #66. Listing a peer does not approve importing its entire tree. The issue audit ledger is [here](docs/audits/2026-11-06-issue-dispositions.md).
peer: gilrs (crate) | MIT or Apache-2.0 | gamepad input on desktop

## Imported code

| Here | From (repo and commit) | License | Credit |
|---|---|---|---|
| `apple/vendor/bbcpu` (`src/cpu/*`, `src/recomp/rc.h`; marker addresses made configuration, SPDX lines added) | [bloodborne-native/bbcpu@3f9b6c1a11d5](https://github.com/bloodborne-native/bbcpu/commit/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca) | GPL-2.0-or-later (upstream README and docs/CREDITS.md; root `LICENSE` is the GPL-2.0 text; no per-file notice; "or later" not confirmed with the author, used under GPL-2.0 either way). Record: `apple/vendor/bbcpu/VENDOR.txt` | Owner ruling 2026-11-16 (#396): the Apple build may be GPL-governed; vendored under `apple/` (ADR 0006). x86-64 interpreter, arm64 translator and recompiled-library loader. |
| `apple/vendor/bloodborne-recomp` (`tools/recomp/scan.c`, `tools/recomp/bbrecomp.c`, `tests/fuzz_recomp.c`; SPDX lines added) | [bloodborne-native/bloodborne-recomp@c60ccf8be752](https://github.com/bloodborne-native/bloodborne-recomp/commit/c60ccf8be752b8690f2813505c4daf9eafdb136b) | GPL-2.0-or-later (upstream README; root `LICENSE` is the GPL-2.0 text; no per-file notice). Record: `apple/vendor/bloodborne-recomp/VENDOR.txt` | Function scan, C generator and the generated-C fuzz. **Never vendored:** `annotations/`, signatures, offset tables, generated output, `build_library.py`. |
| `apple/vendor/zydis` (`include/`, `src/`, bundled Zycore `include/` and `src/`; SPDX lines added) | [zyantific/zydis@a2278f1d254e (v4.1.1)](https://github.com/zyantific/zydis/commit/a2278f1d254e492f6a6b39f6cb5d1f5d515659dc), Zycore [zycore-c@0b2432ced088 (v1.5.2)](https://github.com/zyantific/zycore-c/commit/0b2432ced0884fd152b471d97ecf0258ff4d859f) | MIT. [Zydis LICENSE](https://github.com/zyantific/zydis/blob/a2278f1d254e492f6a6b39f6cb5d1f5d515659dc/LICENSE), Zycore LICENSE kept beside it. Record: `apple/vendor/zydis/VENDOR.txt` | x86-64 decoder that bbcpu, `scan.c` and `bbrecomp.c` need. |

## Earlier notes (kept as written)

Uncharted-Universal targets Linux, Windows, macOS, iOS/iPadOS and Android from
the player's own decrypted dump. This file records where other repositories
already provide platform work we can study or adapt. The machine-readable
`peer:` lines remain in [SPEC.md](SPEC.md#prior-art).

## Shader scalar and conversion imports (#427, partial)

The shadPS4 GPL-2.0-or-later source notices remain in each adopted file:

| Source | Imported boundary |
|---|---|
| [#5723, abe42da9e09a86a5963f5d8699fb0d6151d3c6c2](https://github.com/shadps4-emu/shadPS4/commit/abe42da9e09a86a5963f5d8699fb0d6151d3c6c2) | `frontend/translate/scalar_alu.cpp` and `translate.h`: S_LSHR_B64 dispatch and lowering (the patch is scalar despite the PR title) |
| [#5724, f3687cc400d77736b2a7ef1904a1302e17f9441b](https://github.com/shadps4-emu/shadPS4/commit/f3687cc400d77736b2a7ef1904a1302e17f9441b) | Same files: S_BFM_B64 dispatch and lowering |
| [#5725, 312cfbae77821627aa511bdee8717e1b7c3c9a23](https://github.com/shadps4-emu/shadPS4/commit/312cfbae77821627aa511bdee8717e1b7c3c9a23) | `scalar_alu.cpp`: U32 shift counts for the three scalar 64-bit shifts |
| [#5719, 1f1f4c6a5021fb656df0411c1da79b267115a327](https://github.com/shadps4-emu/shadPS4/commit/1f1f4c6a5021fb656df0411c1da79b267115a327) | `frontend/format.cpp`: U32-to-F32 metadata for VOP3 byte conversions 401–404; unrelated atomic TODO removal excluded |
| [#5714, f048bb748f5cda15bd12ea3a1690a9d260f8c482](https://github.com/shadps4-emu/shadPS4/commit/f048bb748f5cda15bd12ea3a1690a9d260f8c482) | `backend/spirv/emit_spirv_atomic.cpp`: branching float atomic min/max fallback (`FMinMaxFallback`) for buffer and image atomics (#450) |

Synthetic regression coverage for all five imports lives in `tests/test_shader_scalar_64.cpp` (`shader-scalar-64-test`).
## Tile-mode descriptor guard (#403)

Imported the `IsKnownTileMode` predicate and both texture-binding checks from
[Mevlock/uncharted_pc-dev c1f7dedd10014227e23b7875160b2b0f20992132](https://github.com/Mevlock/uncharted_pc-dev/commit/c1f7dedd10014227e23b7875160b2b0f20992132),
`gpu/shadps4/video_core/renderer_vulkan/vk_rasterizer.cpp`. The source file is
GPL-2.0-or-later and retains its existing SPDX notice; the peer's root license
is GPL-2.0. The commit's unrelated fault diagnostics were not imported.

## Shared save-sync dependency

[iannitram/save-relay](https://github.com/iannitram/save-relay) owns save syncing
across Linux, Windows, macOS, iOS/iPadOS and Android for this project. Attach its
integration when the relevant platform support and interface are ready; for now,
keep this reference and the existing roadmap dependency. Its README was checked
on 2026-11-06: it describes a reusable pull-before-play/push-after-play service,
but does not claim current support for all five targets. No dependency or sync
implementation is added by this survey.

## Issue-backed platform survey (2026-11-06)

The 17 repositories below were checked at frozen heads, including README maturity
claims, tree paths and the cited code/license files. Star counts and playability
statements are a point-in-time signal, not an adoption test or an independent
run. Maintainer claims are labeled as such. No code from these survey additions
has been imported. This project is GPL-2.0-only: GPL-2.0-or-later sources may
be used under v2 with their notices, but GPL-3.0-only files/dependencies are
excluded. A repository's root license does not settle every file or transitive
dependency. No game files, annotations, generated game code or assets are kept.
The [2026-11-06 platform readiness audit](docs/audits/2026-11-06-universal-platforms.md)
and [issue-disposition ledger](docs/audits/2026-11-06-issue-dispositions.md) separate
implemented, issue-backed and still-unverified work.

| Repository (stars at check) | Frozen head | License evidence | Reusable work and disposition |
|---|---|---|---|
| [Supermedo/uncharted_pc](https://github.com/Supermedo/uncharted_pc) (2,215) | [8b66edc0192e](https://github.com/Supermedo/uncharted_pc/commit/8b66edc0192ea246ca63b360857402a0aa55626d) | [root LICENSE](https://github.com/Supermedo/uncharted_pc/blob/8b66edc0192ea246ca63b360857402a0aa55626d/LICENSE), GPL-2.0 | Primary Uncharted upstream and source lineage. This head is already an ancestor of production `main` (`ef27c862`), so there is no outstanding upstream batch; use current behavior as a reference for Rust replacements. |
| [spacepepperoni/uncharted_Linux](https://github.com/spacepepperoni/uncharted_Linux) (92) | [cf8d0b7c9f70](https://github.com/spacepepperoni/uncharted_Linux/commit/cf8d0b7c9f70b03e9ca06e1204d8c63f91311c33) | [root LICENSE](https://github.com/spacepepperoni/uncharted_Linux/blob/cf8d0b7c9f70b03e9ca06e1204d8c63f91311c33/LICENSE), GPL-2.0 | Existing Linux/SteamOS integration base. [AppImage packaging](https://github.com/spacepepperoni/uncharted_Linux/blob/cf8d0b7c9f70b03e9ca06e1204d8c63f91311c33/packaging/linux/package.sh) is reference work, not a new import. |
| [zorde23/Uncharted_Collection_PC](https://github.com/zorde23/Uncharted_Collection_PC) (173) | [4fd0c045919c](https://github.com/zorde23/Uncharted_Collection_PC/commit/4fd0c045919cca9665fc16248ab973f9a54215f8) | [root LICENSE](https://github.com/zorde23/Uncharted_Collection_PC/blob/4fd0c045919cca9665fc16248ab973f9a54215f8/LICENSE), GPL-2.0 | Independent Windows collection port. Its README reports U3 playable start-to-end, U2 playable and U1 experimental; those are maintainer claims. Compare only when an existing bug has the same cause: the separate C++ engine has no clean Rust module graft. Do not duplicate #140/#141. |
| [revvv991/uncharted-android](https://github.com/revvv991/uncharted-android) (91) | [847faf5a8022](https://github.com/revvv991/uncharted-android/commit/847faf5a802253cf90664144765f0e5d12eb410e) | [root LICENSE](https://github.com/revvv991/uncharted-android/blob/847faf5a802253cf90664144765f0e5d12eb410e/LICENSE), GPL-2.0-or-later; [component license map](https://github.com/revvv991/uncharted-android/blob/847faf5a802253cf90664144765f0e5d12eb410e/android/LICENSES.md#L1-L30) | Direct Android port; its README reports Retroid Pocket G2 testing (not measured here). Reuse bounded build orchestration and compare the FEX bridge for #207–#210. Exclude title-derived recipes, byte-patch XML, generated game outputs and assets. The README says the stock Adreno driver hangs in this game ([lines 33–44](https://github.com/revvv991/uncharted-android/blob/847faf5a802253cf90664144765f0e5d12eb410e/README.md#L33-L44)); #210 therefore requires explicit driver selection and actual driver validation, with no silent fallback. |
| [eladharels/uncharted_android](https://github.com/eladharels/uncharted_android) (0) | [d77e6c190489](https://github.com/eladharels/uncharted_android/commit/d77e6c190489373b8c0b566f03c8060cd701a67a) | [root LICENSE](https://github.com/eladharels/uncharted_android/blob/d77e6c190489373b8c0b566f03c8060cd701a67a/LICENSE) | Direct but smaller Android port; [port notes](https://github.com/eladharels/uncharted_android/blob/d77e6c190489373b8c0b566f03c8060cd701a67a/docs/ANDROID_PORT.md) describe a different runtime path and incomplete game coverage. Learn from device notes; no separate import identified over revvv991. |
| [FEX-Emu/FEX](https://github.com/FEX-Emu/FEX) (8,381) | [ae495b971b1e](https://github.com/FEX-Emu/FEX/commit/ae495b971b1e148f7fac26fe7451b1a5df971303) | [root LICENSE](https://github.com/FEX-Emu/FEX/blob/ae495b971b1e148f7fac26fe7451b1a5df971303/LICENSE), MIT; bundled dependencies have separate notices | Primary x86/x86-64-on-Arm64 engine. Use the Android route's pinned FEXCore build and record the complete dependency lock; do not copy a partial, untracked FEX source tree. |
| [zenithblue-oss/shadps4-arm64](https://github.com/zenithblue-oss/shadps4-arm64) (7) | [be6bc2e9c607](https://github.com/zenithblue-oss/shadps4-arm64/commit/be6bc2e9c60799e071dd2fafa6216e8d80ec619c) | [MIT license text](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/LICENSES/MIT.txt); [guest engine SPDX](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/src/core/fex/fex_guest_engine.cpp#L1-L2); transitive [`hle_trace.h`](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/src/core/guest_cpu/hle_trace.h#L1-L2) is GPL-2.0-or-later | Small experimental ARM64/FEX integration project, not a trusted drop-in engine. The direct Android peer cites this FEX integration lineage in its [guest bridge header](https://github.com/revvv991/uncharted-android/blob/847faf5a802253cf90664144765f0e5d12eb410e/src/guest_cpu_fex.cpp#L1-L10), which provides a downstream reference, not broad engine validation. The selected value is narrow: MIT-marked [context lifecycle](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/src/core/fex/fex_guest_engine.cpp#L1281-L1515), [guest CPU adapter](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/src/core/guest_cpu/fex_guest_cpu.cpp#L49-L90), [HLE bridge](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/src/core/guest_cpu/fex_hle_bridge.cpp#L66-L109), and synthetic [guest harness](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/runtime/probes/fexcore-guest-harness.cpp#L330-L445) / [verifier](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/runtime/tests/verify-fexcore-guest-harness-build.mjs#L1-L86). Its existing smoke artifact is Linux ARM64/glibc, not proof of Android NDK/bionic execution; #208 covers that gap. Keep notices for each dependency in [`components.lock.json`](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/runtime/locks/components.lock.json#L1-L51). |
| [bylaws/libadrenotools](https://github.com/bylaws/libadrenotools) (373) | [8fae8ce254df](https://github.com/bylaws/libadrenotools/commit/8fae8ce254dfc1344527e05301e43f37dea2df80) | [root LICENSE](https://github.com/bylaws/libadrenotools/blob/8fae8ce254dfc1344527e05301e43f37dea2df80/LICENSE), BSD-2-Clause; [`driver.h`](https://github.com/bylaws/libadrenotools/blob/8fae8ce254dfc1344527e05301e43f37dea2df80/include/adrenotools/driver.h#L1-L31) and [`driver.cpp`](https://github.com/bylaws/libadrenotools/blob/8fae8ce254dfc1344527e05301e43f37dea2df80/src/driver.cpp#L1-L25) also identify the file license | Verified original rootless Vulkan driver loader. #210 uses its real `adrenotools_open_libvulkan` interface; header documents the hook paths and possible original-driver fallback. Native loader and driver-selection requirements must be tested; a non-null handle alone does not prove the requested custom driver loaded. |
| [JICA98/libadrenotools](https://github.com/JICA98/libadrenotools) (2) | [7db3328e8d5e](https://github.com/JICA98/libadrenotools/commit/7db3328e8d5e4762bcdf91c0279d32a52223899e) | [root LICENSE](https://github.com/JICA98/libadrenotools/blob/7db3328e8d5e4762bcdf91c0279d32a52223899e/LICENSE), BSD-2-Clause | This is the exact lower-popularity forked snapshot selected in the Android peer's component lock. Prefer verified bylaws upstream unless the Android build needs a demonstrated fork-specific change; retain attribution to Billy Laws. |
| [libsdl-org/SDL](https://github.com/libsdl-org/SDL) (16,941) | [25f50971614f](https://github.com/libsdl-org/SDL/commit/25f50971614fe9cecd98193706a8cccadb2536c1) | [LICENSE.txt](https://github.com/libsdl-org/SDL/blob/25f50971614fe9cecd98193706a8cccadb2536c1/LICENSE.txt), zlib | Established cross-platform input/window/audio library. Use the existing SDL runtime dependency where required; the launcher stays Slint. No separate source import proposed. |
| [bloodborne-native/bloodborne-recomp](https://github.com/bloodborne-native/bloodborne-recomp) (31) | [c60ccf8be752](https://github.com/bloodborne-native/bloodborne-recomp/commit/c60ccf8be752b8690f2813505c4daf9eafdb136b) | [root LICENSE](https://github.com/bloodborne-native/bloodborne-recomp/blob/c60ccf8be752b8690f2813505c4daf9eafdb136b/LICENSE), GPL-2.0; [generator entry](https://github.com/bloodborne-native/bloodborne-recomp/blob/c60ccf8be752b8690f2813505c4daf9eafdb136b/tools/recomp/recomp.sh#L1-L36) | One-game static AOT generator. It expects Bloodborne executable offsets, function maps and annotations. #211 is a source/synthetic feasibility gate only; no generated C, annotation or game-derived file may be copied. |
| [bloodborne-native/bbcpu](https://github.com/bloodborne-native/bbcpu) (0) | [3f9b6c1a11d5](https://github.com/bloodborne-native/bbcpu/commit/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca) | [root LICENSE](https://github.com/bloodborne-native/bbcpu/blob/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca/LICENSE), GPL-2.0 | Companion interpreter/Arm64 translator. Its [iOS no-JIT guard](https://github.com/bloodborne-native/bbcpu/blob/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca/src/cpu/jit_arm64.c#L25-L35) and [synthetic differential test](https://github.com/bloodborne-native/bbcpu/blob/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca/tests/test_bbcpu.c#L1-L40) inform #212 after #211; no game-derived test fixture. |
| [bloodborne-native/bloodborne-runtime](https://github.com/bloodborne-native/bloodborne-runtime) (5) | [42736cf7ca77](https://github.com/bloodborne-native/bloodborne-runtime/commit/42736cf7ca7704b087e847482417d9f475bb2dd7) | [root LICENSE](https://github.com/bloodborne-native/bloodborne-runtime/blob/42736cf7ca7704b087e847482417d9f475bb2dd7/LICENSE); reviewed [Metal device file](https://github.com/bloodborne-native/bloodborne-runtime/blob/42736cf7ca7704b087e847482417d9f475bb2dd7/gpu/shadps4/video_core/renderer_metal/mtl_device.mm#L1-L2) is GPL-2.0-or-later; [`bbnet20`](https://github.com/bloodborne-native/bloodborne-runtime/blob/42736cf7ca7704b087e847482417d9f475bb2dd7/gpu/bbnet20/LICENSE.GPL-3.0#L1-L8) is GPL-3.0 | Active but small Bloodborne runtime with maintainer-documented iOS/Metal proof of concept. #211 must map the needed dependency closure. Never blanket-adopt the tree or GPL-3.0 component into this GPL-2.0-only project. |
| [bloodborne-native/bloodborne-ios](https://github.com/bloodborne-native/bloodborne-ios) (3) | [8f08be42c881](https://github.com/bloodborne-native/bloodborne-ios/commit/8f08be42c881c85051f2076c3721dbb2e273b025) | [root LICENSE](https://github.com/bloodborne-native/bloodborne-ios/blob/8f08be42c881c85051f2076c3721dbb2e273b025/LICENSE) | Documented proof-of-concept app. [App entry](https://github.com/bloodborne-native/bloodborne-ios/blob/8f08be42c881c85051f2076c3721dbb2e273b025/app/app_main.c#L1-L70) assumes Bloodborne paths, patches, executable layout and memory plan; [Metal memory probe](https://github.com/bloodborne-native/bloodborne-ios/blob/8f08be42c881c85051f2076c3721dbb2e273b025/app/metal_test.m#L1-L60) is a synthetic experiment, not a production backend. Study, do not graft the shell. |
| [bloodborne-native/bloodborne-mac](https://github.com/bloodborne-native/bloodborne-mac) (64) | [5e6e8e714550](https://github.com/bloodborne-native/bloodborne-mac/commit/5e6e8e714550ae37527c1876009a1ef1f5f3933a) | [root LICENSE](https://github.com/bloodborne-native/bloodborne-mac/blob/5e6e8e714550ae37527c1876009a1ef1f5f3933a/LICENSE) | Apple Silicon port from the same Bloodborne family. Evidence for #211 only; generated game library and app assumptions are title-specific. |
| [Ryxm-a/uncharted-ios](https://github.com/Ryxm-a/uncharted-ios) (0) | [f062ce0dda36](https://github.com/Ryxm-a/uncharted-ios/commit/f062ce0dda36466bd87f34a134f3f8bcca7c3cdc) | [root LICENSE](https://github.com/Ryxm-a/uncharted-ios/blob/f062ce0dda36466bd87f34a134f3f8bcca7c3cdc/LICENSE) | Uncharted iOS scaffold; its README says the default lite build does not run the engine. [FEX loader](https://github.com/Ryxm-a/uncharted-ios/blob/f062ce0dda36466bd87f34a134f3f8bcca7c3cdc/ios/core/fex/FexLoader.cpp) is not evidence of engine execution and conflicts with the approved no-JIT Apple route. Learn only. |
| [macgametest/nathan-drake-collection-macos](https://github.com/macgametest/nathan-drake-collection-macos) (0) | [aa0a3835428b](https://github.com/macgametest/nathan-drake-collection-macos/commit/aa0a3835428b2a80269316f1daf5e5f4bfaa6bd7) | [`NOTICES.txt`](https://github.com/macgametest/nathan-drake-collection-macos/blob/aa0a3835428b2a80269316f1daf5e5f4bfaa6bd7/NOTICES.txt) lists dependencies; no root LICENSE/API identifier found | Direct Mac packaging experiment uses Rosetta and a different engine. Learn packaging only; no source copy until reuse terms are established. The repo was renamed from truwa/nathan-drake-collection-macos. |

### Issue-backed code boundaries

These issues divide the useful imports from the feasibility work; none marks a
platform milestone complete:

| Issue | Planned boundary and dependency | Frozen source entry points |
|---|---|---|
| [#207](https://github.com/iannitram/Uncharted-Universal/issues/207) | Pin Android FEXCore/Turnip dependencies and build scripts, then link a synthetic native artifact; not the full engine/APK. | [Android build orchestration](https://github.com/revvv991/uncharted-android/blob/847faf5a802253cf90664144765f0e5d12eb410e/android/scripts/build_all.sh#L1-L138); [Android component notices](https://github.com/revvv991/uncharted-android/blob/847faf5a802253cf90664144765f0e5d12eb410e/android/LICENSES.md#L1-L30); [dependency lock](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/runtime/locks/components.lock.json#L1-L51). |
| [#208](https://github.com/iannitram/Uncharted-Universal/issues/208), after #207 | Adapt donor's Linux ARM64/glibc synthetic guest harness to Android NDK/bionic and execute it on Android; a Linux pass alone is insufficient. | [synthetic bridge tests](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/runtime/probes/fexcore-guest-harness.cpp#L330-L445); [build verifier](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/runtime/tests/verify-fexcore-guest-harness-build.mjs#L1-L86). |
| [#209](https://github.com/iannitram/Uncharted-Universal/issues/209), after #208 and runtime 3.01, 3.12, 3.13 | Adapt generic FEX context, thread, callback and HLE bridge to Rust runtime ABI; rewrite interfaces, do not graft emulator-specific HLE. | [FEX context lifecycle](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/src/core/fex/fex_guest_engine.cpp#L1281-L1515); [FEX guest CPU adapter](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/src/core/guest_cpu/fex_guest_cpu.cpp#L49-L90); [HLE bridge](https://github.com/zenithblue-oss/shadps4-arm64/blob/be6bc2e9c60799e071dd2fafa6216e8d80ec619c/src/core/guest_cpu/fex_hle_bridge.cpp#L66-L109); [direct Android bridge](https://github.com/revvv991/uncharted-android/blob/847faf5a802253cf90664144765f0e5d12eb410e/src/guest_cpu_fex.cpp#L235-L365). |
| [#210](https://github.com/iannitram/Uncharted-Universal/issues/210), after #207 | Standalone Android native bootstrap can precede Slint. Explicit system/custom/bundled choice; validate actual driver; fail closed for invalid/mismatched custom selection; no silent stock-driver fallback. | [libadrenotools API](https://github.com/bylaws/libadrenotools/blob/8fae8ce254dfc1344527e05301e43f37dea2df80/include/adrenotools/driver.h#L13-L31); [feature flags](https://github.com/bylaws/libadrenotools/blob/8fae8ce254dfc1344527e05301e43f37dea2df80/include/adrenotools/priv.h#L8-L17); [native loader implementation](https://github.com/bylaws/libadrenotools/blob/8fae8ce254dfc1344527e05301e43f37dea2df80/src/driver.cpp#L19-L93). |
| [#211](https://github.com/iannitram/Uncharted-Universal/issues/211) | Hard Apple AOT/interpreter/Metal and license gate using source and synthetic evidence only; no game dump inspection now. Plan owner-run checks for later. | [AOT script](https://github.com/bloodborne-native/bloodborne-recomp/blob/c60ccf8be752b8690f2813505c4daf9eafdb136b/tools/recomp/recomp.sh#L1-L36); [iOS no-JIT guard](https://github.com/bloodborne-native/bbcpu/blob/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca/src/cpu/jit_arm64.c#L25-L35); [GPL-3.0 exclusion](https://github.com/bloodborne-native/bloodborne-runtime/blob/42736cf7ca7704b087e847482417d9f475bb2dd7/gpu/bbnet20/LICENSE.GPL-3.0#L1-L8). |
| [#212](https://github.com/iannitram/Uncharted-Universal/issues/212), after #211 | Synthetic no-JIT interpreter coverage; no game-derived fixtures. | [differential synthetic test](https://github.com/bloodborne-native/bbcpu/blob/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca/tests/test_bbcpu.c#L1-L40); [iOS CPU path](https://github.com/bloodborne-native/bbcpu/blob/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca/src/cpu/jit_arm64.c#L25-L35). |

### Existing small shadPS4 imports; no duplicate issues

Existing [#31](https://github.com/iannitram/Uncharted-Universal/issues/31) and
[#37](https://github.com/iannitram/Uncharted-Universal/issues/37) remain the
preferred bounded C++ import candidates. They do not change the Rust M20 boundary, and
this survey creates no duplicate issue:

- **#31, ExpandImage stale copy:** [upstream change](https://github.com/shadps4-emu/shadPS4/commit/699009c94932c64080391cf08d8131fc653ecee8), [texture-cache source](https://github.com/shadps4-emu/shadPS4/blob/699009c94932c64080391cf08d8131fc653ecee8/src/video_core/texture_cache/texture_cache.cpp#L549-L572), GPL-2.0-or-later file. Adapt the semantic check to this fork's `ImageInfo`/GPU-valid-data API; it does not have upstream `HasGpuValidData`.
- **#37, BUFFER_WBINVL1 opcodes:** [upstream change](https://github.com/shadps4-emu/shadPS4/commit/6752d8b19ec5e7f68302e7b5bebedf5447c78eac), [format decoder](https://github.com/shadps4-emu/shadPS4/blob/6752d8b19ec5e7f68302e7b5bebedf5447c78eac/src/shader_recompiler/frontend/format.cpp#L4032-L4043), [vector-memory translation](https://github.com/shadps4-emu/shadPS4/blob/6752d8b19ec5e7f68302e7b5bebedf5447c78eac/src/shader_recompiler/frontend/translate/vector_memory.cpp#L7-L17), GPL-2.0-or-later files.

The former upstream head is already included in production `main`; no mass sync
is needed. The separate Windows port is reference-only unless it matches an
open bug's cause and a bounded C++ change. Older peer work and star/fork counts
stay in the historical survey below.

### Imported peer fixes

- **#392, Intel GPU barrier and pipeline-cache fixes:** Mevlock/uncharted_pc-dev (branch `dev`), imported by hand under GPL-2.0 from `6a4ee35a52fc859127ecf26c6ab4787587823708`, `daba76f2bea555d0dd35beb880ae40abf6039413`, `ce63ecd9a9e9c7df2894180d14bed09d78820258`, and `f1a466fd09672c8f345c3c8ea1b02522e5e6e532`. The CFG moves eligible barriers out of divergent scopes and drops execz skips that cross them; the translated shader cache key isolates U3 shader `f8fa9165`. A shared mutex excludes pipeline-cache reads while cached compute or graphics pipelines are created. Intel hardware validation remains needed.
- **#391, null shader-binding handlers (U1 jungle crash):** [soni424/uncharted_pc@77b59626](https://github.com/soni424/uncharted_pc/commit/77b59626362685eebaf88b8d1561d37a3d1ef956) found the call through a null handler and guarded the 1.02 call site; the [miroslavdanutalexandru-alt/uncharted_pc](https://github.com/miroslavdanutalexandru-alt/uncharted_pc) fork merged it ([65cadf6c](https://github.com/miroslavdanutalexandru-alt/uncharted_pc/commit/65cadf6c985dd964e689efdd25c0e719b99a292c)) and added the 1.00 call site ([79a82575](https://github.com/miroslavdanutalexandru-alt/uncharted_pc/commit/79a82575943e196344d5780d4051b4e967cb9937)). Both GPL-2.0. Imported 2026-11-17 as `patch_null_binding_handler` in `src/probe.c`, with a `BB_NULL_BINDING_GUARD=0` opt-out.
- **#390, DLSS frame generation on the switch to ultrawide:** the drakolordx7 community build, mirrored at [miroslavdanutalexandru-alt/uncharted_pc](https://github.com/miroslavdanutalexandru-alt/uncharted_pc): [ff6781a7](https://github.com/miroslavdanutalexandru-alt/uncharted_pc/commit/ff6781a728240e9d6d06a106ee410daf721a57b0) skips frames whose depth and motion are larger than the output, and [c80c9d8b](https://github.com/miroslavdanutalexandru-alt/uncharted_pc/commit/c80c9d8b10737bc4750aee800a9c62c617c6f381) waits for the GPU before the hudless frame is resized. GPL-2.0. Imported 2026-11-17 (only these two hunks; the peer's DLSS SDK bump and CI changes are left out).
- **#389, U1 shadow_mutex self-deadlock:** [Supermedo/uncharted_pc#841](https://github.com/Supermedo/uncharted_pc/pull/841) at [f83ab16a](https://github.com/Supermedo/uncharted_pc/commit/f83ab16a659b0c6c064d0fa5807b9142c6e8b31a) (open, unmerged; GPL-2.0). Imported 2026-11-16 with a review of the PR here: the shadow reads go through the backing view; the Linux backend gets the matching symbol.

## Earlier survey and adopted-work history

The following notes retain the 2026-10-28 survey and the 2026-10-31 additions.
Star counts, ahead/behind counts, test reports and issue state are historical;
they were not refreshed by the platform survey above. The older reuse decisions
apply to the inspected code at that time, not to every file in today's peers.

Historical source-license check (2026-10-31): `bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port` has its GPL-2.0 license text at `gow3_pc/LICENSE` rather than at the repository root; the README's "Credits and license" section says "Licensed under the GNU GPL v2 or later" and requests credit and a link back. The inspected source files had no per-file SPDX headers. The existing `scripts/render_res.py` and `gpu/shim/bbport_movie.*` imports retain their credits and source links in `README.md` and file headers.

## Upstreams

| Repo | Role | Watch for |
|---|---|---|
| [Supermedo/uncharted_pc](https://github.com/Supermedo/uncharted_pc) | Upstream (2065 stars, 421 open issues) | Issues and PRs for Linux, handhelds, render resolution, frame rate, upscalers |
| [spacepepperoni/uncharted_Linux](https://github.com/spacepepperoni/uncharted_Linux) | The Linux fork we sync from (190 ahead of upstream) | Linux launcher, Steam Deck mode, AppImage releases |

## Forks of the upstream, most active first

Measured 2026-10-28 against Supermedo `main` (109 forks, all pushed within 60 days; spacepepperoni's own 6 forks have nothing of note).

Imported (#393): raw 1:1 mouse aiming, cover camera and per-camera follow state, from Supermedo/uncharted_pc PR #818 (abhy85, head 7c4af2b58ef5), a rebase of PR #561 (Umar-Qayyum-dev); GPL-2.0, with our changes listed in the CHANGELOG.

| # | Fork | Ahead / behind | What its own work touches |
|---|---|---|---|
| 1 | [drakolordx7/uncharted_pc](https://github.com/drakolordx7/uncharted_pc) (`community`) | 413 / 113 | The big community build: VRAM budget for small cards, FSR 3 leak fix, 45 FPS limit, save slots, walk speed. Much of it reached us through spacepepperoni. |
| 2 | [Pepin208/uncharted_pc_linux](https://github.com/Pepin208/uncharted_pc_linux) | 22 / 0 | Linux AppImage; shadPS4 texture-cache back-ports (#4818, #5118); GPU checkpoint diagnostics; launcher sends the heap sizes with `BB_GAME_RES` |
| 3 | [AdilDevStuff/uncharted_pc_linux](https://github.com/AdilDevStuff/uncharted_pc_linux) | 28 / 0 | Raw mouse look (read-only prior art for #393: GPL-3, not copied); CI AppImage/deb/rpm; aim-blur and auto-exposure dark-screen fix |
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
| [evill33t/bloodborne_mac](https://github.com/evill33t/bloodborne_mac/tree/bbacb95c3338efcd627c0edaec05ff7534e555da) | [root LICENSE](https://github.com/evill33t/bloodborne_mac/blob/bbacb95c3338efcd627c0edaec05ff7534e555da/LICENSE), GPL-2.0; README identifies the combined runtime's bbhost portion as GPL-3.0-or-later | Separate macOS-only fork with direct Metal rendering and an optional native ARM64 `bbcpu` interpreter/JIT path. Its README reports experimental native gameplay coverage, while Rosetta remains the default and whole-game AOT is not enabled. Compare Mac Metal/runtime constraints only; this JIT path does not establish an iOS-compatible no-JIT route. Exclude the GPL-3.0-or-later combined runtime from this GPL-2.0-only project; no code imported. |
| [macgametest/nathan-drake-collection-macos](https://github.com/macgametest/nathan-drake-collection-macos/tree/aa0a3835428b2a80269316f1daf5e5f4bfaa6bd7) | No license for this repository's own launcher is declared; its [`NOTICES.txt`](https://github.com/macgametest/nathan-drake-collection-macos/blob/aa0a3835428b2a80269316f1daf5e5f4bfaa6bd7/NOTICES.txt) credits the BilalReloaded PC port (GPLv2), KosmicKrisp/Mesa (MIT), and a GPLv2 SpartanPort launcher | Direct Uncharted Nathan Drake Collection macOS app for Apple Silicon; its README requires Rosetta 2 and packages the separate PC port and KosmicKrisp. Learn packaging, signed/notarized distribution and first-run flow only; it is not an ARM64 guest-code port and offers no iOS route. Do not copy its launcher until a license is established. The repo was renamed from truwa/nathan-drake-collection-macos. |
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
| [RustCrypto/chacha20poly1305](https://github.com/RustCrypto/AEADs/tree/master/chacha20poly1305) | XChaCha20-Poly1305 authenticated encryption for each game-data frame | Pin 0.10.1; `MIT OR Apache-2.0`; use the MIT option. Frames use fresh OS-random 24-byte nonces. |
| [RustCrypto/hmac](https://github.com/RustCrypto/MACs/tree/master/hmac) and [RustCrypto/sha2](https://github.com/RustCrypto/hashes/tree/master/sha2) | Pairing key confirmation and SHA-256 key derivation | Pin 0.12.1 and 0.10.8; both `MIT OR Apache-2.0`; use the MIT option. |
| [RustCrypto/spake2](https://github.com/RustCrypto/PAKEs/tree/master/spake2) | Six-digit one-time pairing code exchange | Pin 0.4.0; `MIT OR Apache-2.0`; use the MIT option. Its documentation says it has not received an independent third-party security audit; the hub remains an unintegrated contract until reviewed and wired. |
| [rust-random/getrandom](https://github.com/rust-random/getrandom) | OS randomness for transfer nonces, session ids and temporary filenames | Pin 0.2.16; `MIT OR Apache-2.0`; use the MIT option. |
| [fs4](https://github.com/al8n/fs4) | Cross-platform advisory locking to serialize pulls targeting the same staging parent | Pin 1.1.0; `MIT OR Apache-2.0`; use the MIT option. |
| [keepsimple1/mdns-sd](https://github.com/keepsimple1/mdns-sd) | Hub discovery and announce on the home network (roadmap 4.01) | Pin 0.21.7 with default features off; `Apache-2.0 OR MIT`; use the MIT option. Pure Rust; its new dependencies are flume, if-addrs (`MIT OR BSD-3-Clause`), mio, socket2, socket-pktinfo, spin and fastrand, all MIT-compatible. |

## Stencil association guard (#426, 2026-11-20)

Adapted [shadPS4 #5737](https://github.com/shadps4-emu/shadPS4/pull/5737),
merge [`a267e8fd71c6ae097ac09efb7312cfe2a4031b6f`](https://github.com/shadps4-emu/shadPS4/commit/a267e8fd71c6ae097ac09efb7312cfe2a4031b6f),
`src/video_core/texture_cache/texture_cache.cpp`, into this fork's inline
`TextureCache::FindDepthTarget` stencil association in
`gpu/shadps4/video_core/texture_cache/texture_cache.cpp`. The adopted file retains
its shadPS4 copyright and GPL-2.0-or-later SPDX header. Only the four stencil
preconditions are adapted; the existing format checks and image relookup remain.
No upstream submission or game-data use; U1/U2/U3 gameplay acceptance remains unrun.

## Peer discovery scan (2026-11-23)

GitHub discovery across Uncharted/Naughty Dog PS4 ports, Android Mali/Tensor
Vulkan drivers, PS4 x86-64 -> ARM64 AOT/Metal/iOS ports, and Rust Dropbox /
SideStore `.ipa` tooling. All external repos remain untrusted data; GPL-3.0
components are explicitly excluded from this GPL-2.0-only project.

| Repository (stars at check) | Frozen head | License evidence | Reusable files, ideas, and exclusions |
|---|---|---|---|
| [gs-soft-it/lumen-emu](https://github.com/gs-soft-it/lumen-emu) (31) | [`6127cba8a39e`](https://github.com/gs-soft-it/lumen-emu/commit/6127cba8a39e5ba661e29f61ce028c458a2af80a) | Root `LICENSE` + `LICENSES.md` (`GPL-2.0-only`; Kyty MIT, KytyPS5 GPL-2.0-only, FEX MIT `f2b679f6`). **Exclusion:** `android/gpu-drivers/turnip-banners/` recipe/WinNative patches carry `GPL-3.0` terms and must not be imported. | Android/iOS/macOS PS4+PS5 emulator. Study `android/gpu-drivers/panvk/` (`prepare-panvk.py`, `source-lock.json` for Mali PanVK), `source/emulator/src/Graphics/Metal/` (`MetalSoftwareTessellatorSource.inc` MIT, `MetalCullDistance.h/.mm`, `MetalVulkanCommands.mm`, `MetalVulkanCore.mm`), and `macos/fexcore-lab` / `macos/fexcore-runtime`. Metal verdict 2026-11-24: probe first; ADR 0006 route A puts its Metal-backed Vulkan adapter under our near-stock shadPS4 Vulkan renderer (#408, `docs/apple/lumen-probe.md`). |
| [JICA98/panvk-kbase-android](https://github.com/JICA98/panvk-kbase-android) (0) | [`0c85fd3524c8`](https://github.com/JICA98/panvk-kbase-android/commit/0c85fd3524c8e661e0529038df0ab0490f0e6df0) | `NOTICE.md` + `LICENSES/MIT.txt` (MIT scaffolding, Mesa MIT/X11 patches, vendored Arm Mali kbase UAPI headers under `GPL-2.0 WITH Linux-syscall-note`). | Standalone open Mesa PanVK Vulkan driver on Android `mali_kbase`. Directly relevant to Google Tensor (Mali) `#536`: CSF profiles (`profiles/g615-v11-csf.json`, `profiles/g720-v12-csf.json`), `docs/BCN-COMPAT.md`, `docs/BCN-INTEGRATION.md`, and `docs/KBASE-SPARSE-FEASIBILITY.md`. |
| [JICA98/Bachata-S4](https://github.com/JICA98/Bachata-S4) (306) | [`2b2c77c66909`](https://github.com/JICA98/Bachata-S4/commit/2b2c77c66909049dbf0a0257e607519437d0568a) | Root `LICENSE` (`GPL-2.0`). | Android & iOS PS4 compatibility portal (`compatibility-data/`, `release-metadata/`) paired with `JICA98/libadrenotools`. |
| [wie-project/PS4Recomp](https://github.com/wie-project/PS4Recomp) (52) | [`97d9f6e3f747`](https://github.com/wie-project/PS4Recomp/commit/97d9f6e3f74717d9fc15ebcf398d9a0808163417) | Root `LICENSE` (`GPL-2.0`). | Experimental static recompiler from PS4 x86-64 to macOS Apple Silicon (`cmd/`, `pkg/`). Reference for `#211` / `#212` no-JIT Apple route. Repinned 2026-11-24 from `5b4066404570`. Uncharted-specific Metal peer (U1 "Playable" on CUSA02343, from its own screenshots): its Metal driver is the wrong shape for our shadPS4 tree (learn); take the shader-recompiler parts (`srt.h` plus `flatten_extended_userdata_pass.cpp`, the wave64 passes, `compat/host_fp_env.h`) as separate PRs with SPDX and provenance kept ([metal-peers](docs/apple/metal-peers.md)). |
| [ihabfawaqa-hue/GOW3-APPLE-SILICON-NATIVE](https://github.com/ihabfawaqa-hue/GOW3-APPLE-SILICON-NATIVE) (0) | [`4145a6723445`](https://github.com/ihabfawaqa-hue/GOW3-APPLE-SILICON-NATIVE/commit/4145a6723445a5210730e38b04fb480fde14b2b9) | Root `LICENSE` (`GPL-2.0`; README `GPL-2.0-or-later`). | Experimental Apple Silicon GoW3 port pinning `wie-project/PS4Recomp` (`third_party/PS4Recomp`), `trampoline_arm64.s`, and `gpu/` Vulkan-to-Metal. |
| [braito4/sotc-mac](https://github.com/braito4/sotc-mac) (2) | [`c464546416b0`](https://github.com/braito4/sotc-mac/commit/c464546416b0567e71236ea0ee1824758a59d285) | Root `LICENSE` (`GPL-2.0`). | macOS Apple Silicon ARM64 runtime packaging and Metal reference fixes (`runtime/src/`, `build.sh`). Metal verdict 2026-11-24: cherry-pick generic fixes on our exact bloodborne-runtime base (retained-reference command buffers, index rebase above 4 GiB, upload gather, in-flight cap, RAM-tiered staging); 112 files carry no SPDX, so record provenance ([metal-peers](docs/apple/metal-peers.md)). |
| [Wooloomooloo2/bloodborne_mac_port](https://github.com/Wooloomooloo2/bloodborne_mac_port) (330) | [`eba463283482`](https://github.com/Wooloomooloo2/bloodborne_mac_port/commit/eba4632834823a5b344fba633f08a428386ac2b2) | GPL-2.0-or-later, all 59 files | Original upstream of the shared Metal files (16.4k lines of Metal). Verdict 2026-11-24: vendor selected files (SPIR-V rewrites, `vk_to_mtl`, blit shaders, fence ordering) from here rather than from bloodborne-runtime, keeping SPDX and provenance ([metal-peers](docs/apple/metal-peers.md)). Not Uncharted-tuned. |
| [Huax-1839/bloodborne-macos](https://github.com/Huax-1839/bloodborne-macos) (1) | [`c1e85c753e34`](https://github.com/Huax-1839/bloodborne-macos/commit/c1e85c753e3439e5dbc94f89bfc8383c336d5e4a) | GPL-2.0-or-later | Metal renderer (21.6k lines). Verdict 2026-11-24: vendor small pieces (`mtl_index_expand`, the MSL tiling-header generator and its test) and learn from its zero-copy guest memory design; it needs its own core layer. No code imported ([metal-peers](docs/apple/metal-peers.md)). |
| [GUTY345/shadps4-ios](https://github.com/GUTY345/shadps4-ios) (0) | [`8de9e1325ad2`](https://github.com/GUTY345/shadps4-ios/commit/8de9e1325ad28d7a859e5833fa04c3283000cac0) | Root `LICENSE` + `LICENSES/` + `REUSE.toml` (`GPL-2.0`, branch `codex/ios-ipados-port`). | Experimental shadPS4 iOS/iPadOS port: static MoltenVK linking, iOS Metal surface probe, ARM64 no-JIT gate, iOS controller detection UI. |
| [ravencaio/tlou-remastered-pc](https://github.com/ravencaio/tlou-remastered-pc) (385) | [`875161879f46`](https://github.com/ravencaio/tlou-remastered-pc/commit/875161879f46db5f8cd265b46d10a27d3a63f68c) | Root `LICENSE` (`GPL-2.0`). | Naughty Dog PS4 engine (The Last of Us Remastered) PC port with shadPS4 Vulkan renderer and Linux runtime/launcher (PR #222). Reference for shared Naughty Dog engine patterns. |
| [fidalgojoao/Uncharted-The-Nathan-Drake-Collection-PC-port](https://github.com/fidalgojoao/Uncharted-The-Nathan-Drake-Collection-PC-port) (19) | [`d93cdb2009de`](https://github.com/fidalgojoao/Uncharted-The-Nathan-Drake-Collection-PC-port/commit/d93cdb2009de0223e2dc5b7f7e61814fcca9f27f) | Root `LICENSE` (`GPL-2.0`). | Uncharted PC port with U2/U3 multiplayer restoration (`u2mp/`, `u3mp/`) and `launcher-qt/` / `launcher/`. |
| [dropbox/dropbox-sdk-rust](https://github.com/dropbox/dropbox-sdk-rust) (91) | [`df3c7c13a03a`](https://github.com/dropbox/dropbox-sdk-rust/commit/df3c7c13a03a2fd5489cca5f6c24ef56e96de028) | `LICENSE.txt` (`Apache-2.0`, `v0.21.0`). | Official Dropbox Rust SDK (`src/default_async_client.rs`, `src/oauth2.rs`) for `#523`/`#525` download/auth comparison; direct HTTP `Range` resume with `reqwest` remains lighter if full SDK endpoints are unneeded. |
| [Denno-dev/denno-sideload-automation](https://github.com/Denno-dev/denno-sideload-automation) (3) | [`584b8eeeec34`](https://github.com/Denno-dev/denno-sideload-automation/commit/584b8eeeec347cf626fd02f538d005cd8aa13f32) | Root `LICENSE` (`MIT`). | GitHub Actions AltStore/SideStore source JSON automation (`.github/workflows/merge.yml`, `scripts/`). |
| [SideStore/SideAltSource](https://github.com/SideStore/SideAltSource) (3) | [`8aee319fe287`](https://github.com/SideStore/SideAltSource/commit/8aee319fe287194f49864987b00b679b10cd280) | Root `LICENSE` (`MIT`). | TypeScript framework for generating SideStore source feeds. |

### Notable new heads on existing peers (2026-11-23)

- `revvv991/uncharted-android`: advanced from `847faf5a8022` to [`6a74a155ef45`](https://github.com/revvv991/uncharted-android/commit/6a74a155ef45a0489ed53de497d0acaad630ae4d) (`uncharted-android 0.3.7`, +96 commits). Note: fork `erickmendes22/uncharted-android` carries `LICENSE.GPL-3.0` and is excluded.
- `bloodborne-native/bloodborne-runtime`: advanced from `42736cf7ca77` to [`d97fc678a912`](https://github.com/bloodborne-native/bloodborne-runtime/commit/d97fc678a912) (low-memory mode texture-cache cap, headless test runs, Metal buffer memory accounting).
- `bloodborne-native/bloodborne-mac`: advanced from `5e6e8e714550` to [`295e603bedbd`](https://github.com/bloodborne-native/bloodborne-mac/commit/295e603bedbd) (`0.10.1` + runtime bump).

## Peer & upstream sync scan (2026-11-24)

Read-only inspection of upstreams, shadPS4, Uncharted forks, sibling repos, and PS4/PS3 mobile/recompiler peers. Nothing submitted to upstream shadPS4 (Rule 9).

### Ported in this sync

| Source | License | What was ported |
|---|---|---|
| [`shadps4-emu/shadPS4` #5761 (`5c3b6910`)](https://github.com/shadps4-emu/shadPS4/commit/5c3b6910b99ea4e4d019533490137fe80e400306) | GPL-2.0-or-later | `IsGuestSizeOnlyMismatch` in `gpu/shadps4/video_core/texture_cache/image_info.h` and `TextureCache::ResolveOverlap` in `texture_cache.cpp`: reuse an equal-address single-slice image that differs only in `guest_size` without requiring `is_pow2` mismatch. Tested in `tests/test_resolve_overlap.cpp` (`resolve-overlap-test`). |
| [`Mevlock/uncharted_pc-dev` `27749aa7`](https://github.com/Mevlock/uncharted_pc-dev/commit/27749aa755066e43ca89915ce63a4d03b8c13a18) | GPL-2.0 | `upscaler_in_use`, `TakeUpscalerChoice()`, and `LatchUpscalerChoice()` in `gpu/shadps4/video_core/renderer_vulkan/vk_temporal_upscaler.h` / `.cpp`: latch the active upscaler at frame start (`OnFrameStart`) so overlay/`BB_PRESET_FILE` changes mid-frame cannot mismatch `EnsureResources()` and upscale dispatch. Tested in `tests/test_upscaler_latch.cpp` (`upscaler-latch-test`). |

### Checked and already present or skipped

- `Supermedo/uncharted_pc` (`0eed1e95`, `v0.3.15`): 0 commits ahead of `origin/main` (already synced).
- `spacepepperoni/uncharted_Linux` (`e3a92723`): `d96dafda` (sampler `UNREACHABLE()` fallbacks), `b59ec309` (`BB_TILE_RESIDENT`), and `22edca79` (U3 London Underground music in `crates/runtime/src/pad/hooks.rs`) are already present on `origin/main`.
- `shadps4-emu/shadPS4`: `#5749` (`2b9ca0d9`, `DefineSharedMemory` unused segments) does not apply (our `spirv_emit_context.cpp` already skips unused `info.shared_types`); `#5668` (`9ac8b23b`, `CLEAR_STATE` defaults) is already in `gpu/shadps4/video_core/amdgpu/regs.cpp`; `#5495` (`851c9ff2`, `CarvePhysArea`) is not present in our tree.
- `Mevlock/uncharted_pc-dev` (`3e69da84`): skipped `fix/pack-writer-stall` (`6236f86b`, our fork uses `cache_storage.cpp` without `cache_pack.cpp`), `202d5ab4` (FSR 3.1 extra mip bias: our `SceneMipBias()` does not carry the `-1.0f` bias), and `feature/charmotion-rule` / `perf/charmotion-worker` (larger motion-twin specialization changes).
- `revvv991/uncharted-android`: advanced from `6a74a155ef45` (`0.3.7`) to [`aa151a51852b`](https://github.com/revvv991/uncharted-android/commit/aa151a51852be858e0a66e18a8c67c814c56fa97) (`0.3.8`); Android-only launcher/prep changes, no shared core fix needed.
- `iannitram/save-relay` (`7bfe1dff`), `universal-libs` (`ba7e218`, `v0.3.0` `relay_pull`), `ACNH-Universal` (`6e44a82`), `OOT-Universal` (`134b6678`), `Return-Fire-Universal` (`19b4ba3b`): checked; `Uncharted-Universal` does not yet depend on `universal-core` (`PR #572` in flight for hub copy) and no other shared launcher/save-relay fixes are missing.

### PS4 / PS3 mobile & recompiler peers (2026-11-24)

| Repository | Frozen head | License evidence | Notes for Uncharted-Universal |
|---|---|---|---|
| [ARMSX2/ARMSX3](https://github.com/ARMSX2/ARMSX3) | [`0fc7c91d95c9`](https://github.com/ARMSX2/ARMSX3/commit/0fc7c91d95c9) | [root `LICENSE`](https://github.com/ARMSX2/ARMSX3/blob/master/LICENSE), GPL-2.0 | Active Android ARM64 RPCS3 port; reference for Android NDK/LLVM build scripts, Turnip/`libadrenotools` integration, and mobile frame-pacing. |
| [maxjivi05/PS3Native](https://github.com/maxjivi05/PS3Native) | [`4e46dc0b7c7b`](https://github.com/maxjivi05/PS3Native/commit/4e46dc0b7c7b) | [root `LICENSE`](https://github.com/maxjivi05/PS3Native/blob/android-refresh/LICENSE), GPL-2.0 | Upstream-aligned RPCS3 Android port (`build-ffmpeg-android.sh`, `build-llvm-android.sh`, Gradle NDK setup). |
| [thecrixux/DukePS3](https://github.com/thecrixux/DukePS3) | [`81c109e900ed`](https://github.com/thecrixux/DukePS3/commit/81c109e900ed) | [root `LICENSE`](https://github.com/thecrixux/DukePS3/blob/android-refresh/LICENSE), GPL-2.0 | Android PS3 port derived from `PS3Native` with mobile frame-pacing and scaling experiments. |
| [aenu1/aps3e](https://github.com/aenu1/aps3e) | [`b5ae1af50d5e`](https://github.com/aenu1/aps3e/commit/b5ae1af50d5e) | No root `LICENSE` found | Experimental Android PS3 emulator based on RPCS3 with native Android UI and Mali Valhall Vulkan workarounds. Learn only; do not copy code until license terms are established. |
