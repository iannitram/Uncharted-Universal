# Android dependency build boundary

`scripts/build_dependencies.sh` is an issue-scoped adaptation of the build
orchestration in `revvv991/uncharted-android` at
`847faf5a802253cf90664144765f0e5d12eb410e` (GPL-2.0-or-later). It does not
copy that project's game recipes, extra patch XML, GPU tree, app, assets, or
generated output. The lock at `components.lock.json` freezes the selected source
revisions. Source repositories retain their own license files in the external
work directory; this file records their identifiers and upstream notice paths.

| Component | Pinned source | License | Notice location |
| --- | --- | --- | --- |
| FEXCore and pinned submodules | [FEX](https://github.com/FEX-Emu/FEX/tree/f2b679f6028ce1c38875233aecfcf5d3f8ebecec) | MIT; bundled projects have individual notices | `FEX/LICENSE`, `FEX/External/*` |
| Mesa Turnip | [Mesa](https://gitlab.freedesktop.org/mesa/mesa/-/tree/mesa-26.2.5) | MIT, with per-file notices | `mesa/docs/license.rst`, source-file SPDX headers |
| libadrenotools | [bylaws/libadrenotools](https://github.com/bylaws/libadrenotools/tree/8fae8ce254dfc1344527e05301e43f37dea2df80) | BSD-2-Clause | `libadrenotools/LICENSE` |
| SDL 3 | [SDL](https://github.com/libsdl-org/SDL/tree/release-3.4.18) | Zlib | `SDL/LICENSE.txt` |
| Android NDK toolchain | [Android NDK r30](https://developer.android.com/ndk/downloads) | Android SDK/NDK license terms | Installed SDK/NDK package licenses |

The build keeps fetched sources and outputs outside the repository. The target
artifact contains authored synthetic code only. No executable, dump, firmware,
keys, game data, game-derived recipe, generated game code, or binary-derived
annotation is used or retained.

## Vendored patches (`patches/`)

`patches/mesa/` and `patches/fex/` are unmodified copies of the `mesa-*.patch`
and `fex-*.patch` files in `android/scripts/port-arm64/patches/` of
`revvv991/uncharted-android` at `82fb03069` (#247). They change Mesa and FEX
source files and are distributed under those projects' MIT license, the license
the peer states for them; `mesa-0016` is upstream Mesa's own commit 118ec7f4
(Connor Abbott), and the new files added by `mesa-0004` carry
`Copyright © 2026 uncharted-android` with `SPDX-License-Identifier: MIT`. The
rest of that repository (GPL-2.0-or-later), its `experimental/` patches,
`scripts/g2/`, `np3/` and GPU tree are not included. Source list and per-patch
notes: `patches/README.md`.

MIT License (applies to the patch contents above):

    Permission is hereby granted, free of charge, to any person obtaining a
    copy of this software and associated documentation files (the "Software"),
    to deal in the Software without restriction, including without limitation
    the rights to use, copy, modify, merge, publish, distribute, sublicense,
    and/or sell copies of the Software, and to permit persons to whom the
    Software is furnished to do so, subject to the following conditions:

    The above copyright notice and this permission notice shall be included in
    all copies or substantial portions of the Software.

    THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
    FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
    AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
    LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
    FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
    DEALINGS IN THE SOFTWARE.

## Selected peer-reference allowlist (#208, #209, #210)

The Android FEX bridge (`#209`), synthetic FEX smoke harness (`#208`), and Vulkan
driver loader (`#210`) are **selected-subset adaptations with interface rewrites**
into this `GPL-2.0-only` repository (except `android/driver_loader.{h,cpp}` and
`tests/test_android_vulkan_loader.cpp`, which retain `BSD-2-Clause` from
`bylaws/libadrenotools`). They are **not** a blanket tree import of any peer
repository.

PM import boundary clarification (2026-11-06, `#209`): the selected
`zenithblue-oss/shadps4-arm64` FEX translation units carry `MIT` SPDX markers on
the files themselves, **but their local and transitive header closure is not all
MIT** (`Common/*`, `core/libraries/kernel/threads/exception.h`, and `hle_trace.h`
include `GPL-2.0-or-later` material). `revvv991/uncharted-android` is under root
`GPL-2.0-or-later` (`android/LICENSES.md` in that repository). Those
`GPL-2.0-or-later` dependencies are compatible with this project's `GPL-2.0-only`
license when selecting GPLv2, and no claim is made that the full donor
emulator/bridge closure is MIT.

### Allowlist of reviewed donor reference files

| Issue | Donor repository & pinned revision | Reviewed source path / range | Donor file / transitive license | Local destination file(s) & license |
| --- | --- | --- | --- | --- |
| `#208` | `zenithblue-oss/shadps4-arm64` @ `be6bc2e9c60799e071dd2fafa6216e8d80ec619c` | `runtime/probes/fexcore-guest-harness.cpp#L330-L445`, `runtime/tests/verify-fexcore-guest-harness-build.mjs#L1-L86` | File `MIT` (`LICENSES/MIT.txt`); transitive host harness headers excluded | `android/synthetic/fex_smoke.{h,cpp}`, `android/synthetic/CMakeLists.txt`, `tests/test_android_fex_smoke.cpp` (`GPL-2.0-only`) |
| `#209` | `zenithblue-oss/shadps4-arm64` @ `be6bc2e9c60799e071dd2fafa6216e8d80ec619c` | `src/core/fex/fex_guest_engine.cpp#L1281-L1515` | File `MIT`; transitive `Common/*` and `core/libraries/kernel/threads/exception.h` include `GPL-2.0-or-later` | `crates/runtime/src/android_fex.rs`, `android/fex_bridge.{h,cpp}`, `tests/test_android_fex_bridge.cpp` (`GPL-2.0-only`) |
| `#209` | `zenithblue-oss/shadps4-arm64` @ `be6bc2e9c60799e071dd2fafa6216e8d80ec619c` | `src/core/guest_cpu/fex_guest_cpu.cpp#L49-L90` | File `MIT` (`LICENSES/MIT.txt`) | `crates/runtime/src/android_fex.rs`, `android/fex_bridge.{h,cpp}` (`GPL-2.0-only`) |
| `#209` | `zenithblue-oss/shadps4-arm64` @ `be6bc2e9c60799e071dd2fafa6216e8d80ec619c` | `src/core/guest_cpu/fex_hle_bridge.cpp#L66-L109` | File `MIT`; transitive `hle_trace.h` is `GPL-2.0-or-later` | `crates/runtime/src/android_fex.rs`, `android/fex_bridge.{h,cpp}` (`GPL-2.0-only`) |
| `#209` | `revvv991/uncharted-android` @ `847faf5a802253cf90664144765f0e5d12eb410e` | `src/guest_cpu_fex.cpp#L235-L365` | Root `GPL-2.0-or-later` (`android/LICENSES.md`) | `crates/runtime/src/android_fex.rs`, `android/fex_bridge.{h,cpp}` (`GPL-2.0-only`) |
| `#210` | `bylaws/libadrenotools` @ `8fae8ce254dfc1344527e05301e43f37dea2df80` | `include/adrenotools/driver.h#L13-L31`, `src/driver.cpp#L19-L93` | `BSD-2-Clause` (`LICENSE`) | `android/driver_loader.{h,cpp}`, `tests/test_android_vulkan_loader.cpp` (`BSD-2-Clause`) |

### Explicitly excluded from import

- PS4 HLE/NID tables, title patches, hardcoded guest addresses, and binary-derived annotations
- Donor emulator/game test suites and runtime executables
- `revvv991/uncharted-android` `experimental/` patches, `scripts/g2/`, `np3/`, game recipes, and GPU tree
- `zenithblue-oss/shadps4-arm64` emulator core outside the bounded bridge/harness seams above

### Retained upstream notices

- `zenithblue-oss/shadps4-arm64` (`be6bc2e9c60799e071dd2fafa6216e8d80ec619c`, `LICENSES/MIT.txt`): MIT notice applies to the selected MIT-marked donor files only; transitive `GPL-2.0-or-later` headers (`hle_trace.h`, `Common/*`, `core/libraries/kernel/threads/exception.h`) remain governed by GNU GPL v2 (or later, selected as GPL-2.0-only in this repository).
- `revvv991/uncharted-android` (`847faf5a802253cf90664144765f0e5d12eb410e`, `android/LICENSES.md`): GNU GPL v2 or later (selected as GPL-2.0-only in this repository).
- `bylaws/libadrenotools` (`8fae8ce254dfc1344527e05301e43f37dea2df80`, `LICENSE`): BSD-2-Clause.
- FEXCore (`FEX-Emu/FEX` @ `f2b679f6028ce1c38875233aecfcf5d3f8ebecec`, `LICENSE`, `External/*`): MIT with individual notices for bundled submodules.
