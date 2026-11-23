// SPDX-License-Identifier: GPL-2.0-or-later

#include "video_core/amdgpu/regs.h"
#include "video_core/renderer_vulkan/vk_scheduler.h"

#include <cassert>

int main() {
    // Verify CLEAR_STATE register defaults from shadPS4 #5668 (9ac8b23bdc).
    AmdGpu::Regs regs{};
    regs.SetDefaults();

    assert(regs.reg_array[AmdGpu::Regs::ContextRegWordOffset + 0x0083] == 0x0000ffffu);
    assert(regs.reg_array[AmdGpu::Regs::ContextRegWordOffset + 0x008e] == 0xffffffffu);
    assert(regs.reg_array[AmdGpu::Regs::ContextRegWordOffset + 0x008f] == 0xffffffffu);
    assert(regs.color_target_mask.raw == 0xffffffffu);
    assert(regs.color_shader_mask.raw == 0xffffffffu);
    assert(regs.reg_array[AmdGpu::Regs::ContextRegWordOffset + 0x02aa] == 0x000000ffu);
    assert(regs.reg_array[AmdGpu::Regs::ContextRegWordOffset + 0x030e] == 0xffffffffu);
    assert(regs.reg_array[AmdGpu::Regs::ContextRegWordOffset + 0x030f] == 0xffffffffu);

    // Verify SubmitWaitStageMasks() from shadPS4 #5231 (43e4c470):
    // 1. Covers every slot in SubmitInfo::wait_semas (no OOB read when num_wait_semas > 2).
    // 2. Every slot waits on eAllCommands (no eColorAttachmentOutput-only wait).
    constexpr auto masks = Vulkan::SubmitWaitStageMasks();
    static_assert(masks.size() == std::tuple_size_v<decltype(Vulkan::SubmitInfo::wait_semas)>);
    for (const auto mask : masks) {
        assert(mask == vk::PipelineStageFlagBits::eAllCommands);
    }

    return 0;
}
