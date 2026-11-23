// SPDX-FileCopyrightText: Copyright 2024-2026 shadPS4 Emulator Project
// SPDX-License-Identifier: GPL-2.0-or-later

#pragma once

#include "common/types.h"
#include "video_core/amdgpu/cb_db_extent.h"
#include "video_core/amdgpu/tiling.h"
#include "video_core/renderer_vulkan/vk_common.h"
#include "video_core/texture_cache/types.h"

namespace AmdGpu {
struct ColorBuffer;
struct DepthBuffer;
struct Image;
enum class ImageType : u64;
} // namespace AmdGpu

namespace Libraries::VideoOut {
struct BufferAttributeGroup;
}

namespace Shader {
struct ImageResource;
}

namespace VideoCore {

struct ImageProperties {
    u32 is_volume : 1;
    u32 is_tiled : 1;
    u32 is_pow2 : 1;
    u32 is_block : 1;
    u32 is_depth : 1;
    u32 has_stencil : 1;
};

struct ImageInfo {
    ImageInfo() = default;
    ImageInfo(const Libraries::VideoOut::BufferAttributeGroup& group, VAddr cpu_address) noexcept;
    ImageInfo(const AmdGpu::ColorBuffer& buffer, AmdGpu::CbDbExtent hint) noexcept;
    ImageInfo(const AmdGpu::DepthBuffer& buffer, u32 num_slices, VAddr htile_address,
              AmdGpu::CbDbExtent hint, bool write_buffer = false) noexcept;
    ImageInfo(const AmdGpu::Image& image, const Shader::ImageResource& desc) noexcept;

    bool IsTiled() const {
        return tile_mode != AmdGpu::TileMode::DisplayLinearAligned;
    }

    Extent2D BlockDim() const {
        const auto dim = props.is_block ? 2 : 0;
        return Extent2D{pitch >> dim, size.height >> dim};
    }

    s32 MipOf(const ImageInfo& info) const;
    s32 SliceOf(const ImageInfo& info, s32 mip) const;

    bool IsCompatible(const ImageInfo& info) const;
    void UpdateSize();

    struct {
        VAddr cmask_addr;
        VAddr fmask_addr;
        VAddr htile_addr;
        s32 htile_clear_mask = -1;
    } meta_info{};

    ImageProperties props{};
    vk::Format pixel_format = vk::Format::eUndefined;
    AmdGpu::ImageType type;
    SubresourceExtent resources;
    Extent3D size{1, 1, 1};
    u32 num_bits{};
    u32 num_samples = 1;
    u32 pitch{};
    AmdGpu::TileMode tile_mode = AmdGpu::TileMode::DisplayLinearAligned;
    AmdGpu::ArrayMode array_mode = AmdGpu::ArrayMode::ArrayLinearAligned;
    struct MipInfo {
        u32 size;
        u32 pitch;
        u32 height;
        u32 offset;
    };
    std::array<MipInfo, 16> mips_layout;
    VAddr guest_address{};
    u32 guest_size{};
    u8 bank_swizzle{};
    bool alt_tile{};

    VAddr stencil_addr{};
    u32 stencil_size{};
};

/// Returns true if `info` qualifies for tiny non-blocking GPU->CPU readback (e.g. U2's 1x1 R32F
/// autofocus target or <= 4x4 linear targets).
[[nodiscard]] inline bool IsTinyReadbackInfo(const ImageInfo& info, bool enabled) noexcept {
    const auto& size = info.size;
    return enabled && info.guest_address != 0 && size.depth == 1 &&
           size.width * size.height <= 16 &&
           (!info.props.is_tiled || (size.width == 1 && size.height == 1));
}

/// Returns the number of valid bytes `vkCmdCopyImageToBuffer` writes for `info`. For a 1x1x1
/// single-layer image (whose tiled `pitch` may exceed 1 texel), only one texel is copied; writing
/// the full pitched `download_size` would copy uninitialized staging bytes into guest memory.
[[nodiscard]] inline u32 DownloadWriteSize(const ImageInfo& info) noexcept {
    const u32 bpp = info.num_bits / 8;
    if (info.size.width == 1 && info.size.height == 1 && info.size.depth == 1 &&
        info.resources.layers == 1) {
        return bpp;
    }
    return info.pitch * info.size.height * info.size.depth * info.resources.layers * bpp;
}

} // namespace VideoCore
