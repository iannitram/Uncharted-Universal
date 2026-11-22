// SPDX-License-Identifier: GPL-2.0-only
#pragma once

#include <cstddef>
#include <cstdint>

extern "C" {

typedef struct BbFexVector128 {
    uint64_t lo;
    uint64_t hi;
} BbFexVector128;

typedef struct BbFexGuestState {
    uint64_t gpr[16];
    uint64_t rsp;
    uint64_t rip;
    uint64_t rflags;
    BbFexVector128 xmm[16];
    uint64_t fs_base;
    uint64_t gs_base;
} BbFexGuestState;

typedef uint64_t (*BbFexHostCallback)(BbFexGuestState* state, uint64_t arg0, uint64_t arg1,
                                      void* user_data);

typedef struct BbFexSmokeReport {
    int scalar_ok;
    int vector_ok;
    int callback_ok;
    int nested_callback_ok;
    int tls_isolation_ok;
    int memory_invalidation_ok;
    int teardown_ok;
} BbFexSmokeReport;

// Runs the synthetic FEXCore guest smoke checks (register/flags/vector state, guest-to-host
// callback ABI, thread/TLS isolation, memory mapping, invalidation, and teardown).
// Returns 0 when all checks pass, or a negative error code.
int bb_android_run_fex_smoke(BbFexSmokeReport* out_report);

} // extern "C"
