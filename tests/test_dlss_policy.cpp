// SPDX-License-Identifier: GPL-2.0-or-later
// #155: the DLSS / frame generation gate, its BB_FORCE_FRAME_GEN override, and the size checks
// that keep ultrawide (3440x1440) sizes away from the NVIDIA driver unless it can take them.
#include <cassert>
#include <cstdio>
#include <cstring>
#include "bbport_dlss_policy.h"

using namespace BbDlssPolicy;

int main() {
    // The override: only "1", default off.
    assert(!ForceFrameGen(nullptr));
    assert(!ForceFrameGen(""));
    assert(!ForceFrameGen("0"));
    assert(!ForceFrameGen("10"));
    assert(ForceFrameGen("1"));

    // The gate: an RTX 40 (supported, available) gets what NGX reports, 1..3.
    assert(FrameGenMaxCount(true, true, 1, false) == 1);
    assert(FrameGenMaxCount(true, true, 3, false) == 3);
    assert(FrameGenMaxCount(true, true, 0, false) == 1);
    assert(FrameGenMaxCount(true, true, 9, true) == 3);
    // An RTX 30 (3090 Ti): gated off by default, 2x only when forced.
    assert(FrameGenMaxCount(false, false, 0, false) == 0);
    assert(FrameGenMaxCount(false, true, 3, false) == 0);
    assert(FrameGenMaxCount(true, false, 3, false) == 0);
    assert(FrameGenMaxCount(false, false, 0, true) == 1);
    assert(FrameGenMaxCount(false, false, 3, true) == 1);

    // Quality modes: DLAA for equal sizes, never for an upscale.
    assert(QualityForSizes(3440, 1440, 3440, 1440) == 0);
    assert(QualityForSizes(2560, 1440, 3440, 1440) == 1); // 16:9 scene on the ultrawide output
    assert(QualityForSizes(2293, 960, 3440, 1440) == 1);  // 1.5x: Quality
    assert(QualityForSizes(1720, 720, 3440, 1440) == 3);  // 2x
    assert(QualityForSizes(1147, 480, 3440, 1440) == 4);  // 3x
    assert(QualityForSizes(0, 0, 3440, 1440) == 0);
    // The axis with the smaller ratio decides (1.0 tall here), the mode stays an upscale one.
    assert(QualityForSizes(1720, 1440, 3440, 1440) == 1);

    // Feature sizes: the ultrawide cases.
    assert(!FeatureSizeProblem(3440, 1440, 3440, 1440));
    assert(!FeatureSizeProblem(1720, 720, 3440, 1440));
    assert(!FeatureSizeProblem(2560, 1440, 3440, 1440));
    assert(FeatureSizeProblem(3440, 1440, 2560, 1440)); // scene wider than a 16:9 output
    assert(FeatureSizeProblem(2560, 1600, 3440, 1440)); // taller on one side only
    assert(FeatureSizeProblem(0, 1440, 3440, 1440));
    assert(FeatureSizeProblem(1720, 720, 0, 0));
    assert(FeatureSizeProblem(1720, 720, 40000, 1440));

    // Frame generation sizes: depth and motion never larger than the frame, and the HUD-less
    // copy the frame's size or absent.
    assert(!FrameGenSizeProblem(3440, 1440, 3440, 1440));
    assert(!FrameGenSizeProblem(3440, 1440, 1720, 720));
    assert(!FrameGenSizeProblem(3440, 1440, 1720, 720, 3440, 1440));
    assert(FrameGenSizeProblem(2560, 1440, 3440, 1440)); // the first wide scene at a 16:9 frame
    assert(FrameGenSizeProblem(3440, 1440, 1720, 720, 2560, 1440));
    assert(FrameGenSizeProblem(3440, 1440, 1720, 720, 3440, 0));
    assert(FrameGenSizeProblem(0, 0, 0, 0));
    assert(FrameGenSizeProblem(20000, 1440, 100, 100));

    std::puts("dlss-policy-test: PASS");
    return 0;
}
