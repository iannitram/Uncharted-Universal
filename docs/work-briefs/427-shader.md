# Issue 427: shader recompiler peer audit

Base `origin/main` 69672033bf92d102573862a66ac800082ca59a77. Source files retain their shadPS4 GPL-2.0-or-later SPDX notices; the project is GPL-2.0-only. All inputs and tests are synthetic.

1. [#5723 merge abe42da9e09a86a5963f5d8699fb0d6151d3c6c2](https://github.com/shadps4-emu/shadPS4/pull/5723): despite the title, the actual `src/shader_recompiler/frontend/translate/{scalar_alu.cpp,translate.h}` patch implements **S_LSHR_B64**, absent here. Imported dispatch, declaration and lowering, with #5725's corrected operand. `vector_alu.cpp` already dispatches and implements V_LSHR_B64; no vector import required. Test decode scalar opcode 33 with M0 and verify right-shift IR and SCC update.
2. [#5724 merge f3687cc400d77736b2a7ef1904a1302e17f9441b](https://github.com/shadps4-emu/shadPS4/pull/5724): `scalar_alu.cpp` / `translate.h`, S_BFM_B64 absent; imported two masked U32 operands, U64 mask and destination, without SCC change. Test opcode 37 with M0, two 64-bit left shifts, subtraction and no SCC write.
3. [#5725 merge 312cfbae77821627aa511bdee8717e1b7c3c9a23](https://github.com/shadps4-emu/shadPS4/pull/5725): `scalar_alu.cpp` S_LSHL_B64 and S_ASHR_I64 previously fetched 64-bit shift operands; S_LSHR_B64 above is imported with the fix. Use GetSrc and U32 mask 0x3f, preserving SCC. Test opcodes 31, 33 and 35 using M0, which cannot be fetched as a 64-bit pair.
4. [#5719 merge 1f1f4c6a5021fb656df0411c1da79b267115a327](https://github.com/shadps4-emu/shadPS4/pull/5719): `frontend/format.cpp` VOP3 entries 401–404 were Undefined/Undefined; set U32 input and F32 output, matching the existing V_CVT_F32_UBYTE translator. Test all four format entries. The unrelated upstream TODO removal is not imported.
5. [#5714 merge f048bb748f5cda15bd12ea3a1690a9d260f8c482](https://github.com/shadps4-emu/shadPS4/pull/5714): **deferred to #450, not already present**. `backend/spirv/emit_spirv_atomic.cpp` EmitBufferAtomicFMin32/FMax32 and EmitImageAtomicFMin32/FMax32 use OpSelect with two emitted side-effecting atomics; both execute. Peer adds branching fallback and shared-float path plus changes to `emit_spirv_context_get_set.cpp`, `spirv_emit_context.h` and `video_core/renderer_vulkan/vk_instance.cpp`. Local emitter also has `FixImageAtomicCoords`; preserve it. `emit_spirv.cpp` Traverse/PatchPhiNodes uses first-to-last label mapping and needs validation across new control-flow blocks. The peer's new buffer access and shared feature APIs do not exist unchanged here. Recommend a separate child issue for a bounded adaptation: compile/validate SPIR-V with successor phis; execute buffer and image positive/negative values with fallback forced and prove exactly one atomic side effect; cover native capability and shared scope separately. Do not mechanically transplant unrelated refactors.

Acceptance for this partial work: tests in `tests/test_shader_scalar_64.cpp` decode synthetic scalar instructions and inspect generated IR, and check four VOP3 format entries; build target `shader-scalar-64-test`. Windows and Linux builds/CI and this compiled test remain owed. Parent #427 remains open until #5714 is resolved and CI is green; no game or device claim.

## Independent review and CI wiring

The review agent checked the exact upstream four PR diffs against the local
production hunks. No production correction was needed. The original synthetic
target was excluded from normal builds and absent from `build.sh --test`;
review added it to the Windows target/run list and Linux target/non-Vulkan run
list. Those are invoked by the hosted Windows check and Linux build workflows.
The test now also checks decoder identity, U32 shift-count/U64 base IR types,
0x3f masks, and both destination registers. Source provenance is in PEERS.md.

Machine class: Linux/SteamOS Strix Halo tablet (AMD Ryzen AI MAX+ 395 / Radeon
8060S), not the Phoenix handheld. The earlier issue comment used the wrong
machine class. The preflight failure recorded on #427 is environmental: missing
pkg-config, Vulkan/SDL3 development metadata, CMake and Ninja. The review did
not execute the translation-unit test locally; hosted Linux/Windows CI remains
a required gate. No Vulkan execution or U1/U2/U3 gameplay was tested.
