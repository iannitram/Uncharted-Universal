# Apple feasibility gate (#211, piece 8.01)

Status: **partial; evidence from source review and synthetic CPU tests run on an Apple-silicon Mac.** No game dump, executable, generated C, annotation, signature or offset table was read, built or copied. Nothing here is a claim about Uncharted coverage, game readiness or device readiness. ADR 0006 holds the architecture decision and the earlier plan-only assessment; this file supersedes its "not executed" limitation for the bbcpu CPU tests only.

## Source revisions reviewed (read only; no code imported)

| Peer | Revision | Root licence | Reviewed |
|---|---|---|---|
| bloodborne-recomp | `c60ccf8be752b8690f2813505c4daf9eafdb136b` | GPL-2.0 (`LICENSE`) | `tools/recomp/recomp.sh` L1-36 |
| bbcpu | `3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca` | GPL-2.0 (`LICENSE`); `src/` carries no per-file SPDX | `src/cpu/jit_arm64.c` L25-35, `tests/test_bbcpu.c` L1-40, build and test scripts |
| bloodborne-ios | `8f08be42c881c85051f2076c3721dbb2e273b025` | GPL-2.0 (`LICENSE`) | `app/app_main.c` L1-70, `app/metal_test.m` L1-60 |
| bloodborne-runtime | `42736cf7` (frozen link) and `23400676` (checked-out head, covered by the 11-11 survey) | GPL-2.0 (`LICENSE`) | `mtl_device.mm` L1-3, `gpu/` tree licences |

No notice is carried because no source is copied. If any file is later adopted, retain its SPDX line and the GPL text, and record the file in the allowlist below first.

## What the source shows

- `recomp.sh` is an offline pipeline: a generator (`bbrecomp.c`, linked with bbcpu's decoder/interpreter) reads a game ELF and a function list, writes C, and clang compiles it to a dylib the game loads. Its inputs are an extracted game ELF, a scanner-built function table and a per-title annotations file. The scripts are generic; the inputs are the game-specific part. For us the generated library would be built only on the owner's machine from the owner's dump and never distributed.
- bbcpu's no-JIT guard (`jit_arm64.c` L25-35) is a compile-time switch: on `TARGET_OS_IPHONE` it sets `BB_NO_JIT`, stubs `pthread_jit_write_protect_np`, and leaves uncovered guest code to the interpreter. At runtime, `BB_JIT=0` selects the interpreter on macOS. The guard shows intent; the synthetic run below shows the interpreter path works on macOS arm64. It does not show an iOS build runs without JIT.
- `tests/test_bbcpu.c` is a differential test for **x86-64 hosts** (Mach-O sections, `immintrin.h`); it does not build on this arm64 Mac. The arm64 equivalent is `tools/cpu/test_jit.sh`, which builds an authored x86-64 guest, takes expected output from native execution under Rosetta, then compares interpreter and translator results.
- The iOS app (`app_main.c`) is a proof-of-concept shell around a Bloodborne probe: SDL3 app, game folder in Documents, a pre-linked `librecomp.dylib` in Frameworks, interpretation for the rest. Its layout and probe are Bloodborne-specific. `metal_test.m` is generic in itself: it maps a file at a fixed address, wraps it in a no-copy `MTLBuffer`, has the GPU fill it and logs available memory. This is a useful pattern for our memory gate; its fixed guest address is a Bloodborne assumption.
- The Metal renderer is vendored shadPS4 code (GPL-2.0-or-later, `gpu/VENDOR.txt`) adapted with `bbport:` markers. It is a PS4 GCN-to-Metal renderer. Whether our collection's draw paths fit it is untested.

## Synthetic execution recorded here

Target: `arm64-apple-darwin27.0.0` (Apple silicon Mac, 8 GB); toolchain Apple clang 21.0.0 (clang-2100.4.1.1), Xcode toolchain, cmake and ninja from Homebrew; Zydis v4.1.1 fetched by bbcpu's `build.sh`. Run on a scratch copy of bbcpu `3f9b6c1` under `/tmp`.

| Command | Result |
|---|---|
| `bash build.sh` (`-Wall -Wextra -Werror`) | exit 0; `libbbcpu.a` and the `hello` example built |
| `tests/test_a64.c` built with `clang -arch arm64`, run | prints `a64: ok`, exit 0 (emitter encodings agree with the system assembler) |
| `bash tools/cpu/test_jit.sh` | exit 0. Interpreter (`BB_JIT=0`): 0 of 5000 cases failed. Translator: 0 of 5000 cases failed. Fault-watch test PASS. Hook/native-call test PASS in both modes |
| `BB_JIT=0 out/hello` | interpreter mode, authored guest returns 42, exit 0 |

What this proves: on a macOS arm64 host, bbcpu's interpreter and translator match native results on its own authored corpus (5000 random cases each). The Rosetta-assisted comparison is macOS-only.

What it does **not** prove: anything on iOS (no iOS target, no signing, no device was used); behavior with JIT forbidden by the OS rather than by an environment switch; AOT correctness (`recomp.sh` was not run: it needs a game ELF); Metal. **The synthetic Metal probe was not executed** in this run; it remains to be authored and run (see the plan).

## Go / no-go

| Game | macOS Apple silicon | iPhone / iPad |
|---|---|---|
| Uncharted 1 | **No-go to claim or implement support.** Go to run the synthetic plan and owner checks. | Same |
| Uncharted 2 | Same; independent evidence required | Same |
| Uncharted 3 | Same; independent evidence required | Same |

Established from source and synthetic tests alone: the bbcpu interpreter and translator work on a macOS arm64 host for authored code; the build is self-contained (clang, cmake, ninja, Zydis); the no-JIT fallback exists in source. Not established: any coverage, ABI fit, memory headroom, renderer fit or signing outcome for any Uncharted title or any iOS device. A result for one title does not transfer to another.

## Gates

1. **Coverage** (per title, owner-run): fraction of executed functions the AOT path covers, and whether the interpreter can serve the remainder at playable speed. Unsupported-instruction behaviour must be a clean refusal, not silent wrong results.
2. **ABI**: bbcpu and the renderer are C/C++/ObjC. They must sit behind a narrow C ABI called from our Rust runtime (SPEC M20); the generated-library entry points, host-call bridge, TLS, callbacks and thread model must be expressible without per-title patches in the shared layer. Bloodborne-specific fixed addresses (guest ranges, the `0x7020000000` mapping) must become configuration.
3. **Memory**: iOS per-app limits and no-copy GPU mapping measured per device class with `os_proc_available_memory`, in boot, load, sustained play, memory pressure and suspend/resume. Do not reuse any Bloodborne figure.
4. **Licence**: see allowlist. Linking GPL code into the runtime makes the distributed Apple build GPL-governed. **Resolved 2026-11-16 (owner, recorded in #396):** asked whether the Apple build may be GPL-governed, the owner answered "Yes, GPL is fine (Recommended)": "may adopt the allowlisted GPL-2.0 files (never bbnet20's GPL-3.0 code), with SPDX lines and notices kept." App Store distribution of GPL code is a separate legal question to be settled before choosing a route; this document gives no conclusion on it.
5. **Signing** (verify against current Apple documentation and measured behaviour): macOS Hardened Runtime needs `com.apple.security.cs.allow-jit` for any runtime translator; iOS provides no JIT for ordinary apps, so the shipped path must be AOT library plus interpreter, with the library an ordinary build-time app input. Install is by the owner's own provisioning (Xcode signing over a cable). Expiry, entitlements for extra memory and any distribution route are open.

## Legal allowlist (what tests link; nothing vendored)

**Update 2026-11 (#396):** the components in the table below, and the bloodborne-recomp files `tools/recomp/scan.c`, `tools/recomp/bbrecomp.c` and `tests/fuzz_recomp.c` at `c60ccf8be752b8690f2813505c4daf9eafdb136b`, are now vendored under `apple/vendor/` (ADR 0006, owner ruling of 2026-11-16) with a `VENDOR.txt` each; the record with revisions is the "Imported code" table in `PEERS.md`. `packaging/licence_check.py apple` fails the build when anything GPL-3.0 or `bbnet20` appears under `apple/`. The list below is the original review and still governs what may be added next.

Eligible after a per-file review and a recorded revision: bbcpu `src/cpu/*` (root GPL-2.0; confirm the intended "or later" terms with the author before mixing with GPL-2.0-or-later files), bbcpu generic tests and `tools/cpu` scripts, `tools/recomp/*.sh` and `bbrecomp.c` as recipes (without the annotations they consume), `bloodborne-ios` `metal_test.m` pattern, and renderer files marked `SPDX-License-Identifier: GPL-2.0-or-later` under `gpu/shadps4`, `gpu/shim`, `gpu/layer`, `gpu/cp`, and `gpu/bbnet` (all files listed by SPDX; vendored shadPS4 core is GPL-2.0-or-later).

Per-component licences found in the runtime `gpu/` tree, each to be re-checked file by file before use: `gpu/shadps4` (GPL-2.0 text), `gpu/third_party/sirit`, `half`, `httplib`, fonts (Cinzel, DejaVu, Inter each with its own licence file), `gpu/dlss_bridge` (MIT), `gpu/fsr4_metal` (mixed MIT and GPL-2.0-or-later headers). Submodules `fsr-vulkan`, `imgui` and `LibAtrac9` are empty in the checkout and unreviewed; their licences must be enumerated from their own revisions.

**Excluded:** `gpu/bbnet20` (carries `LICENSE.GPL-3.0` and some GPL-3.0-or-later files, plus vendored content), and the dlss bridge and any proprietary-SDK-linked piece unless separately cleared. **Never copy:** `annotations/` (name tables for a specific title), offset/signature tables, generated C, patches, or any game-derived data. Keep only authored synthetic fixtures.

Linked by the #212 test binary (built from a checkout or clone at the pinned revision, nothing copied into this repo; the SPDX lines and licence texts stay with those checkouts, and vendoring with notices is #396):

| Component | Revision | Licence | Use |
| --- | --- | --- | --- |
| bbcpu `src/cpu/*.c`, `hostcall.S`, `bbcpu.h` | `3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca` | GPL-2.0 (root `LICENSE`) | interpreter linked into `tests/apple_nojit` |
| Zydis (with bundled Zycore) | tag `v4.1.1` | MIT | x86 decoder bbcpu needs |

The harness files in `tests/apple_nojit/` are authored here and include no bbcpu code; the test binary as a whole is GPL-2.0 when linked. The bbnet20 GPL-3.0 component is not used.

## Synthetic no-JIT coverage plan (for #212)

1. Reuse the bbcpu pattern, not its Bloodborne inputs: authored guest functions covering integer and flag ops, shifts/rotates, wide multiply/divide, branches, calls/returns, unaligned access, string ops, SSE/AVX and x87 subsets, atomics and ordering, host-call and callback nesting, TLS and faults. Expected state comes from native execution, never from a game.
2. Run every case with `BB_JIT=0` on macOS (done above for bbcpu's own corpus; repeat for ours).
3. Build the same corpus into an iOS-simulator and then a device test target with `BB_NO_JIT` and no JIT entitlement; the AOT output for the authored guest is an ordinary linked library. Record Xcode, SDK, OS and device.
4. Synthetic Metal: create a device, render a known triangle and texture to an offscreen target, read back a checksum, tear down; then the no-copy buffer memory probe on authored data. Record device, OS and result.
5. Record only aggregate pass/fail, tool versions and targets.

## Owner-run checklist, per title (U1, U2, U3), Mac first then each intended iPhone/iPad class

Use only the owner's dump and devices. Keep generated code and traces local; report aggregates only.

- [ ] Dump accepted by the revision gate; no fingerprints or tables persisted in the repo.
- [ ] AOT coverage measured; interpreter-only remainder is acceptable; results match the synthetic oracle and local runs.
- [ ] Imports, threads/fibers, callbacks, unwind, TLS, file and save behaviour inventoried; C ABI serves the Rust runtime without title patches in the shared layer.
- [ ] Peak address space, committed memory and GPU resources measured through boot, load, save and a sustained session, then under memory pressure and suspend/resume.
- [ ] Metal renderer: required shader and command paths, targets, formats, sync, presentation and device-loss recovery exercised on the real title.
- [ ] Controller, audio, files, saves, lifecycle interruption, low-memory termination and resume work; a representative session completes.
- [ ] Signing/provisioning flow and licence terms confirmed for the chosen install route.
