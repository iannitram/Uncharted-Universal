// SPDX-License-Identifier: GPL-2.0-or-later
// Synthetic verification test for the gpu/shim <-> BB_SHADPS4_CORE_DIR boundary (#198).
#include "bbport_core_boundary.h"

#include <cassert>
#include <cstddef>
#include <cstdio>
#include <string_view>

static_assert(BBPORT_SHIM_BOUNDARY_VERSION == 1);
static_assert(BbCoreBoundary::Version == 1);
static_assert(sizeof(BbRenderSize) == 8);
static_assert(sizeof(BbFovScale) == 12);
static_assert(sizeof(BbMovieRect) == 16);

int main() {
    // 1. Verify the boundary allow-list contains the core C ABI and key shim headers.
    bool has_bbgpu = false;
    bool has_toggles = false;
    bool has_dlss_bridge = false;
    for (const char* header : BbCoreBoundary::AllowedShimHeaders) {
        std::string_view sv(header);
        assert(!sv.empty());
        if (sv == "bbgpu.h") {
            has_bbgpu = true;
        } else if (sv == "bbport_toggles.h") {
            has_toggles = true;
        } else if (sv == "dlss_bridge/bbport_dlss_bridge.h") {
            has_dlss_bridge = true;
        }
    }
    assert(has_bbgpu);
    assert(has_toggles);
    assert(has_dlss_bridge);

    // 2. Verify C ABI struct zero-init / field layout invariants.
    BbGpuConfig cfg{};
    assert(cfg.title == nullptr);
    assert(cfg.prepare_render == nullptr);
    assert(cfg.render_context == nullptr);

    // 3. Verify pure inline shim boundary helpers work standalone.
    assert(BbFocusFix::FocusCentreShader(0x9f801363ull));
    assert(!BbFocusFix::FocusCentreShader(0x12345678ull));
    assert(BbNanGuard::NanGuardMode() >= 0 && BbNanGuard::NanGuardMode() <= 2);
    assert(BbVideoPacing::FlipVblankDivisor(60, 0) == 1);
    assert(BbVideoPacing::FlipVblankDivisor(120, 1) == 4);

    // 4. Verify shader preload feature flags can be queried without a Vulkan device.
    const u64 flags = BbShaderPreload::CurrentFeatureFlags();
    (void)flags;

    std::printf("test_shim_boundary: PASS (boundary v%d, %zu allowed headers)\n",
                BbCoreBoundary::Version,
                sizeof(BbCoreBoundary::AllowedShimHeaders) /
                    sizeof(BbCoreBoundary::AllowedShimHeaders[0]));
    return 0;
}
