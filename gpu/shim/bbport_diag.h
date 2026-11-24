// SPDX-License-Identifier: GPL-2.0-or-later
#pragma once

#include <cstdint>
#include <cstdio>
#include <string>
#include <vulkan/vulkan_core.h>

namespace bbport {

struct GpuCapsSummary {
    std::uint32_t vendor_id{};
    std::uint32_t device_id{};
    std::uint32_t driver_id{};
    std::uint32_t subgroup_size{};
    bool subgroup_64{};
    bool vertex_input_dynamic{};
    bool barycentric{};
    bool dynamic_color_write_mask{};
    std::uint32_t max_push_descriptors{};
    std::uint32_t gfx_queue_family{};
    std::uint32_t side_queue_family{};
    std::uint32_t sparse_queue_family{};
    std::uint64_t host_visible_vram_mib{};
    std::uint64_t memory_budget_mib{};
    bool vram_legacy{};
};

/// Returns the total size in bytes of device-local heaps that have at least one host-visible
/// memory type (e.g. 256 MiB BAR without ReBAR vs full VRAM with ReBAR or unified memory).
/// Counts each heap at most once even when multiple memory types share it.
inline std::uint64_t HostVisibleDeviceLocalBytes(const VkPhysicalDeviceMemoryProperties& mem) {
    std::uint32_t counted_heaps = 0;
    std::uint64_t total = 0;
    const std::uint32_t type_count =
        mem.memoryTypeCount <= VK_MAX_MEMORY_TYPES ? mem.memoryTypeCount : VK_MAX_MEMORY_TYPES;
    const std::uint32_t heap_count =
        mem.memoryHeapCount <= VK_MAX_MEMORY_HEAPS ? mem.memoryHeapCount : VK_MAX_MEMORY_HEAPS;
    for (std::uint32_t i = 0; i < type_count; ++i) {
        const auto flags = mem.memoryTypes[i].propertyFlags;
        if ((flags & (VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT | VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT)) !=
            (VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT | VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT)) {
            continue;
        }
        const std::uint32_t heap = mem.memoryTypes[i].heapIndex;
        if (heap >= heap_count) {
            continue;
        }
        if ((mem.memoryHeaps[heap].flags & VK_MEMORY_HEAP_DEVICE_LOCAL_BIT) == 0) {
            continue;
        }
        if ((counted_heaps & (1u << heap)) != 0) {
            continue;
        }
        counted_heaps |= (1u << heap);
        total += mem.memoryHeaps[heap].size;
    }
    return total;
}

inline std::string FormatGpuCapsSummary(const GpuCapsSummary& c) {
    char buf[384];
    std::snprintf(
        buf, sizeof(buf),
        "GPU caps: driverId=%u vendor=%04x device=%04x subgroup=%u (req64=%d) "
        "dynVI=%d bary=%d cwm=%d pushDesc=%u queues(gfx=%u side=%u sparse=%u) "
        "barVram=%lluMiB budget=%lluMiB vramLegacy=%d",
        c.driver_id, c.vendor_id, c.device_id, c.subgroup_size, int(c.subgroup_64),
        int(c.vertex_input_dynamic), int(c.barycentric), int(c.dynamic_color_write_mask),
        c.max_push_descriptors, c.gfx_queue_family, c.side_queue_family, c.sparse_queue_family,
        static_cast<unsigned long long>(c.host_visible_vram_mib),
        static_cast<unsigned long long>(c.memory_budget_mib), int(c.vram_legacy));
    return std::string(buf);
}

} // namespace bbport
