// SPDX-License-Identifier: GPL-2.0-only
#include "android/fex_bridge.h"

namespace bb::android {

uint64_t CreateGuestContext(uint64_t fs_base, uint64_t gs_base) {
    return runtime_fex_context_create(fs_base, gs_base);
}

int DestroyGuestContext(uint64_t handle) {
    return runtime_fex_context_destroy(handle);
}

int InvokeGuestBridge(uint64_t handle, const BbFexCallArgs& args, uint64_t* out_result) {
    return runtime_fex_call(handle, &args, out_result);
}

int ReportGuestFault(int signal, uint64_t fault_rip, uint64_t fault_address) {
    return runtime_fex_fault_handoff(signal, fault_rip, fault_address);
}

} // namespace bb::android
