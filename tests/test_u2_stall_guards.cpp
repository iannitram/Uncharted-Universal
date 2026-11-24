// SPDX-FileCopyrightText: Copyright 2026 shadPS4 Emulator Project
// SPDX-License-Identifier: GPL-2.0-or-later

#include "video_core/buffer_cache/arena_probation.h"
#include "video_core/renderer_vulkan/vk_scheduler.h"

#include <cassert>
#include <vector>

int main() {
    // 1. Failed bindSparse must NOT add a wait on the unsignaled timeline semaphore
    //    and must NOT retire quarantined_runs into free_runs.
    {
        Vulkan::SubmitInfo info{};
        bool has_pending_unbinds = true;
        std::vector<VideoCore::FreeArenaRun> free_runs;
        std::vector<VideoCore::FreeArenaRun> quarantined_runs = {
            {.memory = vk::DeviceMemory{}, .offset = 4096, .blocks = 4},
        };
        const vk::Semaphore sema = reinterpret_cast<VkSemaphore>(uintptr_t{0x1234});

        const bool ok = VideoCore::FinishSparseBindSubmission(
            false, info, sema, 42, has_pending_unbinds, free_runs, quarantined_runs);
        assert(!ok);
        assert(info.num_wait_semas == 0);
        assert(has_pending_unbinds);
        assert(free_runs.empty());
        assert(quarantined_runs.size() == 1);
    }

    // 2. Successful bindSparse adds the timeline wait and retires quarantined_runs.
    {
        Vulkan::SubmitInfo info{};
        bool has_pending_unbinds = true;
        std::vector<VideoCore::FreeArenaRun> free_runs;
        std::vector<VideoCore::FreeArenaRun> quarantined_runs = {
            {.memory = vk::DeviceMemory{}, .offset = 8192, .blocks = 8},
        };
        const vk::Semaphore sema = reinterpret_cast<VkSemaphore>(uintptr_t{0x5678});

        const bool ok = VideoCore::FinishSparseBindSubmission(
            true, info, sema, 99, has_pending_unbinds, free_runs, quarantined_runs);
        assert(ok);
        assert(info.num_wait_semas == 1);
        assert(info.wait_semas[0] == sema);
        assert(info.wait_ticks[0] == 99);
        assert(!has_pending_unbinds);
        assert(free_runs.size() == 1);
        assert(free_runs[0].offset == 8192);
        assert(free_runs[0].blocks == 8);
        assert(quarantined_runs.empty());
    }

    // 3. SubmitInfo::AddWait / AddSignal record semaphore/tick pairs in order.
    {
        Vulkan::SubmitInfo info{};
        const vk::Semaphore s1 = reinterpret_cast<VkSemaphore>(uintptr_t{0x1111});
        const vk::Semaphore s2 = reinterpret_cast<VkSemaphore>(uintptr_t{0x2222});
        info.AddWait(s1, 7);
        info.AddSignal(s2, 11);
        assert(info.num_wait_semas == 1);
        assert(info.wait_semas[0] == s1);
        assert(info.wait_ticks[0] == 7);
        assert(info.num_signal_semas == 1);
        assert(info.signal_semas[0] == s2);
        assert(info.signal_ticks[0] == 11);
    }

    return 0;
}
