// SPDX-License-Identifier: GPL-2.0-or-later

#include "video_core/texture_cache/image_info.h"

#include <cassert>

using VideoCore::DownloadWriteSize;
using VideoCore::ImageInfo;
using VideoCore::IsTinyReadbackInfo;

namespace {

ImageInfo MakeTinyInfo(u32 width, u32 height, u32 pitch, bool tiled, bool is_depth, u32 bpp) {
    ImageInfo info{};
    info.guest_address = 0x10000000;
    info.size = {width, height, 1};
    info.resources.levels = 1;
    info.resources.layers = 1;
    info.num_bits = bpp;
    info.pitch = pitch;
    info.props.is_tiled = tiled ? 1u : 0u;
    info.props.is_depth = is_depth ? 1u : 0u;
    return info;
}

} // namespace

int main() {
    // 1x1 R32F tiled autofocus target qualifies when BB_TINY_READBACK=1.
    const auto autofocus_tiled = MakeTinyInfo(1, 1, 8, true, false, 32);
    assert(IsTinyReadbackInfo(autofocus_tiled, true));
    assert(!IsTinyReadbackInfo(autofocus_tiled, false));

    // Only the single copied texel (4 bytes) should be written back to guest memory, not the
    // full tiled pitch (8 * 4 = 32 bytes) which would copy uninitialized staging bytes.
    assert(DownloadWriteSize(autofocus_tiled) == 4);

    // 1x1 depth target also qualifies for tiny readback when enabled.
    const auto depth_1x1 = MakeTinyInfo(1, 1, 8, true, true, 32);
    assert(IsTinyReadbackInfo(depth_1x1, true));
    assert(DownloadWriteSize(depth_1x1) == 4);

    // Linear images up to 4x4 qualify and write their full pitched size.
    const auto linear_4x4 = MakeTinyInfo(4, 4, 4, false, false, 32);
    assert(IsTinyReadbackInfo(linear_4x4, true));
    assert(DownloadWriteSize(linear_4x4) == 64);

    // Tiled images larger than 1x1 or linear images larger than 16 texels do not qualify.
    const auto tiled_2x2 = MakeTinyInfo(2, 2, 8, true, false, 32);
    assert(!IsTinyReadbackInfo(tiled_2x2, true));

    const auto linear_5x4 = MakeTinyInfo(5, 4, 5, false, false, 32);
    assert(!IsTinyReadbackInfo(linear_5x4, true));

    // Zero guest address or 3D volume images do not qualify.
    auto no_addr = autofocus_tiled;
    no_addr.guest_address = 0;
    assert(!IsTinyReadbackInfo(no_addr, true));

    auto volume = MakeTinyInfo(1, 1, 1, false, false, 32);
    volume.size.depth = 2;
    assert(!IsTinyReadbackInfo(volume, true));
}
