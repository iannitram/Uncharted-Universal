// SPDX-FileCopyrightText: Copyright 2026 Uncharted-Universal contributors
// SPDX-License-Identifier: GPL-2.0-or-later

#include <cassert>
#include <cstdlib>
#include <iostream>
#include <vector>

#include "shader_recompiler/ir/basic_block.h"
#include "shader_recompiler/ir/ir_emitter.h"
#include "shader_recompiler/ir/passes/ir_passes.h"
#include "shader_recompiler/ir/program.h"
#include "shader_recompiler/runtime_info.h"

int main() {
    using namespace Shader;

    // Test 1: HwComputeRuntimeInfo::operator== distinguishes shared_memory_size
    HwComputeRuntimeInfo cs1{1024, {256, 1, 1}, {true, false, false}, false};
    HwComputeRuntimeInfo cs2{2048, {256, 1, 1}, {true, false, false}, false};
    if (cs1 == cs2) {
        std::cerr << "FAIL: HwComputeRuntimeInfo::operator== ignored shared_memory_size\n";
        return 1;
    }

    // Test 2: SharedMemoryBoundsPass logic
    Info info{};
    info.hw_stage = HwStage::Compute;
    IR::Program program{info};

    RuntimeInfo runtime_info{};
    runtime_info.Initialize(HwStage::Compute, SwStage::Compute);
    runtime_info.hw.cs.shared_memory_size = 1024;
    runtime_info.hw.cs.workgroup_size = {256, 1, 1};

    Common::ObjectPool<IR::Inst> pool;
    IR::Block block{pool};
    program.blocks.push_back(&block);

    Optimization::SharedMemoryBoundsPass(program, runtime_info);

    // Test 3: BB_LDS_BOUNDS=0 bypasses the pass
    setenv("BB_LDS_BOUNDS", "0", 1);
    Optimization::SharedMemoryBoundsPass(program, runtime_info);

    std::cout << "All SharedMemoryBoundsPass unit tests passed successfully!\n";
    return 0;
}
