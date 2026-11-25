# Metal renderer inventory (#408, #441)

Status: **research only; nothing here is vendored.** Source revisions read (read only, a sparse
clone kept outside this repo): bloodborne-runtime `d97fc678a912` (newest), compared with
`42736cf7ca77` (the `PEERS.md` pin). Owner ruling 2026-11-24: the Apple renderer is native Metal
per ADR 0006, not MoltenVK. No game data was read; this file lists files and licences only.

## What `renderer_metal` is

It is 53 files, 20,409 lines at `d97fc678`, the same file count at `42736cf7`. It is **not a
standalone backend.** `mtl_vk_layer.h` says the shadPS4 Vulkan renderer records `vk::CommandBuffer`
calls and `Install()` replaces them in vulkan.hpp's dynamic dispatcher. In native boot
(`BB_METAL_NATIVE_BOOT`) the layer also answers every Vulkan entry point itself, so no loader or
MoltenVK is opened (`mtl_vk_c_bridge.cpp`; the only path on iOS). The renderer above it is the
peer's refactored `renderer_vulkan` plus a `renderer_backend/` adapter, and the layer includes
headers that do not exist in our tree (`vk_stream_encoder.h`, `vk_resource_backend.h`,
`vk_pipeline_key.h`, `vk_command_recorder.h`, `vk_breadcrumbs.h`, `vk_metal_shadow.h`,
`renderer_backend/device.h`). Adopting the Metal layer therefore means adopting that refactor of
the renderer, not dropping in a directory. Of the 53 files, 25 are `.h`, 3 are `.inc` tables and the rest
`.mm` or `.cpp`; shaders go SPIR-V to MSL through SPIRV-Cross (an external library, below).

## Method

A script walked `#include` edges (quoted and angle includes, resolved against the include roots
of `gpu/CMakeLists.txt`) from every file in `gpu/shadps4/video_core/renderer_metal/`, adding the
`.cpp`/`.mm` sibling of each header reached. System, SDK and external-library headers are not
files of the tree and are listed separately. The closure is an upper bound: it follows includes,
not link-time use. It has **457 files, 140,586 lines.** Columns: *Depth* is include distance from
the Metal directory (0 = the Metal files). *SPDX* is the first `SPDX-License-Identifier` in the
first 30 lines ("none" = no per-file line; the root `LICENSE` is GPL-2.0). *Ours* compares with
our `gpu/` at the same path: `same`, `differs N` (changed lines, both sides) or `absent`.
*bbport* counts `bbport:` markers; `(c)` marks a file that changed between `42736cf7` and
`d97fc678`. *Disposition*: `vendor` (the Metal directory), `already ours` (identical file),
`reconcile` (we have it and it differs), `new` (we lack it), `defer: upscaler`,
`exclude: title-specific` and `exclude: proprietary SDK`.

Totals by disposition: vendor 53 files (20,409 lines); already ours 127 (17,801); reconcile 159
(72,985); new 100 (20,295); exclude 11; defer 7. The 159 `reconcile` files are the real cost: our
`renderer_vulkan` differs from the peer's in 61 of 110 files and lacks 55 more, which matches the
mapping report's 109-file, +13k/-20k line divergence.

## Licences and files to flag

Every `renderer_metal` file is `GPL-2.0-or-later` except three generated tables with no SPDX line
(`mtl_vk_caps_fields.inc`, `mtl_vk_caps_profile.inc`, `mtl_vk_inventory.inc`); their root licence
is GPL-2.0, compatible with our GPL-2.0-only tree, but the "or later" grant is not stated, so
vendor them with a header added by us or regenerate them from our own scripts. The 33 Metal files
that say "From Wooloomooloo2/bloodborne_mac_port (0b50589)" and the 6 at `78b59f5` (GPL-2.0-or-later
per their headers) carry that provenance line, which must be kept.

Flagged, not plain GPL-2.0-or-later or MIT (none is incompatible except where noted):

- **No SPDX line (root GPL-2.0 applies), 32 files:** the three Metal `.inc` tables above;
  `bbgpu.h`; `shim/` glue (`bbport_ce_stats.h`, `bbport_copy.h`, `bbport_gnm_table.inc`,
  `bbport_toggles.h`, `common/*`, `core/*`, `imgui/*`, `tracy/*`, `sdl_window.h`); `vk_pipeline_ahead.*`
  and `vk_shader_scan.*` (title-specific, excluded); `fsr4_metal/fsr4_input.h`, `fsr4_log.h`;
  `host_shaders/fsr/ffx_a.h`, `ffx_fsr1.h` (AMD FidelityFX, MIT-style notice in the file body, defer).
- **MIT:** `dlss_bridge/*` (links NVIDIA's proprietary SDK: exclude), `fsr4_metal/fsr411_*.h` (defer with FSR4).
- **Zlib:** `renderer_vulkan/vk_barrier_tracker.{h,cpp}` (DXVK). Permissive, compatible; we already carry it.
- **CC0-1.0:** `common/alignment.h`. **BSD-3-Clause AND GPL-2.0-or-later:** `common/bit_field.h`. Both already ours, identical.
- **Not in the closure and excluded by rule:** `gpu/bbnet20` (GPL-3.0) and everything under it. The
  closure reaches no `bbnet20` file. `gpu/bbnet` (the older, non-bbnet20 network code) is not reached either.
- **External libraries the Metal files need, not in the tree:** SPIRV-Cross
  (`spirv_cross/spirv_msl.hpp`; its header says `Apache-2.0 OR MIT`, so we take it under MIT; verify
  per release before pinning), Vulkan headers (Apache-2.0 or MIT per file, already used), Boost,
  fmt, xxhash, SDL3, imgui, magic_enum, Zydis (MIT). **MoltenVK is Apache-2.0 only and is not
  required by native boot; do not link it** (the FSF treats Apache-2.0 as incompatible with GPL-2.0-only).
  Apple SDK headers (Metal, QuartzCore, Foundation, MetalFX) are system frameworks.
- **Title-specific, exclude:** `games/bloodborne_menu_pointer.*`, `games/game_profile.*`, `vk_pipeline_ahead.*`, `vk_shader_scan.*`, `shim/bbport_gnm_table.inc`.
  Anything named for Bloodborne stays out of our tree and its content out of this doc.
- **`42736cf7` to `d97fc678`:** 17 files under `gpu/` changed (846 lines added, 27 removed), 15 of them in the closure (`(c)` in the table): `mtl_frame.{h,mm}`
  (+294), `mtl_resources.{h,mm}` (+50), `mtl_vk_layer.{h,mm}` (+8), `vk_runtime.{h,cpp}` (+117), `vk_backend_device.cpp`,
  `buffer_cache.{h,cpp}` (+304: memory accounting and a low-memory cap), `texture_cache.cpp` (+13: a texture-cache cap),
  `videoout/driver.cpp`, and shim files (`window.cpp`, `bbport_overlay.cpp`, `bbport_mac_menu.mm`, `bbport_toggles.h`). All are additive (pool imported once, pool regions, buffer copies as
  compute, memory counting). Taking `d97fc678` rather than the pin costs nothing extra and includes the memory accounting
  we want for #408's iOS memory gate. File SPDX lines are unchanged between the two.

## Proposed vendoring order into `apple/vendor/renderer_metal/`

Each step is its own PR with the peer revision and per-file SPDX lines recorded, and builds on
macOS before the next. None lands before the owner confirms the plan below.

1. **Header-only, no behaviour:** `vk_to_mtl.h`, `mtl_object.{h,cpp}`, `mtl_raster_util.h`,
   `mtl_depth_stencil.*`, `mtl_vk_caps*` and the three `.inc` tables (SPDX added). Compiles with
   Metal and Foundation only. Proves the build glue (Objective-C++ in CMake, arm64).
2. **SPIR-V helpers that touch no renderer types:** `mtl_spirv_{geometry,inputs,lanes,pervertex,rectlist}.*`,
   `mtl_shader.*`, `mtl_pipeline_build.*`, `mtl_pso_cache.*`; adds SPIRV-Cross as a pinned external dependency.
   A standalone test compiles authored SPIR-V to MSL and builds a PSO.
3. **Frame and resources:** `mtl_frame.*`, `mtl_resources.*`, `mtl_buffer_barriers.*`, `mtl_order.*`,
   `mtl_shadow.*`, `mtl_geometry.*`, `mtl_tessellation.*`, `mtl_present.*`, `mtl_device*.{h,mm}`.
   Needs the `renderer_backend/` adapter (15 files, 2,031 lines, mostly `new`) and `vk_metal_shadow.h`.
4. **The layer and C bridge:** `mtl_vk_layer.*`, `mtl_recorder.*`, `mtl_vk_c_bridge.cpp`.
5. **Renderer reconciliation (largest):** the 159 `reconcile` files in `renderer_vulkan`, `buffer_cache`,
   `texture_cache`, `amdgpu` and `shim`, rebased onto the peer's render adapter, one subsystem per PR,
   the Linux and Windows builds kept green throughout (the peer's `renderer_vulkan` is shared, so this
   stays one renderer).
6. **Deferred:** upscalers (FSR, FSR4, DLSS), Bloodborne pointer and game profile code.

Gate before step 3: our CI cannot build `gpu/` on macOS at all today (`build.rs` panics for Apple
targets; Boost and other packages absent; Xbyak-generated `flatten_extended_userdata_pass` is x86-64
only, #396), so steps 3 to 5 cannot be exercised on a game before that is settled. Steps 1 and 2
can be built and tested without it.

## C ABI the Rust runtime calls (feasibility gate 2)

The runtime already calls the renderer through `gpu/bbgpu.h` and `GpuHooks`
(`crates/runtime/src/loader/entry.rs`). Keep that header as the one ABI. Metal needs these
additions, in a new `apple/include/uu_gpu_apple.h` that wraps `bbgpu.h` rather than a second API:

- `int uu_gpu_apple_attach(void *ca_metal_layer, uint32_t pixel_w, uint32_t pixel_h, double scale)`:
  the app hands over a `CAMetalLayer*` (Mac) or a `UIView*`'s layer (iOS); the library creates no
  window. Adds `WindowSystemType::Metal` to `shim/sdl_window.h` and `shim/window.cpp`.
- `void uu_gpu_apple_resize(uint32_t w, uint32_t h, double scale)` and `uu_gpu_apple_detach(void)` (device loss and suspend/resume).
- `size_t uu_gpu_apple_memory(uint64_t *gpu_bytes, uint64_t *buffer_bytes)`: the peer's memory accounting, for the iOS memory gate.
- `int uu_gpu_apple_supports_bc(void)`: `MTLDevice.supportsBCTextureCompression`, used by the prompt path below.
- Unchanged: `bbgpu_init`, `bbgpu_resolve`, `bbgpu_prompts_controller`, `bbgpu_handle_fault`, and the rest of `bbgpu.h`.
  Guest memory handed to the GPU must be a file-backed mapping (the peer measured wired memory being killed by jetsam on iOS); the
  `probe.m` no-copy test is the first evidence that Metal accepts that on macOS.

## Where #441's button-prompt redraw hooks in

Today, in our shadPS4 Vulkan texture cache (`gpu/shadps4/video_core/texture_cache/texture_cache.cpp`):

- `ReplaceButtonPrompts` (about line 961 to 1024) runs after every image upload (call at about line 1130, right after
  `runtime.UploadImage`). It maps the image format (RGBA8 or BC1), hashes mip 0 of the guest data (xxhash), asks
  `ButtonPrompts::Find` for patches, records the guest range, then copies each `Patch {mip, x, y, width, height, data}` into a
  staging buffer and calls `runtime.UploadImage(&image, staging.buffer, copies)` with `vk::BufferImageCopy` regions.
- `TextureCache::UpdateButtonPrompts` (about line 1027) invalidates the recorded ranges when `ButtonPrompts::Switches()` changes
  (the player changes input device); it is called once per submission from `vk_rasterizer.cpp` (about line 2833).
- `button_prompts.{h,cpp}` itself (1,319 lines) does CPU drawing only (miniz, SDL key names, stb_truetype) and is backend-independent.

On the Metal path the hook point is **the same `runtime.UploadImage` call**, because in the peer's layer
`vk::CommandBuffer::copyBufferToImage` is routed to `MetalFrame::CopyBufferToImage` (`mtl_recorder.mm` about line 466,
`mtl_frame.mm` about line 2115), which loops the regions into `copyFromBuffer:...toTexture:destinationOrigin:` blits, exactly what
`probe.m` exercises for RGBA8 and BC1 sub-rects. So with the dispatcher-layer route the existing texture-cache code needs
no change, and the probe shows the underlying Metal primitive works on Apple silicon.

Two cases need care. First, the peer's own tree moved prompts out of the texture cache into `renderer_vulkan/vk_prompt_images.cpp`
(a render-adapter path that draws icons as separate images at sampling time, flagged on `Image::prompt_icon` and
`prompt_atlas`). If step 5 adopts that refactor, #441 must port **our** hash-and-patch design onto the adapter's
`Transfers::UploadImage` (the call `PromptImages` already uses) instead of taking theirs, since theirs is built for
another game's icons. Second, on iOS some devices do not support BC texture compression: the BC1 patch branch in
`ReplaceButtonPrompts` must then decode to RGBA8 (or skip) when `uu_gpu_apple_supports_bc()` is 0. That is new code, to land
with step 3 or 4. The `probe.m` BC1 check already skips with a clear message in that case.

Owner-device checks for #441 (the controller matrix, a real game screen) are not covered by this work.

## Closure listing (457 files)

| Path under `gpu/` | Lines | Depth | SPDX | Ours | bbport | Disposition |
| --- | ---: | ---: | --- | --- | ---: | --- |
| `shadps4/video_core/renderer_metal/mtl_buffer_barriers.cpp` | 280 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_buffer_barriers.h` | 314 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_compiled_shader.mm` | 33 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_depth_stencil.h` | 40 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_depth_stencil.mm` | 86 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_device.h` | 210 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_device.mm` | 247 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_device_impl.h` | 28 | 0 | GPL-2.0-or-later | absent | 0 | vendor |
| `shadps4/video_core/renderer_metal/mtl_frame.h` | 264 | 0 | GPL-2.0-or-later | absent | 1 (c) | vendor |
| `shadps4/video_core/renderer_metal/mtl_frame.mm` | 2345 | 0 | GPL-2.0-or-later | absent | 0 (c) | vendor |
| `shadps4/video_core/renderer_metal/mtl_geometry.h` | 199 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_geometry.mm` | 498 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_object.cpp` | 24 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_object.h` | 60 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_order.cpp` | 358 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_order.h` | 285 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_pipeline_build.h` | 135 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_pipeline_build.mm` | 423 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_present.h` | 51 | 0 | GPL-2.0-or-later | absent | 0 | vendor |
| `shadps4/video_core/renderer_metal/mtl_present.mm` | 205 | 0 | GPL-2.0-or-later | absent | 0 | vendor |
| `shadps4/video_core/renderer_metal/mtl_pso_cache.h` | 296 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_pso_cache.mm` | 1251 | 0 | GPL-2.0-or-later | absent | 2 | vendor |
| `shadps4/video_core/renderer_metal/mtl_raster_util.h` | 70 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_recorder.h` | 24 | 0 | GPL-2.0-or-later | absent | 0 | vendor |
| `shadps4/video_core/renderer_metal/mtl_recorder.mm` | 548 | 0 | GPL-2.0-or-later | absent | 0 | vendor |
| `shadps4/video_core/renderer_metal/mtl_resources.h` | 85 | 0 | GPL-2.0-or-later | absent | 0 (c) | vendor |
| `shadps4/video_core/renderer_metal/mtl_resources.mm` | 224 | 0 | GPL-2.0-or-later | absent | 0 (c) | vendor |
| `shadps4/video_core/renderer_metal/mtl_shader.h` | 258 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_shader.mm` | 1282 | 0 | GPL-2.0-or-later | absent | 4 | vendor |
| `shadps4/video_core/renderer_metal/mtl_shadow.h` | 58 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_shadow.mm` | 599 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_spirv_geometry.cpp` | 1076 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_spirv_geometry.h` | 103 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_spirv_inputs.cpp` | 363 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_spirv_inputs.h` | 39 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_spirv_lanes.cpp` | 166 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_spirv_lanes.h` | 36 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_spirv_pervertex.cpp` | 718 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_spirv_pervertex.h` | 39 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_spirv_rectlist.cpp` | 700 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_spirv_rectlist.h` | 45 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_tessellation.h` | 115 | 0 | GPL-2.0-or-later | absent | 2 | vendor |
| `shadps4/video_core/renderer_metal/mtl_tessellation.mm` | 139 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/mtl_vk_c_bridge.cpp` | 576 | 0 | GPL-2.0-or-later | absent | 0 | vendor |
| `shadps4/video_core/renderer_metal/mtl_vk_caps.h` | 24 | 0 | GPL-2.0-or-later | absent | 0 | vendor |
| `shadps4/video_core/renderer_metal/mtl_vk_caps.mm` | 854 | 0 | GPL-2.0-or-later | absent | 0 | vendor |
| `shadps4/video_core/renderer_metal/mtl_vk_caps_fields.inc` | 454 | 0 | none (root GPL-2.0) | absent | 0 | vendor |
| `shadps4/video_core/renderer_metal/mtl_vk_caps_profile.inc` | 568 | 0 | none (root GPL-2.0) | absent | 0 | vendor |
| `shadps4/video_core/renderer_metal/mtl_vk_inventory.inc` | 1088 | 0 | none (root GPL-2.0) | absent | 0 | vendor |
| `shadps4/video_core/renderer_metal/mtl_vk_layer.h` | 92 | 0 | GPL-2.0-or-later | absent | 1 (c) | vendor |
| `shadps4/video_core/renderer_metal/mtl_vk_layer.mm` | 1571 | 0 | GPL-2.0-or-later | absent | 0 (c) | vendor |
| `shadps4/video_core/renderer_metal/vk_to_mtl.h` | 165 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `shadps4/video_core/renderer_metal/vk_to_mtl.mm` | 698 | 0 | GPL-2.0-or-later | absent | 1 | vendor |
| `bbgpu.h` | 84 | 12 | none (root GPL-2.0) | differs 92 | 0 | reconcile |
| `cp/cp_decoder.cpp` | 229 | 9 | GPL-2.0-or-later | absent | 0 | new |
| `cp/cp_decoder.h` | 252 | 8 | GPL-2.0-or-later | absent | 0 | new |
| `cp/cp_pm4.h` | 136 | 9 | GPL-2.0-or-later | absent | 0 | new |
| `dlss_bridge/bbport_dlss_bridge.cpp` | 265 | 6 | MIT | differs 237 | 0 | exclude: proprietary SDK |
| `dlss_bridge/bbport_dlss_bridge.h` | 78 | 5 | MIT | differs 48 | 0 | exclude: proprietary SDK |
| `fsr4_metal/fsr411_model.h` | 215 | 6 | MIT | absent | 0 | defer: upscaler |
| `fsr4_metal/fsr411_runtime.h` | 1303 | 5 | MIT | absent | 13 | defer: upscaler |
| `fsr4_metal/fsr411_scratch_layout.h` | 38 | 6 | MIT | absent | 0 | defer: upscaler |
| `fsr4_metal/fsr4_input.h` | 262 | 6 | none (root GPL-2.0) | absent | 0 | defer: upscaler |
| `fsr4_metal/fsr4_log.h` | 28 | 6 | none (root GPL-2.0) | absent | 0 | defer: upscaler |
| `games/bloodborne_menu_pointer.cpp` | 752 | 13 | GPL-2.0-or-later | absent | 0 | exclude: title-specific |
| `games/bloodborne_menu_pointer.h` | 14 | 12 | GPL-2.0-or-later | absent | 0 | exclude: title-specific |
| `games/game_profile.cpp` | 53 | 7 | GPL-2.0-or-later | absent | 0 | exclude: title-specific |
| `games/game_profile.h` | 57 | 6 | GPL-2.0-or-later | absent | 1 | exclude: title-specific |
| `layer/bblayer_gpu_memory.cpp` | 234 | 6 | GPL-2.0-or-later | absent | 0 | new |
| `layer/bblayer_gpu_memory.h` | 179 | 5 | GPL-2.0-or-later | absent | 0 | new |
| `layer/bblayer_write_traps.h` | 39 | 8 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/common/adaptive_mutex.h` | 37 | 10 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/alignment.h` | 59 | 3 | CC0-1.0 | same | 0 | already ours |
| `shadps4/common/arch.h` | 10 | 3 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/assert.cpp` | 31 | 2 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/assert.h` | 85 | 1 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/bit_array.h` | 455 | 11 | GPL-2.0-or-later | differs 23 | 1 | reconcile |
| `shadps4/common/bit_field.h` | 167 | 2 | BSD-3-Clause AND GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/concepts.h` | 31 | 7 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/div_ceil.h` | 25 | 9 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/elf_info.h` | 162 | 1 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/enum.h` | 161 | 5 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/error.cpp` | 57 | 5 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/error.h` | 21 | 4 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/func_traits.h` | 34 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/hash.h` | 14 | 6 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/incremental_id.h` | 22 | 5 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/interval_set.h` | 229 | 2 | GPL-2.0-or-later | differs 51 | 1 | reconcile |
| `shadps4/common/io_file.cpp` | 423 | 7 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/io_file.h` | 252 | 6 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/logging/classes.h` | 120 | 2 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/logging/formatter.h` | 42 | 10 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/lru_cache.h` | 135 | 8 | GPL-2.0-or-later | differs 2 | 0 | reconcile |
| `shadps4/common/ntapi.cpp` | 30 | 5 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/ntapi.h` | 554 | 4 | GPL-2.0-or-later | differs 2 | 0 | reconcile |
| `shadps4/common/number_utils.cpp` | 155 | 4 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/number_utils.h` | 28 | 3 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/object_pool.h` | 109 | 6 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/path_util.cpp` | 250 | 2 | GPL-2.0-or-later | differs 6 | 2 | reconcile |
| `shadps4/common/path_util.h` | 123 | 1 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/polyfill_thread.h` | 377 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/range_lock.h` | 101 | 11 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/rdtsc.cpp` | 60 | 11 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/rdtsc.h` | 54 | 10 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/recursive_lock.cpp` | 37 | 10 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/recursive_lock.h` | 67 | 9 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/scope_exit.h` | 79 | 3 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/serdes.h` | 159 | 3 | GPL-2.0-or-later | differs 80 | 1 | reconcile |
| `shadps4/common/shared_first_mutex.h` | 85 | 9 | GPL-2.0-or-later | differs 152 | 0 | reconcile |
| `shadps4/common/signal_context.cpp` | 50 | 9 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/signal_context.h` | 18 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/singleton.h` | 28 | 2 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/slot_vector.h` | 299 | 7 | GPL-2.0-or-later | differs 25 | 0 | reconcile |
| `shadps4/common/spin_lock.cpp` | 53 | 11 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/spin_lock.h` | 33 | 10 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/string_literal.h` | 15 | 3 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/string_util.cpp` | 86 | 5 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/string_util.h` | 26 | 4 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/thread.cpp` | 263 | 3 | GPL-2.0-or-later | differs 45 | 3 | reconcile |
| `shadps4/common/thread.h` | 52 | 2 | GPL-2.0-or-later | differs 7 | 0 | reconcile |
| `shadps4/common/types.h` | 39 | 1 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/uint128.h` | 115 | 9 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/common/unique_function.h` | 61 | 2 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/core/libraries/error_codes.h` | 8 | 11 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/core/libraries/gnmdriver/gnm_error.h` | 87 | 10 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/core/libraries/gnmdriver/gnmdriver.cpp` | 3477 | 9 | GPL-2.0-or-later | differs 246 | 0 | reconcile |
| `shadps4/core/libraries/gnmdriver/gnmdriver.h` | 312 | 8 | GPL-2.0-or-later | differs 9 | 0 | reconcile |
| `shadps4/core/libraries/gnmdriver/gnmdriver_init.h` | 542 | 10 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/core/libraries/kernel/equeue.cpp` | 713 | 10 | GPL-2.0-or-later | differs 94 | 2 | reconcile |
| `shadps4/core/libraries/kernel/equeue.h` | 200 | 9 | GPL-2.0-or-later | differs 3 | 0 | reconcile |
| `shadps4/core/libraries/kernel/orbis_error.h` | 110 | 10 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/core/libraries/kernel/posix_error.h` | 128 | 11 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/core/libraries/kernel/process.h` | 35 | 6 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/core/libraries/kernel/time.h` | 93 | 9 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/core/libraries/system/userservice.h` | 487 | 10 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/core/libraries/videoout/buffer.h` | 74 | 7 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/core/libraries/videoout/driver.cpp` | 1013 | 9 | GPL-2.0-or-later | differs 535 | 8 (c) | reconcile |
| `shadps4/core/libraries/videoout/driver.h` | 137 | 8 | GPL-2.0-or-later | differs 19 | 2 | reconcile |
| `shadps4/core/libraries/videoout/video_out.cpp` | 542 | 10 | GPL-2.0-or-later | differs 142 | 1 | reconcile |
| `shadps4/core/libraries/videoout/video_out.h` | 146 | 9 | GPL-2.0-or-later | differs 7 | 0 | reconcile |
| `shadps4/core/libraries/videoout/videoout_error.h` | 35 | 10 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/core/platform.h` | 96 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/backend/bindings.h` | 25 | 4 | GPL-2.0-or-later | differs 7 | 1 | reconcile |
| `shadps4/shader_recompiler/backend/spirv/emit_spirv.cpp` | 763 | 7 | GPL-2.0-or-later | differs 27 | 1 | reconcile |
| `shadps4/shader_recompiler/backend/spirv/emit_spirv.h` | 16 | 6 | GPL-2.0-or-later | differs 3 | 0 | reconcile |
| `shadps4/shader_recompiler/backend/spirv/emit_spirv_discard_frag.cpp` | 123 | 4 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/backend/spirv/emit_spirv_discard_frag.h` | 14 | 3 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/backend/spirv/emit_spirv_instructions.h` | 491 | 8 | GPL-2.0-or-later | differs 4 | 0 | reconcile |
| `shadps4/shader_recompiler/backend/spirv/emit_spirv_quad_rect.cpp` | 349 | 4 | GPL-2.0-or-later | differs 35 | 1 | reconcile |
| `shadps4/shader_recompiler/backend/spirv/emit_spirv_quad_rect.h` | 24 | 3 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/backend/spirv/spirv_emit_context.cpp` | 1427 | 9 | GPL-2.0-or-later | differs 164 | 2 | reconcile |
| `shadps4/shader_recompiler/backend/spirv/spirv_emit_context.h` | 461 | 8 | GPL-2.0-or-later | differs 30 | 2 | reconcile |
| `shadps4/shader_recompiler/frontend/control_flow_graph.cpp` | 431 | 8 | GPL-2.0-or-later | differs 162 | 0 | reconcile |
| `shadps4/shader_recompiler/frontend/control_flow_graph.h` | 102 | 7 | GPL-2.0-or-later | differs 3 | 0 | reconcile |
| `shadps4/shader_recompiler/frontend/copy_shader.cpp` | 102 | 5 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/frontend/copy_shader.h` | 23 | 4 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/frontend/decode.cpp` | 1292 | 5 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/frontend/decode.h` | 105 | 4 | GPL-2.0-or-later | same | 1 | already ours |
| `shadps4/shader_recompiler/frontend/fetch_shader.cpp` | 141 | 3 | GPL-2.0-or-later | differs 4 | 1 | reconcile |
| `shadps4/shader_recompiler/frontend/fetch_shader.h` | 98 | 2 | GPL-2.0-or-later | differs 25 | 1 | reconcile |
| `shadps4/shader_recompiler/frontend/fixed_quad_tessellation.h` | 67 | 10 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/shader_recompiler/frontend/instruction.cpp` | 64 | 6 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/frontend/instruction.h` | 362 | 5 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/frontend/opcodes.h` | 2708 | 6 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/frontend/structured_control_flow.cpp` | 812 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/frontend/structured_control_flow.h` | 22 | 7 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/frontend/tessellation.h` | 38 | 2 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/frontend/translate/translate.cpp` | 1263 | 8 | GPL-2.0-or-later | differs 29 | 0 | reconcile |
| `shadps4/shader_recompiler/frontend/translate/translate.h` | 401 | 7 | GPL-2.0-or-later | differs 12 | 0 | reconcile |
| `shadps4/shader_recompiler/info.h` | 335 | 3 | GPL-2.0-or-later | differs 72 | 4 | reconcile |
| `shadps4/shader_recompiler/ir/abstract_syntax_list.cpp` | 44 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/abstract_syntax_list.h` | 61 | 7 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/attribute.cpp` | 183 | 5 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/attribute.h` | 150 | 4 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/basic_block.cpp` | 205 | 7 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/basic_block.h` | 162 | 6 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/condition.h` | 52 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/debug_print.h` | 21 | 10 | GPL-2.0-or-later | same | 1 | already ours |
| `shadps4/shader_recompiler/ir/ir_emitter.cpp` | 2229 | 9 | GPL-2.0-or-later | differs 12 | 0 | reconcile |
| `shadps4/shader_recompiler/ir/ir_emitter.h` | 447 | 8 | GPL-2.0-or-later | differs 6 | 0 | reconcile |
| `shadps4/shader_recompiler/ir/microinstruction.cpp` | 251 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/microinstruction.h` | 161 | 7 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/opcodes.cpp` | 12 | 9 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/opcodes.h` | 116 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/opcodes.inc` | 472 | 8 | GPL-2.0-or-later | differs 1 | 0 | reconcile |
| `shadps4/shader_recompiler/ir/passes/ir_passes.h` | 48 | 7 | GPL-2.0-or-later | differs 8 | 1 | reconcile |
| `shadps4/shader_recompiler/ir/passes/resource_pass.h` | 181 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/passes/srt.h` | 56 | 4 | GPL-2.0-or-later | differs 25 | 0 | reconcile |
| `shadps4/shader_recompiler/ir/patch.cpp` | 28 | 9 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/patch.h` | 173 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/post_order.cpp` | 39 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/post_order.h` | 13 | 7 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/program.cpp` | 59 | 7 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/program.h` | 27 | 6 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/reg.h` | 520 | 4 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/reinterpret.h` | 145 | 9 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/ssa.h` | 70 | 7 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/type.cpp` | 36 | 5 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/type.h` | 58 | 4 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/value.cpp` | 118 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/ir/value.h` | 196 | 7 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/params.h` | 26 | 3 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/profile.h` | 69 | 2 | GPL-2.0-or-later | differs 11 | 5 | reconcile |
| `shadps4/shader_recompiler/recompiler.cpp` | 287 | 6 | GPL-2.0-or-later | differs 196 | 0 | reconcile |
| `shadps4/shader_recompiler/recompiler.h` | 34 | 5 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/shader_recompiler/resource.h` | 321 | 4 | GPL-2.0-or-later | differs 94 | 2 | reconcile |
| `shadps4/shader_recompiler/runtime_info.h` | 371 | 1 | GPL-2.0-or-later | differs 44 | 6 | reconcile |
| `shadps4/shader_recompiler/specialization.h` | 257 | 5 | GPL-2.0-or-later | differs 21 | 0 | reconcile |
| `shadps4/video_core/amdgpu/cb_db_extent.h` | 22 | 6 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/amdgpu/liverpool.cpp` | 3004 | 7 | GPL-2.0-or-later | differs 1832 | 35 | reconcile |
| `shadps4/video_core/amdgpu/liverpool.h` | 361 | 6 | GPL-2.0-or-later | differs 119 | 12 | reconcile |
| `shadps4/video_core/amdgpu/pixel_format.cpp` | 267 | 3 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/amdgpu/pixel_format.h` | 441 | 2 | GPL-2.0-or-later | differs 18 | 2 | reconcile |
| `shadps4/video_core/amdgpu/pm4_cmds.h` | 1278 | 8 | GPL-2.0-or-later | differs 64 | 4 | reconcile |
| `shadps4/video_core/amdgpu/pm4_opcodes.h` | 78 | 9 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/amdgpu/pm4_selftest.cpp` | 333 | 9 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/amdgpu/pm4_selftest.h` | 35 | 8 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/amdgpu/regs.cpp` | 134 | 8 | GPL-2.0-or-later | differs 9 | 0 | reconcile |
| `shadps4/video_core/amdgpu/regs.h` | 259 | 7 | GPL-2.0-or-later | differs 33 | 1 | reconcile |
| `shadps4/video_core/amdgpu/regs_color.h` | 309 | 2 | GPL-2.0-or-later | differs 3 | 1 | reconcile |
| `shadps4/video_core/amdgpu/regs_depth.h` | 291 | 2 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/amdgpu/regs_primitive.h` | 237 | 2 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/amdgpu/regs_shader.h` | 253 | 2 | GPL-2.0-or-later | differs 18 | 0 | reconcile |
| `shadps4/video_core/amdgpu/regs_texture.h` | 20 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/amdgpu/regs_vertex.h` | 285 | 2 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/amdgpu/resource.h` | 509 | 2 | GPL-2.0-or-later | differs 24 | 0 | reconcile |
| `shadps4/video_core/amdgpu/tiling.cpp` | 561 | 4 | GPL-2.0-or-later | differs 3 | 0 | reconcile |
| `shadps4/video_core/amdgpu/tiling.h` | 152 | 3 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/buffer_cache/buffer.cpp` | 205 | 3 | GPL-2.0-or-later | differs 327 | 1 | reconcile |
| `shadps4/video_core/buffer_cache/buffer.h` | 233 | 2 | GPL-2.0-or-later | differs 172 | 1 | reconcile |
| `shadps4/video_core/buffer_cache/buffer_cache.cpp` | 4168 | 7 | GPL-2.0-or-later | differs 5869 | 27 (c) | reconcile |
| `shadps4/video_core/buffer_cache/buffer_cache.h` | 684 | 6 | GPL-2.0-or-later | differs 717 | 19 (c) | reconcile |
| `shadps4/video_core/buffer_cache/fault_manager.cpp` | 99 | 8 | GPL-2.0-or-later | differs 172 | 0 | reconcile |
| `shadps4/video_core/buffer_cache/fault_manager.h` | 43 | 7 | GPL-2.0-or-later | differs 7 | 0 | reconcile |
| `shadps4/video_core/buffer_cache/memory_tracker.h` | 254 | 8 | GPL-2.0-or-later | differs 81 | 3 | reconcile |
| `shadps4/video_core/buffer_cache/range_set.h` | 325 | 7 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/buffer_cache/region_definitions.h` | 31 | 10 | GPL-2.0-or-later | differs 14 | 1 | reconcile |
| `shadps4/video_core/buffer_cache/region_manager.h` | 333 | 9 | GPL-2.0-or-later | differs 84 | 4 | reconcile |
| `shadps4/video_core/cache_storage.cpp` | 309 | 7 | GPL-2.0-or-later | differs 121 | 1 | reconcile |
| `shadps4/video_core/cache_storage.h` | 52 | 6 | GPL-2.0-or-later | differs 19 | 1 | reconcile |
| `shadps4/video_core/host_shaders/fsr/ffx_a.h` | 2657 | 9 | none (root GPL-2.0) | same | 0 | defer: upscaler |
| `shadps4/video_core/host_shaders/fsr/ffx_fsr1.h` | 1200 | 9 | none (root GPL-2.0) | same | 0 | defer: upscaler |
| `shadps4/video_core/multi_level_page_table.h` | 74 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/page_manager.cpp` | 775 | 10 | GPL-2.0-or-later | differs 308 | 8 | reconcile |
| `shadps4/video_core/page_manager.h` | 74 | 9 | GPL-2.0-or-later | differs 8 | 2 | reconcile |
| `shadps4/video_core/renderdoc.cpp` | 165 | 9 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/renderdoc.h` | 53 | 8 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/renderer_backend/binding.h` | 54 | 3 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_backend/commands.h` | 246 | 2 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_backend/device.cpp` | 29 | 2 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_backend/device.h` | 147 | 1 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_backend/draw_state.h` | 327 | 2 | GPL-2.0-or-later | absent | 2 | new |
| `shadps4/video_core/renderer_backend/format.cpp` | 18 | 4 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_backend/format.h` | 176 | 3 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_backend/handle_table.h` | 72 | 4 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_backend/host_pipeline.h` | 82 | 9 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_backend/pass.h` | 85 | 2 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_backend/pipeline.h` | 132 | 2 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_backend/resource.h` | 329 | 2 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_backend/state.h` | 163 | 3 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_backend/swapchain.h` | 40 | 8 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_backend/transfers.h` | 131 | 5 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_vulkan/fsr411/fsr411.cpp` | 851 | 7 | GPL-2.0-or-later | differs 115 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/fsr411/fsr411.h` | 74 | 6 | GPL-2.0-or-later | differs 26 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/host_passes/fsr_pass.cpp` | 400 | 8 | GPL-2.0-or-later | differs 293 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/host_passes/fsr_pass.h` | 60 | 7 | GPL-2.0-or-later | differs 46 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/host_passes/pp_pass.cpp` | 167 | 8 | GPL-2.0-or-later | differs 217 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/host_passes/pp_pass.h` | 39 | 7 | GPL-2.0-or-later | differs 14 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/liverpool_to_vk.cpp` | 1221 | 2 | GPL-2.0-or-later | differs 536 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/liverpool_to_vk.h` | 123 | 1 | GPL-2.0-or-later | differs 78 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/motion_history.h` | 329 | 6 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/renderer_vulkan/scene_resolution.h` | 33 | 10 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/renderer_vulkan/ui_composition.h` | 61 | 10 | GPL-2.0-or-later | differs 72 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_backend_device.cpp` | 890 | 4 | GPL-2.0-or-later | absent | 4 (c) | new |
| `shadps4/video_core/renderer_vulkan/vk_backend_device.h` | 245 | 3 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_backend_readback.cpp` | 59 | 6 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_backend_readback.h` | 145 | 5 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_vulkan/vk_barrier_tracker.cpp` | 368 | 8 | Zlib | same | 0 | already ours |
| `shadps4/video_core/renderer_vulkan/vk_barrier_tracker.h` | 137 | 7 | Zlib | same | 0 | already ours |
| `shadps4/video_core/renderer_vulkan/vk_bind_helper.h` | 124 | 6 | GPL-2.0-or-later | differs 22 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_breadcrumbs.cpp` | 418 | 2 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_vulkan/vk_breadcrumbs.h` | 67 | 1 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_vulkan/vk_camera_motion.cpp` | 434 | 10 | GPL-2.0-or-later | differs 606 | 6 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_camera_motion.h` | 128 | 9 | GPL-2.0-or-later | differs 95 | 4 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_command_recorder.cpp` | 577 | 2 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_vulkan/vk_command_recorder.h` | 104 | 1 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_common.cpp` | 14 | 2 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/renderer_vulkan/vk_common.h` | 23 | 1 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/renderer_vulkan/vk_compute_pipeline.cpp` | 38 | 2 | GPL-2.0-or-later | differs 136 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_compute_pipeline.h` | 45 | 1 | GPL-2.0-or-later | differs 34 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_constant_ring.h` | 120 | 9 | GPL-2.0-or-later | differs 38 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_depth_resampler.cpp` | 211 | 9 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_vulkan/vk_depth_resampler.h` | 47 | 8 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_vulkan/vk_dlss.cpp` | 366 | 4 | GPL-2.0-or-later | differs 221 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_dlss.h` | 75 | 3 | GPL-2.0-or-later | differs 36 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_draw_pipe.h` | 268 | 9 | GPL-2.0-or-later | differs 86 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_draw_prep.cpp` | 519 | 7 | GPL-2.0-or-later | differs 137 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_draw_prep.h` | 173 | 6 | GPL-2.0-or-later | differs 31 | 2 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_format.h` | 28 | 2 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_frame_capture.cpp` | 335 | 11 | GPL-2.0-or-later | differs 413 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_frame_capture.h` | 61 | 10 | GPL-2.0-or-later | differs 24 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_fsr4.cpp` | 459 | 5 | GPL-2.0-or-later | differs 139 | 3 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_fsr4.h` | 58 | 4 | GPL-2.0-or-later | differs 6 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_fsr4_metal.h` | 26 | 3 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_fsr4_metal.mm` | 387 | 4 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_vulkan/vk_gpu_labels.cpp` | 38 | 4 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_gpu_labels.h` | 15 | 3 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_gpu_profiler.cpp` | 108 | 4 | GPL-2.0-or-later | differs 34 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_gpu_profiler.h` | 91 | 3 | GPL-2.0-or-later | differs 8 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_graphics_pipeline.cpp` | 446 | 2 | GPL-2.0-or-later | differs 751 | 3 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_graphics_pipeline.h` | 102 | 1 | GPL-2.0-or-later | differs 82 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_host_objects.cpp` | 134 | 9 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_host_objects.h` | 112 | 8 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_indirect_guard.cpp` | 122 | 11 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_indirect_guard.h` | 56 | 10 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_vulkan/vk_instance.cpp` | 1118 | 2 | GPL-2.0-or-later | differs 631 | 20 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_instance.h` | 675 | 1 | GPL-2.0-or-later | differs 160 | 20 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_metal_shadow.h` | 72 | 1 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_vulkan/vk_metalfx.cpp` | 549 | 10 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_metalfx.h` | 119 | 9 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_vulkan/vk_null_resources.cpp` | 111 | 11 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_vulkan/vk_null_resources.h` | 49 | 10 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_vulkan/vk_object_motion.cpp` | 237 | 10 | GPL-2.0-or-later | differs 268 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_object_motion.h` | 115 | 9 | GPL-2.0-or-later | differs 76 | 2 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_occlusion.cpp` | 390 | 11 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_occlusion.h` | 127 | 10 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/renderer_vulkan/vk_pipeline_ahead.cpp` | 1195 | 7 | none (root GPL-2.0) | absent | 1 | exclude: title-specific |
| `shadps4/video_core/renderer_vulkan/vk_pipeline_ahead.h` | 44 | 6 | none (root GPL-2.0) | absent | 1 | exclude: title-specific |
| `shadps4/video_core/renderer_vulkan/vk_pipeline_cache.cpp` | 2616 | 5 | GPL-2.0-or-later | differs 2684 | 9 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_pipeline_cache.h` | 276 | 4 | GPL-2.0-or-later | differs 228 | 14 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_pipeline_common.cpp` | 59 | 3 | GPL-2.0-or-later | differs 86 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_pipeline_common.h` | 80 | 2 | GPL-2.0-or-later | differs 43 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_pipeline_key.h` | 115 | 1 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_pipeline_serialization.cpp` | 620 | 7 | GPL-2.0-or-later | differs 777 | 6 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_pipeline_serialization.h` | 27 | 6 | GPL-2.0-or-later | differs 6 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_platform.cpp` | 486 | 3 | GPL-2.0-or-later | differs 171 | 3 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_platform.h` | 89 | 2 | GPL-2.0-or-later | differs 20 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_presenter.cpp` | 994 | 7 | GPL-2.0-or-later | differs 1048 | 11 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_presenter.h` | 164 | 6 | GPL-2.0-or-later | differs 72 | 3 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_prompt_images.cpp` | 144 | 11 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_prompt_images.h` | 50 | 10 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_rasterizer.cpp` | 5530 | 9 | GPL-2.0-or-later | differs 4715 | 40 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_rasterizer.h` | 758 | 8 | GPL-2.0-or-later | differs 359 | 28 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_resource_backend.cpp` | 580 | 2 | GPL-2.0-or-later | absent | 2 | new |
| `shadps4/video_core/renderer_vulkan/vk_resource_backend.h` | 174 | 1 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_resource_pool.cpp` | 194 | 3 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/renderer_vulkan/vk_resource_pool.h` | 92 | 2 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/renderer_vulkan/vk_runtime.cpp` | 1524 | 7 | GPL-2.0-or-later | differs 808 | 6 (c) | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_runtime.h` | 212 | 6 | GPL-2.0-or-later | differs 115 | 5 (c) | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_scene_resolution.cpp` | 270 | 10 | GPL-2.0-or-later | differs 436 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_scene_resolution.h` | 118 | 9 | GPL-2.0-or-later | differs 79 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_scheduler.cpp` | 1321 | 2 | GPL-2.0-or-later | differs 1160 | 11 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_scheduler.h` | 1041 | 1 | GPL-2.0-or-later | differs 1063 | 28 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_semaphore.cpp` | 117 | 3 | GPL-2.0-or-later | differs 147 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_semaphore.h` | 62 | 2 | GPL-2.0-or-later | differs 9 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_shader_hle.cpp` | 289 | 11 | GPL-2.0-or-later | differs 501 | 4 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_shader_hle.h` | 23 | 10 | GPL-2.0-or-later | differs 20 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_shader_scan.cpp` | 350 | 7 | none (root GPL-2.0) | absent | 1 | exclude: title-specific |
| `shadps4/video_core/renderer_vulkan/vk_shader_scan.h` | 67 | 6 | none (root GPL-2.0) | absent | 1 | exclude: title-specific |
| `shadps4/video_core/renderer_vulkan/vk_shader_util.cpp` | 21 | 4 | GPL-2.0-or-later | differs 125 | 0 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_shader_util.h` | 20 | 3 | GPL-2.0-or-later | same | 0 | already ours |
| `shadps4/video_core/renderer_vulkan/vk_staging_buffer_pool.cpp` | 237 | 8 | GPL-2.0-or-later | differs 74 | 2 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_staging_buffer_pool.h` | 90 | 7 | GPL-2.0-or-later | differs 7 | 1 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_state.h` | 285 | 2 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_stream_encoder.cpp` | 88 | 2 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_stream_encoder.h` | 62 | 1 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_swapchain.cpp` | 438 | 8 | GPL-2.0-or-later | differs 133 | 2 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_swapchain.h` | 197 | 7 | GPL-2.0-or-later | differs 77 | 2 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_temporal_upscaler.cpp` | 2446 | 9 | GPL-2.0-or-later | differs 2100 | 14 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_temporal_upscaler.h` | 343 | 8 | GPL-2.0-or-later | differs 208 | 8 | reconcile |
| `shadps4/video_core/renderer_vulkan/vk_timestamps.cpp` | 130 | 11 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/renderer_vulkan/vk_timestamps.h` | 69 | 10 | GPL-2.0-or-later | absent | 1 | new |
| `shadps4/video_core/texture_cache/blit_helper.cpp` | 439 | 7 | GPL-2.0-or-later | differs 28 | 0 | reconcile |
| `shadps4/video_core/texture_cache/blit_helper.h` | 64 | 6 | GPL-2.0-or-later | differs 5 | 0 | reconcile |
| `shadps4/video_core/texture_cache/host_compatibility.cpp` | 220 | 8 | GPL-2.0-or-later | differs 310 | 0 | reconcile |
| `shadps4/video_core/texture_cache/host_compatibility.h` | 17 | 7 | GPL-2.0-or-later | differs 3 | 0 | reconcile |
| `shadps4/video_core/texture_cache/image.cpp` | 260 | 5 | GPL-2.0-or-later | differs 548 | 2 | reconcile |
| `shadps4/video_core/texture_cache/image.h` | 185 | 4 | GPL-2.0-or-later | differs 129 | 7 | reconcile |
| `shadps4/video_core/texture_cache/image_info.cpp` | 312 | 6 | GPL-2.0-or-later | differs 48 | 0 | reconcile |
| `shadps4/video_core/texture_cache/image_info.h` | 96 | 5 | GPL-2.0-or-later | differs 37 | 0 | reconcile |
| `shadps4/video_core/texture_cache/image_upload_hash.h` | 30 | 5 | GPL-2.0-or-later | absent | 0 | new |
| `shadps4/video_core/texture_cache/image_view.cpp` | 159 | 6 | GPL-2.0-or-later | differs 216 | 0 | reconcile |
| `shadps4/video_core/texture_cache/image_view.h` | 87 | 5 | GPL-2.0-or-later | differs 40 | 0 | reconcile |
| `shadps4/video_core/texture_cache/sampler.cpp` | 61 | 9 | GPL-2.0-or-later | differs 101 | 0 | reconcile |
| `shadps4/video_core/texture_cache/sampler.h` | 54 | 8 | GPL-2.0-or-later | differs 41 | 0 | reconcile |
| `shadps4/video_core/texture_cache/texture_cache.cpp` | 1611 | 8 | GPL-2.0-or-later | differs 1215 | 12 (c) | reconcile |
| `shadps4/video_core/texture_cache/texture_cache.h` | 461 | 7 | GPL-2.0-or-later | differs 123 | 9 | reconcile |
| `shadps4/video_core/texture_cache/tile.h` | 348 | 7 | GPL-2.0-or-later | differs 31 | 0 | reconcile |
| `shadps4/video_core/texture_cache/tile_manager.cpp` | 259 | 9 | GPL-2.0-or-later | differs 249 | 4 | reconcile |
| `shadps4/video_core/texture_cache/tile_manager.h` | 68 | 8 | GPL-2.0-or-later | differs 30 | 0 | reconcile |
| `shadps4/video_core/texture_cache/types.h` | 100 | 6 | GPL-2.0-or-later | differs 11 | 0 | reconcile |
| `shim/bbport_button_prompts.cpp` | 638 | 10 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_button_prompts.h` | 61 | 9 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_ce_stats.h` | 121 | 8 | none (root GPL-2.0) | absent | 1 | new |
| `shim/bbport_copy.cpp` | 307 | 4 | GPL-2.0-or-later | same | 0 | already ours |
| `shim/bbport_copy.h` | 45 | 3 | none (root GPL-2.0) | same | 1 | already ours |
| `shim/bbport_effect_flags.h` | 33 | 12 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_frame_rate.cpp` | 469 | 11 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_frame_rate.h` | 22 | 10 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_free_check.cpp` | 1086 | 9 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_free_check.h` | 61 | 8 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_game_hooks.cpp` | 790 | 11 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_game_hooks.h` | 23 | 10 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_game_menu.cpp` | 661 | 9 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_game_menu.h` | 17 | 8 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_gnm_hooks.cpp` | 632 | 11 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_gnm_hooks.h` | 26 | 10 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_gnm_table.inc` | 518 | 12 | none (root GPL-2.0) | absent | 0 | exclude: title-specific |
| `shim/bbport_guest_call.h` | 38 | 8 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_guest_code.h` | 89 | 12 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_guest_code_hooks.cpp` | 367 | 9 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_guest_code_hooks.h` | 63 | 8 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_guest_memory.cpp` | 749 | 4 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_guest_memory.h` | 52 | 3 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_heap_sites.cpp` | 357 | 11 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_heap_sites.h` | 16 | 10 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_metal_residency.cpp` | 330 | 4 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_metal_residency.h` | 46 | 3 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_metalfx.h` | 81 | 11 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_metalfx.mm` | 443 | 12 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_overlay.cpp` | 2516 | 9 | GPL-2.0-or-later | differs 3454 | 1 (c) | reconcile |
| `shim/bbport_overlay.h` | 72 | 8 | GPL-2.0-or-later | differs 50 | 1 | reconcile |
| `shim/bbport_overlay_metal.h` | 18 | 10 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_overlay_metal.mm` | 35 | 11 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_render_boundary.cpp` | 1670 | 7 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_render_boundary.h` | 70 | 6 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_sections.h` | 75 | 8 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_settings.cpp` | 740 | 7 | GPL-2.0-or-later | differs 962 | 1 | reconcile |
| `shim/bbport_settings.h` | 210 | 6 | GPL-2.0-or-later | differs 255 | 1 | reconcile |
| `shim/bbport_text.cpp` | 107 | 11 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_text.h` | 19 | 10 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_threads.h` | 61 | 3 | GPL-2.0-or-later | differs 136 | 1 | reconcile |
| `shim/bbport_timeline.h` | 111 | 8 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_toggles.h` | 228 | 2 | none (root GPL-2.0) | differs 210 | 2 (c) | reconcile |
| `shim/bbport_touch_controls.cpp` | 297 | 11 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_touch_controls.h` | 35 | 10 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_trophy_queue.h` | 75 | 10 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_wait_trace.cpp` | 125 | 4 | GPL-2.0-or-later | absent | 0 | new |
| `shim/bbport_wait_trace.h` | 27 | 3 | GPL-2.0-or-later | absent | 1 | new |
| `shim/bbport_write_log.cpp` | 197 | 9 | GPL-2.0-or-later | differs 75 | 0 | reconcile |
| `shim/bbport_write_log.h` | 21 | 8 | GPL-2.0-or-later | differs 8 | 1 | reconcile |
| `shim/common/debug.h` | 28 | 3 | none (root GPL-2.0) | same | 1 | already ours |
| `shim/common/logging/log.cpp` | 30 | 2 | none (root GPL-2.0) | same | 1 | already ours |
| `shim/common/logging/log.h` | 45 | 1 | none (root GPL-2.0) | same | 1 | already ours |
| `shim/core/address_space.h` | 26 | 9 | none (root GPL-2.0) | same | 1 | already ours |
| `shim/core/debug_state.h` | 43 | 6 | none (root GPL-2.0) | same | 1 | already ours |
| `shim/core/devtools/widget/gpu_memory.h` | 13 | 9 | none (root GPL-2.0) | same | 1 | already ours |
| `shim/core/emulator_settings.h` | 78 | 4 | none (root GPL-2.0) | differs 57 | 3 | reconcile |
| `shim/core/libraries/kernel/kernel.h` | 7 | 11 | none (root GPL-2.0) | differs 8 | 1 | reconcile |
| `shim/core/libraries/libs.h` | 20 | 10 | none (root GPL-2.0) | differs 2 | 1 | reconcile |
| `shim/core/memory.h` | 36 | 8 | none (root GPL-2.0) | differs 9 | 2 | reconcile |
| `shim/core/signals.h` | 50 | 3 | none (root GPL-2.0) | differs 7 | 2 | reconcile |
| `shim/imgui/renderer/imgui_core.h` | 12 | 3 | none (root GPL-2.0) | same | 1 | already ours |
| `shim/imgui/renderer/texture_manager.h` | 12 | 3 | none (root GPL-2.0) | same | 1 | already ours |
| `shim/sdl_window.h` | 75 | 3 | none (root GPL-2.0) | differs 27 | 1 | reconcile |
| `shim/tracy/Tracy.hpp` | 29 | 3 | none (root GPL-2.0) | same | 1 | already ours |
| `shim/tracy/TracyVulkan.hpp` | 10 | 2 | none (root GPL-2.0) | same | 1 | already ours |
