// SPDX-FileCopyrightText: Copyright 2026 shadPS4 Emulator Project
// SPDX-License-Identifier: GPL-2.0-or-later

#include <array>
#include <cstdlib>
#include <vector>

#include "shader_recompiler/frontend/decode.h"
#include "shader_recompiler/frontend/translate/translate.h"

int main() {
    using namespace Shader;
    using namespace Shader::Gcn;

    constexpr u32 cache_instruction = static_cast<u32>(InstEncoding::MUBUF);
    constexpr auto encode = [](u32 opcode) {
        return cache_instruction | (opcode << 18);
    };
    // Both cache-control encodings are followed by another 64-bit instruction.
    // This synthetic stream checks instruction boundaries without game shader data.
    std::array<u32, 6> words{
        encode(112), 0, encode(113), 0, encode(112), 0,
    };
    GcnCodeSlice code(words.data(), words.data() + words.size());
    GcnDecodeContext decoder;
    std::vector<GcnInst> instructions;
    while (!code.atEnd()) {
        instructions.push_back(decoder.decodeInstruction(code));
    }
    if (instructions.size() != 3 || instructions[0].opcode != Opcode::BUFFER_WBINVL1_SC ||
        instructions[1].opcode != Opcode::BUFFER_WBINVL1 ||
        instructions[2].opcode != Opcode::BUFFER_WBINVL1_SC || instructions[0].src_count != 0 ||
        instructions[1].src_count != 0 || instructions[2].src_count != 0 ||
        instructions[0].length != 8 || instructions[1].length != 8 ||
        instructions[2].length != 8) {
        return 1;
    }

    Info info{};
    RuntimeInfo runtime{};
    runtime.Initialize(HwStage::Vertex, SwStage::Vertex);
    Profile profile{};
    Translator translator(info, runtime, profile);
    Common::ObjectPool<IR::Inst> pool;
    IR::Block block(pool);
    translator.Translate(&block, 0, IR::Condition::True, instructions);

    u32 barriers = 0;
    for (const auto& inst : block) {
        if (inst.GetOpcode() == IR::Opcode::DeviceMemoryBarrier) {
            ++barriers;
        }
    }
    return barriers == 3 && block.size() == 3 ? 0 : 2;
}
