// SPDX-License-Identifier: GPL-2.0-or-later
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <initializer_list>

#include "vertex_input_deny.h"

int main() {
    using bbport::VertexInputDynamicDenied;
    for (uint32_t id : {0x5912u, 0x3e92u, 0x5917u, 0x3ea0u, 0x3185u}) {
        assert(VertexInputDynamicDenied(0x8086, id));
    }
    // HD/UHD 6xx variants across Gemini, Kaby, Coffee, Whiskey, Amber and Comet Lake.
    for (uint32_t id : {0x3184u, 0x5902u, 0x5906u, 0x590bu, 0x5916u, 0x591au,
                        0x591bu, 0x591cu, 0x591du, 0x591eu, 0x5921u, 0x5923u,
                        0x3e90u, 0x3e91u, 0x3e93u, 0x3e94u, 0x3e96u, 0x3e98u,
                        0x3e99u, 0x3e9au, 0x3e9bu, 0x3e9cu, 0x3ea1u, 0x3ea9u,
                        0x87c0u, 0x9ba5u, 0x9ba8u, 0x9bc5u, 0x9bc6u, 0x9bc8u,
                        0x9be6u, 0x9bf6u}) {
        assert(VertexInputDynamicDenied(0x8086, id));
    }
    // Iris Xe, Arc A770, Skylake HD 530, Iris Plus 640/655, unnamed UHD, unknown IDs.
    // These assertions reject the old all-Intel workaround.
    for (uint32_t id : {0x9a49u, 0x56a0u, 0x1912u, 0x5926u, 0x3ea5u, 0x87cau,
                        0x9bc4u, 0x0000u, 0xffffu, 0x15912u}) {
        assert(!VertexInputDynamicDenied(0x8086, id));
    }
    // No 16-bit PCI device ID, including a deny-list match, may deny another vendor.
    for (uint32_t vendor : {0x1002u, 0x10deu, 0x0000u, 0xffffu}) {
        for (uint32_t id = 0; id <= 0xffff; ++id) {
            assert(!VertexInputDynamicDenied(vendor, id));
        }
    }
    std::puts("vertex input deny tests passed");
}
