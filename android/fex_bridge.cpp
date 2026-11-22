// SPDX-License-Identifier: GPL-2.0-only
#include "android/fex_bridge.h"

namespace bb::android {

static_assert(sizeof(BbFexVector128) == 16 && alignof(BbFexVector128) == 8,
              "BbFexVector128 layout must match crates/runtime FexVector128");
static_assert(sizeof(BbFexGuestContext) == 424 && alignof(BbFexGuestContext) == 8,
              "BbFexGuestContext layout must match crates/runtime FexGuestContext");
static_assert(sizeof(BbFexCallArgs) == 136 && alignof(BbFexCallArgs) == 8,
              "BbFexCallArgs layout must match crates/runtime FexCallArgs");
static_assert(sizeof(BbFexFaultInfo) == 24 && alignof(BbFexFaultInfo) == 8,
              "BbFexFaultInfo layout must match crates/runtime FexFaultInfo");

bool VerifyRuntimeAbi() {
    return uncharted_runtime_version() == BB_FEX_RUNTIME_ABI_VERSION &&
           runtime_fex_abi_version() == BB_FEX_RUNTIME_ABI_VERSION;
}

uint64_t CreateGuestContext(uint64_t fs_base, uint64_t gs_base) {
    if (!VerifyRuntimeAbi()) {
        return 0;
    }
    return runtime_fex_context_create(fs_base, gs_base);
}

int DestroyGuestContext(uint64_t handle) {
    return runtime_fex_context_destroy(handle);
}

int SnapshotGuestContext(uint64_t handle, BbFexGuestContext* out_ctx) {
    return runtime_fex_context_snapshot(handle, out_ctx);
}

int RegisterHostCallback(uint32_t func_id, BbFexBridgeCallback func, void* user_data) {
    return runtime_fex_register_callback(func_id, func, user_data);
}

int UnregisterHostCallback(uint32_t func_id) {
    return runtime_fex_unregister_callback(func_id);
}

int InvokeGuestBridge(uint64_t handle, const BbFexCallArgs& args, uint64_t* out_result) {
    return runtime_fex_call(handle, &args, out_result);
}

int ReportGuestFault(int signal, uint64_t fault_rip, uint64_t fault_address) {
    return runtime_fex_fault_handoff(signal, fault_rip, fault_address);
}

int ReadLastFault(BbFexFaultInfo* out_fault) {
    return runtime_fex_last_fault(out_fault);
}

} // namespace bb::android
