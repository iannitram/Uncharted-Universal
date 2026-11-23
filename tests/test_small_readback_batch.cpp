// SPDX-License-Identifier: GPL-2.0-or-later

#include "video_core/buffer_cache/guest_copy_batch.h"

#include <cassert>
#include <vector>

using VideoCore::GuestCopyBatchItem;
using VideoCore::GuestCopySlice;
using VideoCore::PlanGuestCopyBatches;

int main() {
    const int arena_a = 1;
    const int arena_b = 2;

    // Multiple small buffers in the same arena with the same gated flag coalesce into one batch,
    // taking the maximum tick and 64-byte-aligning each slice's staging offset.
    {
        const std::vector<GuestCopyBatchItem> items = {
            GuestCopyBatchItem{
                .arena_key = &arena_a,
                .tick = 10,
                .gated = false,
                .slices = {GuestCopySlice{.src_offset = 0x100, .target = 0x1000, .size = 16}},
            },
            GuestCopyBatchItem{
                .arena_key = &arena_a,
                .tick = 14,
                .gated = false,
                .slices = {GuestCopySlice{.src_offset = 0x200, .target = 0x2000, .size = 32}},
            },
            GuestCopyBatchItem{
                .arena_key = &arena_a,
                .tick = 12,
                .gated = false,
                .slices = {GuestCopySlice{.src_offset = 0x300, .target = 0x3000, .size = 64}},
            },
        };
        const auto batches = PlanGuestCopyBatches(items, 4096);
        assert(batches.size() == 1);
        assert(batches[0].arena_key == &arena_a);
        assert(batches[0].tick == 14);
        assert(!batches[0].gated);
        assert(batches[0].slices.size() == 3);
        assert(batches[0].slices[0].target == 0x1000);
        assert(batches[0].slices[1].target == 0x2000);
        assert(batches[0].slices[2].target == 0x3000);
        // 16 -> aligned to 64 + 32 = 96 -> aligned to 128 + 64 = 192.
        assert(batches[0].total_staging_bytes == 192);
    }

    // Items in different arenas or with different gated flags stay in separate batches.
    {
        const std::vector<GuestCopyBatchItem> items = {
            GuestCopyBatchItem{
                .arena_key = &arena_a,
                .tick = 5,
                .gated = false,
                .slices = {GuestCopySlice{.src_offset = 0, .target = 0x1000, .size = 16}},
            },
            GuestCopyBatchItem{
                .arena_key = &arena_b,
                .tick = 6,
                .gated = false,
                .slices = {GuestCopySlice{.src_offset = 0, .target = 0x2000, .size = 16}},
            },
            GuestCopyBatchItem{
                .arena_key = &arena_a,
                .tick = 7,
                .gated = true,
                .slices = {GuestCopySlice{.src_offset = 64, .target = 0x3000, .size = 16}},
            },
        };
        const auto batches = PlanGuestCopyBatches(items, 4096);
        assert(batches.size() == 3);
        assert(batches[0].arena_key == &arena_a && !batches[0].gated);
        assert(batches[1].arena_key == &arena_b && !batches[1].gated);
        assert(batches[2].arena_key == &arena_a && batches[2].gated);
    }

    // Exceeding max_staging_bytes splits into a new batch for the same arena.
    {
        const std::vector<GuestCopyBatchItem> items = {
            GuestCopyBatchItem{
                .arena_key = &arena_a,
                .tick = 1,
                .gated = false,
                .slices = {GuestCopySlice{.src_offset = 0, .target = 0x1000, .size = 64}},
            },
            GuestCopyBatchItem{
                .arena_key = &arena_a,
                .tick = 2,
                .gated = false,
                .slices = {GuestCopySlice{.src_offset = 64, .target = 0x1040, .size = 64}},
            },
            GuestCopyBatchItem{
                .arena_key = &arena_a,
                .tick = 3,
                .gated = false,
                .slices = {GuestCopySlice{.src_offset = 128, .target = 0x1080, .size = 64}},
            },
        };
        const auto batches = PlanGuestCopyBatches(items, 128);
        assert(batches.size() == 2);
        assert(batches[0].slices.size() == 2);
        assert(batches[0].tick == 2);
        assert(batches[1].slices.size() == 1);
        assert(batches[1].tick == 3);
    }

    // Invalid items (null arena, zero tick, empty slices) are skipped.
    {
        const std::vector<GuestCopyBatchItem> items = {
            GuestCopyBatchItem{.arena_key = nullptr,
                               .tick = 1,
                               .gated = false,
                               .slices = {GuestCopySlice{.src_offset = 0, .target = 0x1000, .size = 8}}},
            GuestCopyBatchItem{.arena_key = &arena_a,
                               .tick = 0,
                               .gated = false,
                               .slices = {GuestCopySlice{.src_offset = 0, .target = 0x1000, .size = 8}}},
            GuestCopyBatchItem{.arena_key = &arena_a, .tick = 1, .gated = false, .slices = {}},
        };
        const auto batches = PlanGuestCopyBatches(items, 4096);
        assert(batches.empty());
    }
}
