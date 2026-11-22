// SPDX-License-Identifier: GPL-2.0-only
#include "android/fex_bridge.h"

#include <atomic>
#include <cassert>
#include <cstddef>
#include <cstdint>
#include <thread>

namespace {

// ABI layout parity with crates/runtime/src/android_fex.rs.
static_assert(sizeof(BbFexVector128) == 16);
static_assert(offsetof(BbFexVector128, lo) == 0);
static_assert(offsetof(BbFexVector128, hi) == 8);

static_assert(sizeof(BbFexGuestContext) == 424);
static_assert(offsetof(BbFexGuestContext, gpr) == 0);
static_assert(offsetof(BbFexGuestContext, rsp) == 128);
static_assert(offsetof(BbFexGuestContext, rip) == 136);
static_assert(offsetof(BbFexGuestContext, rflags) == 144);
static_assert(offsetof(BbFexGuestContext, xmm) == 152);
static_assert(offsetof(BbFexGuestContext, fs_base) == 408);
static_assert(offsetof(BbFexGuestContext, gs_base) == 416);

static_assert(sizeof(BbFexCallArgs) == 136);
static_assert(offsetof(BbFexCallArgs, func_id) == 0);
static_assert(offsetof(BbFexCallArgs, scalars) == 8);
static_assert(offsetof(BbFexCallArgs, pointers) == 56);
static_assert(offsetof(BbFexCallArgs, vectors) == 72);

static_assert(sizeof(BbFexFaultInfo) == 24);
static_assert(offsetof(BbFexFaultInfo, signal) == 0);
static_assert(offsetof(BbFexFaultInfo, fault_rip) == 8);
static_assert(offsetof(BbFexFaultInfo, fault_address) == 16);

constexpr uint32_t kFuncDispatchAllArgs = 2001;
constexpr uint32_t kFuncNestedOuter = 2002;
constexpr uint32_t kFuncNestedInner = 2003;
constexpr uint32_t kFuncTlsWorker = 2004;
constexpr uint32_t kFuncUnregistered = 2999;

uint64_t OnDispatchAllArgs(BbFexGuestContext* ctx, const BbFexCallArgs* args, void* user_data) {
    assert(ctx != nullptr);
    assert(args != nullptr);
    const auto tag = reinterpret_cast<uintptr_t>(user_data);
    assert(tag == 0x55aaU);

    // Verify full register mapping across scalars[0..6], pointers[0..2], and vectors[0..4].
    assert(ctx->gpr[0] == args->scalars[0]);
    assert(ctx->gpr[1] == args->scalars[1]);
    assert(ctx->gpr[2] == args->pointers[0]);
    assert(ctx->gpr[3] == args->pointers[1]);
    assert(ctx->gpr[4] == args->scalars[2]);
    assert(ctx->gpr[5] == args->scalars[3]);
    assert(ctx->gpr[6] == args->scalars[4]);
    assert(ctx->gpr[7] == args->scalars[5]);
    for (size_t i = 0; i < 4; ++i) {
        assert(ctx->xmm[i].lo == args->vectors[i].lo);
        assert(ctx->xmm[i].hi == args->vectors[i].hi);
    }

    ctx->rip = 0x40008000ULL;
    ctx->rflags |= 0x40ULL;
    ctx->gpr[15] = 0xfeedfaceULL;

    return args->scalars[0] + args->scalars[1] + args->scalars[2] + args->scalars[3] +
           args->scalars[4] + args->scalars[5] + args->pointers[0] + args->pointers[1] +
           args->vectors[0].lo + args->vectors[1].hi + args->vectors[2].lo + args->vectors[3].hi +
           static_cast<uint64_t>(tag);
}

uint64_t OnNestedInner(BbFexGuestContext* ctx, const BbFexCallArgs* args, void*) {
    assert(ctx != nullptr);
    assert(args != nullptr);
    return args->scalars[0] + args->pointers[0] + ctx->xmm[0].hi;
}

uint64_t OnNestedOuter(BbFexGuestContext* ctx, const BbFexCallArgs* args, void* user_data) {
    assert(ctx != nullptr);
    assert(args != nullptr);
    const uint64_t handle = reinterpret_cast<uintptr_t>(user_data);

    BbFexCallArgs inner{};
    inner.func_id = kFuncNestedInner;
    inner.scalars[0] = 30;
    inner.pointers[0] = 0x1000;
    inner.vectors[0] = {9, 77};

    uint64_t inner_out = 0;
    assert(bb::android::InvokeGuestBridge(handle, inner, &inner_out) == 0);
    // Outer frame xmm[0] must remain untouched by inner dispatch.
    assert(ctx->xmm[0].lo == 5 && ctx->xmm[0].hi == 6);
    return inner_out + args->scalars[0] + ctx->xmm[0].lo;
}

uint64_t OnTlsWorker(BbFexGuestContext* ctx, const BbFexCallArgs* args, void*) {
    assert(ctx != nullptr);
    assert(args != nullptr);
    ctx->gpr[12] = ctx->fs_base ^ args->scalars[0];
    ctx->gpr[13] = ctx->gs_base ^ args->scalars[1];
    return ctx->gpr[12] + ctx->gpr[13];
}

void TestAbiAndStructParity() {
    assert(uncharted_runtime_version() == BB_FEX_RUNTIME_ABI_VERSION);
    assert(runtime_fex_abi_version() == BB_FEX_RUNTIME_ABI_VERSION);
    assert(bb::android::VerifyRuntimeAbi());
}

void TestDispatchAndContextSnapshot() {
    const uint64_t h = bb::android::CreateGuestContext(0x11112222ULL, 0x33334444ULL);
    assert(h != 0);

    BbFexGuestContext snap{};
    assert(bb::android::SnapshotGuestContext(h, &snap) == 0);
    assert(snap.fs_base == 0x11112222ULL);
    assert(snap.gs_base == 0x33334444ULL);
    assert(snap.rflags == 0x202ULL);

    assert(bb::android::RegisterHostCallback(
               kFuncDispatchAllArgs, OnDispatchAllArgs,
               reinterpret_cast<void*>(static_cast<uintptr_t>(0x55aaU))) == 0);

    BbFexCallArgs args{};
    args.func_id = kFuncDispatchAllArgs;
    args.scalars[0] = 10;
    args.scalars[1] = 20;
    args.scalars[2] = 30;
    args.scalars[3] = 40;
    args.scalars[4] = 50;
    args.scalars[5] = 60;
    args.pointers[0] = 0x10000;
    args.pointers[1] = 0x20000;
    args.vectors[0] = {1, 2};
    args.vectors[1] = {3, 4};
    args.vectors[2] = {5, 6};
    args.vectors[3] = {7, 8};

    uint64_t out = 0;
    assert(bb::android::InvokeGuestBridge(h, args, &out) == 0);
    const uint64_t expected =
        10 + 20 + 30 + 40 + 50 + 60 + 0x10000 + 0x20000 + 1 + 4 + 5 + 8 + 0x55aaU;
    assert(out == expected);

    // Passing null out_result must also succeed without crashing.
    assert(bb::android::InvokeGuestBridge(h, args, nullptr) == 0);

    assert(bb::android::SnapshotGuestContext(h, &snap) == 0);
    assert(snap.rip == 0x40008000ULL);
    assert((snap.rflags & 0x40ULL) != 0);
    assert(snap.gpr[15] == 0xfeedfaceULL);
    assert(snap.fs_base == 0x11112222ULL);
    assert(snap.gs_base == 0x33334444ULL);

    assert(bb::android::UnregisterHostCallback(kFuncDispatchAllArgs) == 0);
    assert(bb::android::UnregisterHostCallback(kFuncDispatchAllArgs) == -1);
    assert(bb::android::DestroyGuestContext(h) == 0);
    assert(bb::android::DestroyGuestContext(h) == -1);
}

void TestNestedCallbacks() {
    const uint64_t h = bb::android::CreateGuestContext(0xaaaaULL, 0xbbbbULL);
    assert(h != 0);

    assert(bb::android::RegisterHostCallback(
               kFuncNestedOuter, OnNestedOuter,
               reinterpret_cast<void*>(static_cast<uintptr_t>(h))) == 0);
    assert(bb::android::RegisterHostCallback(kFuncNestedInner, OnNestedInner, nullptr) == 0);

    BbFexCallArgs outer{};
    outer.func_id = kFuncNestedOuter;
    outer.scalars[0] = 100;
    outer.vectors[0] = {5, 6};

    uint64_t out = 0;
    assert(bb::android::InvokeGuestBridge(h, outer, &out) == 0);
    assert(out == (30 + 0x1000 + 77) + 100 + 5);

    assert(bb::android::UnregisterHostCallback(kFuncNestedOuter) == 0);
    assert(bb::android::UnregisterHostCallback(kFuncNestedInner) == 0);
    assert(bb::android::DestroyGuestContext(h) == 0);
}

void TestTlsIsolationAcrossThreads() {
    assert(bb::android::RegisterHostCallback(kFuncTlsWorker, OnTlsWorker, nullptr) == 0);
    std::atomic<int> failures{0};

    auto run_worker = [&](uint64_t fs_val, uint64_t gs_val, uint64_t s0, uint64_t s1) {
        const uint64_t h = bb::android::CreateGuestContext(fs_val, gs_val);
        if (h == 0) {
            failures.fetch_add(1);
            return;
        }
        BbFexCallArgs args{};
        args.func_id = kFuncTlsWorker;
        args.scalars[0] = s0;
        args.scalars[1] = s1;
        uint64_t out = 0;
        if (bb::android::InvokeGuestBridge(h, args, &out) != 0 ||
            out != ((fs_val ^ s0) + (gs_val ^ s1))) {
            failures.fetch_add(1);
        }
        std::this_thread::yield();
        BbFexGuestContext snap{};
        if (bb::android::SnapshotGuestContext(h, &snap) != 0 || snap.fs_base != fs_val ||
            snap.gs_base != gs_val || snap.gpr[12] != (fs_val ^ s0) ||
            snap.gpr[13] != (gs_val ^ s1)) {
            failures.fetch_add(1);
        }
        if (bb::android::DestroyGuestContext(h) != 0) {
            failures.fetch_add(1);
        }
    };

    std::thread t1(run_worker, 0x10000001ULL, 0x20000002ULL, 0x11ULL, 0x22ULL);
    std::thread t2(run_worker, 0x30000003ULL, 0x40000004ULL, 0x33ULL, 0x44ULL);
    t1.join();
    t2.join();
    assert(failures.load() == 0);
    assert(bb::android::UnregisterHostCallback(kFuncTlsWorker) == 0);
}

void TestFaultHandoffAndErrorPaths() {
    const uint64_t h = bb::android::CreateGuestContext(0x10, 0x20);
    assert(h != 0);

    // Null callback and null args fail closed.
    assert(bb::android::RegisterHostCallback(kFuncUnregistered, nullptr, nullptr) == -1);
    assert(runtime_fex_call(h, nullptr, nullptr) == -1);

    // Unknown handle and unregistered func_id fail closed with distinct codes.
    BbFexCallArgs args{};
    args.func_id = kFuncUnregistered;
    uint64_t out = 99;
    assert(bb::android::InvokeGuestBridge(0, args, &out) == -2);
    assert(bb::android::InvokeGuestBridge(0xdeadbeefULL, args, &out) == -2);
    assert(bb::android::InvokeGuestBridge(h, args, &out) == -3);
    assert(out == 99);

    // Snapshot / last_fault error paths.
    assert(bb::android::SnapshotGuestContext(h, nullptr) == -1);
    BbFexGuestContext snap{};
    assert(bb::android::SnapshotGuestContext(0, &snap) == -2);
    assert(bb::android::ReadLastFault(nullptr) == -1);

    // Fault handoff records valid signals and rejects signal <= 0 without clobbering.
    assert(bb::android::ReportGuestFault(11, 0x40001234ULL, 0xbadULL) == 0);
    assert(bb::android::ReportGuestFault(0, 0x1ULL, 0x2ULL) == -1);
    assert(bb::android::ReportGuestFault(-1, 0x1ULL, 0x2ULL) == -1);

    BbFexFaultInfo fault{};
    assert(bb::android::ReadLastFault(&fault) == 0);
    assert(fault.signal == 11);
    assert(fault.fault_rip == 0x40001234ULL);
    assert(fault.fault_address == 0xbadULL);

    assert(bb::android::DestroyGuestContext(0) == -1);
    assert(bb::android::DestroyGuestContext(h) == 0);
}

} // namespace

int main() {
    TestAbiAndStructParity();
    TestDispatchAndContextSnapshot();
    TestNestedCallbacks();
    TestTlsIsolationAcrossThreads();
    TestFaultHandoffAndErrorPaths();
    return 0;
}
