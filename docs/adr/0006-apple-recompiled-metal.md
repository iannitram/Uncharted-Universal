# 0006: Mac, iPhone and iPad run a recompiled library drawn with Metal

**Decided:** 2026-11-01, by the owner: 1.0 devices "Full TotK parity", then the spec approval.

## Decision

Adopt bloodborne-native's route (bloodborne-runtime, -mac, -ios; GPL-2.0 per their headers, licence files checked at piece 8.01 before code comes in, credited): on the Mac, the launcher recompiles each game's x86-64 code from my dump into a native ARM64 library (no JIT, so the same library serves iOS), and the game draws through their Metal renderer. Their code is vendored under `apple/` and stays in its language; our Rust runtime builds for `aarch64-apple-darwin` and `aarch64-apple-ios`. The Mac launcher builds the iOS app and installs it over a cable with Xcode signing.

## Risks

- This is engine work for us, unlike totk-native: the recompiler and renderer are written for Bloodborne, and the collection is three executables. 8.01 measures how much is game-specific before 8.03 starts.
- iOS signing, provisioning, executable-memory policy and device memory limits are release gates; verify current Apple requirements and measured device behavior before selecting a distribution route.

## Feasibility assessment (#211, partial plan only; 2026-11-06)

The architecture decision above is a target, not a feasibility finding. This issue has completed source review and written a synthetic test plan only. The CPU and Metal synthetic probes below were **not executed**, so #211 remains partial and the Apple feasibility gate is not complete. This review does **not** establish Uncharted support or justify starting a title-specific port. Status for all three games and both Apple targets is **no-go for implementation or compatibility claims; go only to execute the bounded synthetic and owner-run evaluation below**. There is no public evidence in this review that distinguishes U1, U2 or U3's CPU coverage, system-call needs, memory use, or graphics behavior on Apple. The unknowns must be resolved independently for each title; success on one is not transferable to another.

**Execution limitation:** this review ran in a Linux environment without Xcode or an Apple device. No synthetic CPU interpreter, translator, or Metal test was run. The cited peer tests and probes were inspected as source references only.

| Game | macOS Apple silicon | iPhone / iPad | Evidence and condition to reconsider |
|---|---|---|---|
| Uncharted 1 | No-go to claim or implement support | No-go to claim or implement support | The cited AOT, interpreter and Metal sources are Bloodborne-specific. Proceed only after the owner-run U1 checks below establish useful AOT coverage, runtime ABI fit, memory headroom and renderer requirements on each target. |
| Uncharted 2 | No-go to claim or implement support | No-go to claim or implement support | Same evidence boundary. Its separate executable and behavior require its own owner-run checks; U1/U3 results cannot substitute. |
| Uncharted 3 | No-go to claim or implement support | No-go to claim or implement support | Same evidence boundary. Its separate executable and behavior require its own owner-run checks; U1/U2 results cannot substitute. |

“No-go” here means the inspected evidence is insufficient to approve implementation or advertise compatibility; it does not assert technical impossibility. The narrow “go” is for authored synthetic probes and later evaluation using only the owner's own device and dump. No dump, dump-derived table, annotation, generated game code, signature, offset, or asset belongs in this repository, CI, issue, or report.

### What the sources establish

- The frozen [AOT entry script](https://github.com/bloodborne-native/bloodborne-recomp/blob/c60ccf8be752b8690f2813505c4daf9eafdb136b/tools/recomp/recomp.sh#L1-L36) is a build path for Bloodborne. It does not demonstrate discovery or correct recompilation of any Uncharted function.
- The frozen [bbcpu ARM64 translator](https://github.com/bloodborne-native/bbcpu/blob/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca/src/cpu/jit_arm64.c#L25-L35) has an iOS no-JIT guard; the [CPU test](https://github.com/bloodborne-native/bbcpu/blob/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca/tests/test_bbcpu.c#L1-L40) is a useful pattern for authored input and expected-state tests. Neither source proves Uncharted instruction coverage or a production-quality interpreter fallback.
- The frozen [iOS app entry](https://github.com/bloodborne-native/bloodborne-ios/blob/8f08be42c881c85051f2076c3721dbb2e273b025/app/app_main.c#L1-L70) and [Metal probe](https://github.com/bloodborne-native/bloodborne-ios/blob/8f08be42c881c85051f2076c3721dbb2e273b025/app/metal_test.m#L1-L60) establish that an app shell and a synthetic Metal experiment exist. Their paths, layout, and application assumptions are not the Uncharted contract.
- The frozen [Metal device code](https://github.com/bloodborne-native/bloodborne-runtime/blob/42736cf7ca7704b087e847482417d9f475bb2dd7/gpu/shadps4/video_core/renderer_metal/mtl_device.mm#L1-L15) establishes a macOS Metal implementation exists in that tree. It does not establish equivalent iOS coverage or renderer compatibility with this collection.
- Apple documents that JIT-created writable/executable memory on macOS requires the relevant entitlement under Hardened Runtime, and that third-party apps on iOS require authorization by a provisioning profile. These are signing and deployment gates to verify with the actual app distribution plan, not evidence that this architecture will pass review. See [JIT entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.cs.allow-jit) and [provisioning profile model](https://developer.apple.com/documentation/technotes/tn3125-inside-code-signing-provisioning-profiles).

### Synthetic no-JIT plan

Before any device/game evaluation, build a standalone authored-instruction corpus with expected register, flags, memory and control-flow states. Run each case through the interpreter and, on macOS only, any optional translator used as a differential reference. Cover integer and flag operations, branches, calls/returns, unaligned access, exceptions, atomics/memory ordering and unsupported-instruction refusal. The corpus must contain no copied bytes or instruction sequences from a commercial executable.

For the no-JIT path, execute the same corpus in an iOS test target with JIT disabled and no executable-memory entitlement. Include a small authored ELF/loader fixture, callback and thread/TLS cases, failure paths, and checks that generated ARM64 libraries are ordinary build-time app inputs rather than code emitted at runtime. Run a synthetic Metal test that creates a device, renders a known triangle/texture to an offscreen target, reads back a checksum, and exercises resource teardown. Record the Xcode version, SDK, device/OS, build target and test result. This proves only the synthetic seams.

### Owner-run checks before a go decision

Perform these checks separately for U1, U2 and U3, first on an Apple-silicon Mac and then on each iPhone/iPad class that is intended for 1.0. Use the owner's own dump and devices. Keep generated code and detailed traces local to that machine; report only aggregate pass/fail, tested OS/device class, and non-identifying counters.

1. Confirm the dump and revision gate accepts the owner's intended revision without persisting executable-derived fingerprints or title tables in this repository. Measure which executable functions can be covered by the proposed AOT workflow and whether the remaining interpreter path is sufficient. Validate AOT results against the synthetic oracle and local execution; do not treat SFO version text as coverage evidence.
2. Inventory runtime imports, thread/fiber behavior, callbacks, exception/unwind needs, TLS, file and save behavior for the title. Confirm the proposed C ABI can serve the Rust runtime without per-title patches entering the common layer.
3. Measure peak address-space use, committed memory and GPU resource use during boot, loading, saves and a sustained session. Establish headroom on the Mac and each mobile device. Repeat after memory pressure and suspend/resume. Do not assume the Bloodborne iOS memory figure applies to this collection or current devices.
4. Exercise the Metal renderer with authored probes first, then the owner's game. Check each game's required shader/command paths, render targets, synchronization, formats, presentation and recovery from device loss. A device-creation probe alone is not renderer compatibility.
5. Validate controller mapping, audio, file access, saves, lifecycle interruptions, low-memory termination and resume. Each game must independently complete a representative session before a broader campaign plan.
6. Build and install on a physical iPhone/iPad with Xcode using the intended signing method and cable workflow. Verify app identifier, signing certificate, provisioning profile/device authorization, required capabilities, install/launch, sandbox storage and update behavior. For App Store distribution, verify the current review and entitlement requirements at that time. Do not add JIT or unsigned-executable-memory entitlements to make the test pass; such a result would violate the approved no-JIT path.

A future go decision requires documented evidence for each title/target pair, no-JIT synthetic coverage, an audited source/license allowlist and owner approval. If any title needs runtime JIT on iOS, requires retained game-derived tables, exceeds measured memory limits, or cannot meet the signing/distribution route, that target remains no-go and the scope must return to the owner.

### Source and license boundary

The [bloodborne-recomp](https://github.com/bloodborne-native/bloodborne-recomp/tree/c60ccf8be752b8690f2813505c4daf9eafdb136b), [bbcpu](https://github.com/bloodborne-native/bbcpu/tree/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca), [bloodborne-runtime](https://github.com/bloodborne-native/bloodborne-runtime/tree/42736cf7ca7704b087e847482417d9f475bb2dd7), and [bloodborne-ios](https://github.com/bloodborne-native/bloodborne-ios/tree/8f08be42c881c85051f2076c3721dbb2e273b025) repositories report GPL-2.0 roots. Reviewed runtime Metal files are marked GPL-2.0-or-later. Before any source import, preserve the exact file notices and audit the selected files' complete dependency/submodule closure. The runtime's `gpu/bbnet20` is explicitly GPL-3.0 and excluded from this GPL-2.0 project. No blanket tree import is approved by this feasibility note.

| Frozen file | Candidate use if a later import is approved | Boundary |
|---|---|---|
| [`bloodborne-recomp/tools/recomp/recomp.sh`](https://github.com/bloodborne-native/bloodborne-recomp/blob/c60ccf8be752b8690f2813505c4daf9eafdb136b/tools/recomp/recomp.sh#L1-L36), GPL-2.0 under repo root | Learn the local build/orchestration shape for an offline AOT tool | No Bloodborne paths, offsets, maps, annotations, generated C, or default coverage assumptions |
| [`bbcpu/src/cpu/jit_arm64.c`](https://github.com/bloodborne-native/bbcpu/blob/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca/src/cpu/jit_arm64.c#L25-L35), GPL-2.0 under repo root | Study the no-JIT guard and fallback boundary | No JIT path in the iOS product; full file and dependency audit before copying any code |
| [`bbcpu/tests/test_bbcpu.c`](https://github.com/bloodborne-native/bbcpu/blob/3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca/tests/test_bbcpu.c#L1-L40), GPL-2.0 under repo root | Use its authored-input/differential-test pattern | Write new synthetic instructions and expected state; do not copy game-derived vectors |
| [`bloodborne-ios/app/metal_test.m`](https://github.com/bloodborne-native/bloodborne-ios/blob/8f08be42c881c85051f2076c3721dbb2e273b025/app/metal_test.m#L1-L60), GPL-2.0 under repo root | Learn the shape of a minimal synthetic Metal probe | The app shell is not an Uncharted app; audit dependencies before source import |
| [`bloodborne-ios/app/app_main.c`](https://github.com/bloodborne-native/bloodborne-ios/blob/8f08be42c881c85051f2076c3721dbb2e273b025/app/app_main.c#L1-L70), GPL-2.0 under repo root | Read-only reference to target integration points | Do not transplant Bloodborne paths, memory plan, patch layout, title logic, or generated artifacts |
| [`bloodborne-runtime/gpu/shadps4/video_core/renderer_metal/mtl_device.mm`](https://github.com/bloodborne-native/bloodborne-runtime/blob/42736cf7ca7704b087e847482417d9f475bb2dd7/gpu/shadps4/video_core/renderer_metal/mtl_device.mm#L1-L15), GPL-2.0-or-later file notice | Study Metal device setup for a later renderer port | Renderer-wide imports need a file/dependency allowlist; exclude GPL-3.0 `gpu/bbnet20` and unreviewed assets/dependencies |

This is a research allowlist, not authorization to import the listed files. Any later adoption needs a separate scope, complete file-level notice/dependency review and owner approval. No code was imported by #211.

Potentially reusable later: generic build shape, no-JIT guard behavior, authored CPU test pattern, and app/Metal probe structure, each after file-level license review. Excluded categorically: Bloodborne annotations, executable offsets/signatures, generated game functions/code, binary-derived fixtures, copyrighted assets, and any game dump. See [PEERS.md](../../PEERS.md) for the frozen source and notice ledger.
