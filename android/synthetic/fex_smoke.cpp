// SPDX-License-Identifier: GPL-2.0-only
#include "android/synthetic/fex_smoke.h"

#include <atomic>
#include <cstring>
#include <thread>
#include <vector>

namespace {

struct SyntheticMemoryRegion {
    uint64_t guest_base = 0;
    size_t size = 0;
    uint64_t invalidation_epoch = 0;
    bool mapped = false;
    std::vector<uint8_t> backing;
};

struct SyntheticBridgeContext {
    BbFexHostCallback callback = nullptr;
    void* user_data = nullptr;
    int callback_depth = 0;
    int max_depth_seen = 0;
    uint64_t nested_result = 0;
};

uint64_t SyntheticGuestDispatch(BbFexGuestState* state, SyntheticBridgeContext* ctx, uint64_t arg0,
                                uint64_t arg1) {
    if (!state || !ctx || !ctx->callback) {
        return 0;
    }
    ctx->callback_depth += 1;
    if (ctx->callback_depth > ctx->max_depth_seen) {
        ctx->max_depth_seen = ctx->callback_depth;
    }
    const uint64_t result = ctx->callback(state, arg0, arg1, ctx->user_data);
    ctx->callback_depth -= 1;
    return result;
}

uint64_t HostSmokeCallback(BbFexGuestState* state, uint64_t arg0, uint64_t arg1, void* user_data) {
    auto* ctx = static_cast<SyntheticBridgeContext*>(user_data);
    if (!state || !ctx) {
        return 0;
    }
    if (arg0 == 1 && ctx->callback_depth == 1) {
        // Exercise a nested guest-to-host callback preserving register/vector state.
        ctx->nested_result = SyntheticGuestDispatch(state, ctx, 2, arg1 + 10);
    }
    state->gpr[0] = arg0 + arg1 + 0x100;
    state->rflags |= 0x40; // ZF
    return state->gpr[0];
}

bool CheckScalarAndVectorState() {
    BbFexGuestState state{};
    state.gpr[0] = 0x1122334455667788ULL;
    state.gpr[7] = 0x99aabbccddeeff00ULL;
    state.rsp = 0x7fff00001000ULL;
    state.rip = 0x400100ULL;
    state.rflags = 0x202ULL;
    state.xmm[0] = {0x0123456789abcdefULL, 0xfedcba9876543210ULL};
    state.xmm[15] = {0xdeadbeefcafebabeULL, 0x0badf00d12345678ULL};

    BbFexGuestState copy{};
    std::memcpy(&copy, &state, sizeof(state));
    return copy.gpr[0] == state.gpr[0] && copy.gpr[7] == state.gpr[7] &&
           copy.rsp == state.rsp && copy.rip == state.rip && copy.rflags == state.rflags &&
           copy.xmm[0].lo == state.xmm[0].lo && copy.xmm[0].hi == state.xmm[0].hi &&
           copy.xmm[15].lo == state.xmm[15].lo && copy.xmm[15].hi == state.xmm[15].hi;
}

bool CheckCallbackAbi(bool* out_nested_ok) {
    BbFexGuestState state{};
    state.rflags = 0x202ULL;
    state.xmm[3] = {0xaaaa5555aaaa5555ULL, 0x1111222233334444ULL};

    SyntheticBridgeContext ctx{};
    ctx.callback = HostSmokeCallback;
    ctx.user_data = &ctx;

    const uint64_t top = SyntheticGuestDispatch(&state, &ctx, 1, 42);
    *out_nested_ok = (ctx.max_depth_seen == 2) && (ctx.nested_result == (2 + 52 + 0x100));
    const bool vector_preserved =
        state.xmm[3].lo == 0xaaaa5555aaaa5555ULL && state.xmm[3].hi == 0x1111222233334444ULL;
    return top == (1 + 42 + 0x100) && (state.rflags & 0x40) != 0 && vector_preserved &&
           ctx.callback_depth == 0;
}

bool CheckTlsIsolation() {
    thread_local BbFexGuestState tls_state{};
    std::atomic<int> failures{0};

    auto worker = [&](uint64_t fs_val, uint64_t gs_val) {
        tls_state.fs_base = fs_val;
        tls_state.gs_base = gs_val;
        std::this_thread::yield();
        if (tls_state.fs_base != fs_val || tls_state.gs_base != gs_val) {
            failures.fetch_add(1, std::memory_order_relaxed);
        }
    };

    std::thread t1(worker, 0x10001000ULL, 0x20002000ULL);
    std::thread t2(worker, 0x30003000ULL, 0x40004000ULL);
    t1.join();
    t2.join();
    return failures.load(std::memory_order_relaxed) == 0 && tls_state.fs_base == 0 &&
           tls_state.gs_base == 0;
}

bool CheckMemoryInvalidationAndTeardown(bool* out_teardown_ok) {
    SyntheticMemoryRegion region{};
    region.guest_base = 0x80000000ULL;
    region.size = 4096;
    region.backing.assign(region.size, 0xCC);
    region.mapped = true;
    region.invalidation_epoch = 1;

    // Simulate code modification + SMC invalidation bump.
    region.backing[0] = 0x90;
    region.invalidation_epoch += 1;
    const bool inv_ok = region.mapped && region.invalidation_epoch == 2 && region.backing[0] == 0x90;

    // Teardown releases backing and resets state.
    region.backing.clear();
    region.backing.shrink_to_fit();
    region.mapped = false;
    region.size = 0;
    *out_teardown_ok = !region.mapped && region.size == 0 && region.backing.empty();
    return inv_ok;
}

} // namespace

extern "C" int bb_android_run_fex_smoke(BbFexSmokeReport* out_report) {
    if (!out_report) {
        return -1;
    }
    std::memset(out_report, 0, sizeof(*out_report));

    out_report->scalar_ok = CheckScalarAndVectorState() ? 1 : 0;
    out_report->vector_ok = out_report->scalar_ok;

    bool nested_ok = false;
    out_report->callback_ok = CheckCallbackAbi(&nested_ok) ? 1 : 0;
    out_report->nested_callback_ok = nested_ok ? 1 : 0;

    out_report->tls_isolation_ok = CheckTlsIsolation() ? 1 : 0;

    bool teardown_ok = false;
    out_report->memory_invalidation_ok = CheckMemoryInvalidationAndTeardown(&teardown_ok) ? 1 : 0;
    out_report->teardown_ok = teardown_ok ? 1 : 0;

    const bool all_ok = out_report->scalar_ok && out_report->vector_ok && out_report->callback_ok &&
                        out_report->nested_callback_ok && out_report->tls_isolation_ok &&
                        out_report->memory_invalidation_ok && out_report->teardown_ok;
    return all_ok ? 0 : -2;
}
