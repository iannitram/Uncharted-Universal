# Lumen Metal adapter probe (#408)

Status: **route A chosen for #408 (2026-11-24, ADR 0006 amendment); no lumen code is vendored yet.** Owner ruling 2026-11-24:
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

## Independent verification (2026-11-24, two Opus reviewers)

Two separate reviews re-checked this probe against the adapter source at pin `6127cba8` and against our `gpu/`. One re-derived what our renderer needs; the other spot-checked 21 adapter claims (all 21 held). Their corrections change the estimate and the order of work:

**New hard gaps (block a first frame):**
- **Per-instance vertex input is rejected outright,** not just the divisor: any `inputRate != VERTEX` returns `VK_ERROR_FEATURE_NOT_PRESENT` (`MetalVulkanCommands.mm:4465-4466`, tessellation path `:2616`), and bindings ≥ 16 are refused (`:4479`). Our pipelines use per-instance bindings for step-rate fetch (`vk_graphics_pipeline.cpp:477-489`, `vk_draw_prep.cpp:161-167`), so every instanced pipeline fails and its draws are skipped (`vk_graphics_pipeline.cpp:443-446`). About 3–5 days in the adapter.
- **Metal binding-slot limits:** the adapter binds directly to Metal slots, not argument buffers (`MetalVulkanCommands.mm:2302-2328`). It reports 29 storage buffers, 16 samplers and 8 storage images per stage (`MetalVulkanCore.mm:1605-1609`), and vertex buffers share that slot table. Our recompiler allows 40 buffers and 64 images per shader (`shader_recompiler/resource.h:15-17`). How often real shaders exceed this is unmeasured. Argument buffers or slot remapping: 5–15 days.
- **RectList and QuadList always go through tessellation** (`vk_graphics_pipeline.cpp:92-96`, `:195-215`). On the adapter that is the software tessellator, which is on the path of ordinary full-screen passes.
- **`vkCmdSetLineWidth` ≠ 1.0 fails the whole command buffer** (`MetalVulkanCommands.mm:6129` → `RecordError` → `vkEndCommandBuffer` error → our `ASSERT_MSG`). We set it on every draw (`vk_rasterizer.cpp:5345`). Clamp it: 0.1 days. Recording errors in general need to be survivable.
- **Mutable-format views are narrower than Vulkan allows:** depth↔colour and cross-family BC views are refused, and `minLod != 0` is refused (`MetalVulkanCore.mm:901-916`, `:3144-3146`). Our texture cache makes every image `eMutableFormat` (`texture_cache/image.cpp:234`). Effort unknown.
- **The sparse arena reaches into shader code:** `GetSparsePageShift()` feeds the pipeline cache and the shaders' `get_bda_pointer` page table (`vk_rasterizer.cpp:238`, `vk_pipeline_cache.cpp:306`, `spirv_emit_context.cpp:1213-1240`). Replacing it touches the recompiler and the fault manager too, and the `BufferCache` constructor calls `vkGetDeviceBufferMemoryRequirements` (`buffer_cache.cpp:154`) before any draw. So the sparse replacement is a **prerequisite**, not a parallel fallback condition.
- **Shader capabilities the adapter reports false:** `GroupNonUniformQuad` (when `uses_group_quad`), `StorageImageReadWithoutFormat` (always, for storage images), `DemoteToHelperInvocation` (on discard). Subgroups are reported for compute and fragment only (`MetalVulkanCore.mm:1786-1790`).
- **Vulkan 1.3 properties read as zero** (no handler for `VkPhysicalDeviceVulkan13Properties`). So `GetMaxBufferSize()` is 0 and the subgroup-size checks read zeros. Unknown feature structs in `vkCreateDevice` are silently ignored (`MetalVulkanCore.mm:1908-1952`), so a missing `nullDescriptor` only shows up at the first null binding.

**Rows corrected downward:**
- `vkCmdUpdateBuffer`: either not reached, or reached only through `Runtime::InlineData`, which writes one u32. Use `vkCmdFillBuffer` either way: about 0.1 days.
- Buffer views: no texel-buffer descriptors exist in our tree: 0 days.
- Debug-utils labels: the calls are gated by `BB_VK_MARKERS` (default off): soft.
- Geometry shaders: ES/GS pipelines are skipped with a warning (missing geometry, not a crash).
- `vkGetDeviceImageMemoryRequirements` is a separate row (texture-cache allocation fallback, `image.cpp:131-147`): about 0.5 days, or guard the call. VMA at 1.3 also wants the `vkGetDevice*MemoryRequirements` pair.

**Revised estimate:** A rises from 30–55 to roughly **40–70 days**, which overlaps B's 40–80. "C entered through A" is no longer clearly cheaper; it is a bet that pays off only if the decisive unknowns come out small.

**Revised order (replaces the plan above):**
1. **Cheap decisive tests first (days, no renderer port):**
   - run SPIRV-Cross MSL (pin `d8e3e2b1`) offline over our recompiler's SPIR-V;
   - count per-stage buffer, image and sampler bindings against the 29/16/8 limits;
   - count shaders needing quad subgroups, read-without-format or demote.

   These need real shader SPIR-V from a game run. Keep it local and report aggregate counts only (M9).
2. Device creation on the adapter (assert patches, 1.2 version path, 1.3 properties).
3. Buffer cache without sparse, behind a two-week stop rule.
4. First pipelines and draws: instance-rate input, line width, survivable recording errors, mutable views.

**Stop rules (fall back to B, the `renderer_metal` reconcile):**
- step 1 shows more than a handful of the first 50 pipelines over the slot limits, or SPIRV-Cross failing broadly;
- instance rate and the binding limits can't be fixed in the adapter within about a week;
- the sparse replacement runs past about two weeks.

## Step 1 results: measured shader counts (2026-11-24)

These are aggregates only, from local runs on the Windows 3090 machine, reported by the Uncharted - Windows session. No shader bytes, MSL text or file names left that machine (M9).

| Item | Value |
| --- | --- |
| Runs | U1: menus and the opening cutscene. U2: a save loaded into gameplay, then 2 minutes of play. U3: not measured. |
| Tool | `BB_DUMP_SHADERS` dumps, `spirv-cross` `d8e3e2b1 --msl --msl-version 30000` (macOS platform) |
| Pipelines (graphics / compute) | U1: 88 (69 / 19). U2: 262 (233 / 29). |
| Distinct modules | U1: 123. U2: 364. |
| MSL pass as dumped | U1: 70. U2: 206. |
| The one failure | `PerVertexKHR` decoration (NVIDIA-shaped profile). Every module passes once it is removed. |
| Pipelines over 29 buffers / 16 samplers / 8 storage images | 0 / 0 / 0 in both games |
| Peak bindings per stage (U2, max / p95) | Vertex: 9 / 8 buffers. Fragment: 4 / 2 buffers, 12 / 10 textures, 4 / 3 samplers. Compute: 14 / 11 buffers, 1 / 1 storage image. |
| Instance-rate vertex input | 0 of 69 and 0 of 233 graphics pipelines |
| Topology | QuadList: U1 25, U2 29. RectList: 0. Everything else is TriangleList. |
| Features (modules using each) | Demote: 3 / 15. Quad subgroup: 2 / 2. Other subgroup ops: 4 / 6. `StorageImageReadWithoutFormat` declared but never used. `Int64` in every module, with no 64-bit atomics. `Float64`, `Int64Atomics`, `AtomicFloat32MinMax`, `StencilExport`: 0. |

**No stop rule trips.** No pipeline goes over a slot limit, the converter fails in one way only and that has a known fix, and the instance-rate and slot-limit week is no longer needed.

## Second verification round (four investigators, one adversarial critic)

**Corrections to this note:**
- **QuadList does not use the software tessellator.** The adapter builds the tessellator and capture path only when the TES writes `CullDistance` (`MetalVulkanCommands.mm:2568`, `:2683-2716`). Our aux TES does not, so QuadList goes through native `drawPatches` (`:3596-3602`). This replaces the RectList/QuadList row above.
- **The cost moves elsewhere.** A tessellated draw on the adapter ends the render pass once per instance and runs 2–5 compute passes and seven fresh buffer allocations per draw (`:3411-3605`). Indirect draws on a tessellation pipeline are rejected (`:5100-5101`).
- **Route B has no QuadList support either.** `bloodborne-runtime` skips quad-list draws (`renderer_metal/mtl_pipeline_build.mm:234-235`).
- **The adapter is not macOS-only.** lumen-emu ships an iOS app that links the same adapter. Its files compile for `arm64-apple-ios15.0`/`16.0` with 0 errors and 0 warnings (compile only; nothing was linked).
- **The measured `PerVertexKHR` failures come from the NVIDIA profile.** Under the adapter, manual interpolation is off, so only `V_INTERP_MOV` marks inputs PerVertex. With barycentrics reported off, the P10/P20 reads assert instead (`vector_interpolation.cpp:104-106`), so that path needs counting.

**New cost items for A:**
- **Convert QuadList to an indexed TriangleList in our renderer: 2–5 days.**
  - Non-indexed draws use a fixed pattern buffer.
  - Indexed draws expand quads to triangles (port of Huax-1839/bloodborne-macos `mtl_index_expand`, GPL-2.0-or-later).
  - Indirect draws need a small pre-pass.
  - It runs on the Windows Vulkan build too, so it can be compared there first.
- **Metal-shaped shader profile: 3–8 days.**
  - Barycentrics off.
  - Set `ios_use_simdgroup_functions`, otherwise subgroup ops turn into quad ops on iOS.
  - The adapter reports zero 1.3 properties and a default driver ID.
  - Tail risk: 5–15 days if P10/P20 reads on non-flat attributes are common.
- **The adapter compiles shaders at MSL 2.3, not 3.0.** The gates for Int64, demote and the other measured features are all at 2.3 or below, but the counts above were taken at 3.0 with the macOS platform.
- **iOS memory: 12–25 days.** There is no zero-copy guest import, the adapter allocates twice, and there is no memory budget. B needs about 5–10 days, because the peer already runs its renderer on an iPhone with a free Apple ID.
- **Implied minimum iOS device:** Apple7 (A14) or newer.

**Revised estimate:**

| | Mac | Mac + iOS |
| --- | --- | --- |
| A | 37–65 days (42–80 if the P10/P20 tail lands) | 49–90 |
| B | 42–85 (adds QuadList) | 47–95 |

**Recommendation from the round:** route A, Mac first, still entered through cheap tests.
- **Biggest risk:** the sparse-arena replacement. It is a prerequisite, it reaches the recompiler's page table, and it must be designed with iOS host-memory import in mind.

**Next steps (replaces step 1 of the revised order):**
- **1a.** Re-dump with a forced Metal profile (barycentrics off, non-NVIDIA driver). Count P10/P20 reads on non-flat attributes, and rerun SPIRV-Cross at MSL 2.3 for `--msl` and `--msl-ios` (with and without simdgroup functions). About half a day, local, aggregates only.
- **1b.** A 2–3 day spike on the 3090: an arena of plain buffers with a fixed page shift, running the same U1 menus and U2 save load. The two-week stop rule still applies.

**What could still overturn this:**
- Later content and U3 could bring per-instance input, indirect or GPU-written QuadList index buffers, 64-bit atomics, or shaders over the slot limits.
- Each would push A toward the high end of its range. None favours B, which shares the recompiler and also lacks QuadList support.

Nothing in this round ran on a Mac, an iPhone or through the adapter.

## Step 1a results: Metal-shaped profile (2026-11-24)

These counts are aggregates only, from the Windows 3090 machine. They use the same scenes as step 1, with a cold shader cache.

The profile was forced locally and not committed:
- barycentrics off;
- no AMD explicit vertex parameter;
- manual interpolation off;
- `support_float64=false`;
- `max_shared_memory_size=32768`;
- `subgroup_size=32`.

| Item | U1 | U2 |
| --- | --- | --- |
| Pipelines / modules | 88 / 123 | 262 / 364 |
| `PerVertexKHR` emitted | 0 | 0 |
| V_INTERP_MOV P10/P20 on a non-flat attribute | 0 of 57 fs | 0 of 168 fs |
| SPIRV-Cross `--msl --msl-version 20300` | 123/123 | 364/364 |
| SPIRV-Cross `--msl --msl-ios --msl-version 20300` | 123/123 | 364/364 |
| SPIRV-Cross `--msl --msl-ios --msl-version 20300 --msl-ios-use-simdgroup-functions` | 123/123 | 364/364 |
| Subgroup ops, compute | Broadcast 1, BroadcastFirst 2, ShuffleXor 2 | Broadcast 1, BroadcastFirst 3, ShuffleXor 2 |
| Subgroup ops, fragment | BroadcastFirst 1, QuadBroadcast 2 | Ballot 1, BroadcastFirst 2, QuadBroadcast 2 |
| Subgroup ops, vertex | none | none |
| QuadList draws | 234,442 of about 500,000 draws | 350,231 of about 11.38 M draws |
| QuadList draw kinds | all direct indexed | all direct indexed |

**What this settles:**
- **The P10/P20 tail risk (5–15 days) did not appear.** The profile cost stays at the low end of 3–8 days.
- **MSL 2.3 and iOS conversion pass everywhere.** That includes the adapter's own setting, `ios_use_simdgroup_functions` unset. Setting it is still right, because Ballot and ShuffleXor are only correct with simdgroup functions.
- **No subgroup ops appear in vertex shaders.**
- **QuadList needs indexed expansion only.** There are no indirect or non-indexed QuadList draws, so the conversion is at the low end of 2–5 days.

QuadList is about half of U1's draws in these scenes, so the expansion has to be fast. Two options:
- expand on the GPU;
- expand on the CPU and cache by index-buffer range.

Measure it on the 3090 build before the Mac.

**Caveats:**
- The draw counters were sampled every 20,000 draws, so the tail of each run is missing.
- The aux TCS/TES are still not in the dump. This no longer matters once QuadList is converted.
- There is no per-call count of flat or selector-2 V_INTERP_MOV.
