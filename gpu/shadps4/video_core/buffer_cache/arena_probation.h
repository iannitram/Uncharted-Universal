// SPDX-FileCopyrightText: Copyright 2026 shadPS4 Emulator Project
// SPDX-License-Identifier: GPL-2.0-or-later
//
// bbport (after BilalReloaded's port / Mevlock, uncharted_pc-dev@2d0b81552 & 8174c3153, GPL-2.0):
// Buffer-arena VRAM budget, probation watch, and GPU-timeline-deferred sparse unbinds.
#pragma once

#include <algorithm>
#include <vector>

#include "common/types.h"
#include "video_core/renderer_vulkan/vk_common.h"

namespace VideoCore {

/// A contiguous run of arena backing memory that has been unmapped and is available for reuse.
struct FreeArenaRun {
    vk::DeviceMemory memory;
    u64 offset; ///< bytes into `memory`
    u64 blocks;
};

/// Unbinds (and their backing runs) of ranges released while the command buffer with scheduler
/// tick `tick` was recorded: held until the GPU has finished that tick.
template <typename ArenaBindsT>
struct DeferredArenaUnbinds {
    u64 tick;
    std::vector<ArenaBindsT> binds;
    std::vector<FreeArenaRun> runs;
};

/// A 1 MiB guest-address window on probation before its backing memory is given back: its page
/// table entries carry the watch bit (bit 0), so a shader reading through a pointer reports it
/// like a fault and rescues the window.
struct ProbationWindow {
    u64 window;
    u16 since;      ///< reclaim epoch when the watch started
    u16 last_epoch; ///< window_epoch when the watch started
    u64 blocks;     ///< resident blocks when the watch started
};

/// Drops pending sparse unbinds of `arena` overlapping `[lo, hi)` (in arena-relative bytes),
/// splitting any partially overlapping unbind so re-bound ranges are not unmapped afterwards.
template <typename ArenaBindsT, typename ArenaPtr>
inline void TrimArenaUnbinds(ArenaBindsT& pending, ArenaPtr arena, u64 lo, u64 hi) {
    if (pending.arena != arena) {
        return;
    }
    decltype(pending.binds) kept;
    bool changed = false;
    for (const auto& bind : pending.binds) {
        if (bind.memory || bind.resourceOffset >= hi || bind.resourceOffset + bind.size <= lo) {
            kept.push_back(bind);
            continue;
        }
        changed = true;
        if (bind.resourceOffset < lo) {
            kept.push_back({.resourceOffset = bind.resourceOffset, .size = lo - bind.resourceOffset});
        }
        if (bind.resourceOffset + bind.size > hi) {
            kept.push_back({.resourceOffset = hi, .size = bind.resourceOffset + bind.size - hi});
        }
    }
    if (changed) {
        pending.binds = std::move(kept);
    }
}

/// Computes the resident-arena byte limit above which unused windows are reclaimed.
inline u64 ComputeArenaReclaimLimitBytes(u64 total_memory_budget_bytes, u64 env_limit_mib) {
    const u64 mib = env_limit_mib ? env_limit_mib
                                  : std::max<u64>(768, (total_memory_budget_bytes >> 20) / 4);
    return std::max<u64>(mib, 64) << 20;
}

/// Computes the probation duration in 2-second epochs from `age_epochs` (quarter of age, min 3).
inline u16 ComputeProbationEpochs(u16 age_epochs, bool disabled) {
    return disabled ? u16(0) : std::max<u16>(3, age_epochs / 4);
}

} // namespace VideoCore
