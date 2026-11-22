// SPDX-License-Identifier: GPL-2.0-only
#pragma once

#ifdef __cplusplus
#include <cstddef>
#include <cstdint>
extern "C" {
#else
#include <stddef.h>
#include <stdint.h>
#endif

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

typedef struct BbFexSmokeDetails {
    uint64_t page_size;
    int is_android;
    int is_bionic;
    int is_arm64;
    int is_libcpp;
    int fexcore_backed;
    int rflags_ok;
    int unaligned_ok;
    int pointer_arg_ok;
    int stack_arg_ok;
    int vector_lanes_ok;
    int overlap_rejected_ok;
    int pthread_tls_dtor_ok;
    int mapping_protect_ok;
    int icache_invalidation_ok;
    uint32_t threads_tested;
    uint64_t invalidation_epochs;
    char target_triple[64];
    char fex_revision[48];
} BbFexSmokeDetails;

// Runs the synthetic FEXCore guest smoke checks (register/flags/vector state, guest-to-host
// callback ABI, thread/TLS isolation, memory mapping, invalidation, and teardown).
// Returns 0 when all checks pass, or a negative error code.
int bb_android_run_fex_smoke(BbFexSmokeReport* out_report);

// Extended smoke entry point returning both the summary report and platform/sub-check details.
// `out_details` may be NULL if only `out_report` is needed.
int bb_android_run_fex_smoke_detailed(BbFexSmokeReport* out_report, BbFexSmokeDetails* out_details);

// Formats a single-line structured marker string into `out_buf`.
// Returns the number of characters written (excluding NUL), or -1 on invalid arguments/buffer.
int bb_android_format_fex_smoke_report(const BbFexSmokeReport* report,
                                       const BbFexSmokeDetails* details, char* out_buf,
                                       size_t out_cap);

#ifdef __cplusplus
} // extern "C"
#endif
