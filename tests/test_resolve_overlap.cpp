// SPDX-License-Identifier: GPL-2.0-or-later

#include "video_core/texture_cache/image_info.h"

#include <cassert>

namespace {

VideoCore::ImageInfo MakeSingleSliceInfo(u32 width, u32 height, u32 pitch, u32 guest_size,
                                         bool is_pow2, AmdGpu::TileMode tile_mode) {
    VideoCore::ImageInfo info{};
    info.size = {width, height, 1};
    info.pitch = pitch;
    info.guest_size = guest_size;
    info.tile_mode = tile_mode;
    info.props.is_pow2 = is_pow2;
    info.resources.levels = 1;
    info.resources.layers = 1;
    return info;
}

} // namespace

int main() {
    // shadPS4 #5761: equal-address images sharing tile_mode, size, pitch, and 1x1 levels/layers
    // are reusable even when props.is_pow2 matches and only guest_size differs.
    const auto cached =
        MakeSingleSliceInfo(1920, 1080, 1920, 1920 * 1080 * 4, false, AmdGpu::TileMode::DisplayLinearGeneral);
    const auto larger_guest_same_pow2 =
        MakeSingleSliceInfo(1920, 1080, 1920, 1920 * 1088 * 4, false, AmdGpu::TileMode::DisplayLinearGeneral);
    assert(VideoCore::IsGuestSizeOnlyMismatch(larger_guest_same_pow2, cached));

    // Also true when is_pow2 differs (the older pow2_padding_only case).
    const auto diff_pow2 =
        MakeSingleSliceInfo(1920, 1080, 1920, 2048 * 1080 * 4, true, AmdGpu::TileMode::DisplayLinearGeneral);
    assert(VideoCore::IsGuestSizeOnlyMismatch(diff_pow2, cached));

    // Mismatched dimensions, pitch, tile_mode, or multi-level/layer resources must return false.
    auto diff_size = cached;
    diff_size.size.width = 1280;
    assert(!VideoCore::IsGuestSizeOnlyMismatch(diff_size, cached));

    auto diff_pitch = cached;
    diff_pitch.pitch = 2048;
    assert(!VideoCore::IsGuestSizeOnlyMismatch(diff_pitch, cached));

    auto diff_tile = cached;
    diff_tile.tile_mode = AmdGpu::TileMode::Thin1DThin;
    assert(!VideoCore::IsGuestSizeOnlyMismatch(diff_tile, cached));

    auto multi_mip = cached;
    multi_mip.resources.levels = 2;
    assert(!VideoCore::IsGuestSizeOnlyMismatch(multi_mip, cached));

    auto multi_layer = cached;
    multi_layer.resources.layers = 2;
    assert(!VideoCore::IsGuestSizeOnlyMismatch(multi_layer, cached));

    return 0;
}
