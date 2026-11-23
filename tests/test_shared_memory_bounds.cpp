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
        unsetenv("BB_LDS_BOUNDS");
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
        const IR::U32 in_bounds_load = ir.LoadSharedU32(ir.Imm32(2044));
        // Boundary-straddling immediate U64 (offset 2044 + 8 > 2048): must be rewritten.
        const IR::U64 straddling_load = ir.LoadSharedU64(ir.Imm32(2044));
        // Dynamic U32 load + downstream use: must produce Select(in_bounds, load, 0) with no cycle.
        const IR::U32 dyn_addr = ir.GetLocalInvocationId();
        const IR::U32 dyn_load = ir.LoadSharedU32(dyn_addr);
        const IR::U32 consumer = ir.IAdd(dyn_load, ir.Imm32(1));
        // Dynamic U16 load: must use U16 zero.
        const IR::U16 dyn_u16 = ir.LoadSharedU16(dyn_addr);
        // Dynamic 32-bit atomic: must use U32 zero (not U64).
        const IR::U32 dyn_atomic32 = ir.SharedAtomicISub(dyn_addr, ir.Imm32(1));
        // Dynamic write: redirected to sink, no Select on result.
        ir.WriteSharedU32(dyn_addr, consumer);
        (void)straddling_load;
        (void)dyn_u16;
        (void)dyn_atomic32;

        Optimization::SharedMemoryBoundsPass(program, runtime_info);

        // Sink reserved at AlignUp(2048, 8) = 2048, +8 bytes scratch.
        assert(program.info.shared_memory_scratch_size >= 8u);

        // In-bounds load still has immediate 2044.
        assert(in_bounds_load.InstRecursive()->Arg(0).IsImmediate());
        assert(in_bounds_load.InstRecursive()->Arg(0).U32() == 2044u);

        // Boundary-straddling U64 load was rewritten to a Select address.
        assert(!straddling_load.InstRecursive()->Arg(0).IsImmediate());

        // Consumer now reads a Select whose true branch is dyn_load (no self-referential cycle)
        // and false branch is Imm32(0).
        const IR::Value consumer_arg0 = consumer.InstRecursive()->Arg(0);
        assert(!consumer_arg0.IsImmediate());
        const IR::Inst* sel = consumer_arg0.InstRecursive();
        assert(sel->GetOpcode() == IR::Opcode::SelectU32);
        assert(sel->Arg(1).InstRecursive() == dyn_load.InstRecursive());
        assert(sel->Arg(1).InstRecursive() != sel);
        assert(sel->Arg(2).IsImmediate() && sel->Arg(2).U32() == 0u);

        // Every rewritten shared op's Select uses sink_offset == 2048 as false branch.
        const IR::Inst* addr_sel = dyn_load.InstRecursive()->Arg(0).InstRecursive();
        assert(addr_sel->GetOpcode() == IR::Opcode::SelectU32);
        assert(addr_sel->Arg(2).IsImmediate() && addr_sel->Arg(2).U32() == 2048u);
    }

    // 3. BB_LDS_BOUNDS=0 disables the pass completely.
    {
        setenv("BB_LDS_BOUNDS", "0", 1);
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
        const IR::U32 oob_load = ir.LoadSharedU32(ir.Imm32(1024));

        Optimization::SharedMemoryBoundsPass(program, runtime_info);

        assert(program.info.shared_memory_scratch_size == 0u);
        assert(oob_load.InstRecursive()->Arg(0).IsImmediate());
        assert(oob_load.InstRecursive()->Arg(0).U32() == 1024u);
        unsetenv("BB_LDS_BOUNDS");
    }

    std::cout << "All SharedMemoryBoundsPass unit tests passed successfully!\n";
    return 0;
}
