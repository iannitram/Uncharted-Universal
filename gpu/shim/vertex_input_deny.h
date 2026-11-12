// SPDX-License-Identifier: GPL-2.0-or-later
#pragma once

#include <cstdint>

namespace bbport {

// Default workaround for Intel HD/UHD 6xx; the renderer owns the environment override.
bool VertexInputDynamicDenied(uint32_t vendor_id, uint32_t device_id);

} // namespace bbport
