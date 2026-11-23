// SPDX-FileCopyrightText: Copyright 2026 shadPS4 Emulator Project
// SPDX-License-Identifier: GPL-2.0-or-later

#include <cstdlib>
#include "common/alignment.h"
#include "common/logging/classes.h"
#include "shader_recompiler/ir/ir_emitter.h"
#include "shader_recompiler/ir/passes/ir_passes.h"
#include "shader_recompiler/ir/program.h"
#include "shader_recompiler/runtime_info.h"

namespace Shader::Optimization {

static bool IsLoadShared(const IR::Inst& inst) {
    const auto op = inst.GetOpcode();
    return op == IR::Opcode::LoadSharedU16 || op == IR::Opcode::LoadSharedU32 ||
           op == IR::Opcode::LoadSharedU64;
}

static bool IsWriteShared(const IR::Inst& inst) {
    const auto op = inst.GetOpcode();
    if (op == IR::Opcode::WriteSharedU16 || op == IR::Opcode::WriteSharedU32 ||
        op == IR::Opcode::WriteSharedU64) {
        return true;
    }
    return op >= IR::Opcode::SharedAtomicIAdd32 && op <= IR::Opcode::SharedAtomicCmpSwap64;
}

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
            if (!IsLoadShared(inst) && !IsWriteShared(inst)) {
                continue;
            }

            const IR::Value& addr = inst.Arg(0);
            if (addr.IsImmediate() && addr.U32() < shared_memory_size) {
                continue;
            }

            used_sink = true;
            IR::IREmitter ir{*block, IR::Block::InstructionList::s_iterator_to(inst)};

            const bool is_load = IsLoadShared(inst);
            const bool is_atomic = !is_load && inst.GetOpcode() >= IR::Opcode::SharedAtomicIAdd32;
            const auto op = inst.GetOpcode();

            const IR::U1 in_bounds = ir.ILessThan(IR::U32{addr}, ir.Imm32(shared_memory_size), false);
            const IR::U32 safe_addr = IR::U32{ir.Select(in_bounds, IR::U32{addr}, ir.Imm32(sink_offset))};

            inst.SetArg(0, safe_addr);

            if (is_load || is_atomic) {
                IR::IREmitter ir_after{*block, std::next(IR::Block::InstructionList::s_iterator_to(inst))};
                const bool is_64 = (op == IR::Opcode::LoadSharedU64 ||
                                    (op >= IR::Opcode::SharedAtomicIAdd64 && op <= IR::Opcode::SharedAtomicCmpSwap64));
                const IR::Value zero = is_64 ? IR::Value{ir_after.Imm64(u64{0})} : IR::Value{ir_after.Imm32(u32{0})};
                const IR::Value selected = ir_after.Select(in_bounds, IR::Value{&inst}, zero);
                inst.ReplaceUsesWith(selected);
            }
        }
    }

    if (used_sink) {
        program.info.shared_memory_scratch_size =
            std::max(program.info.shared_memory_scratch_size, sink_offset + 8u - shared_memory_size);
    }
}

} // namespace Shader::Optimization
