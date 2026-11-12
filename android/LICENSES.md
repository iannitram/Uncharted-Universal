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
