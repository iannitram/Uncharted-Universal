// SPDX-FileCopyrightText: Copyright 2026 Uncharted-Universal contributors
// SPDX-License-Identifier: GPL-2.0-or-later

#include <cassert>
#include <cstdlib>
#include <iostream>

#include "shader_recompiler/ir/basic_block.h"
#include "shader_recompiler/ir/ir_emitter.h"
#include "shader_recompiler/ir/passes/ir_passes.h"
#include "shader_recompiler/ir/program.h"
#include "shader_recompiler/runtime_info.h"

static void SetEnv(const char* key, const char* value) {
#ifdef _WIN32
    if (_putenv_s(key, value) != 0) std::abort();
#else
    if (setenv(key, value, 1) != 0) std::abort();
#endif
}

static void UnsetEnv(const char* key) {
#ifdef _WIN32
    if (_putenv_s(key, "") != 0) std::abort();
#else
    if (unsetenv(key) != 0) std::abort();
#endif
}

int main() {
    using namespace Shader;

    // 1. HwComputeRuntimeInfo::operator== distinguishes shared_memory_size.
    {
        HwComputeRuntimeInfo cs1{1024, {256, 1, 1}, {true, false, false}, false};
        HwComputeRuntimeInfo cs2{2048, {256, 1, 1}, {true, false, false}, false};
        HwComputeRuntimeInfo cs3{1024, {256, 1, 1}, {true, false, false}, false};
        assert(!(cs1 == cs2));
        assert(cs1 == cs3);
    }

    // 2. Static in-bounds immediates stay untouched; boundary-straddling and dynamic accesses
    //    are guarded, redirected to AlignUp(size, 8), and masked to typed zero without SSA cycles.
    {
        UnsetEnv("BB_LDS_BOUNDS");
        Info info{};
        info.hw_stage = HwStage::Compute;
        IR::Program program{info};

        RuntimeInfo runtime_info{};
        runtime_info.Initialize(HwStage::Compute, SwStage::Compute);
        runtime_info.hw.cs.shared_memory_size = 2048;
        runtime_info.hw.cs.workgroup_size = {256, 1, 1};

        Common::ObjectPool<IR::Inst> pool;
        IR::Block block{pool};
        program.blocks.push_back(&block);

        IR::IREmitter ir{block};
        // In-bounds immediate U32 (offset 2044 + 4 == 2048): must stay untouched.
        const IR::Value in_bounds_load = ir.LoadShared(32, false, ir.Imm32(2044));
        // Boundary-straddling immediate U64 (offset 2044 + 8 > 2048): must be rewritten.
        const IR::U64 straddling_load = IR::U64{ir.LoadShared(64, false, ir.Imm32(2044))};
        const IR::U64 straddle_consumer = IR::U64{ir.IAdd(straddling_load, ir.Imm64(u64{1}))};
        // Dynamic U32 load + downstream use: must produce Select(in_bounds, load, 0) with no cycle.
        const IR::U32 dyn_addr = ir.LaneId();
        const IR::U32 dyn_load = IR::U32{ir.LoadShared(32, false, dyn_addr)};
        const IR::U32 consumer = IR::U32{ir.IAdd(dyn_load, ir.Imm32(1))};
        // Dynamic U16 load: must widen to U32, Select, and narrow back to U16.
        const IR::U16 dyn_u16 = IR::U16{ir.LoadShared(16, false, dyn_addr)};
        const IR::U32 u16_consumer = ir.UConvert(32, dyn_u16);
        // Dynamic 32-bit atomic: must use SelectU32 (not SelectU64).
        const IR::U32 dyn_atomic32 = IR::U32{ir.SharedAtomicISub(dyn_addr, ir.Imm32(1), false)};
        const IR::U32 atomic_consumer = IR::U32{ir.IAdd(dyn_atomic32, ir.Imm32(1))};
        // Dynamic write: redirected to sink, no Select on result.
        ir.WriteShared(32, consumer, dyn_addr);

        Optimization::SharedMemoryBoundsPass(program, runtime_info);

        // Sink reserved at AlignUp(2048, 8) = 2048, +8 bytes scratch.
        assert(program.info.shared_memory_scratch_size >= 8u);

        // In-bounds load still has immediate 2044.
        assert(in_bounds_load.Inst()->Arg(0).IsImmediate());
        assert(in_bounds_load.Inst()->Arg(0).U32() == 2044u);

        // Boundary-straddling U64 load (2044 + 8 > 2048) was replaced by SelectU64
        // over a cloned LoadSharedU64 whose address is a SelectU32 to sink_offset (2048).
        const IR::Inst* straddle_sel = straddle_consumer.Inst()->Arg(0).Inst();
        assert(straddle_sel->GetOpcode() == IR::Opcode::SelectU64);
        assert(straddle_sel->Arg(2).IsImmediate() && straddle_sel->Arg(2).U64() == 0u);
        const IR::Inst* cloned_u64 = straddle_sel->Arg(1).Inst();
        assert(cloned_u64->GetOpcode() == IR::Opcode::LoadSharedU64);
        const IR::Inst* straddle_addr = cloned_u64->Arg(0).Inst();
        assert(straddle_addr->GetOpcode() == IR::Opcode::SelectU32);
        assert(straddle_addr->Arg(2).IsImmediate() && straddle_addr->Arg(2).U32() == 2048u);

        // Consumer's arg0 is SelectU32(in_bounds, cloned_load, 0) with no SSA cycle.
        const IR::Inst* sel = consumer.Inst()->Arg(0).Inst();
        assert(sel->GetOpcode() == IR::Opcode::SelectU32);
        const IR::Inst* cloned_u32 = sel->Arg(1).Inst();
        assert(cloned_u32->GetOpcode() == IR::Opcode::LoadSharedU32);
        assert(cloned_u32 != sel);
        assert(sel->Arg(2).IsImmediate() && sel->Arg(2).U32() == 0u);

        // Cloned load's address Select uses sink_offset == 2048 as false branch.
        const IR::Inst* addr_sel = cloned_u32->Arg(0).Inst();
        assert(addr_sel->GetOpcode() == IR::Opcode::SelectU32);
        assert(addr_sel->Arg(2).IsImmediate() && addr_sel->Arg(2).U32() == 2048u);

        // U16 load widens to U32, selects against 0, and narrows back to U16.
        assert(u16_consumer.Inst()->Arg(0).Inst()->GetOpcode() == IR::Opcode::ConvertU16U32);

        // 32-bit atomic uses SelectU32 (not SelectU64).
        assert(atomic_consumer.Inst()->Arg(0).Inst()->GetOpcode() == IR::Opcode::SelectU32);
    }

    // 3. BB_LDS_BOUNDS=0 disables the pass completely.
    {
        SetEnv("BB_LDS_BOUNDS", "0");
        Info info{};
        info.hw_stage = HwStage::Compute;
        IR::Program program{info};

        RuntimeInfo runtime_info{};
        runtime_info.Initialize(HwStage::Compute, SwStage::Compute);
        runtime_info.hw.cs.shared_memory_size = 512;
        runtime_info.hw.cs.workgroup_size = {256, 1, 1};

        Common::ObjectPool<IR::Inst> pool;
        IR::Block block{pool};
        program.blocks.push_back(&block);

        IR::IREmitter ir{block};
        const IR::Value oob_load = ir.LoadShared(32, false, ir.Imm32(1024));

        Optimization::SharedMemoryBoundsPass(program, runtime_info);

        assert(program.info.shared_memory_scratch_size == 0u);
        assert(oob_load.Inst()->Arg(0).IsImmediate());
        assert(oob_load.Inst()->Arg(0).U32() == 1024u);
        UnsetEnv("BB_LDS_BOUNDS");
    }

    std::cout << "All SharedMemoryBoundsPass unit tests passed successfully!\n";
    return 0;
}
