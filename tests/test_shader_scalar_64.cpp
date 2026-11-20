// SPDX-FileCopyrightText: Copyright 2026 Uncharted-Universal contributors
// SPDX-License-Identifier: GPL-2.0-or-later

#include <array>
#include <utility>

#include "shader_recompiler/frontend/decode.h"
#include "shader_recompiler/frontend/translate/translate.h"
#include "shader_recompiler/profile.h"

int main() {
    using namespace Shader;
    using namespace Shader::Gcn;
    // SOP2: source 0 is s0:s1, source 1 is M0 (a 32-bit-only operand), destination s2:s3.
    constexpr auto sop2 = [](u32 op) {
        return static_cast<u32>(InstEncoding::SOP2) | (op << 23) | (2u << 16) | (124u << 8);
    };
    for (const auto& [op, expected] : std::array{
             std::pair{31u, IR::Opcode::ShiftLeftLogical64},
             std::pair{33u, IR::Opcode::ShiftRightLogical64},
             std::pair{35u, IR::Opcode::ShiftRightArithmetic64},
             std::pair{37u, IR::Opcode::ShiftLeftLogical64},
         }) {
        const u32 word = sop2(op);
        GcnCodeSlice code(&word, &word + 1);
        GcnDecodeContext decoder;
        const auto inst = decoder.decodeInstruction(code);
        if (inst.opcode != static_cast<Opcode>(op + static_cast<u32>(OpcodeMap::OP_MAP_SOP2)) ||
            inst.length != sizeof(u32) || inst.src_count != 2 || inst.dst_count != 1) {
            return 1;
        }
        Info info{};
        RuntimeInfo runtime{};
        runtime.Initialize(HwStage::Vertex, SwStage::Vertex);
        Profile profile{};
        Translator translator(info, runtime, profile);
        Common::ObjectPool<IR::Inst> pool;
        IR::Block block(pool);
        translator.Translate(&block, 0, IR::Condition::True, std::array{inst});
        u32 shifts = 0, m0_reads = 0, scc_writes = 0, subtractions = 0, masks = 0;
        u32 destination_writes = 0;
        for (const auto& ir_inst : block) {
            shifts += ir_inst.GetOpcode() == expected;
            m0_reads += ir_inst.GetOpcode() == IR::Opcode::GetM0;
            scc_writes += ir_inst.GetOpcode() == IR::Opcode::SetScc;
            subtractions += ir_inst.GetOpcode() == IR::Opcode::ISub64;
            if (ir_inst.GetOpcode() == expected &&
                (ir_inst.Arg(0).Type() != IR::Type::U64 ||
                 ir_inst.Arg(1).Type() != IR::Type::U32)) {
                return 2;
            }
            if (ir_inst.GetOpcode() == IR::Opcode::BitwiseAnd32) {
                ++masks;
                const auto mask = ir_inst.Arg(1);
                if (!mask.IsImmediate() || mask.Type() != IR::Type::U32 || mask.U32() != 0x3f) {
                    return 3;
                }
            }
            if (ir_inst.GetOpcode() == IR::Opcode::SetScalarRegister) {
                const auto destination = ir_inst.Arg(0).ScalarReg();
                if (destination != IR::ScalarReg(2) && destination != IR::ScalarReg(3)) {
                    return 4;
                }
                destination_writes |= destination == IR::ScalarReg(2) ? 1u : 2u;
            }
        }
        if (m0_reads != 1 || shifts != (op == 37 ? 2u : 1u) ||
            scc_writes != (op == 37 ? 0u : 1u) || subtractions != (op == 37 ? 1u : 0u) ||
            masks != (op == 37 ? 2u : 1u) || destination_writes != 3u) {
            return 5;
        }
    }
    for (u32 op = 401; op <= 404; ++op) {
        const auto format = InstructionFormat(InstEncoding::VOP3, op);
        if (format.src_type != ScalarType::Uint32 || format.dst_type != ScalarType::Float32 ||
            format.src_count != 1 || format.dst_count != 1) {
            return 6;
        }
    }
    return 0;
}
