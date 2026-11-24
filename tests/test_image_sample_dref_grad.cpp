// SPDX-FileCopyrightText: Copyright 2026 Uncharted-Universal contributors
// SPDX-License-Identifier: GPL-2.0-or-later

#include <array>
#include <bit>
#include <cassert>
#include <cstring>
#include <unordered_map>
#include <vector>

#include "shader_recompiler/backend/bindings.h"
#include "shader_recompiler/backend/spirv/emit_spirv.h"
#include "shader_recompiler/frontend/decode.h"
#include "shader_recompiler/frontend/translate/translate.h"
#include "shader_recompiler/ir/basic_block.h"
#include "shader_recompiler/ir/ir_emitter.h"
#include "shader_recompiler/ir/passes/ir_passes.h"
#include "shader_recompiler/ir/program.h"
#include "shader_recompiler/profile.h"
#include "shader_recompiler/runtime_info.h"
#include "video_core/amdgpu/resource.h"

namespace {

std::array<u32, 2> EncodeMimgSample(u32 op, u32 dmask, u32 vaddr, u32 vdst, u32 srsrc, u32 ssamp) {
    // MIMG encoding:
    // dword0: [31:26]=0b111100 (0x3c), [24:18]=op, [11:8]=dmask
    // dword1: [25:21]=ssamp, [20:16]=srsrc, [15:8]=vdst, [7:0]=vaddr
    const u32 dword0 = (0x3cu << 26) | ((op & 0x7fu) << 18) | ((dmask & 0xfu) << 8);
    const u32 dword1 = ((ssamp & 0x1fu) << 21) | ((srsrc & 0x1fu) << 16) | ((vdst & 0xffu) << 8) |
                       (vaddr & 0xffu);
    return {dword0, dword1};
}

std::unordered_map<u16, u32> CountSpirvOpcodes(const std::vector<u32>& spv) {
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

} // namespace

int main() {
    using namespace Shader;
    using namespace Shader::Gcn;

    // 1. Verify GCN decode + translate of IMAGE_SAMPLE_C_D (42), IMAGE_SAMPLE_C_D_O (58),
    //    and IMAGE_SAMPLE_CD (104).
    for (const auto& [op, expect_depth, expect_deriv] : std::array{
             std::tuple{42u, true, true},
             std::tuple{58u, true, true},
             std::tuple{104u, false, true},
         }) {
        const auto words = EncodeMimgSample(op, 0x1u, 0u, 8u, 0u, 4u);
        GcnCodeSlice code(words.data(), words.data() + words.size());
        GcnDecodeContext decoder;
        const auto inst = decoder.decodeInstruction(code);
        assert(inst.opcode == static_cast<Opcode>(op + static_cast<u32>(OpcodeMap::OP_MAP_MIMG)));

        Info info{};
        RuntimeInfo runtime{};
        runtime.Initialize(HwStage::Fragment, SwStage::Fragment);
        Profile profile{};
        Translator translator(info, runtime, profile);
        Common::ObjectPool<IR::Inst> pool;
        IR::Block block(pool);
        translator.Translate(&block, 0, IR::Condition::True, std::array{inst});

        u32 sample_raw_count = 0;
        for (const auto& ir_inst : block) {
            if (ir_inst.GetOpcode() == IR::Opcode::ImageSampleRaw) {
                ++sample_raw_count;
                const auto tex_info = ir_inst.Flags<IR::TextureInstInfo>();
                assert(static_cast<bool>(tex_info.is_depth) == expect_depth);
                assert(static_cast<bool>(tex_info.has_derivatives) == expect_deriv);
            }
        }
        assert(sample_raw_count == 1);
    }

    // 2. Verify ResourcePatchingPass lowers (is_depth && has_derivatives) to ImageGradientDref,
    //    propagates ImageResource::is_depth across multiple samples of the same T#, and EmitSPIRV
    //    emits OpImageSampleDrefExplicitLod (90) rather than OpImageSampleExplicitLod (88).
    {
        Info info{};
        info.hw_stage = HwStage::Fragment;

        // Populate user_data with a valid 2D Image T# (8 dwords at s0..s7) and Sampler S# (4 dwords at s8..s11).
        std::array<u32, 16> user_data_buf{};
        const AmdGpu::Image tsharp = AmdGpu::Image::Null(true);
        std::array<u32, 8> tsharp_dwords{};
        std::memcpy(tsharp_dwords.data(), &tsharp, sizeof(tsharp));
        for (u32 i = 0; i < 8; ++i) {
            user_data_buf[i] = tsharp_dwords[i];
        }
        AmdGpu::Sampler ssharp{};
        ssharp.depth_compare_func.Assign(AmdGpu::DepthCompare::LessEqual);
        std::array<u32, 4> ssharp_dwords{};
        std::memcpy(ssharp_dwords.data(), &ssharp, sizeof(ssharp));
        for (u32 i = 0; i < 4; ++i) {
            user_data_buf[8 + i] = ssharp_dwords[i];
        }
        info.user_data = user_data_buf;

        IR::Program program{info};
        RuntimeInfo runtime{};
        runtime.Initialize(HwStage::Fragment, SwStage::Fragment);
        Profile profile{};

        Common::ObjectPool<IR::Inst> pool;
        IR::Block entry{pool};
        IR::IREmitter ir(entry, entry.end());

        const IR::Value tsharp_low = ir.CompositeConstruct(
            ir.GetUserData(IR::ScalarReg(0)), ir.GetUserData(IR::ScalarReg(1)),
            ir.GetUserData(IR::ScalarReg(2)), ir.GetUserData(IR::ScalarReg(3)));
        const IR::Value tsharp_high = ir.CompositeConstruct(
            ir.GetUserData(IR::ScalarReg(4)), ir.GetUserData(IR::ScalarReg(5)),
            ir.GetUserData(IR::ScalarReg(6)), ir.GetUserData(IR::ScalarReg(7)));
        const IR::Value image_handle = ir.ImageHandle(tsharp_low, tsharp_high);
        const IR::Value sampler_handle = ir.CompositeConstruct(
            ir.GetUserData(IR::ScalarReg(8)), ir.GetUserData(IR::ScalarReg(9)),
            ir.GetUserData(IR::ScalarReg(10)), ir.GetUserData(IR::ScalarReg(11)));

        // First sample on T#: non-depth sample.
        IR::TextureInstInfo plain_info{};
        plain_info.is_depth.Assign(0);
        const IR::Value addr1_plain = ir.CompositeConstruct(ir.Imm32(0.5f), ir.Imm32(0.5f),
                                                            ir.Imm32(0.0f), ir.Imm32(0.0f));
        const IR::Value zero_vec4 = ir.CompositeConstruct(ir.Imm32(0.0f), ir.Imm32(0.0f),
                                                          ir.Imm32(0.0f), ir.Imm32(0.0f));
        (void)ir.ImageSampleRaw(image_handle, sampler_handle, addr1_plain, zero_vec4, zero_vec4,
                                ir.Imm32(0.0f), plain_info);

        // Second sample on same T#: depth-compare with explicit derivatives (IMAGE_SAMPLE_C_D).
        // 2D layout: addr[0]=dref, addr[1..4]=dx/dy, addr[5..6]=coords.
        IR::TextureInstInfo dref_grad_info{};
        dref_grad_info.is_depth.Assign(1);
        dref_grad_info.has_derivatives.Assign(1);
        const IR::Value addr1_dref = ir.CompositeConstruct(ir.Imm32(0.25f), ir.Imm32(0.01f),
                                                           ir.Imm32(0.0f), ir.Imm32(0.0f));
        const IR::Value addr2_dref = ir.CompositeConstruct(ir.Imm32(0.01f), ir.Imm32(0.5f),
                                                           ir.Imm32(0.5f), ir.Imm32(0.0f));
        (void)ir.ImageSampleRaw(image_handle, sampler_handle, addr1_dref, addr2_dref, zero_vec4,
                                ir.Imm32(0.0f), dref_grad_info);

        IR::AbstractSyntaxNode block_node{};
        block_node.type = IR::AbstractSyntaxNode::Type::Block;
        block_node.data.block = &entry;
        IR::AbstractSyntaxNode ret_node{};
        ret_node.type = IR::AbstractSyntaxNode::Type::Return;
        program.syntax_list = {block_node, ret_node};
        program.blocks = {&entry};
        program.post_order_blocks = {&entry};

        Optimization::FlattenExtendedUserdataPass(program);
        const auto resources = Optimization::ResourceDiscoverPass(program, profile);
        Optimization::ResourcePatchingPass(program.info, resources, profile);
        Optimization::DeadCodeEliminationPass(program);

        assert(program.info.images.size() == 1);
        assert(program.info.images[0].is_depth);
        assert(program.info.samplers.size() == 2);
        assert(!program.info.samplers[0].is_depth);
        assert(program.info.samplers[1].is_depth);

        u32 grad_dref_count = 0;
        u32 grad_plain_count = 0;
        for (const auto& ir_inst : entry) {
            grad_dref_count += ir_inst.GetOpcode() == IR::Opcode::ImageGradientDref;
            grad_plain_count += ir_inst.GetOpcode() == IR::Opcode::ImageGradient;
        }
        assert(grad_dref_count == 1);
        assert(grad_plain_count == 0);

        Optimization::CollectShaderInfoPass(program, profile);
        Backend::Bindings bindings{};
        const auto spv = Backend::SPIRV::EmitSPIRV(profile, runtime, program, bindings);
        const auto op_counts = CountSpirvOpcodes(spv);
        assert(!op_counts.empty());
        // OpImageSampleDrefExplicitLod = 90, OpImageSampleExplicitLod = 88.
        assert(op_counts.contains(90) && op_counts.at(90) == 1);
        assert(!op_counts.contains(88));
    }

    return 0;
}
