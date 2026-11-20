# Building and running on Linux

How the port (`bb-probe` and `gpu/libbbgpu.so`) was built from source and run on a SteamOS handheld on 2026-10-19, with every build and runtime dependency. For players the AppImage in the README is easier; this page is for building it yourself.

**Labels.** *Floor* is the lowest version the project itself requires (from `gpu/CMakeLists.txt`, `build.sh` or a library's shared-object version), and is **read** from those files. *Tested* is the version that was actually used, **measured** (`pacman -Q`, `git describe`, the program's own log). Nobody has searched for the true lowest working version of each library: where no floor is written down, use the tested version or newer.

**Tested setup (measured).** Port: branch `linux` at 1669a55. Build: an Arch Linux container (distrobox, Arch archive snapshot of 2025-07-28 with glibc 2.41, the same glibc as the host). Run: on the host, SteamOS 3.8.28 (build 20260922.1), kernel 6.18.50-valve2, AMD Radeon 8060S (RADV STRIX_HALO). Uncharted 3 ran at 60 FPS for 30-minute sessions in normal and stereo modes.

## 1. Build toolchain

| Tool | Floor (read) | Tested (measured) | Notes |
|---|---|---|---|
| clang / clang++ | C++23 support (`CMAKE_CXX_STANDARD 23`, required) | 20.1.8 | `build.sh` passes `CMAKE_C_COMPILER=clang`, `CMAKE_CXX_COMPILER=clang++` for the GPU library. |
| C compiler (`cc`) | C11 | gcc 15.1.1 | `build.sh` compiles `src/probe.c` and the runtime with `cc` (`-std=c11 -Werror`). |
| CMake | 3.24 (`cmake_minimum_required`) | 4.0.3 | |
| Ninja | not stated | 1.12.1 | |
| pkg-config | not stated | pkgconf 2.5.1 | |
| Python 3 | not stated | 3.13.5 | tools and tests |
| glslang (`glslang` or `glslangValidator`) | not stated | 1.4.321.0 | `find_program(... REQUIRED)` |
| glibc (build machine) | = the oldest glibc you want to run on | 2.41 | Build on a system whose glibc is not newer than the players'. The README's runtime floor is glibc 2.41. |

## 2. Libraries

**System** means installed with the distribution's packages; **bundled** means the copy in this repository (a git submodule under `gpu/third_party/` or `third_party/`: run `git submodule update --init --recursive` first).

| Library | Floor (read) | Tested (measured) | System or bundled |
|---|---|---|---|
| SDL3 | not stated (pkg-config `sdl3`) | 3.2.18 | system |
| FFmpeg (libavformat, libavcodec, libavutil, libswscale, libswresample) | the build links `libavcodec.so.61`, `libavutil.so.59`, `libswresample.so.5`, `libswscale.so.8`: FFmpeg 7.x | 7.1.1 | system |
| Vulkan loader | Vulkan 1.3 device (README) | 1.4.321.0 | system |
| Vulkan headers | 1.4.344 (`BB_VULKAN_HEADERS_MIN` in `gpu/CMakeLists.txt`, checked at configure time); newer than some distributions ship (`build.sh` comment) | v1.4.344; 1.4.365 failed on a GCC `vk::Extent2D` assignment before the explicit-construction source fix, and a 1.4.365 build after that fix is not yet reproduced | **bundled** (`third_party/Vulkan-Headers`) |
| VulkanMemoryAllocator | not stated | header 3.3.0 (Z13); 3.2.1 also builds (Legion Go) | system package, or only the header `vk_mem_alloc.h` (see 4) |
| Boost | not stated | 1.88.0 | system |
| fmt | links `libfmt.so.11`: fmt 11 | 11.2.0 | system |
| robin-map (tsl) | not stated | 1.3.0 | system |
| Zydis (and Zycore) | links `libZydis.so.4.1`, `libZycore.so.1.5`: Zydis 4.1 | 4.1.0 | system |
| nlohmann-json | not stated | 3.12.0 | system |
| Protobuf | links `libprotobuf.so.31.1.0` (and Abseil 2505) | 31.1 | system |
| OpenSSL | links `libssl.so.3`: OpenSSL 3 | 3.5.1 | system |
| zlib | not stated | 1.3.1 | system |
| miniupnpc | links `libminiupnpc.so.21` | 2.3.3 | system |
| xxHash | not stated | 0.8.3 | system |
| libX11 | not stated | 1.8.12 | system |
| magic_enum | not stated | v0.9.7 | **bundled** (used when no system package is found) |
| miniz | needs `MZ_ZIP_FLAG_READ_ALLOW_WRITING` | 3.1.2-12 (2948eeb) | **bundled**: see 4, do not install system miniz 3.0.2 |
| xbyak | not stated | v7.43 | **bundled** |
| Dear ImGui | not stated | v1.92.9b | **bundled** |
| FidelityFX (`fsr-vulkan`), sirit, half, httplib, wepoll, LibAtrac9 | not stated | the submodule commits | **bundled** |
| SPIRV-Cross | not used by the CMake files (listed in `shell.nix` only) | not installed | |

On Arch Linux (tested) the system packages are: `clang cmake ninja pkgconf python sdl3 ffmpeg glslang spirv-tools vulkan-headers vulkan-icd-loader boost fmt robin-map zydis nlohmann-json protobuf openssl zlib miniupnpc xxhash libx11`.

## 3. Steps

```bash
git clone https://github.com/iannitram/uncharted_pc.git && cd uncharted_pc
git checkout linux
git submodule update --init --recursive
bash ./build.sh          # writes out/bb-probe and out/gpu/libbbgpu.so
```

`build.sh` may have lost its executable bit after a copy or a sync: run it through `bash` as above. Set `BB_JOBS=<n>` to limit the GPU library's parallel compile.

Run with your own decrypted dump of the collection (CUSA02320; the folder holding `eboot.bin`, `big2-ps4_Shipping.elf`, `big3-ps4_Shipping.elf`, `sce_sys` and `u1data` / `u2data` / `u3data`). No game files are included or needed for the build.

```bash
cargo run -p uncharted_cli -- play u3 --dump <your dump folder>
```

`BB_EXE` is `u1`, `u2` or `u3`. `BB_FRAME_STATS=1` prints frame statistics every 5 seconds.

## 4. Known problems

- **System miniz 3.0.2 breaks the build** (measured): `cache_storage.cpp:106: 'MZ_ZIP_FLAG_READ_ALLOW_WRITING' was not declared in this scope`. CMake prefers a system miniz when it finds one, so remove the package (`pacman -R miniz` on Arch); the bundled copy is then used.
- **No VulkanMemoryAllocator package** in some distribution snapshots (measured, the Arch archive snapshot of 2025-07-28): CMake stops at `Could not find VMA_INCLUDE_DIR using the following files: vk_mem_alloc.h`. Install the package, or put the single header from VulkanMemoryAllocator (tested: 3.3.0 and 3.2.1) in any folder and point CMake at it: `CMAKE_INCLUDE_PATH=<that folder> bash ./build.sh`.
- **miniupnpc** is required (`find_path(MINIUPNPC_INCLUDE ...)`) even though nothing on the build line names it; install it before the first build.

## 5. Running on a machine other than the build machine

`bb-probe` and `libbbgpu.so` need these shared libraries at run time (measured with `readelf -d` and `ldd`): SDL3 (`libSDL3.so.0`), FFmpeg 7 (`libavcodec.so.61`, `libavformat.so.61`, `libavutil.so.59`, `libswresample.so.5`, `libswscale.so.8`), Vulkan loader (`libvulkan.so.1`), OpenSSL 3, zlib, libX11, xxHash, Protobuf 31.1 with Abseil 2505, fmt 11, Zydis 4.1 with Zycore 1.5, and miniupnpc (`.so.21`).

SteamOS 3.8.28 (measured) has all of them **except four**: `libfmt.so.11`, `libZydis.so.4.1`, `libZycore.so.1.5` and `libminiupnpc.so.21`. Copy those four from the build system into one folder and add it to `LD_LIBRARY_PATH` when running. (The AppImage carries them itself.)

| Host runtime | Floor (read) | Tested (measured) |
|---|---|---|
| glibc | 2.41 (README) | 2.41 |
| Vulkan driver | Vulkan 1.3, 6 GB video memory or more (README) | RADV, Mesa 26.2.0-devel (git-035ae2f854), Vulkan 1.4.350, as reported in the port's log |
| Kernel | not stated | 6.18.50-valve2 (SteamOS) |
| Memory | about 8 GB free (README) | lowest free during a 30-minute run: 43 GB of 62 GB |

## 6. Fixed: host memory grew during play

Measured on 2026-10-19: before the fix the process heap grew by about 37 MiB per minute for as long as the game ran (Uncharted 3, normal and stereo). The cause was in the bundled shadPS4 video core: finished nested command-buffer tasks (IndirectBuffer, IndirectBufferConst) were never destroyed. shadPS4 fixed it upstream in 12c5978f (#5258, "liverpool: free the frames of nested indirect buffer tasks"). The same three lines are on `linux` since uncharted_pc#3 (305c3fa); with them the heap stays flat (107-110 MiB over 15 minutes, measured on a Z13).
