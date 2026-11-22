#include "common/lru_cache.h"
#include "video_core/renderer_vulkan/vk_vram.h"

#include <cassert>
#include <cstdint>
#include <vector>

namespace {

struct FakeReclaimState {
    int calls = 0;
    bool recursive_result = true;
    bool aggressive_seen = false;
    std::uint64_t last_bytes = 0;
    std::uint64_t freed_budget = 0;
};

bool FakeReclaimCallback(void* ctx, std::uint64_t needed_bytes, bool aggressive) {
    auto* state = static_cast<FakeReclaimState*>(ctx);
    ++state->calls;
    state->last_bytes = needed_bytes;
    state->aggressive_seen = aggressive;
    // Simulate a nested allocation (e.g. DownloadImageMemory requesting a host staging buffer)
    // failing and calling Vram::Reclaim while already inside a reclaim pass.
    state->recursive_result = Vulkan::Vram::Reclaim(4096, true);
    if (!aggressive) {
        return false;
    }
    state->freed_budget += 64ull << 20;
    return true;
}

} // namespace

int main() {
    // 1. Re-entrancy guard and context-safe registration.
    FakeReclaimState state{};
    int other_ctx = 0;
    Vulkan::Vram::SetReclaim(&FakeReclaimCallback, &state);
    assert(!Vulkan::Vram::Reclaim(16ull << 20, false));
    assert(state.calls == 1);
    assert(!state.aggressive_seen);
    assert(!state.recursive_result); // Re-entrant call must be suppressed, not recurse.

    // Guard must reset after returning so subsequent aggressive reclaim succeeds.
    assert(Vulkan::Vram::Reclaim(32ull << 20, true));
    assert(state.calls == 2);
    assert(state.aggressive_seen);
    assert(state.last_bytes == (32ull << 20));
    assert(!state.recursive_result);
    assert(state.freed_budget == (64ull << 20));

    // ClearReclaim must only clear when ctx matches.
    Vulkan::Vram::ClearReclaim(&other_ctx);
    assert(Vulkan::Vram::reclaim != nullptr);
    Vulkan::Vram::ClearReclaim(&state);
    assert(Vulkan::Vram::reclaim == nullptr);
    assert(!Vulkan::Vram::Reclaim(4096, false));

    // 2. Integrated / large-memory GPUs (>8.5 GiB) keep Legacy() true for sizing rules
    // unless overridden, while ExplicitLegacy() stays false so OOM fallback still executes.
    const std::uint64_t saved_dev_mem = Vulkan::Vram::device_memory.load();
    Vulkan::Vram::device_memory.store(16ull << 30);
    assert(!Vulkan::Vram::ExplicitLegacy());
    assert(Vulkan::Vram::Legacy() || std::getenv("BB_VRAM_LEGACY") != nullptr);
    Vulkan::Vram::device_memory.store(4ull << 30);
    assert(!Vulkan::Vram::ExplicitLegacy());
    Vulkan::Vram::device_memory.store(saved_dev_mem);

    // 3. LRU critical-pressure invariant: skipping the non-aggressive pass when already
    // over critical prevents Touch(id, gc_tick) from hiding >60s-old items from drop.
    Common::LeastRecentlyUsedCache<std::uint32_t, std::uint64_t> lru;
    const std::size_t old_tiled_id = lru.Insert(1, 10);
    const std::size_t newer_id = lru.Insert(2, 50);
    const std::uint64_t gc_tick = 100;
    const std::uint64_t below_tick = gc_tick - 40; // 60
    std::vector<std::uint32_t> visited_aggressive;
    lru.ForEachItemBelow(below_tick, [&](std::uint32_t id) {
        visited_aggressive.push_back(id);
        return false;
    });
    assert(visited_aggressive == (std::vector<std::uint32_t>{1, 2}));
    // Touching old_tiled_id to gc_tick would have hidden it from ForEachItemBelow(below_tick).
    lru.Touch(old_tiled_id, gc_tick);
    visited_aggressive.clear();
    lru.ForEachItemBelow(below_tick, [&](std::uint32_t id) {
        visited_aggressive.push_back(id);
        return false;
    });
    assert(visited_aggressive == (std::vector<std::uint32_t>{2}));
    lru.Free(old_tiled_id);
    lru.Free(newer_id);

    return 0;
}
