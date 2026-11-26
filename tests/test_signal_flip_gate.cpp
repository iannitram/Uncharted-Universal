// SPDX-License-Identifier: GPL-2.0-or-later

#include "bbport_copy.h"
#include "video_core/amdgpu/liverpool.h"

#include <atomic>
#include <cassert>
#include <chrono>
#include <thread>

namespace Common {
void SetCurrentThreadName(const char*) {}
} // namespace Common

int main() {
    // 1. SignalFlip defers GfxFlip onto the landing thread only while a landing is pending, on
    // Stage B with RecorderFences on. Otherwise the flip handler runs on the GPU command thread,
    // which owns the draw scheduler the frame is prepared on.
    for (int bits = 0; bits < 8; ++bits) {
        const bool gate = bits & 1, stage_b = bits & 2, fences = bits & 4;
        assert(AmdGpu::ShouldDeferFlipSignal(gate, stage_b, fences) == (bits == 7));
    }
    static_assert(AmdGpu::ShouldDeferFlipSignal(true, true, true));
    static_assert(!AmdGpu::ShouldDeferFlipSignal(false, true, true));

    // 2. BbCopy::AfterCopies must deliver its callback strictly after queued async copies
    // complete, even when no LandingGate copy is pending.
    std::atomic<int> step{0};
    BbCopy::Async([&] {
        std::this_thread::sleep_for(std::chrono::milliseconds(5));
        step.store(1, std::memory_order_release);
    });
    BbCopy::AfterCopies([&] {
        assert(step.load(std::memory_order_acquire) == 1);
        step.store(2, std::memory_order_release);
    });

    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(2);
    while (step.load(std::memory_order_acquire) < 2 &&
           std::chrono::steady_clock::now() < deadline) {
        std::this_thread::yield();
    }
    assert(step.load(std::memory_order_acquire) == 2);
    return 0;
}
