# Issue 403: tile-mode guard

- Base: `origin/main` at `69672033bf92d102573862a66ac800082ca59a77`.
- Source: [Mevlock/uncharted_pc-dev c1f7dedd10014227e23b7875160b2b0f20992132](https://github.com/Mevlock/uncharted_pc-dev/commit/c1f7dedd10014227e23b7875160b2b0f20992132), `gpu/shadps4/video_core/renderer_vulkan/vk_rasterizer.cpp`: `IsKnownTileMode`, `TexturesBindableOnHelper`, and `BindTextures`. The source file retains its GPL-2.0-or-later SPDX notice.
- Existing `TileMode` enum (`gpu/shadps4/video_core/amdgpu/tiling.h`) defines 0–26 and 31; both binding paths already reject invalid mappings and formats. Import only the predicate and checks, leaving unrelated commit changes out.
- Acceptance: reject 27–30 before descriptor cache/texture processing in both paths, keep 0–26 and 31 accepted by this check; Windows CI builds. U1/U2/U3 checkpoint loads using garbage T# descriptors require device validation.
- Verification on Linux/SteamOS Strix Halo tablet (AMD Ryzen AI MAX+ 395 / Radeon 8060S, x86-64): `bash build.sh` stopped at dependency detection (pkg-config with Vulkan and SDL3, CMake and Ninja required; pkg-config and CMake absent). A standalone C++20 harness extracted the actual enum and predicate and passed all 32 encoded values; a source check found the guard in both paths. Its first compilation failed because system `features.h` is missing; rerun without standard headers passed. `git diff --check` passed. This is not a full translation-unit build. Windows CI and U1/U2/U3 checkpoint loads were not run; no game files used.

## Independent review

The coordinator's review agent compared the exact peer diff and confirmed the
predicate and both call-site checks match. A fresh C++20 harness compiled the
actual extracted enum and production predicate with `-Wall -Wextra -Werror` and
passed all 32 encodings plus 32 and the maximum unsigned value. Source ordering
checks confirmed rejection precedes `CachedImageDescEntry` in both binding paths
and preserves each existing null-descriptor continuation. This remains focused
predicate/source evidence, not a translation-unit, Vulkan or gameplay test;
Linux and Windows CI must pass before merge. Review also corrected the machine
class above and wrapped the imported comment without changing its meaning.
