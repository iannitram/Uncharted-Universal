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
