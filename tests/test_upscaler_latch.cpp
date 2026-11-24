// SPDX-License-Identifier: GPL-2.0-or-later

#include "video_core/renderer_vulkan/vk_temporal_upscaler.h"

#include <atomic>
#include <cassert>

int main() {
    std::atomic<int> in_use{1};
    bool failed = true;
    bool fsr4_failed = true;
    bool dlss_failed = true;
    bool xess_failed = false;

    // Keeping the same upscaler does not clear failure flags or report a switch.
    assert(!Vulkan::LatchUpscalerChoice(1, in_use, failed, fsr4_failed, dlss_failed, xess_failed));
    assert(in_use.load() == 1);
    assert(failed);
    assert(fsr4_failed);
    assert(dlss_failed);

    // Switching to another upscaler (e.g. DLSS -> TAA) latches the new choice and resets flags.
    xess_failed = true;
    assert(Vulkan::LatchUpscalerChoice(2, in_use, failed, fsr4_failed, dlss_failed, xess_failed));
    assert(in_use.load() == 2);
    assert(!failed);
    assert(!fsr4_failed);
    assert(!dlss_failed);
    assert(!xess_failed);

    return 0;
}
