#include "video_core/texture_cache/image.h"

#include <array>
#include <cassert>

using VideoCore::HasGpuValidData;
using VideoCore::ImageFlagBits;

int main() {
    // Reproduce expansion after a streamed pool slot was rewritten by the guest. Refresh uploads
    // these new bytes; an unconditional copy from the stale cached image would replace them.
    constexpr std::array<unsigned char, 4> new_guest_bytes{0x10, 0x20, 0x30, 0x40};
    constexpr std::array<unsigned char, 4> stale_cached_pixels{0xA0, 0xB0, 0xC0, 0xD0};
    auto expanded = new_guest_bytes;
    const auto cpu_dirty = ImageFlagBits::CpuDirty | ImageFlagBits::GpuModified;
    if (HasGpuValidData(cpu_dirty)) {
        expanded = stale_cached_pixels;
    }
    assert(expanded == new_guest_bytes);

    // Conversely, retain real GPU-only image data when guest memory is not dirty.
    auto gpu_valid = ImageFlagBits::GpuModified;
    if (HasGpuValidData(gpu_valid)) {
        expanded = stale_cached_pixels;
    }
    assert(expanded == stale_cached_pixels);
}
