// SPDX-FileCopyrightText: Copyright 2026 shadPS4 Emulator Project
// SPDX-License-Identifier: GPL-2.0-or-later

#include <algorithm>
#include <cstdlib>
#include <string_view>

#include "common/alignment.h"
#include "shader_recompiler/ir/ir_emitter.h"
#include "shader_recompiler/ir/passes/ir_passes.h"
#include "shader_recompiler/ir/program.h"
#include "shader_recompiler/runtime_info.h"

namespace Shader::Optimization {

namespace {

struct SharedAccess {
    bool is_shared = false;
    bool has_result = false;
    u32 access_bytes = 0;
    IR::Type result_type = IR::Type::Void;
};

SharedAccess ClassifySharedAccess(IR::Opcode op) {
    switch (op) {
    case IR::Opcode::LoadSharedU16:
        return {true, true, 2, IR::Type::U16};
    case IR::Opcode::LoadSharedU32:
        return {true, true, 4, IR::Type::U32};
    case IR::Opcode::LoadSharedU64:
        return {true, true, 8, IR::Type::U64};
    case IR::Opcode::WriteSharedU16:
        return {true, false, 2, IR::Type::Void};
    case IR::Opcode::WriteSharedU32:
        return {true, false, 4, IR::Type::Void};
    case IR::Opcode::WriteSharedU64:
        return {true, false, 8, IR::Type::Void};
    case IR::Opcode::SharedAtomicIAdd32:
    case IR::Opcode::SharedAtomicISub32:
    case IR::Opcode::SharedAtomicSMin32:
    case IR::Opcode::SharedAtomicUMin32:
    case IR::Opcode::SharedAtomicSMax32:
    case IR::Opcode::SharedAtomicUMax32:
    case IR::Opcode::SharedAtomicInc32:
    case IR::Opcode::SharedAtomicDec32:
    case IR::Opcode::SharedAtomicAnd32:
    case IR::Opcode::SharedAtomicOr32:
    case IR::Opcode::SharedAtomicXor32:
    case IR::Opcode::SharedAtomicCmpSwap32:
        return {true, true, 4, IR::Type::U32};
    case IR::Opcode::SharedAtomicIAdd64:
    case IR::Opcode::SharedAtomicISub64:
    case IR::Opcode::SharedAtomicSMin64:
    case IR::Opcode::SharedAtomicUMin64:
    case IR::Opcode::SharedAtomicSMax64:
    case IR::Opcode::SharedAtomicUMax64:
    case IR::Opcode::SharedAtomicInc64:
    case IR::Opcode::SharedAtomicDec64:
    case IR::Opcode::SharedAtomicAnd64:
    case IR::Opcode::SharedAtomicOr64:
    case IR::Opcode::SharedAtomicXor64:
    case IR::Opcode::SharedAtomicCmpSwap64:
        return {true, true, 8, IR::Type::U64};
    default:
        return {};
    }
}

} // namespace

void SharedMemoryBoundsPass(IR::Program& program, const RuntimeInfo& runtime_info) {
    if (program.info.hw_stage != HwStage::Compute) {
        return;
    }
    if (const char* env = std::getenv("BB_LDS_BOUNDS"); env && std::string_view(env) == "0") {
        return;
    }

    const u32 shared_memory_size = runtime_info.hw.cs.shared_memory_size;
    if (shared_memory_size == 0) {
        return;
    }

    const u32 sink_offset = Common::AlignUp(shared_memory_size, 8u);
    bool used_sink = false;

    for (IR::Block* const block : program.blocks) {
        for (auto inst_it = block->begin(); inst_it != block->end(); ++inst_it) {
            IR::Inst& inst = *inst_it;
            const SharedAccess access = ClassifySharedAccess(inst.GetOpcode());
            if (!access.is_shared || inst.Flags<bool>()) {
                continue;
            }

            const bool can_fit = shared_memory_size >= access.access_bytes;
            const u32 max_valid_addr = can_fit ? (shared_memory_size - access.access_bytes) : 0u;
            const IR::Value addr = inst.Arg(0);
            if (can_fit && addr.IsImmediate() && addr.U32() <= max_valid_addr) {
                continue;
            }

            used_sink = true;
            IR::IREmitter ir{*block, inst_it};

            const IR::U1 in_bounds =
                can_fit ? ir.ILessThanEqual(IR::U32{addr}, ir.Imm32(max_valid_addr), false)
                        : ir.Imm1(false);
            const IR::U32 safe_addr =
                IR::U32{ir.Select(in_bounds, IR::U32{addr}, ir.Imm32(sink_offset))};

            if (!access.has_result) {
                inst.SetArg(0, safe_addr);
                continue;
            }

            // Clone the load/atomic with safe_addr before inst, select 0 when out of bounds,
            // and turn inst into Identity(selected) via ReplaceUsesWith.
            const auto cloned_it = block->PrependNewInst(inst_it, inst);
            cloned_it->SetArg(0, safe_addr);
            const IR::Value loaded{&*cloned_it};

            IR::Value masked;
            if (access.result_type == IR::Type::U16) {
                const IR::U32 wide = ir.UConvert(32, IR::U16{loaded});
                const IR::Value selected_u32 = ir.Select(in_bounds, wide, ir.Imm32(u32{0}));
                masked = ir.UConvert(16, IR::U32{selected_u32});
            } else {
                const IR::Value zero = access.result_type == IR::Type::U64
                                           ? IR::Value{ir.Imm64(u64{0})}
                                           : IR::Value{ir.Imm32(u32{0})};
                masked = ir.Select(in_bounds, loaded, zero);
            }
            inst.ReplaceUsesWith(masked);
        }
    }

    if (used_sink) {
        program.info.shared_memory_scratch_size = std::max(
            program.info.shared_memory_scratch_size, sink_offset + 8u - shared_memory_size);
    }
}

} // namespace Shader::Optimization
