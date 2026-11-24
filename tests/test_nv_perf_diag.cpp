// SPDX-FileCopyrightText: Copyright 2026 shadPS4 Emulator Project
// SPDX-License-Identifier: GPL-2.0-or-later

#include "bbport_diag.h"
#include "bbport_toggles.h"

#include <cassert>
#include <cstdint>
#include <string>

namespace {

void TestHostVisibleDeviceLocalBytesNoDoubleCount() {
    VkPhysicalDeviceMemoryProperties mem{};
    mem.memoryHeapCount = 3;
    // Heap 0: 8 GiB VRAM (device-local only)
    mem.memoryHeaps[0] = {8ull << 30, VK_MEMORY_HEAP_DEVICE_LOCAL_BIT};
    // Heap 1: 32 GiB system RAM (host-only)
    mem.memoryHeaps[1] = {32ull << 30, 0};
    // Heap 2: 256 MiB BAR (device-local + host-visible)
    mem.memoryHeaps[2] = {256ull << 20, VK_MEMORY_HEAP_DEVICE_LOCAL_BIT};

    mem.memoryTypeCount = 5;
    mem.memoryTypes[0] = {VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT, 0};
    mem.memoryTypes[1] = {VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT, 1};
    mem.memoryTypes[2] = {VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT |
                              VK_MEMORY_PROPERTY_HOST_CACHED_BIT,
                          1};
    // Two memory types on the same 256 MiB BAR heap: must only count heap 2 once.
    mem.memoryTypes[3] = {VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT | VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT |
                              VK_MEMORY_PROPERTY_HOST_COHERENT_BIT,
                          2};
    mem.memoryTypes[4] = {VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT | VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT |
                              VK_MEMORY_PROPERTY_HOST_CACHED_BIT,
                          2};

    assert(bbport::HostVisibleDeviceLocalBytes(mem) == (256ull << 20));
}

void TestHostVisibleDeviceLocalBytesRebarOrUnified() {
    VkPhysicalDeviceMemoryProperties mem{};
    mem.memoryHeapCount = 2;
    mem.memoryHeaps[0] = {16ull << 30, VK_MEMORY_HEAP_DEVICE_LOCAL_BIT};
    mem.memoryHeaps[1] = {16ull << 30, 0};
    mem.memoryTypeCount = 2;
    mem.memoryTypes[0] = {VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT | VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT |
                              VK_MEMORY_PROPERTY_HOST_COHERENT_BIT,
                          0};
    mem.memoryTypes[1] = {VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT, 1};

    assert(bbport::HostVisibleDeviceLocalBytes(mem) == (16ull << 30));
}

void TestFormatGpuCapsSummary() {
    const bbport::GpuCapsSummary caps{
        .vendor_id = 0x10de,
        .device_id = 0x1e87,
        .driver_id = 4,
        .subgroup_size = 32,
        .subgroup_64 = false,
        .vertex_input_dynamic = true,
        .barycentric = true,
        .dynamic_color_write_mask = true,
        .max_push_descriptors = 32,
        .gfx_queue_family = 0,
        .side_queue_family = 1,
        .sparse_queue_family = 0,
        .host_visible_vram_mib = 256,
        .memory_budget_mib = 7680,
        .vram_legacy = false,
    };
    const std::string text = bbport::FormatGpuCapsSummary(caps);
    assert(text.find("driverId=4 vendor=10de device=1e87") != std::string::npos);
    assert(text.find("subgroup=32 (req64=0)") != std::string::npos);
    assert(text.find("queues(gfx=0 side=1 sparse=0)") != std::string::npos);
    assert(text.find("barVram=256MiB budget=7680MiB vramLegacy=0") != std::string::npos);
}

void TestBbStatsCounters() {
    assert(BbStats::present_wait_ns.load() == 0);
    assert(BbStats::worker_compiles.load() == 0);
    assert(BbStats::worker_compile_ns.load() == 0);
    BbStats::present_wait_ns.fetch_add(1234, std::memory_order_relaxed);
    BbStats::worker_compiles.fetch_add(2, std::memory_order_relaxed);
    BbStats::worker_compile_ns.fetch_add(5678, std::memory_order_relaxed);
    assert(BbStats::present_wait_ns.exchange(0) == 1234);
    assert(BbStats::worker_compiles.exchange(0) == 2);
    assert(BbStats::worker_compile_ns.exchange(0) == 5678);
}

} // namespace

int main() {
    TestHostVisibleDeviceLocalBytesNoDoubleCount();
    TestHostVisibleDeviceLocalBytesRebarOrUnified();
    TestFormatGpuCapsSummary();
    TestBbStatsCounters();
    return 0;
}
