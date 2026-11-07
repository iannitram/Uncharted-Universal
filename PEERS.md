# Peers

Uncharted-Universal targets Linux, Windows, macOS, iOS/iPadOS and Android from
the player's own decrypted dump. This file records where other repositories
already provide platform work we can study or adapt. The machine-readable
`peer:` lines remain in [SPEC.md](SPEC.md#prior-art).

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
| [truwa/nathan-drake-collection-macos](https://github.com/truwa/nathan-drake-collection-macos) (0) | [aa0a3835428b](https://github.com/truwa/nathan-drake-collection-macos/commit/aa0a3835428b2a80269316f1daf5e5f4bfaa6bd7) | [`NOTICES.txt`](https://github.com/truwa/nathan-drake-collection-macos/blob/aa0a3835428b2a80269316f1daf5e5f4bfaa6bd7/NOTICES.txt) lists dependencies; no root LICENSE/API identifier found | Direct Mac packaging experiment uses Rosetta and a different engine. Learn packaging only; no source copy until reuse terms are established. |

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
| [evill33t/bloodborne_mac](https://github.com/evill33t/bloodborne_mac/tree/bbacb95c3338efcd627c0edaec05ff7534e555da) | [root LICENSE](https://github.com/evill33t/bloodborne_mac/blob/bbacb95c3338efcd627c0edaec05ff7534e555da/LICENSE), GPL-2.0; README identifies the combined runtime's bbhost portion as GPL-3.0-or-later | Separate macOS-only fork with direct Metal rendering and an optional native ARM64 `bbcpu` interpreter/JIT path. Its README reports experimental native gameplay coverage, while Rosetta remains the default and whole-game AOT is not enabled. Compare Mac Metal/runtime constraints only; this JIT path does not establish an iOS-compatible no-JIT route. Exclude the GPL-3.0-or-later combined runtime from this GPL-2.0-only project; no code imported. |
| [truwa/nathan-drake-collection-macos](https://github.com/truwa/nathan-drake-collection-macos/tree/aa0a3835428b2a80269316f1daf5e5f4bfaa6bd7) | No license for this repository's own launcher is declared; its [`NOTICES.txt`](https://github.com/truwa/nathan-drake-collection-macos/blob/aa0a3835428b2a80269316f1daf5e5f4bfaa6bd7/NOTICES.txt) credits the BilalReloaded PC port (GPLv2), KosmicKrisp/Mesa (MIT), and a GPLv2 SpartanPort launcher | Direct Uncharted Nathan Drake Collection macOS app for Apple Silicon; its README requires Rosetta 2 and packages the separate PC port and KosmicKrisp. Learn packaging, signed/notarized distribution and first-run flow only; it is not an ARM64 guest-code port and offers no iOS route. Do not copy its launcher until a license is established. |
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
