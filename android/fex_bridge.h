// SPDX-License-Identifier: GPL-2.0-only
// Selected-subset interface adaptation for Android FEXCore guest calls (#209).
// Reference boundary and notices: see android/LICENSES.md.
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// Must match UNCHARTED_RUNTIME_ABI_VERSION in crates/runtime/src/abi.rs.
#define BB_FEX_RUNTIME_ABI_VERSION 9u

#ifdef __cplusplus
extern "C" {
#endif

#ifndef BB_FEX_VECTOR128_DEFINED
#define BB_FEX_VECTOR128_DEFINED
typedef struct BbFexVector128 {
    uint64_t lo;
    uint64_t hi;
} BbFexVector128;
#endif

typedef struct BbFexGuestContext {
    uint64_t gpr[16];
    uint64_t rsp;
    uint64_t rip;
    uint64_t rflags;
    BbFexVector128 xmm[16];
    uint64_t fs_base;
    uint64_t gs_base;
} BbFexGuestContext;

typedef struct BbFexCallArgs {
    uint32_t func_id;
    uint32_t _pad;
    uint64_t scalars[6];
    uint64_t pointers[2];
    BbFexVector128 vectors[4];
} BbFexCallArgs;

typedef struct BbFexFaultInfo {
    int32_t signal;
    int32_t _pad;
    uint64_t fault_rip;
    uint64_t fault_address;
} BbFexFaultInfo;

typedef uint64_t (*BbFexBridgeCallback)(BbFexGuestContext* ctx, const BbFexCallArgs* args,
                                        void* user_data);

uint32_t uncharted_runtime_version(void);
uint32_t runtime_fex_abi_version(void);

uint64_t runtime_fex_context_create(uint64_t initial_fs, uint64_t initial_gs);
int runtime_fex_context_destroy(uint64_t handle);
int runtime_fex_context_snapshot(uint64_t handle, BbFexGuestContext* out_ctx);
int runtime_fex_register_callback(uint32_t func_id, BbFexBridgeCallback func, void* user_data);
int runtime_fex_unregister_callback(uint32_t func_id);
int runtime_fex_call(uint64_t handle, const BbFexCallArgs* args, uint64_t* out_result);
int runtime_fex_fault_handoff(int signal, uint64_t fault_rip, uint64_t fault_address);
int runtime_fex_last_fault(BbFexFaultInfo* out_fault);

#ifdef __cplusplus
} // extern "C"

namespace bb::android {

bool VerifyRuntimeAbi();
uint64_t CreateGuestContext(uint64_t fs_base, uint64_t gs_base);
int DestroyGuestContext(uint64_t handle);
int SnapshotGuestContext(uint64_t handle, BbFexGuestContext* out_ctx);
int RegisterHostCallback(uint32_t func_id, BbFexBridgeCallback func, void* user_data);
int UnregisterHostCallback(uint32_t func_id);
int InvokeGuestBridge(uint64_t handle, const BbFexCallArgs& args, uint64_t* out_result);
int ReportGuestFault(int signal, uint64_t fault_rip, uint64_t fault_address);
int ReadLastFault(BbFexFaultInfo* out_fault);

} // namespace bb::android
#endif
