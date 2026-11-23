#include "video_core/amdgpu/resource.h"
#include "video_core/amdgpu/tiling.h"
#include "video_core/texture_cache/image_info.h"
#include "video_core/texture_cache/tile.h"

#include <cassert>

namespace {

VideoCore::ImageInfo MakeMacroTiledInfo(u32 width, u32 height, u32 levels) {
    VideoCore::ImageInfo info{};
    info.tile_mode = AmdGpu::TileMode::Thin2DThin;
    info.array_mode = AmdGpu::GetArrayMode(info.tile_mode);
    info.type = AmdGpu::ImageType::Color2D;
    info.props.is_tiled = 1;
    info.size = {width, height, 1};
    info.pitch = width;
    info.resources.levels = levels;
    info.resources.layers = 1;
    info.num_bits = 32;
    info.num_samples = 1;
    info.UpdateSize();
    return info;
}

} // namespace

int main() {
    // 1. 512x512 32-bpp Thin2DThin (macro tile extent 128x64) with 8 mip levels:
    //    mips 0..2 (512x512, 256x256, 128x128) stay macro-tiled;
    //    mips 3..7 (64x64 down to 4x4) downgrade to micro-tiled.
    {
        const auto info = MakeMacroTiledInfo(512, 512, 8);
        assert(info.resources.levels == 8);
        assert((info.micro_mip_mask & 0b00000111u) == 0u);
        assert((info.micro_mip_mask & 0b11111000u) == 0b11111000u);
    }

    // 2. Non-square mip where unaligned width < macro tile pitch (120 < 128) at mip 1:
    //    must still be marked micro-tiled even if micro-tile padding aligns pitch.
    {
        bool downgraded = false;
        const auto [pitch, height, size] = VideoCore::ImageSizeMacroTiled(
            120, 64, 1, 32, 1, AmdGpu::TileMode::Thin2DThin, 1, false, &downgraded);
        assert(downgraded);
        assert(pitch == 120);
        assert(height == 64);
        assert(size == 120 * 64 * 4);

        const auto info = MakeMacroTiledInfo(240, 128, 2);
        assert((info.micro_mip_mask & 0b01u) == 0u);
        assert((info.micro_mip_mask & 0b10u) == 0b10u);
    }

    // 3. Mip 0 is never downgraded even when smaller than the macro-tile extent.
    {
        bool downgraded = true;
        VideoCore::ImageSizeMacroTiled(
            64, 64, 1, 32, 1, AmdGpu::TileMode::Thin2DThin, 0, false, &downgraded);
        assert(!downgraded);

        const auto info = MakeMacroTiledInfo(64, 64, 1);
        assert(info.micro_mip_mask == 0u);
    }
}
