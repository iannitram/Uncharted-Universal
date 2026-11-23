// SPDX-FileCopyrightText: Copyright 2026 shadPS4 Emulator Project
// SPDX-License-Identifier: GPL-2.0-or-later

#include "common/interval_set.h"
#include "video_core/buffer_cache/arena_probation.h"

#include <cassert>
#include <cstdint>
#include <vector>
#include <boost/container/small_vector.hpp>

namespace {

struct TestBacking : public Interval {
    vk::DeviceMemory memory;
    u64 offset; ///< in blocks, matching BufferCache::Backing
    constexpr bool CanMergeWith(const TestBacking& other) const noexcept {
        return memory == other.memory && offset + (end - start) == other.offset;
    }
    constexpr TestBacking SubRange(u64 a, u64 b) const noexcept {
        return {{a, b}, memory, offset + (a - start)};
    }
};

struct TestArenaBinds {
    const void* arena;
    boost::container::small_vector<vk::SparseMemoryBind, 8> binds;
};

void TestIntervalRemoveAndBlockOffsets() {
    IntervalList<TestBacking> ranges;
    const auto mem_a = vk::DeviceMemory{reinterpret_cast<VkDeviceMemory>(uintptr_t{0x1000})};
    const auto mem_b = vk::DeviceMemory{reinterpret_cast<VkDeviceMemory>(uintptr_t{0x2000})};

    // Add two adjacent ranges backed by contiguous blocks in mem_a -> should merge.
    ranges.Add(TestBacking{{100, 120}, mem_a, 0});
    ranges.Add(TestBacking{{120, 140}, mem_a, 20});
    ranges.Add(TestBacking{{200, 220}, mem_b, 5});

    // Punch a hole in the middle of [100, 140): remove [110, 125).
    ranges.Remove(110, 125, [](const TestBacking& b, u64 s, u64 e) { return b.SubRange(s, e); });

    std::vector<TestBacking> out;
    ranges.ForEachInRange(0, 300, [&](const TestBacking& b) { out.push_back(b); });
    assert(out.size() == 3);
    assert(out[0].start == 100 && out[0].end == 110 && out[0].offset == 0 && out[0].memory == mem_a);
    assert(out[1].start == 125 && out[1].end == 140 && out[1].offset == 25 && out[1].memory == mem_a);
    assert(out[2].start == 200 && out[2].end == 220 && out[2].offset == 5 && out[2].memory == mem_b);

    // Removing across multiple intervals [105, 210) trims left of [100,110), drops [125,140), trims left of [200,220).
    ranges.Remove(105, 210, [](const TestBacking& b, u64 s, u64 e) { return b.SubRange(s, e); });
    out.clear();
    ranges.ForEachInRange(0, 300, [&](const TestBacking& b) { out.push_back(b); });
    assert(out.size() == 2);
    assert(out[0].start == 100 && out[0].end == 105 && out[0].offset == 0 && out[0].memory == mem_a);
    assert(out[1].start == 210 && out[1].end == 220 && out[1].offset == 15 && out[1].memory == mem_b);
}

void TestTrimArenaUnbinds() {
    const int arena1 = 1;
    const int arena2 = 2;
    const auto mem = vk::DeviceMemory{reinterpret_cast<VkDeviceMemory>(uintptr_t{0x3000})};

    TestArenaBinds pending{
        .arena = &arena1,
        .binds =
            {
                // Real bind (memory != null) overlapping [1000, 2000) must be preserved.
                vk::SparseMemoryBind{.resourceOffset = 1000, .size = 1000, .memory = mem},
                // Unbind completely inside [1000, 2000) -> dropped.
                vk::SparseMemoryBind{.resourceOffset = 1200, .size = 400},
                // Unbind spanning [800, 2400) -> split into [800, 1000) and [2000, 2400).
                vk::SparseMemoryBind{.resourceOffset = 800, .size = 1600},
                // Unbind disjoint [3000, 3500) -> kept untouched.
                vk::SparseMemoryBind{.resourceOffset = 3000, .size = 500},
            },
    };

    // Other arena must be ignored.
    VideoCore::TrimArenaUnbinds(pending, &arena2, 0, 10000);
    assert(pending.binds.size() == 4);

    VideoCore::TrimArenaUnbinds(pending, &arena1, 1000, 2000);
    assert(pending.binds.size() == 4);
    assert(pending.binds[0].memory == mem && pending.binds[0].resourceOffset == 1000 &&
           pending.binds[0].size == 1000);
    assert(!pending.binds[1].memory && pending.binds[1].resourceOffset == 800 &&
           pending.binds[1].size == 200);
    assert(!pending.binds[2].memory && pending.binds[2].resourceOffset == 2000 &&
           pending.binds[2].size == 400);
    assert(!pending.binds[3].memory && pending.binds[3].resourceOffset == 3000 &&
           pending.binds[3].size == 500);
}

void TestDeferredUnbindTimeline() {
    const int arena = 1;
    const auto mem1 = vk::DeviceMemory{reinterpret_cast<VkDeviceMemory>(uintptr_t{0x4000})};
    const auto mem2 = vk::DeviceMemory{reinterpret_cast<VkDeviceMemory>(uintptr_t{0x5000})};

    std::vector<VideoCore::DeferredArenaUnbinds<TestArenaBinds>> deferred;
    deferred.push_back({
        .tick = 10,
        .binds = {{.arena = &arena, .binds = {vk::SparseMemoryBind{.resourceOffset = 0, .size = 64}}}},
        .runs = {{mem1, 0, 1}},
    });
    deferred.push_back({
        .tick = 12,
        .binds = {{.arena = &arena, .binds = {vk::SparseMemoryBind{.resourceOffset = 64, .size = 64}}}},
        .runs = {{mem2, 64, 1}},
    });

    const u64 gpu_completed_tick = 10;
    const auto is_free = [&](u64 tick) { return tick <= gpu_completed_tick; };

    std::vector<vk::SparseMemoryBind> submitted_unbinds;
    std::vector<VideoCore::FreeArenaRun> quarantined;
    auto it = deferred.begin();
    for (; it != deferred.end() && is_free(it->tick); ++it) {
        for (const auto& b : it->binds) {
            submitted_unbinds.insert(submitted_unbinds.end(), b.binds.begin(), b.binds.end());
        }
        quarantined.insert(quarantined.end(), it->runs.begin(), it->runs.end());
    }
    deferred.erase(deferred.begin(), it);

    assert(submitted_unbinds.size() == 1 && submitted_unbinds[0].resourceOffset == 0);
    assert(quarantined.size() == 1 && quarantined[0].memory == mem1);
    assert(deferred.size() == 1 && deferred[0].tick == 12);
}

void TestBudgetAndProbationHelpers() {
    constexpr u64 MiB = 1ull << 20;
    constexpr u64 GiB = 1ull << 30;
    // Small card: 2 GiB budget -> floor of 768 MiB.
    assert(VideoCore::ComputeArenaReclaimLimitBytes(2 * GiB, 0) == 768 * MiB);
    // Medium card: 4 GiB budget -> 1024 MiB.
    assert(VideoCore::ComputeArenaReclaimLimitBytes(4 * GiB, 0) == 1024 * MiB);
    // Explicit override with 64 MiB floor.
    assert(VideoCore::ComputeArenaReclaimLimitBytes(4 * GiB, 32) == 64 * MiB);
    assert(VideoCore::ComputeArenaReclaimLimitBytes(4 * GiB, 512) == 512 * MiB);

    // Probation epochs: quarter of age, minimum 3, or 0 when disabled.
    assert(VideoCore::ComputeProbationEpochs(30, false) == 7);
    assert(VideoCore::ComputeProbationEpochs(8, false) == 3);
    assert(VideoCore::ComputeProbationEpochs(30, true) == 0);
}

} // namespace

int main() {
    TestIntervalRemoveAndBlockOffsets();
    TestTrimArenaUnbinds();
    TestDeferredUnbindTimeline();
    TestBudgetAndProbationHelpers();
    return 0;
}
