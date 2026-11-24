# Shared shadPS4 Core Boundary & Delta Inventory (Refs #198)

This document tracks the decoupling boundary between Uncharted-Universal's
runtime/hook layer (`gpu/shim/`, `gpu/bbgpu.h`, `gpu/dlss_bridge/`) and the
shadPS4 graphics/recompiler core (`gpu/shadps4/`, or an external
`iannitram/shadPS4` `gpucore` tree).

For the per-file triage of `gpu/shadps4/` against upstream commit `b4e7ae7`,
see [shadps4-triage.md](shadps4-triage.md) and [../gpu/VENDOR.txt](../gpu/VENDOR.txt).

## Building Against an External shadPS4 Core (`BB_SHADPS4_CORE_DIR`)

`gpu/CMakeLists.txt` exposes two cache variables:

- `BB_SHADPS4_CORE_DIR` (default: `${CMAKE_CURRENT_SOURCE_DIR}/shadps4`):
  Path to the shadPS4 core source tree containing `video_core/`,
  `shader_recompiler/`, `common/`, and `core/libraries/`.
- `BB_SHADPS4_NET_DIR` (default: `${CMAKE_CURRENT_SOURCE_DIR}/shadps4`):
  Path to the `shadnet/` and `core/libraries/{network,np}` co-op sources
  (vendored from `shadp2p`, kept separate from `gpucore`).

Example configure against an external core checkout:

```bash
cmake -S gpu -B out/gpu -G Ninja -DBB_SHADPS4_CORE_DIR=/path/to/shadPS4/src
```

CMake validates at configure time that `BB_SHADPS4_CORE_DIR` contains
`video_core`, `shader_recompiler`, `common`, and `core/libraries`.

## Core-to-Shim Boundary Surface

`gpu/shadps4/` crosses into `gpu/` and `gpu/shim/` through two surfaces,
codified in [../gpu/shim/bbport_core_boundary.h](../gpu/shim/bbport_core_boundary.h):

1. **Host C ABI**: [../gpu/bbgpu.h](../gpu/bbgpu.h)
2. **Shim Hook Headers (`gpu/shim/`)**:
   - `bbport_toggles.h` (runtime optimization disable bitmasks)
   - `bbport_settings.h` (upscaler, frame-gen, low-latency, resolution settings)
   - `bbport_copy.h` (GPU/CPU guest memory copy helpers)
   - `bbport_movie.h` (Videodec2 / NV12 movie scaling & shader key remap)
   - `bbport_threads.h` (thread priority / affinity helpers)
   - `bbport_nan_guard.h` (`BB_NAN_GUARD` export guard)
   - `bbport_focus_fix.h` (aiming autofocus shader center patch)
   - `bbport_video_pacing.h` (60 Hz movie vblank pacing & flip-rate divisor)
   - `bbport_shader_preload.h` (shader cache compatibility identity)
   - `bbport_overlay.h`, `bbport_resize.h`, `bbport_write_log.h`, `bbport_diag.h`
   - `sdl_window.h`, `vertex_input_deny.h`
   - `dlss_bridge/bbport_dlss_bridge.h`
3. **shadPS4-Compatible Platform Shims (`gpu/shim/`)**:
   - `common/{assert.cpp,debug.h,logging/log.{h,cpp},slot_vector_pages.cpp}`
   - `core/{address_space,bloodborne_re,debugger,debug_state,emulator_settings,memory,signals,tls,user_manager,user_settings}.h`
   - `core/file_sys/fs.h`, `core/libraries/libs.h`
   - `imgui/{invitation_prompt_layer,shadnet_notifications_layer,renderer/imgui_core,renderer/texture_manager}.h`
   - `tracy/{Tracy,TracyVulkan}.hpp`

## Verification

- `ninja -C out/gpu shim-boundary-test && ./out/gpu/shim-boundary-test`:
  verifies `bbport_core_boundary.h` and `bbgpu.h` ABI/helper invariants.
- `python3 -m unittest tests/test_core_delta.py`:
  verifies `gpu/CMakeLists.txt` has no hardcoded `shadps4/` core paths,
  `gpu/shadps4/` has no `../../../` relative includes escaping the core tree,
  and all shim includes from `gpu/shadps4/` are in the boundary allow-list.
