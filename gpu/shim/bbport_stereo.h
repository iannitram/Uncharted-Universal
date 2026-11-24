// SPDX-License-Identifier: GPL-2.0-or-later
#pragma once

#include "bbport_settings.h"

#include <algorithm>
#include <cstdint>

namespace BbStereo {

struct Rect {
    std::int32_t x{0};
    std::int32_t y{0};
    std::int32_t width{0};
    std::int32_t height{0};
};

/// True for modes that composite left and right eyes into a single output frame.
inline bool IsComposed(int stereo_mode) {
    return stereo_mode == BbSettings::StereoSideBySide ||
           stereo_mode == BbSettings::StereoTopBottom;
}

/// Computes the destination sub-rectangle within `fit` for `eye` (0 = left, 1 = right)
/// under `stereo_mode`. Non-composed modes (Off, Passthrough) return `fit` unchanged.
inline Rect EyeRect(int stereo_mode, int eye, std::int32_t fit_x, std::int32_t fit_y,
                    std::int32_t fit_width, std::int32_t fit_height) {
    const std::int32_t w = std::max<std::int32_t>(0, fit_width);
    const std::int32_t h = std::max<std::int32_t>(0, fit_height);
    const int e = eye & 1;
    switch (stereo_mode) {
    case BbSettings::StereoSideBySide: {
        const std::int32_t half_w = w / 2;
        return e == 0 ? Rect{fit_x, fit_y, half_w, h}
                      : Rect{fit_x + half_w, fit_y, w - half_w, h};
    }
    case BbSettings::StereoTopBottom: {
        const std::int32_t half_h = h / 2;
        return e == 0 ? Rect{fit_x, fit_y, w, half_h}
                      : Rect{fit_x, fit_y + half_h, w, h - half_h};
    }
    default:
        return Rect{fit_x, fit_y, w, h};
    }
}

} // namespace BbStereo
