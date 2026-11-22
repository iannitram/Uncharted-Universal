// SPDX-License-Identifier: GPL-2.0-only
#pragma once

#include <cstdint>

extern "C" {

typedef struct BbFexVector128 {
    uint64_t lo;
    uint64_t hi;
} BbFexVector128;

typedef struct BbFexCallArgs {
    uint32_t func_id;
    uint32_t _pad;
    uint64_t scalars[6];
    uint64_t pointers[2];
    BbFexVector128 vectors[4];
} BbFexCallArgs;

uint64_t runtime_fex_context_create(uint64_t initial_fs, uint64_t initial_gs);
int runtime_fex_context_destroy(uint64_t handle);
int runtime_fex_call(uint64_t handle, const BbFexCallArgs* args, uint64_t* out_result);
int runtime_fex_fault_handoff(int signal, uint64_t fault_rip, uint64_t fault_address);

} // extern "C"
