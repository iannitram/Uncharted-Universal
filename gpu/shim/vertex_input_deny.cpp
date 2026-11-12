// SPDX-License-Identifier: GPL-2.0-or-later
#include "vertex_input_deny.h"

#include <algorithm>
#include <iterator>

namespace bbport {
namespace {

// Mesa, tag mesa-24.3.0, include/pci_ids/iris_pci_ids.h (CHIPSET marketing names):
// https://gitlab.freedesktop.org/mesa/mesa/-/blob/mesa-24.3.0/include/pci_ids/iris_pci_ids.h
// Only HD/UHD 6xx names: exclude Skylake HD 5xx, Iris/Iris Plus and unnamed HD/UHD entries.
constexpr uint32_t denied_device_ids[] = {
    0x3184, // Gemini Lake UHD 605
    0x3185, // Gemini Lake UHD 600
    0x3e90, // Coffee Lake UHD 610
    0x3e91, 0x3e92, // Coffee Lake UHD 630
    0x3e93, // Coffee Lake UHD 610
    0x3e94, 0x3e96, // Coffee Lake UHD P630
    0x3e98, // Coffee Lake UHD 630
    0x3e99, // Coffee Lake UHD 610
    0x3e9a, // Coffee Lake UHD P630
    0x3e9b, // Coffee Lake UHD 630
    0x3e9c, // Coffee Lake UHD 610
    0x3ea0, // Whiskey Lake UHD 620
    0x3ea1, // Whiskey Lake UHD 610
    0x3ea9, // Coffee Lake UHD 620
    0x5902, 0x5906, 0x590b, // Kaby Lake HD 610
    0x5912, // Kaby Lake HD 630
    0x5916, // Kaby Lake HD 620
    0x5917, // Kaby Lake-R UHD 620
    0x591a, // Kaby Lake HD P630
    0x591b, // Kaby Lake HD 630
    0x591c, // Amber Lake UHD 615
    0x591d, // Kaby Lake HD P630
    0x591e, // Kaby Lake HD 615
    0x5921, // Kaby Lake HD 620
    0x5923, // Kaby Lake HD 635
    0x87c0, // Amber Lake UHD 617
    0x9ba5, 0x9ba8, // Comet Lake UHD 610
    0x9bc5, // Comet Lake UHD 630
    0x9bc6, // Comet Lake UHD P630
    0x9bc8, // Comet Lake UHD 630
    0x9be6, 0x9bf6, // Comet Lake UHD P630
};
static_assert(std::is_sorted(std::begin(denied_device_ids), std::end(denied_device_ids)));

} // namespace

bool VertexInputDynamicDenied(uint32_t vendor_id, uint32_t device_id) {
    return vendor_id == 0x8086 &&
           std::binary_search(std::begin(denied_device_ids), std::end(denied_device_ids), device_id);
}

} // namespace bbport
