# Synthetic Metal probe results (#408, feasibility plan step 4)

`probe.m` is authored here from the Metal documentation: no game data, no peer code. It creates a
device, compiles an MSL vertex, fragment and compute shader at run time (so no Metal Toolchain
component is needed), renders a textured triangle to an offscreen RGBA8 target, uploads an RGBA8
and a BC1 sub-rectangle through a staging buffer (the shape of a button-prompt patch upload),
reads each back and compares a CRC-32 with a value computed outside the program, then runs the
no-copy buffer probe: `newBufferWithBytesNoCopy` over an `mmap`'d scratch file, written by a
compute kernel and checked word by word from the mapping. On iOS it logs `os_proc_available_memory`
before and after. One JSON record is printed (device name, OS, Xcode and clang versions, per-check
status, result).

Expected CRCs (Python `zlib.crc32` over the byte layouts in the comments in `probe.m`):
triangle 64x64 `0x9c3590c2`, RGBA8 sub-rect `0xa63d7215`, BC1 sub-rect raw `0x11dddd28`, BC1
decoded by sampling `0xe5dd0b95`.

Run: `sh tests/apple_metal/run.sh macos [report.json]` (exit 0 on pass or on a reported skip when
there is no Metal device, 1 on a failed check). `run.sh build-sim` only compiles for the iOS
Simulator. `run.sh sim` would create a device named `UU-metal-probe`, run there and delete it.

| Run | Target | Device | Result |
| --- | --- | --- | --- |
| macOS, local | arm64-apple-macos13.0 | Apple M1 (unified memory, BC supported, 16 KiB pages) | **pass**: setup, triangle, RGBA8 sub-rect (blit readback and sampled), BC1 sub-rect (raw and decoded), 4 MiB no-copy buffer (0 mismatches, `contents` equals the mapping) |
| iOS Simulator | arm64-apple-ios16.4-simulator | none | **not run: simulator paused on the shared machine** (coordinator instruction, 2026-11-24). The build step compiles and CI compiles it; a first attempt hit "Domain is tearing down" when the simulators were shut down. |
| macOS runner, CI | arm64-apple-macos13.0 | recorded by `apple-gpu.yml` | not yet run (first run is this PR's check); a runner is a VM with a paravirtual GPU, so its result is not a hardware result |

Date: 2026-11-24. macOS 27.0.1 (26A434), Xcode 27.2 (27B52), Apple clang 21.0.0 (clang-2100.4.1.1),
8 GB Apple-silicon Mac mini.

What this does and does not show. It shows that on this Mac the Metal calls a native renderer
needs for ordinary textures, BC1 sub-rect uploads, runtime shader compile and a no-copy buffer over
a file mapping work, and that the readback matches hand-computed values. It does not show anything
about iOS (no simulator or device run; the simulator has no real GPU anyway, and
`os_proc_available_memory` is iOS-only), about memory headroom, about the iOS BC1 case (iOS
devices may report `supportsBCTextureCompression` false; the probe skips with a message), or about
any game.
