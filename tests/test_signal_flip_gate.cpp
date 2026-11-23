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
    // 1. SignalFlip must defer GfxFlip onto the landing thread whenever Stage B and
    // RecorderFences are active, regardless of whether LandingGate::Active() is true.
    static_assert(AmdGpu::ShouldDeferFlipSignal(true, true));
    static_assert(!AmdGpu::ShouldDeferFlipSignal(false, true));
    static_assert(!AmdGpu::ShouldDeferFlipSignal(true, false));
    static_assert(!AmdGpu::ShouldDeferFlipSignal(false, false));

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
