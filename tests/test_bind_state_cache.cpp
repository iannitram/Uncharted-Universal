// SPDX-License-Identifier: GPL-2.0-or-later

#include "video_core/renderer_vulkan/vk_scheduler.h"

#include <array>
#include <cassert>
#include <cstdint>

namespace {

vk::Pipeline FakePipeline(uintptr_t v) {
    return vk::Pipeline{reinterpret_cast<VkPipeline>(v)};
}

vk::Buffer FakeBuffer(uintptr_t v) {
    return vk::Buffer{reinterpret_cast<VkBuffer>(v)};
}

void TestPipelineDeduplication() {
    Vulkan::BindState state;
    const auto gfx1 = FakePipeline(0x1001);
    const auto gfx2 = FakePipeline(0x1002);
    const auto cmp1 = FakePipeline(0x2001);

    assert(state.ShouldBindPipeline(vk::PipelineBindPoint::eGraphics, gfx1));
    assert(!state.ShouldBindPipeline(vk::PipelineBindPoint::eGraphics, gfx1));

    // Compute binding point is independent of graphics.
    assert(state.ShouldBindPipeline(vk::PipelineBindPoint::eCompute, cmp1));
    assert(!state.ShouldBindPipeline(vk::PipelineBindPoint::eCompute, cmp1));
    assert(!state.ShouldBindPipeline(vk::PipelineBindPoint::eGraphics, gfx1));

    assert(state.ShouldBindPipeline(vk::PipelineBindPoint::eGraphics, gfx2));
    assert(!state.ShouldBindPipeline(vk::PipelineBindPoint::eGraphics, gfx2));
    assert(state.ShouldBindPipeline(vk::PipelineBindPoint::eGraphics, gfx1));

    state.Invalidate();
    assert(state.ShouldBindPipeline(vk::PipelineBindPoint::eGraphics, gfx1));
    assert(state.ShouldBindPipeline(vk::PipelineBindPoint::eCompute, cmp1));
}

void TestVertexInputDeduplication() {
    Vulkan::BindState state;
    std::array<vk::VertexInputBindingDescription2EXT, 2> bindings{{
        {.binding = 0, .stride = 32, .inputRate = vk::VertexInputRate::eVertex, .divisor = 1},
        {.binding = 1, .stride = 16, .inputRate = vk::VertexInputRate::eInstance, .divisor = 1},
    }};
    std::array<vk::VertexInputAttributeDescription2EXT, 2> attrs{{
        {.location = 0, .binding = 0, .format = vk::Format::eR32G32B32Sfloat, .offset = 0},
        {.location = 1, .binding = 1, .format = vk::Format::eR32G32B32A32Sfloat, .offset = 16},
    }};

    assert(state.ShouldSetVertexInput(bindings, attrs));
    assert(!state.ShouldSetVertexInput(bindings, attrs));

    // Mutate attribute offset -> must re-emit.
    attrs[1].offset = 20;
    assert(state.ShouldSetVertexInput(bindings, attrs));
    assert(!state.ShouldSetVertexInput(bindings, attrs));

    // Mutate binding stride -> must re-emit.
    bindings[0].stride = 48;
    assert(state.ShouldSetVertexInput(bindings, attrs));

    // Shorter slice -> must re-emit.
    assert(state.ShouldSetVertexInput(std::span{bindings}.first(1), std::span{attrs}.first(1)));
    assert(!state.ShouldSetVertexInput(std::span{bindings}.first(1), std::span{attrs}.first(1)));

    state.Invalidate();
    assert(state.ShouldSetVertexInput(std::span{bindings}.first(1), std::span{attrs}.first(1)));
}

void TestVertexBufferDeduplication() {
    Vulkan::BindState state;
    const auto b0 = FakeBuffer(0x3000);
    const auto b1 = FakeBuffer(0x3001);
    const auto b2 = FakeBuffer(0x3002);
    const auto b3 = FakeBuffer(0x3003);

    std::array<vk::Buffer, 3> bufs{b0, b1, b2};
    std::array<vk::DeviceSize, 3> offs{0, 64, 128};
    std::array<vk::DeviceSize, 3> sizes{1024, 2048, 512};
    std::array<vk::DeviceSize, 3> strides{32, 16, 12};

    assert(!state.ShouldBindVertexBuffers(true, {}, {}, {}, {}));
    assert(state.ShouldBindVertexBuffers(true, bufs, offs, sizes, strides));
    assert(!state.ShouldBindVertexBuffers(true, bufs, offs, sizes, strides));

    // Prefix subset (0..1) is already bound -> skip, and full (0..2) remains valid.
    assert(!state.ShouldBindVertexBuffers(true, std::span{bufs}.first(2), std::span{offs}.first(2),
                                          std::span{sizes}.first(2), std::span{strides}.first(2)));
    assert(!state.ShouldBindVertexBuffers(true, bufs, offs, sizes, strides));

    // Changing offset in slot 1 -> must re-emit.
    offs[1] = 128;
    assert(state.ShouldBindVertexBuffers(true, bufs, offs, sizes, strides));
    assert(!state.ShouldBindVertexBuffers(true, bufs, offs, sizes, strides));

    // Growing beyond previously bound count -> must re-emit.
    std::array<vk::Buffer, 4> bufs4{b0, b1, b2, b3};
    std::array<vk::DeviceSize, 4> offs4{0, 128, 128, 256};
    std::array<vk::DeviceSize, 4> sizes4{1024, 2048, 512, 256};
    std::array<vk::DeviceSize, 4> strides4{32, 16, 12, 8};
    assert(state.ShouldBindVertexBuffers(true, bufs4, offs4, sizes4, strides4));
    assert(!state.ShouldBindVertexBuffers(true, bufs4, offs4, sizes4, strides4));

    // Non-dynamic-input mode checks sizes and strides as well.
    state.Invalidate();
    assert(state.ShouldBindVertexBuffers(false, bufs, offs, sizes, strides));
    assert(!state.ShouldBindVertexBuffers(false, bufs, offs, sizes, strides));
    strides[2] = 24;
    assert(state.ShouldBindVertexBuffers(false, bufs, offs, sizes, strides));
    assert(!state.ShouldBindVertexBuffers(false, bufs, offs, sizes, strides));
}

void TestIndexBufferDeduplication() {
    Vulkan::BindState state;
    const auto ib1 = FakeBuffer(0x4001);
    const auto ib2 = FakeBuffer(0x4002);

    assert(state.ShouldBindIndexBuffer(ib1, 0, vk::IndexType::eUint16));
    assert(!state.ShouldBindIndexBuffer(ib1, 0, vk::IndexType::eUint16));

    assert(state.ShouldBindIndexBuffer(ib1, 16, vk::IndexType::eUint16));
    assert(!state.ShouldBindIndexBuffer(ib1, 16, vk::IndexType::eUint16));

    assert(state.ShouldBindIndexBuffer(ib1, 16, vk::IndexType::eUint32));
    assert(state.ShouldBindIndexBuffer(ib2, 16, vk::IndexType::eUint32));

    state.Invalidate();
    assert(state.ShouldBindIndexBuffer(ib2, 16, vk::IndexType::eUint32));
}

void TestForgetCompute() {
    // #155: a pass that binds its own compute pipeline inside Record() calls ForgetCompute(), so
    // the next guest dispatch of the pipeline bound before binds it again.
    Vulkan::BindState state;
    const auto gfx = FakePipeline(0x5001);
    const auto cmp = FakePipeline(0x5002);
    const auto ib = FakeBuffer(0x5003);
    assert(state.ShouldBindPipeline(vk::PipelineBindPoint::eGraphics, gfx));
    assert(state.ShouldBindPipeline(vk::PipelineBindPoint::eCompute, cmp));
    assert(state.ShouldBindIndexBuffer(ib, 0, vk::IndexType::eUint16));

    state.ForgetCompute();
    assert(state.ShouldBindPipeline(vk::PipelineBindPoint::eCompute, cmp));
    assert(!state.ShouldBindPipeline(vk::PipelineBindPoint::eCompute, cmp));
    // Graphics and index-buffer state are untouched.
    assert(!state.ShouldBindPipeline(vk::PipelineBindPoint::eGraphics, gfx));
    assert(!state.ShouldBindIndexBuffer(ib, 0, vk::IndexType::eUint16));
}

} // namespace

int main() {
    TestPipelineDeduplication();
    TestVertexInputDeduplication();
    TestVertexBufferDeduplication();
    TestIndexBufferDeduplication();
    TestForgetCompute();
    return 0;
}
