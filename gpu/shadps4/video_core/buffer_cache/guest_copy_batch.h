// SPDX-License-Identifier: GPL-2.0-or-later

#pragma once

#include <algorithm>
#include <span>
#include <vector>
#include "common/alignment.h"
#include "common/types.h"

namespace VideoCore {

struct GuestCopySlice {
    u64 src_offset{};
    VAddr target{};
    u64 size{};
};

struct GuestCopyBatchItem {
    const void* arena_key{};
    u64 tick{};
    bool gated{};
    std::vector<GuestCopySlice> slices;
};

struct PlannedGuestCopyBatch {
    const void* arena_key{};
    u64 tick{};
    bool gated{};
    u64 total_staging_bytes{};
    std::vector<GuestCopySlice> slices;
};

/// Groups deferred guest-copy items sharing `(arena_key, gated)` into batches whose 64-byte-aligned
/// staging footprint does not exceed `max_staging_bytes`. Preserves relative order within each
/// `(arena_key, gated)` group.
[[nodiscard]] inline std::vector<PlannedGuestCopyBatch> PlanGuestCopyBatches(
    std::span<const GuestCopyBatchItem> items, u64 max_staging_bytes) {
    std::vector<PlannedGuestCopyBatch> batches;
    for (const auto& item : items) {
        if (item.slices.empty() || !item.arena_key || item.tick == 0) {
            continue;
        }
        u64 item_bytes = 0;
        for (const auto& slice : item.slices) {
            if (slice.size == 0) {
                continue;
            }
            item_bytes = Common::AlignUp(item_bytes, 64) + slice.size;
        }
        if (item_bytes == 0 || item_bytes > max_staging_bytes) {
            continue;
        }
        PlannedGuestCopyBatch* target_batch = nullptr;
        for (auto it = batches.rbegin(); it != batches.rend(); ++it) {
            if (it->arena_key == item.arena_key && it->gated == item.gated) {
                const u64 aligned_base = Common::AlignUp(it->total_staging_bytes, 64);
                if (aligned_base + item_bytes <= max_staging_bytes) {
                    target_batch = &(*it);
                }
                break;
            }
        }
        if (!target_batch) {
            batches.push_back(PlannedGuestCopyBatch{
                .arena_key = item.arena_key,
                .tick = item.tick,
                .gated = item.gated,
                .total_staging_bytes = 0,
                .slices = {},
            });
            target_batch = &batches.back();
        } else {
            target_batch->tick = std::max(target_batch->tick, item.tick);
        }
        for (const auto& slice : item.slices) {
            if (slice.size == 0) {
                continue;
            }
            target_batch->total_staging_bytes =
                Common::AlignUp(target_batch->total_staging_bytes, 64) + slice.size;
            target_batch->slices.push_back(slice);
        }
    }
    return batches;
}

} // namespace VideoCore
