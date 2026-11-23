// SPDX-FileCopyrightText: Copyright 2026 Uncharted-Universal contributors
// SPDX-License-Identifier: GPL-2.0-or-later

#include <cassert>
#include <cstdlib>
#include <cstring>
#include <unordered_map>
#include <vector>

#include "bbport_nan_guard.h"
#include "shader_recompiler/backend/bindings.h"
#include "shader_recompiler/backend/spirv/emit_spirv.h"
#include "shader_recompiler/ir/attribute.h"
#include "shader_recompiler/ir/basic_block.h"
#include "shader_recompiler/ir/ir_emitter.h"
#include "shader_recompiler/ir/passes/ir_passes.h"
#include "shader_recompiler/ir/program.h"
#include "shader_recompiler/profile.h"
#include "shader_recompiler/runtime_info.h"
#include "video_core/amdgpu/types.h"

namespace {

using namespace Shader;

std::unordered_map<u16, u32> CountOpcodes(const std::vector<u32>& spv) {
    std::unordered_map<u16, u32> counts;
    if (spv.size() < 5 || spv[0] != 0x07230203u) {
        return counts;
    }
    for (size_t i = 5; i < spv.size();) {
        const u32 wc = spv[i] >> 16;
        const u16 op = static_cast<u16>(spv[i] & 0xffffu);
        if (wc == 0 || i + wc > spv.size()) {
            counts.clear();
            return counts;
        }
        ++counts[op];
        i += wc;
    }
    return counts;
}

std::vector<u32> EmitComputeImageWrite(AmdGpu::NumberFormat nfmt) {
    Info info{};
    info.hw_stage = HwStage::Compute;
    RuntimeInfo runtime{};
    runtime.Initialize(HwStage::Compute, SwStage::Compute);
    runtime.hw.cs.workgroup_size = {64, 1, 1};

    ImageResource img{};
    img.nfmt = nfmt;
    img.is_written = true;
    info.images.push_back(img);

    Profile profile{};
    Common::ObjectPool<IR::Inst> inst_pool;
    IR::Block entry(inst_pool);
    IR::IREmitter ir(entry, entry.end());
    const auto img_handle = ir.Imm32(0u);
    const auto coord = ir.CompositeConstruct(ir.Imm32(0u), ir.Imm32(0u));
    const auto color =
        ir.CompositeConstruct(ir.Imm32(1.0f), ir.Imm32(0.5f), ir.Imm32(0.25f), ir.Imm32(1.0f));
    ir.ImageWrite(img_handle, coord, {}, {}, color, {});

    IR::Program program{info};
    IR::AbstractSyntaxNode block_node{};
    block_node.type = IR::AbstractSyntaxNode::Type::Block;
    block_node.data.block = &entry;
    IR::AbstractSyntaxNode ret_node{};
    ret_node.type = IR::AbstractSyntaxNode::Type::Return;
    program.syntax_list = {block_node, ret_node};
    program.blocks = {&entry};
    program.post_order_blocks = {&entry};
    Optimization::CollectShaderInfoPass(program, profile);
    Backend::Bindings bindings{};
    return Backend::SPIRV::EmitSPIRV(profile, runtime, program, bindings);
}

std::vector<u32> EmitFragmentMrtWrite() {
    Info info{};
    info.hw_stage = HwStage::Fragment;
    RuntimeInfo runtime{};
    runtime.Initialize(HwStage::Fragment, SwStage::Fragment);
    runtime.hw.fs.num_color_buffers = 1;
    runtime.hw.fs.color_buffers[0].export_format = AmdGpu::ExportFormat::R16G16B16A16_FLOAT;

    Profile profile{};
    Common::ObjectPool<IR::Inst> inst_pool;
    IR::Block entry(inst_pool);
    IR::IREmitter ir(entry, entry.end());
    ir.SetAttribute(IR::Attribute::RenderTarget0, ir.Imm32(1.0f), 0);
    ir.SetAttribute(IR::Attribute::RenderTarget0, ir.Imm32(0.5f), 1);
    ir.SetAttribute(IR::Attribute::RenderTarget0, ir.Imm32(0.25f), 2);
    ir.SetAttribute(IR::Attribute::RenderTarget0, ir.Imm32(1.0f), 3);

    IR::Program program{info};
    IR::AbstractSyntaxNode block_node{};
    block_node.type = IR::AbstractSyntaxNode::Type::Block;
    block_node.data.block = &entry;
    IR::AbstractSyntaxNode ret_node{};
    ret_node.type = IR::AbstractSyntaxNode::Type::Return;
    program.syntax_list = {block_node, ret_node};
    program.blocks = {&entry};
    program.post_order_blocks = {&entry};
    Optimization::CollectShaderInfoPass(program, profile);
    Backend::Bindings bindings{};
    return Backend::SPIRV::EmitSPIRV(profile, runtime, program, bindings);
}

} // namespace

int main() {
    // Default BB_NAN_GUARD mode (1: NaN -> 0 on float outputs).
    unsetenv("BB_NAN_GUARD");
    assert(BbNanGuard::NanGuardMode() == 1);
    assert(BbNanGuard::ExportNanGuard());
    assert(std::strcmp(BbNanGuard::ShaderCacheSuffix(), "-nan1") == 0);

    // SPIR-V opcodes: OpImageWrite=99, OpIsNan=156, OpSelect=169.
    constexpr u16 kOpImageWrite = 99;
    constexpr u16 kOpIsNan = 156;
    constexpr u16 kOpSelect = 169;

    // 1. Float storage image write must emit OpIsNan + OpSelect before OpImageWrite.
    const auto float_img_counts = CountOpcodes(EmitComputeImageWrite(AmdGpu::NumberFormat::Float));
    assert(!float_img_counts.empty());
    assert(float_img_counts.at(kOpImageWrite) == 1);
    assert(float_img_counts.at(kOpIsNan) == 1);
    assert(float_img_counts.at(kOpSelect) == 1);

    // 2. Integer storage image write must NOT emit OpIsNan or OpSelect.
    const auto int_img_counts = CountOpcodes(EmitComputeImageWrite(AmdGpu::NumberFormat::Uint));
    assert(!int_img_counts.empty());
    assert(int_img_counts.at(kOpImageWrite) == 1);
    assert(!int_img_counts.contains(kOpIsNan));
    assert(!int_img_counts.contains(kOpSelect));

    // 3. Float MRT fragment export continues to emit OpIsNan + OpSelect per component.
    const auto frag_counts = CountOpcodes(EmitFragmentMrtWrite());
    assert(!frag_counts.empty());
    assert(frag_counts.at(kOpIsNan) == 4);
    assert(frag_counts.at(kOpSelect) == 4);

    return 0;
}
