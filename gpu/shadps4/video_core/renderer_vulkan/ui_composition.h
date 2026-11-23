// SPDX-License-Identifier: GPL-2.0-or-later
#pragma once

#include <array>
#include <cmath>
#include <cstdint>

namespace Vulkan::UiComposition {

enum class Background { None, Copy, Temporal };

// UI composition must not require scene depth/camera or an active FSR context.
constexpr Background Choose(bool scaled, bool ui_draw, bool scene_ready, bool fsr_active) {
    if (!scaled || !ui_draw) {
        return Background::None;
    }
    return scene_ready && fsr_active ? Background::Temporal : Background::Copy;
}

inline bool NativeViewport(float width, float height) {
    return std::abs(std::abs(width) - 1920.0f) < 0.5f &&
           std::abs(std::abs(height) - 1080.0f) < 0.5f;
}

// Scaleform draws, including the first stencil/movie pass. A native-size viewport
// alone also matches every fullscreen post pass when guest targets stay at 1080p.
constexpr bool MovieShader(uint64_t hash) {
    return hash == 0x34e8a281 || hash == 0x81d336ce || hash == 0x09957251 ||
           hash == 0x24042a9b || hash == 0xa400228b;
}

struct SafeRect {
    uint32_t x = 0, y = 0, width = 0, height = 0;
};

struct Transform {
    std::array<float, 2> scale{1.0f, 1.0f};
    std::array<float, 2> offset{0.0f, 0.0f};
};

// Matches aspect::NATIVE_TOLERANCE_PERCENT (1%) in crates/core/src/aspect.rs.
constexpr bool IsSixteenByNine(uint32_t width, uint32_t height) {
    if (width == 0 || height == 0) {
        return false;
    }
    const uint64_t wide = uint64_t(width) * 9u;
    const uint64_t tall = uint64_t(height) * 16u;
    const uint64_t diff = wide > tall ? wide - tall : tall - wide;
    return diff * 100u <= tall;
}

// Anchors 16:9 UI/HUD/menu/pause/map coordinates inside the centered 16:9 safe area of
// output_width x output_height so non-16:9 screens (6:5, 3:2, 16:10, 17.4:9) do not stretch
// the 2D layer. Non-native-UI display copies and non-16:9 guest targets fill the output frame.
inline Transform Anchor(uint32_t guest_width, uint32_t guest_height,
                        uint32_t output_width, uint32_t output_height,
                        bool native_ui, bool native_coordinates,
                        SafeRect safe_area) {
    if (output_width == 0 || output_height == 0) {
        return {};
    }
    const uint32_t src_w = native_coordinates ? 1920u : guest_width;
    const uint32_t src_h = native_coordinates ? 1080u : guest_height;
    if (src_w == 0 || src_h == 0) {
        return {};
    }
    if (native_ui && (native_coordinates || IsSixteenByNine(src_w, src_h)) &&
        safe_area.width > 0 && safe_area.height > 0) {
        return {
            {float(safe_area.width) / float(src_w), float(safe_area.height) / float(src_h)},
            {float(safe_area.x), float(safe_area.y)},
        };
    }
    return {
        {float(output_width) / float(src_w), float(output_height) / float(src_h)},
        {0.0f, 0.0f},
    };
}

inline std::array<float, 2> Scale(uint32_t guest_width, uint32_t guest_height,
                                uint32_t output_width, uint32_t output_height,
                                bool native_coordinates) {
    return {float(output_width) / float(native_coordinates ? 1920 : guest_width),
            float(output_height) / float(native_coordinates ? 1080 : guest_height)};
}

} // namespace Vulkan::UiComposition
