// SPDX-License-Identifier: GPL-2.0-or-later
#include <cassert>
#include <cmath>
#include <cstdio>
#include "gpu/shadps4/video_core/renderer_vulkan/ui_composition.h"

namespace {

// Mirrors bbgpu_safe_area / aspect::safe_area integer math for standalone C++ unit testing.
Vulkan::UiComposition::SafeRect ComputeSafeArea(uint32_t screen_w, uint32_t screen_h) {
    if (screen_w == 0 || screen_h == 0) {
        return {};
    }
    const uint64_t cw = screen_w, ch = screen_h;
    const uint32_t width = cw * 9 >= ch * 16 ? uint32_t(ch * 16 / 9) : screen_w;
    const uint32_t height = cw * 9 >= ch * 16 ? screen_h : uint32_t(cw * 9 / 16);
    return {(screen_w - width) / 2, (screen_h - height) / 2, width, height};
}

void AssertAnchoredNoStretch(uint32_t out_w, uint32_t out_h,
                             uint32_t expected_x, uint32_t expected_y,
                             uint32_t expected_w, uint32_t expected_h) {
    using namespace Vulkan::UiComposition;
    const SafeRect safe = ComputeSafeArea(out_w, out_h);
    assert(safe.x == expected_x && safe.y == expected_y &&
           safe.width == expected_w && safe.height == expected_h);

    // 1920x1080 native UI (HUD, menus, pause, map) anchors inside safe area with uniform scale.
    const Transform native_t = Anchor(960, 540, out_w, out_h, true, true, safe);
    assert(std::abs(native_t.offset[0] - float(expected_x)) < 0.01f);
    assert(std::abs(native_t.offset[1] - float(expected_y)) < 0.01f);
    assert(std::abs(native_t.scale[0] - float(expected_w) / 1920.0f) < 1e-5f);
    assert(std::abs(native_t.scale[1] - float(expected_h) / 1080.0f) < 1e-5f);
    assert(std::abs(native_t.scale[0] - native_t.scale[1]) < 0.002f);
    assert(native_t.offset[0] + 1920.0f * native_t.scale[0] <= float(out_w) + 0.01f);
    assert(native_t.offset[1] + 1080.0f * native_t.scale[1] <= float(out_h) + 0.01f);

    // 16:9 guest UI target (960x540) also anchors inside safe area without stretching.
    const Transform guest169_t = Anchor(960, 540, out_w, out_h, true, false, safe);
    assert(std::abs(guest169_t.offset[0] - float(expected_x)) < 0.01f);
    assert(std::abs(guest169_t.offset[1] - float(expected_y)) < 0.01f);
    assert(std::abs(guest169_t.scale[0] - guest169_t.scale[1]) < 0.002f);

    // Final display copy (native_ui = false) fills the output frame without squashing.
    const Transform display_t = Anchor(960, 540, out_w, out_h, false, false, safe);
    assert(display_t.offset[0] == 0.0f && display_t.offset[1] == 0.0f);
    assert(std::abs(display_t.scale[0] - float(out_w) / 960.0f) < 1e-5f);
    assert(std::abs(display_t.scale[1] - float(out_h) / 540.0f) < 1e-5f);
}

} // namespace

int main() {
    using namespace Vulkan::UiComposition;
    // Title/loading menus have no depth/camera. UI must still go to the native target.
    assert(Choose(true, true, false, true) == Background::Copy);
    assert(Choose(true, true, false, false) == Background::Copy);
    // In-game HUD: first reconstruct the scene, then draw the native UI over it.
    assert(Choose(true, true, true, true) == Background::Temporal);
    // Runtime FSR disable/failure must not change UI resolution.
    assert(Choose(true, true, true, false) == Background::Copy);
    assert(Choose(true, false, true, true) == Background::None);
    assert(Choose(false, true, true, true) == Background::None);
    assert(NativeViewport(1920, -1080));
    assert(!NativeViewport(960, -540));
    assert(MovieShader(0x34e8a281) && MovieShader(0x24042a9b));
    assert(!MovieShader(0x0b0acf50)); // fullscreen post/tonemap also has a native viewport
    assert(!MovieShader(0x6c62a79f)); // shadow geometry
    const auto display = Scale(960, 540, 1920, 1080, false);
    assert(display[0] == 2 && display[1] == 2);
    const auto ui = Scale(960, 540, 1920, 1080, true);
    assert(ui[0] == 1 && ui[1] == 1); // don't double an already native UI viewport
    const auto larger = Scale(960, 540, 3840, 2160, true);
    assert(larger[0] == 2 && larger[1] == 2);

    // M21b safe-area anchoring across all target aspects: 6:5, 3:2, 16:9, 16:10, 17.4:9.
    AssertAnchoredNoStretch(2208, 1840, 0, 299, 2208, 1242); // 6:5 (Fold unfolded)
    AssertAnchoredNoStretch(2266, 1488, 0, 107, 2266, 1274); // ~3:2 (iPad mini)
    AssertAnchoredNoStretch(2160, 1440, 0, 112, 2160, 1215); // exact 3:2
    AssertAnchoredNoStretch(1920, 1080, 0, 0, 1920, 1080);   // 16:9 (1080p)
    AssertAnchoredNoStretch(2560, 1440, 0, 0, 2560, 1440);   // 16:9 (1440p)
    AssertAnchoredNoStretch(2560, 1600, 0, 80, 2560, 1440);  // 16:10
    AssertAnchoredNoStretch(2092, 1080, 86, 0, 1920, 1080);  // 17.4:9 (Fold cover)

    // Non-16:9 guest target (e.g. BB_GAME_RES=2208x1840) must not be squashed into 16:9.
    const Transform native_aspect_t =
        Anchor(2208, 1840, 2208, 1840, true, false, ComputeSafeArea(2208, 1840));
    assert(native_aspect_t.scale[0] == 1.0f && native_aspect_t.scale[1] == 1.0f);
    assert(native_aspect_t.offset[0] == 0.0f && native_aspect_t.offset[1] == 0.0f);

    // Zero-dimension guards.
    assert(!IsSixteenByNine(0, 1080));
    assert(!IsSixteenByNine(1920, 0));
    const Transform zero_t = Anchor(0, 0, 1920, 1080, true, false, {0, 0, 1920, 1080});
    assert(zero_t.scale[0] == 1.0f && zero_t.scale[1] == 1.0f);

    std::puts("UI composition: PASS (menu without camera, HUD, FSR off, viewport scaling, M21b safe-area anchoring)");
}
