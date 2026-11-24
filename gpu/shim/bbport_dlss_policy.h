// SPDX-License-Identifier: GPL-2.0-or-later OR MIT
// (both the port and the MIT-licensed bbport_dlss bridge include it)
// bbport: the pure decisions around DLSS and DLSS Frame Generation (#155), kept free of Vulkan
// and NGX so a unit test can run them (tests/test_dlss_policy.cpp). nvoglv64.dll, where the
// museum crashes of Supermedo/uncharted_pc#641 and #290 fault, is NVIDIA's Vulkan driver on
// Windows as much as its OpenGL one (the Vulkan ICD manifest names it), so those faults are the
// driver rejecting what the port handed it: sizes it cannot take, null resources, a feature
// used after its device. These checks refuse such input before the driver sees it, and the
// callers fail over (DLSS to FSR 3.1, frame generation to off) instead of crashing.
#pragma once

#include <algorithm>
#include <cstdint>

namespace BbDlssPolicy {

/// Largest image side NGX is handed (every Vulkan 1.3 device's maxImageDimension2D floor is
/// far lower than a driver that would accept more; this only stops garbage sizes).
inline constexpr uint32_t MaxSide = 16384;

/// BB_FORCE_FRAME_GEN=1: try DLSS Frame Generation on a GPU the NGX support query rejects
/// (a GeForce RTX 30 such as the 3090 Ti). Testing only; off by default.
inline bool ForceFrameGen(const char* env) {
    return env && env[0] == '1' && env[1] == '\0';
}

/// Frames DLSS Frame Generation may add per game frame (0 none, 1 = 2x .. 3 = 4x), from what NGX
/// reported. `force` bypasses the support gate with the 2x count: the feature creation that
/// follows still has to succeed, and when it does not, frame generation falls over to FSR 3.1.
inline int32_t FrameGenMaxCount(bool device_supported, bool ngx_available, unsigned ngx_multi_max,
                                bool force) {
    if (device_supported && ngx_available) {
        return int32_t(std::clamp(ngx_multi_max, 1u, 3u));
    }
    return force ? 1 : 0;
}

/// DLSS quality mode for an output/render size ratio (0 DLAA .. 4 Ultra Performance).
/// DLAA only for equal sizes: any real upscale needs at least the Quality mode.
inline int QualityForScale(float scale) {
    return scale >= 2.9f ? 4 : scale >= 1.95f ? 3 : scale >= 1.65f ? 2 : scale > 1.01f ? 1 : 0;
}

/// The mode for a render size and an output size. The smaller of the two axis ratios counts, so
/// a scene that is wider than it is taller than the output (a 16:9 scene on a 3440x1440
/// output) is not graded by its width alone; DLAA stays for identical sizes.
inline int QualityForSizes(uint32_t in_w, uint32_t in_h, uint32_t out_w, uint32_t out_h) {
    if (!in_w || !in_h) {
        return 0;
    }
    const int mode =
        QualityForScale(std::min(float(out_w) / float(in_w), float(out_h) / float(in_h)));
    return mode == 0 && (in_w != out_w || in_h != out_h) ? 1 : mode;
}

/// Null when DLSS can take a feature of these sizes, else why not. DLSS only upscales: a render
/// size larger than the output on either side (the first wide frames before the ultrawide
/// output size is settled) is refused here, not left to the driver.
inline const char* FeatureSizeProblem(uint32_t in_w, uint32_t in_h, uint32_t out_w,
                                      uint32_t out_h) {
    if (!in_w || !in_h || !out_w || !out_h) {
        return "a size is zero";
    }
    if (in_w > MaxSide || in_h > MaxSide || out_w > MaxSide || out_h > MaxSide) {
        return "a size is beyond 16384";
    }
    if (in_w > out_w || in_h > out_h) {
        return "the render size is larger than the output size";
    }
    return nullptr;
}

/// Null when DLSS Frame Generation can take these sizes: the backbuffer `w` x `h`, depth and
/// motion vectors `render_w` x `render_h` (the upscaled modes make them smaller, never larger),
/// and the HUD-less copy (0 x 0 when there is none, else the backbuffer's size).
inline const char* FrameGenSizeProblem(uint32_t w, uint32_t h, uint32_t render_w,
                                       uint32_t render_h, uint32_t hudless_w = 0,
                                       uint32_t hudless_h = 0) {
    if (!w || !h || !render_w || !render_h) {
        return "a size is zero";
    }
    if (w > MaxSide || h > MaxSide) {
        return "a size is beyond 16384";
    }
    if (render_w > w || render_h > h) {
        return "depth and motion are larger than the frame";
    }
    if ((hudless_w || hudless_h) && (hudless_w != w || hudless_h != h)) {
        return "the picture before the UI differs in size from the frame";
    }
    return nullptr;
}

} // namespace BbDlssPolicy
