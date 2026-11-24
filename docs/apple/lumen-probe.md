# Lumen Metal adapter probe (#408)

Status: **research and measurement only; no lumen code is vendored.** Owner ruling 2026-11-24:
"Yes, probe first". Question: can the Metal-backed Vulkan C API adapter in `gs-soft-it/lumen-emu`
serve as the static Vulkan provider under our near-stock shadPS4 renderer in `gpu/`, instead of
reconciling `bloodborne-runtime`'s `renderer_metal` (159 files differ, 100 missing; see
`metal-renderer-inventory.md`)? Stacked on #590 so this note sits beside the inventory.

## Method and pins

| Item | Pin |
| --- | --- |
| lumen-emu | `6127cba8a39e5ba661e29f61ce028c458a2af80a` (2026-11-21, shallow clone, scratch only) |
| Adapter files | `source/emulator/src/Graphics/Metal/`: `MetalVulkanCore.mm` (4,096 lines), `MetalVulkanCommands.mm` (6,193), `MetalVulkanInternal.h` (548), `MetalCullDistance.{h,mm}` (71 + 340), `MetalSoftwareTessellatorSource.inc` (1,556), `MetalTessellationCapture.inc` (203): 13,007 lines |
| SPIRV-Cross | `d8e3e2b141b8c8a167b2e3984736a6baacff316c` (the pin in lumen's `source/3rdparty/spirv_cross_msl.cmake`), built from a shallow fetch: core, glsl and msl, C API and CLI off, `SPIRV_CROSS_EXCEPTIONS_TO_ASSERTIONS=ON` |
| Vulkan headers | Homebrew 1.4.363 (lumen vendors 1.2.176 in-tree and pins its real headers outside the repo, so this is a substitution) |
| Toolchain | Apple clang 21.0.0, Xcode 27.2 macOS SDK, arm64 (Apple M1 Mac mini) |
| Our side | `gpu/` at this branch (`b4e7ae77` shadPS4 base plus `bbport:` changes) |

How the needs list was made: `docs/apple/lumen-probe-scan.py` maps every vulkan.hpp method to the
`vk*` function it dispatches (parsed from `vulkan_funcs.hpp`), then greps `gpu/shadps4/video_core`
for those method names. That is an **upper bound** (a method name such as `destroy` also matches
unrelated handle types), so every row below marked "needed" was then checked by hand in the source.
`vk_instance.cpp` and `vk_platform.cpp` were read for the extension, feature and version rules.
The adapter's exports were read from its `vkGetInstanceProcAddr` table (`ResolveFunction`,
167 entries plus 34 `KHR`/`EXT` aliases), which is what a vulkan.hpp dispatcher actually sees.

## Licence findings

- Root `LICENSE` is GPL-2.0 text; `NOTICE` (updated 2026-10-22) says Lumen's "new integration code
  and original adapters" are `GPL-2.0-only` unless a file states otherwise; `LICENSES.md` repeats it.
  **None of the seven Metal files carries an SPDX line or copyright header** except the tessellator
  `.inc`. The adapter's licence therefore rests on the repo-level `NOTICE`, not on the files. That is
  compatible with our `GPL-2.0-only` tree (`Cargo.toml`), but before vendoring we should ask
  gs-soft-it for per-file SPDX headers or record the `NOTICE` commit in `PEERS.md`.
- `MetalSoftwareTessellatorSource.inc`: MIT, a mechanical MSL port of Microsoft's D3D11 reference
  tessellator via Mesa 26.1.0 `src/poly/cl/tessellator.h`, with Valve wrapper definitions (MIT). The
  Microsoft and Valve notices must travel with the file. Compatible.
- SPIRV-Cross is `Apache-2.0 OR MIT`; lumen's selection record picks **MIT** for every file. We must
  do the same: Apache-2.0 alone is not compatible with GPLv2-only (lumen's own `LICENSES.md` flags
  that problem for its PS5 SPIRV-Tools link, which is unrelated to this adapter).
- Excluded as asked: `android/`, `gpu-drivers/` and the turnip-banners package (GPL-3.0 recipe and
  patches). Nothing in `Graphics/Metal/` touches them.
- Lumen's `LICENSES.md` lists unresolved release issues (PS5 SPIRV-Tools, Gen4 LibreSSL, closure
  audits) and says the combined binaries are not declared distributable. None concerns the Metal
  adapter, but it is a reason to vendor only these files and never lumen's runtime.
- **Not verified:** I did not diff the adapter against MoltenVK or any other Vulkan-on-Metal source
  for copied code; the files make no such claim and mention neither MoltenVK nor Apache anywhere.

## What the adapter is

- A static library `vulkan-1` (`add_library(vulkan-1 STATIC ...)` in lumen's `3rdparty/CMakeLists.txt`)
  that exports the Vulkan C entry points directly (`extern "C" vkGetInstanceProcAddr` and 166 more).
  Built **standalone** here from `-I Metal/ -I SPIRV-Cross -I Vulkan-Headers` and nothing else: the
  seven files have no dependency on the rest of lumen (no emulator headers, no logging layer).
  Compile flags that matter: ObjC++ with ARC, `-fno-rtti -fno-exceptions` plus
  `SPIRV_CROSS_EXCEPTIONS_TO_ASSERTIONS` for `MetalVulkanCommands.mm` (it `#error`s otherwise).
  Link: Metal, Foundation, QuartzCore, CoreGraphics, IOSurface, and the three SPIRV-Cross archives.
- Shaders: SPIR-V to MSL 2.3 through `spirv_cross::CompilerMSL` (a local subclass overrides prototype
  and entry-point emission), then `newLibraryWithSource`. Software tessellation (MSL compute from the
  MIT `.inc`) stands in for hull/domain stages; `MetalCullDistance` rewrites cull distance in SPIR-V.
- Platforms: macOS and iOS (`TARGET_OS_IPHONE` branches in about 20 places, including surface creation via
  `VK_EXT_metal_surface`, `VK_MVK_ios_surface` and `VK_MVK_macos_surface`). Our `vk_platform.cpp`
  already creates a `vk::MetalSurfaceCreateInfoEXT` from a `CAMetalLayer`, which matches.
- Memory model: one device-local heap sized from `recommendedMaxWorkingSetSize`, two memory types
  (device-local; host-visible/coherent shared). No `memoryBudget`, no sparse, no separate VRAM.
- Reports Vulkan **1.2.0** (instance and device) and rejects `VkApplicationInfo.apiVersion > 1.2`
  with `VK_ERROR_INCOMPATIBLE_DRIVER`, while exposing the 1.3 features `dynamicRendering` and
  `synchronization2`. Ten device extensions: swapchain, maintenance1, push_descriptor,
  depth_clip_control, image_robustness, dynamic_rendering, synchronization2,
  extended_dynamic_state, extended_dynamic_state2, fragment_shader_barycentric (when the GPU has it).
- Reported features include `shaderInt64`, `shaderInt16`, `shaderFloat16`, `shaderInt8`, storage 8/16
  bit, timeline semaphores, buffer device address (probed with a Metal pointer-query kernel), draw
  indirect count, `multiDrawIndirect`, `shaderClipDistance`, `shaderCullDistance` (probed),
  `dualSrcBlend`, `tessellationShader`, `depthClamp`, `multiViewport`, `independentBlend`, image
  cube array, BC compression when the GPU supports it. Reported false: geometry shader, float64,
  sparse, robustBufferAccess, 64-bit atomics, hostQueryReset, scalarBlockLayout, maintenance4,
  subgroup size control, demote to helper invocation, depth bounds, `shaderStorageImageReadWithoutFormat`.
- Dynamic-state/pipeline model: static pipelines plus `extended_dynamic_state` 1 and 2 setters;
  pass objects (`vkCmdBeginRenderPass`) and dynamic rendering both work.
- Stubs: the adapter does not return stub results for missing calls; it leaves them out of the table,
  so `vkGetDeviceProcAddr` returns null and a vulkan.hpp call would jump through a null pointer. Per
  feature it returns `VK_ERROR_FEATURE_NOT_PRESENT` or `VK_ERROR_EXTENSION_NOT_PRESENT` at
  `vkCreateDevice` for anything requested beyond the list above.
- Evidence it runs a shadPS4 renderer at all: lumen's own Gen4 core (`source/gen4`, shadPS4 via
  `zenithblue-oss/shadps4-arm64` at `be6bc2e9`) uses it on iOS. But that renderer is **not stock**:
  it has `LUMEN_GEN4_NATIVE_METAL` branches in `vk_instance.cpp`, `vk_platform.{h,cpp}`,
  `vk_pipeline_cache.cpp`, and a `src/platform/macos/video_core/` tree of 19 non-shader files
  (11,166 lines) that replaces `vk_rasterizer`, `buffer_cache`, `texture_cache`, `image`, `liverpool`,
  `page_manager` and the tile manager, plus macOS detiling compute shaders. So lumen did not
  put a stock shadPS4 renderer on this adapter; it forked the renderer too.

## Smoke result (measured, 2026-11-24, Apple M1, macOS)

Built the adapter standalone as a static library (3 translation units, 775 KB archive at `-O1`;
SPIRV-Cross built in 27 s with `ninja -j3`) and
linked two programs against it:

1. Our own `tests/test_vulkan_smoke.c`, unmodified: **PASS** ("Vulkan: Apple M1; command submission
   and readback 4096-byte readback PASS").
2. `docs/apple/lumen-probe-smoke.c` (authored for this probe): instance, device enumeration, report
   of extensions and features, `vkCreateDevice`, `vkCmdFillBuffer` + host readback (**PASS**, 0 bad
   words), `vkCmdClearColorImage` + `vkCmdCopyImageToBuffer` + readback (**PASS**, first pixel
   255 128 0 255 as cleared). It also prints the report that fed the gap table: device
   `Apple M1`, API 1.2.0, the ten extensions above, and, for our four hard requirements in
   `vk_instance.cpp`, swapchain and push descriptor present, **vertex_attribute_divisor and
   robustness2 missing**.

What this proves: the adapter builds and runs on this Mac without lumen's runtime, accepts a
plain Vulkan 1.0-style client, and executes transfer work correctly. It does not prove anything
about graphics pipelines, SPIR-V translation, swapchains or our renderer.

### Building `gpu/` against the adapter (not achieved)

`cmake -G Ninja gpu/` on macOS stops at the first `find_package`: **Boost not found**
(Homebrew has fmt, SDL3, libx11, MoltenVK and the Vulkan loader; it lacks boost, robin-map,
vulkan-memory-allocator, xxhash, zydis and glslang; `magic_enum` and `miniz` have fallbacks). I did not
install packages on the shared Mac, so the configure was not taken past the first error and no `gpu/`
object was compiled. Known further blockers, read from the sources and not exercised:

- `vk_platform.cpp` loads `vkGetInstanceProcAddr` with `vk::detail::DynamicLoader` (dlopen/dlsym) and
  on Apple points `VK_DRIVER_FILES` at an ICD next to the executable. For a static provider it must
  take the address of the linked `vkGetInstanceProcAddr` instead (a few lines, `#ifdef`).
- `CMakeLists.txt` requires X11 outside Windows (`pkg_check_modules(X11 REQUIRED ...)`).
- `build.rs` panics for Apple targets and the Xbyak-generated pass is x86-64 only (#396, from the
  inventory). These are independent of the renderer choice.

## Gap table: what `gpu/` needs against what the adapter offers

"Needed" is checked in source. Effort is a rough estimate in person-days for one engineer who
already knows the code, not a measurement. Rows are ordered by how soon they stop a first frame.

| Needed by `gpu/` | Adapter status | Hard / soft | Effort |
| --- | --- | --- | --- |
| Vulkan 1.3 instance and device (`TargetVulkanApiVersion`, asserted in `vk_platform.cpp` and `vk_instance.cpp`) | Reports 1.2, refuses an app asking for 1.3 | Hard in ours; lumen drops it to 1.2 under `#if` | 0.5 (renderer `#if`) |
| `VK_EXT_robustness2` with `robustBufferAccess2`, `robustImageAccess2`, `nullDescriptor` (three `ASSERT_MSG`) | Missing; the sType is not handled; Metal has no null descriptor | Hard in ours; lumen makes it optional | 3 to 5 (make optional, plus a dummy-descriptor fallback in the renderer, or adapter-side substitution) |
| `VK_EXT_vertex_attribute_divisor` (assert) | Missing; Metal has per-attribute step rate so it could be added | Hard in ours; lumen makes it optional (instanced fetch degrades to divisor 1, which is wrong for real instanced draws) | 2 to 4 (adapter: map to step function and rate) |
| `VK_KHR_swapchain`, `VK_KHR_push_descriptor`, `vkCmdPushDescriptorSetKHR` | Implemented | Hard | 0 |
| Metal surface from `CAMetalLayer` (`vkCreateMetalSurfaceEXT`) | Implemented (plus MVK macOS and iOS surfaces) | Hard | 0 (untested with our presenter) |
| Timeline semaphores, `vkQueueSubmit` with timeline pNext (we use `vk::SubmitInfo`, not `Submit2`) | Implemented | Hard | 0 (untested under load) |
| Dynamic rendering, `vkCmdPipelineBarrier2`, extended dynamic state 1 and 2 | Implemented | Hard | 0 |
| Sparse buffers: `sparseBinding`, `sparseResidencyBuffer`, `vkQueueBindSparse`, `vkGetDeviceBufferMemoryRequirements` (maintenance4) for the buffer-cache arena (`buffer_cache.cpp:144`, `buffer.cpp:246`, `:2420`) | **Missing entirely** (features false, three entry points absent) | Hard for our buffer cache; lumen replaced the buffer cache | 10 to 20 (replace the arena with plain or heap-backed buffers, or emulate sparse in the adapter with Metal placement heaps; neither exists today) |
| `vkCmdUpdateBuffer` (`vk_runtime.cpp:188`) | Missing | Hard where reached | 1 (encode as a blit or compute write in the adapter) |
| Descriptor sets, layouts, pools, `vkUpdateDescriptorSets`, pipeline layouts, pipeline cache | Implemented | Hard | 0 |
| Buffer views for texel buffers (`vkDestroyBufferView` is called; no `createBufferView` found in our tree, only a texel-view write path in `vk_pipeline_common.cpp`) | `vkCreateBufferView` and `vkDestroyBufferView` absent | Hard if texel buffers are used; unverified whether any path reaches it | 1 to 2 |
| Query pool: `vkCreateQueryPool`, `vkCmdWriteTimestamp2`, `vkGetQueryPoolResults`, `vkResetQueryPool` (GPU profiler behind `BB_GPU_PROFILE`; `fsr411.cpp` profile pool) | Missing; `hostQueryReset` false | Soft (profiling) | 1 to 2 (or leave off) |
| Debug-utils command labels (`vkCmd{Begin,End,Insert}DebugUtilsLabelEXT`, `vkQueue*Label`, used in rasterizer and passes) | Instance extension advertised, but only messenger and object-name entry points exist | Soft; **a null call if debug utils is enabled** | 1 (add no-op labels) |
| `vkCmdSetVertexInputEXT`, `vkCmdSetColorWriteMaskEXT`, `vkCmdSetAttachmentFeedbackLoopEnableEXT`, `vkCmdSetDepthBounds`, `vkCmdSetDepthBoundsTestEnable` | First three absent; depth bounds test is a setter but `depthBounds` is false (Metal has no depth bounds) | Soft; our code probes extension and feature first (not traced per path) | 1 to 3 (fallbacks exist for the first two) |
| Geometry shaders (SPIR-V `Capability::Geometry`; `geometryShader` feature copied from device) | Reports false; no GS stage | Hard for titles that bind ES/GS pipelines | Unknown; needs the peer's or lumen's GS handling, not measured |
| Shader capabilities from `shader_recompiler`: Int64, Int16, Int8, Float16, StorageBuffer 8/16, image query, `PhysicalStorageBufferAddresses`, `DrawParameters`, ShaderLayer, MultiViewport, GroupNonUniform (ballot, shuffle, quad) | Adapter reports the first group; subgroup ops, `Float64` (reported false), `Int64Atomics`, `AtomicFloat32MinMax`, `StencilExportEXT`, `ShaderClockKHR`, `DemoteToHelperInvocation`, `InterpolationFunction`, workgroup-memory-explicit-layout, `StorageImageReadWithoutFormat`: **not verified either way**; these go through SPIRV-Cross MSL conversion | Mixed | Unknown until real shaders run; first-shader testing is the cost |
| Format and image-view features (`GetFormatProperties` table, 2D view of 3D, min LOD, mixed samples) | Table implemented from Metal; extras optional in ours | Mostly soft | Unknown |
| `VK_EXT_memory_budget`, `VK_EXT_calibrated_timestamps`, vendor and NV/AMD extensions | Missing | Soft (optional in `vk_instance.cpp`) | 0 |
| VMA (`VmaVulkanFunctions` from the two proc-addr functions) | The adapter exports the Vulkan 1.1 memory-requirement and bind-2 functions VMA uses | Hard | 0 expected; unverified with our VMA version |
| Upscalers (DLSS, XeSS, FSR4 DX path), `renderdoc` | Out of scope; NVIDIA or Windows only | Defer | 0 |

The scan lists 239 entry points as possibly needed; the adapter exports 135 of them. Of the 104
it lacks, most are false positives from name collisions (display, video, Xlib, Fuchsia and vendor
calls). The real ones, checked by hand, are in the table: query pool (4), debug-utils labels (6),
dynamic state (4), sparse and maintenance4 (2), buffer view (1 to 2), `vkCmdUpdateBuffer` (1).

## Recommendation

Three options for #408, with the cost I can support from this probe.

**A. Lumen adapter under our renderer.** What exists: a 13k-line self-contained library that
builds and runs on this Mac today, passes our own smoke test, is MIT/GPL-2.0-only clean, and is
the same kind of thing as the peer's `mtl_vk_layer` (Vulkan C API over Metal) with about 7,000 fewer
lines (13,007 against 20,409) and no dependency on a refactored renderer. What does not exist: any evidence it runs *our*
renderer. Lumen needed 11k lines of macOS-specific renderer replacements plus ifdefs to run
shadPS4 on it, and the biggest of our gaps (sparse buffer arena, robustness2 nullDescriptor,
GS) are exactly the places lumen forked. Estimate to a first frame: adapter vendoring and build
glue 3 to 5 days; renderer gating 3 to 5; buffer cache without sparse 10 to 20; adapter gap-fill
15 to 25 (vertex divisor, nullDescriptor behaviour, UpdateBuffer, labels, buffer views, queries);
then shader and format bring-up, unmeasured. About 30 to 55 days before game debugging.

**B. Reconcile `renderer_metal` from bloodborne-runtime.** 159 files differ and 100 are missing
(about 73k and 20k lines in the inventory), plus the 20k-line Metal layer, and the work is adopting
the peer's renderer refactor and not just a directory. Benefit: it is proven by the peer on a real
title with Bloodborne-specific fixes. Cost: our `gpu/` is a near-stock shadPS4 with our `bbport:`
changes, and every one of the 159 files needs a judgment call. Estimate 40 to 80 days with
continuing merge cost whenever either side moves. Same licence cleanliness (GPL-2.0-or-later per
file, SPDX present).

**C. Hybrid.** Use A's adapter as the provider, and take targeted pieces from B (index expand, the
MSL tiling-header generator and its test, caps tables) where the adapter has holes, without
reconciling the renderer. This is A with a donor.

**Recommendation: C, entered through A, with a hard stop.** Do the cheap first milestone before
committing to either: vendor the adapter (new PR, owner decision), patch only the three hard
asserts and the loader line in `vk_platform.cpp`, and get `Instance::CreateDevice` to complete on
this Mac. That is days, and it answers the one question this probe could not: whether our
renderer's *pipeline creation and first draw* survive the adapter's SPIR-V path. Stop and fall
back to B if the sparse buffer arena cannot be replaced by plain buffers in about two weeks, or
if the shader recompiler's output fails SPIRV-Cross translation broadly (more than a handful of
distinct failures in the first 50 shaders).

Why not B first: its cost is front-loaded and certain, and what it buys (a proven Bloodborne
path) is title-specific, while this project's titles are Uncharted. Why not A alone: it may reduce to
rewriting the buffer cache and a number of adapter features that B already has.

## Risks

- **Bus factor and stability.** One vendor (`gs-soft-it`), a single public snapshot (`SOURCE_SNAPSHOT.json`
  records an export from uncommitted changes), an iOS-first preview with "coming soon" Mac. The clone's `source/tests`
  holds only a cull-distance and a tessellator host test (its CMake names more test sources that are not in the snapshot). We would
  own a fork on day one.
- **Licence by notice, not by header.** See the licence section; ask for SPDX before vendoring.
- **GPL-2.0-only combination.** Fine for our tree; if upstream sync of `gpu/` ever wants "or later",
  the adapter files cannot supply that grant.
- **Silent failure mode.** Missing entry points are null in the dispatch table, so a code path we
  did not trace crashes instead of failing at device creation. Mitigate by a startup check that
  every function in the needs list resolves.
- **Performance unknown.** Software tessellation, CPU-visible shared memory and one heap are
  adapter choices; none was measured. No game files were used, so no real shader was translated.
- **Headers.** I built against Vulkan 1.4.363, not lumen's pinned headers; no incompatibility
  showed, but the adapter has not been tested against the exact set it was written for.

## What I did not verify

- Whether any `gpu/` object compiles or links against the adapter (cmake stops at Boost).
- Any graphics pipeline, SPIR-V to MSL translation, swapchain presentation, descriptor update or
  push-descriptor use on the adapter. Only transfer commands and fences ran.
- The status of per-capability SPIR-V features in the gap table marked unverified.
- Whether `vkCreateBufferView`-class calls are reachable in our renderer (no direct call found).
- Anything on iOS or in the Simulator.
- Absence of copied code from other Vulkan-on-Metal projects.
- The effort numbers: they are estimates.

## Reproduction

Scratch layout used (deleted at the end): `lumen-emu` clone at the pin, SPIRV-Cross at
`d8e3e2b1`, then

```
SDK=/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk
cmake -G Ninja SPIRV-Cross -DSPIRV_CROSS_STATIC=ON -DSPIRV_CROSS_EXCEPTIONS_TO_ASSERTIONS=ON \
  -DSPIRV_CROSS_CLI=OFF -DSPIRV_CROSS_ENABLE_TESTS=OFF -DSPIRV_CROSS_ENABLE_HLSL=OFF \
  -DSPIRV_CROSS_ENABLE_C_API=OFF -DCMAKE_BUILD_TYPE=Release -DCMAKE_OSX_SYSROOT=$SDK && ninja -j3
for f in MetalVulkanCore MetalCullDistance; do clang++ -x objective-c++ -std=gnu++17 -fobjc-arc -O1 \
  -DLUMEN_APPLE_NATIVE_METAL=1 -isysroot $SDK -IMetal -ISPIRV-Cross -c Metal/$f.mm; done
clang++ ... -fno-rtti -fno-exceptions -DSPIRV_CROSS_EXCEPTIONS_TO_ASSERTIONS -c Metal/MetalVulkanCommands.mm
ar rcs libvulkan-1.a *.o
clang++ smoke.o libvulkan-1.a libspirv-cross-{msl,glsl,core}.a -framework Metal -framework Foundation \
  -framework QuartzCore -framework CoreGraphics -framework IOSurface -o smoke
```
