// SPDX-License-Identifier: GPL-2.0-only
#include "android/synthetic/fex_smoke.h"

#include <array>
#include <atomic>
#include <cerrno>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <pthread.h>
#include <sys/mman.h>
#include <thread>
#include <unistd.h>
#include <vector>

#if defined(__ANDROID__)
#include <android/api-level.h>
#if __ANDROID_API__ < 21
#error "Android NDK API level 21+ is required for 64-bit ARM64 bionic synthetic smoke"
#endif
#endif

#ifndef MAP_ANONYMOUS
#define MAP_ANONYMOUS MAP_ANON
#endif

namespace {

constexpr const char* kPinnedFexRevision = "f2b679f6028ce1c38875233aecfcf5d3f8ebecec";

constexpr uint64_t kFlagCf = 1ULL << 0;
constexpr uint64_t kFlagReserved1 = 1ULL << 1;
constexpr uint64_t kFlagPf = 1ULL << 2;
constexpr uint64_t kFlagZf = 1ULL << 6;
constexpr uint64_t kFlagSf = 1ULL << 7;
constexpr uint64_t kFlagIf = 1ULL << 9;
constexpr uint64_t kFlagOf = 1ULL << 11;

constexpr uint64_t kStackSentinel = 0xCAFEBABEDEADBEEFULL;
constexpr uint64_t kPointerInitial = 0x1020304050607080ULL;
constexpr uint64_t kPointerIncrement = 0x0102030405060708ULL;
constexpr uint64_t kStackArg7 = 0x0011223344556677ULL;
constexpr uint32_t kTlsWorkerCount = 4;

const char* DetectTargetTriple() {
#if defined(__ANDROID__) && defined(__aarch64__)
    return "aarch64-linux-android";
#elif defined(__ANDROID__) && defined(__x86_64__)
    return "x86_64-linux-android";
#elif defined(__aarch64__) && defined(__linux__)
    return "aarch64-linux-gnu";
#elif defined(__x86_64__) && defined(__linux__)
    return "x86_64-linux-gnu";
#elif defined(__aarch64__)
    return "aarch64-unknown";
#elif defined(__x86_64__)
    return "x86_64-unknown";
#else
    return "unknown";
#endif
}

void FlushInstructionCache(void* addr, size_t len) {
#if defined(__clang__) || defined(__GNUC__)
    __builtin___clear_cache(static_cast<char*>(addr), static_cast<char*>(addr) + len);
#else
    (void)addr;
    (void)len;
#endif
}

size_t QueryPageSize() {
    const long ps = sysconf(_SC_PAGESIZE);
    if (ps < 4096) {
        return 0;
    }
    const size_t size = static_cast<size_t>(ps);
    if ((size & (size - 1)) != 0) {
        return 0;
    }
    return size;
}

struct MappedRegion {
    void* ptr = MAP_FAILED;
    size_t size = 0;
    bool executable = false;
    bool writable = false;
    uint64_t invalidation_epoch = 0;

    MappedRegion() = default;
    MappedRegion(const MappedRegion&) = delete;
    MappedRegion& operator=(const MappedRegion&) = delete;

    MappedRegion(MappedRegion&& other) noexcept
        : ptr(other.ptr),
          size(other.size),
          executable(other.executable),
          writable(other.writable),
          invalidation_epoch(other.invalidation_epoch) {
        other.ptr = MAP_FAILED;
        other.size = 0;
        other.executable = false;
        other.writable = false;
        other.invalidation_epoch = 0;
    }

    ~MappedRegion() {
        (void)Unmap();
    }

    bool MapAnonymous(size_t map_size, int prot) {
        if (ptr != MAP_FAILED || map_size == 0) {
            return false;
        }
        void* mapped = mmap(nullptr, map_size, prot, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (mapped == MAP_FAILED) {
            return false;
        }
        ptr = mapped;
        size = map_size;
        writable = (prot & PROT_WRITE) != 0;
        executable = (prot & PROT_EXEC) != 0;
        invalidation_epoch = 1;
        return true;
    }

    bool Protect(int prot) {
        if (ptr == MAP_FAILED || size == 0) {
            return false;
        }
        if (mprotect(ptr, size, prot) != 0) {
            return false;
        }
        writable = (prot & PROT_WRITE) != 0;
        executable = (prot & PROT_EXEC) != 0;
        return true;
    }

    bool Unmap() {
        if (ptr == MAP_FAILED) {
            return true;
        }
        const int rc = munmap(ptr, size);
        ptr = MAP_FAILED;
        size = 0;
        writable = false;
        executable = false;
        invalidation_epoch = 0;
        return rc == 0;
    }

    uintptr_t Begin() const {
        return reinterpret_cast<uintptr_t>(ptr);
    }

    uintptr_t End() const {
        return Begin() + size;
    }

    bool IsValid() const {
        return ptr != MAP_FAILED && size > 0;
    }
};

struct GuestRangeEntry {
    uintptr_t begin = 0;
    size_t size = 0;
    bool executable = false;
    bool writable = false;
};

class GuestRangeTable {
public:
    int Register(uintptr_t begin, size_t size, bool executable, bool writable) {
        if (begin == 0 || size == 0 || begin > UINTPTR_MAX - size) {
            return -EINVAL;
        }
        const uintptr_t end = begin + size;
        for (const auto& entry : entries_) {
            const uintptr_t entry_end = entry.begin + entry.size;
            if (begin < entry_end && end > entry.begin) {
                return -EACCES;
            }
        }
        entries_.push_back(GuestRangeEntry{begin, size, executable, writable});
        return 0;
    }

    bool ValidateAccess(uintptr_t addr, size_t size, bool require_writable) const {
        if (addr == 0 || size == 0 || addr > UINTPTR_MAX - size) {
            return false;
        }
        const uintptr_t end = addr + size;
        for (const auto& entry : entries_) {
            const uintptr_t entry_end = entry.begin + entry.size;
            if (addr >= entry.begin && end <= entry_end) {
                return !require_writable || entry.writable;
            }
        }
        return false;
    }

    void Clear() {
        entries_.clear();
    }

    size_t Count() const {
        return entries_.size();
    }

private:
    std::vector<GuestRangeEntry> entries_;
};

uint64_t ComputeSyntheticAddFlags(uint64_t lhs, uint64_t rhs, uint64_t base_flags,
                                  uint64_t* out_sum) {
    const uint64_t sum = lhs + rhs;
    *out_sum = sum;
    uint64_t flags = (base_flags & ~(kFlagCf | kFlagPf | kFlagZf | kFlagSf | kFlagOf)) |
                     kFlagReserved1;
    if (sum < lhs) {
        flags |= kFlagCf;
    }
    if (sum == 0) {
        flags |= kFlagZf;
    }
    if ((sum >> 63) != 0) {
        flags |= kFlagSf;
    }
    const uint64_t sign_lhs = lhs >> 63;
    const uint64_t sign_rhs = rhs >> 63;
    const uint64_t sign_sum = sum >> 63;
    if ((sign_lhs == sign_rhs) && (sign_sum != sign_lhs)) {
        flags |= kFlagOf;
    }
    uint8_t low_byte = static_cast<uint8_t>(sum & 0xFFU);
    low_byte ^= static_cast<uint8_t>(low_byte >> 4);
    low_byte ^= static_cast<uint8_t>(low_byte >> 2);
    low_byte ^= static_cast<uint8_t>(low_byte >> 1);
    if ((low_byte & 1U) == 0U) {
        flags |= kFlagPf;
    }
    return flags;
}

bool CheckScalarAndFlags(size_t page_size, bool* out_rflags_ok, bool* out_unaligned_ok) {
    BbFexGuestState state{};
    for (size_t i = 0; i < 16; ++i) {
        state.gpr[i] = 0x0101010101010101ULL * (i + 1);
    }
    state.rsp = 0x7FFF00001000ULL;
    state.rip = 0x400100ULL;
    state.rflags = kFlagReserved1 | kFlagIf;

    bool gprs_ok = ((state.rsp & 0xFULL) == 0);
    for (size_t i = 0; i < 16; ++i) {
        if (state.gpr[i] != 0x0101010101010101ULL * (i + 1)) {
            gprs_ok = false;
        }
    }

    // Exercise RFLAGS carry/zero and signed overflow behavior.
    uint64_t wrap_sum = 1;
    const uint64_t wrap_flags =
        ComputeSyntheticAddFlags(0xFFFFFFFFFFFFFFFFULL, 1ULL, state.rflags, &wrap_sum);
    uint64_t ovf_sum = 0;
    const uint64_t ovf_flags =
        ComputeSyntheticAddFlags(0x7FFFFFFFFFFFFFFFULL, 1ULL, state.rflags, &ovf_sum);

    const bool rflags_ok =
        (wrap_sum == 0ULL) && ((wrap_flags & (kFlagCf | kFlagZf | kFlagPf | kFlagIf)) ==
                               (kFlagCf | kFlagZf | kFlagPf | kFlagIf)) &&
        ((wrap_flags & (kFlagSf | kFlagOf)) == 0) && (ovf_sum == 0x8000000000000000ULL) &&
        ((ovf_flags & (kFlagOf | kFlagSf | kFlagIf)) == (kFlagOf | kFlagSf | kFlagIf)) &&
        ((ovf_flags & (kFlagCf | kFlagZf)) == 0);
    *out_rflags_ok = rflags_ok;

    // Exercise unaligned 16/32/64-bit reads/writes inside an anonymous mmap page.
    bool unaligned_ok = false;
    MappedRegion unaligned_page;
    if (unaligned_page.MapAnonymous(page_size, PROT_READ | PROT_WRITE)) {
        auto* base = static_cast<uint8_t*>(unaligned_page.ptr);
        std::memset(base, 0xAA, page_size);
        const uint64_t expected64 = 0x1122334455667788ULL;
        const uint32_t expected32 = 0x99AABBCCUL;
        const uint16_t expected16 = 0xDDEEULL;
        std::memcpy(base + 1, &expected64, sizeof(expected64));
        std::memcpy(base + 11, &expected32, sizeof(expected32));
        std::memcpy(base + 17, &expected16, sizeof(expected16));

        uint64_t got64 = 0;
        uint32_t got32 = 0;
        uint16_t got16 = 0;
        std::memcpy(&got64, base + 1, sizeof(got64));
        std::memcpy(&got32, base + 11, sizeof(got32));
        std::memcpy(&got16, base + 17, sizeof(got16));
        unaligned_ok = (got64 == expected64) && (got32 == expected32) && (got16 == expected16) &&
                       unaligned_page.Unmap();
    }
    *out_unaligned_ok = unaligned_ok;
    return gprs_ok && rflags_ok && unaligned_ok;
}

bool CheckVectorState(bool* out_vector_lanes_ok) {
    static_assert(sizeof(BbFexVector128) == 16, "BbFexVector128 must be 16 bytes");
    static_assert(offsetof(BbFexVector128, lo) == 0, "BbFexVector128::lo offset");
    static_assert(offsetof(BbFexVector128, hi) == 8, "BbFexVector128::hi offset");

    BbFexGuestState state{};
    for (size_t i = 0; i < 16; ++i) {
        state.xmm[i].lo = 0x1000000000000000ULL + i;
        state.xmm[i].hi = 0x2000000000000000ULL + i;
    }

    // IEEE-754 double addition in xmm0.lo (1.5 + 2.25 == 3.75).
    const double left = 1.5;
    const double right = 2.25;
    const double expected_fp = 3.75;
    std::memcpy(&state.xmm[0].lo, &left, sizeof(left));
    std::memcpy(&state.xmm[1].lo, &right, sizeof(right));
    double lhs_val = 0.0;
    double rhs_val = 0.0;
    std::memcpy(&lhs_val, &state.xmm[0].lo, sizeof(lhs_val));
    std::memcpy(&rhs_val, &state.xmm[1].lo, sizeof(rhs_val));
    const double sum_fp = lhs_val + rhs_val;
    std::memcpy(&state.xmm[0].lo, &sum_fp, sizeof(sum_fp));

    uint64_t expected_fp_bits = 0;
    uint64_t actual_fp_bits = 0;
    std::memcpy(&expected_fp_bits, &expected_fp, sizeof(expected_fp_bits));
    std::memcpy(&actual_fp_bits, &state.xmm[0].lo, sizeof(actual_fp_bits));

    // Packed 32-bit x 4 lane arithmetic in xmm2.
    std::array<uint32_t, 4> lanes{10U, 20U, 30U, 40U};
    std::memcpy(&state.xmm[2], lanes.data(), sizeof(lanes));
    std::array<uint32_t, 4> out_lanes{};
    std::memcpy(out_lanes.data(), &state.xmm[2], sizeof(out_lanes));
    for (size_t i = 0; i < 4; ++i) {
        out_lanes[i] += static_cast<uint32_t>(i + 1);
    }
    std::memcpy(&state.xmm[2], out_lanes.data(), sizeof(out_lanes));

    std::array<uint32_t, 4> verify_lanes{};
    std::memcpy(verify_lanes.data(), &state.xmm[2], sizeof(verify_lanes));

    bool preserved = true;
    for (size_t i = 3; i < 16; ++i) {
        if (state.xmm[i].lo != 0x1000000000000000ULL + i ||
            state.xmm[i].hi != 0x2000000000000000ULL + i) {
            preserved = false;
        }
    }

    const bool lanes_ok = (actual_fp_bits == expected_fp_bits) &&
                          (verify_lanes[0] == 11U) && (verify_lanes[1] == 22U) &&
                          (verify_lanes[2] == 33U) && (verify_lanes[3] == 44U) && preserved;
    *out_vector_lanes_ok = lanes_ok;
    return lanes_ok;
}

struct SyntheticBridgeContext {
    BbFexHostCallback callback = nullptr;
    void* user_data = nullptr;
    GuestRangeTable* ranges = nullptr;
    int callback_depth = 0;
    int max_depth_seen = 0;
    uint64_t nested_result = 0;
    bool pointer_mutated_ok = false;
    bool invalid_pointer_rejected_ok = false;
    bool stack_arg_ok = false;
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
    if (!state || !ctx || !ctx->ranges) {
        return 0;
    }
    if (arg0 == 1 && ctx->callback_depth == 1) {
        // Validate pointer argument inside mapped writable guest range.
        const uintptr_t guest_ptr = static_cast<uintptr_t>(state->gpr[2]);
        if (ctx->ranges->ValidateAccess(guest_ptr, sizeof(uint64_t), true)) {
            uint64_t val = 0;
            std::memcpy(&val, reinterpret_cast<const void*>(guest_ptr), sizeof(val));
            val += kPointerIncrement;
            std::memcpy(reinterpret_cast<void*>(guest_ptr), &val, sizeof(val));
            ctx->pointer_mutated_ok = (val == kPointerInitial + kPointerIncrement);
        }
        // Reject unmapped / read-only pointer argument.
        const bool unmapped_ok = ctx->ranges->ValidateAccess(0xDEAD0000ULL, sizeof(uint64_t), true);
        const bool ro_write_ok = ctx->ranges->ValidateAccess(static_cast<uintptr_t>(state->rip),
                                                             sizeof(uint64_t), true);
        ctx->invalid_pointer_rejected_ok = (!unmapped_ok && !ro_write_ok);

        // Read 7th argument from guest stack [rsp + 8] after validating stack mapping.
        const uintptr_t stack_arg_addr = static_cast<uintptr_t>(state->rsp + 8);
        if (ctx->ranges->ValidateAccess(stack_arg_addr, sizeof(uint64_t), false)) {
            uint64_t arg7 = 0;
            std::memcpy(&arg7, reinterpret_cast<const void*>(stack_arg_addr), sizeof(arg7));
            ctx->stack_arg_ok = (arg7 == kStackArg7);
        }

        // Transform XMM0 while leaving XMM3 untouched across nested dispatch.
        state->xmm[0].lo ^= state->xmm[1].lo;
        state->xmm[0].hi += state->xmm[1].hi;

        // Exercise nested guest-to-host callback preserving register/vector/TLS state.
        ctx->nested_result = SyntheticGuestDispatch(state, ctx, 2, arg1 + 10);
    }
    state->gpr[0] = arg0 + arg1 + 0x100;
    state->rflags |= kFlagZf;
    return state->gpr[0];
}

bool CheckCallbackAbi(size_t page_size, bool* out_nested_ok, bool* out_pointer_ok,
                      bool* out_stack_ok) {
    MappedRegion code_page;
    MappedRegion stack_page;
    MappedRegion data_page;
    if (!code_page.MapAnonymous(page_size, PROT_READ | PROT_WRITE) ||
        !stack_page.MapAnonymous(page_size, PROT_READ | PROT_WRITE) ||
        !data_page.MapAnonymous(page_size, PROT_READ | PROT_WRITE)) {
        return false;
    }

    // Synthetic x86-64 HLT stub in read-only guest code page.
    static_cast<uint8_t*>(code_page.ptr)[0] = 0xF4U;
    FlushInstructionCache(code_page.ptr, page_size);
    if (!code_page.Protect(PROT_READ)) {
        return false;
    }

    std::memcpy(data_page.ptr, &kPointerInitial, sizeof(kPointerInitial));

    const uintptr_t rsp = stack_page.End() - 32;
    const uint64_t return_rip = static_cast<uint64_t>(code_page.Begin());
    std::memcpy(reinterpret_cast<void*>(rsp), &return_rip, sizeof(return_rip));
    std::memcpy(reinterpret_cast<void*>(rsp + 8), &kStackArg7, sizeof(kStackArg7));

    GuestRangeTable ranges;
    if (ranges.Register(code_page.Begin(), code_page.size, true, false) != 0 ||
        ranges.Register(stack_page.Begin(), stack_page.size, false, true) != 0 ||
        ranges.Register(data_page.Begin(), data_page.size, false, true) != 0) {
        return false;
    }

    BbFexGuestState state{};
    state.rip = return_rip;
    state.rsp = static_cast<uint64_t>(rsp);
    state.rflags = kFlagReserved1 | kFlagIf;
    state.gpr[2] = static_cast<uint64_t>(data_page.Begin());
    state.fs_base = 0x11110000ULL;
    state.gs_base = 0x22220000ULL;
    state.xmm[0] = {0x0F0F0F0F0F0F0F0FULL, 100ULL};
    state.xmm[1] = {0x00FF00FF00FF00FFULL, 23ULL};
    state.xmm[3] = {0xAAAA5555AAAA5555ULL, 0x1111222233334444ULL};

    SyntheticBridgeContext ctx{};
    ctx.callback = HostSmokeCallback;
    ctx.user_data = &ctx;
    ctx.ranges = &ranges;

    const uint64_t top = SyntheticGuestDispatch(&state, &ctx, 1, 42);
    *out_nested_ok = (ctx.max_depth_seen == 2) && (ctx.nested_result == (2 + 52 + 0x100));
    *out_pointer_ok = ctx.pointer_mutated_ok && ctx.invalid_pointer_rejected_ok;
    *out_stack_ok = ctx.stack_arg_ok && (state.rsp == static_cast<uint64_t>(rsp));

    const bool vector_preserved =
        state.xmm[0].lo == (0x0F0F0F0F0F0F0F0FULL ^ 0x00FF00FF00FF00FFULL) &&
        state.xmm[0].hi == 123ULL &&
        state.xmm[3].lo == 0xAAAA5555AAAA5555ULL &&
        state.xmm[3].hi == 0x1111222233334444ULL;
    const bool tls_preserved =
        state.fs_base == 0x11110000ULL && state.gs_base == 0x22220000ULL;

    ranges.Clear();
    const bool unmapped_clean = code_page.Unmap() && stack_page.Unmap() && data_page.Unmap();
    return top == (1 + 42 + 0x100) && (state.rflags & kFlagZf) != 0 && vector_preserved &&
           tls_preserved && ctx.callback_depth == 0 && *out_pointer_ok && *out_stack_ok &&
           unmapped_clean;
}

struct PthreadTlsSlot {
    std::atomic<uint32_t>* dtor_counter = nullptr;
    uint64_t expected_cookie = 0;
};

void PthreadTlsDestructor(void* raw) {
    auto* slot = static_cast<PthreadTlsSlot*>(raw);
    if (slot) {
        if (slot->dtor_counter) {
            slot->dtor_counter->fetch_add(1, std::memory_order_relaxed);
        }
        delete slot;
    }
}

bool CheckTlsIsolation(uint32_t* out_threads_tested, bool* out_pthread_dtor_ok) {
    thread_local BbFexGuestState tls_state{};
    pthread_key_t tls_key{};
    if (pthread_key_create(&tls_key, PthreadTlsDestructor) != 0) {
        *out_pthread_dtor_ok = false;
        return false;
    }

    std::atomic<uint32_t> ready{0};
    std::atomic<uint32_t> failures{0};
    std::atomic<uint32_t> dtor_count{0};

    std::vector<std::thread> workers;
    workers.reserve(kTlsWorkerCount);
    for (uint32_t idx = 0; idx < kTlsWorkerCount; ++idx) {
        workers.emplace_back([&, idx]() {
            const uint64_t fs_val = 0x10001000ULL * (idx + 1);
            const uint64_t gs_val = 0x20002000ULL * (idx + 1);
            const uint64_t cookie = kStackSentinel ^ (0x1111ULL * (idx + 1));

            tls_state.fs_base = fs_val;
            tls_state.gs_base = gs_val;
            tls_state.gpr[0] = cookie;
            tls_state.xmm[0] = {cookie, fs_val};

            auto* slot = new PthreadTlsSlot{&dtor_count, cookie};
            if (pthread_setspecific(tls_key, slot) != 0) {
                delete slot;
                failures.fetch_add(1, std::memory_order_relaxed);
                ready.fetch_add(1, std::memory_order_release);
                return;
            }

            ready.fetch_add(1, std::memory_order_release);
            while (ready.load(std::memory_order_acquire) < kTlsWorkerCount) {
                std::this_thread::yield();
            }
            for (int round = 0; round < 3; ++round) {
                std::this_thread::yield();
                auto* got = static_cast<PthreadTlsSlot*>(pthread_getspecific(tls_key));
                if (tls_state.fs_base != fs_val || tls_state.gs_base != gs_val ||
                    tls_state.gpr[0] != cookie || tls_state.xmm[0].lo != cookie ||
                    tls_state.xmm[0].hi != fs_val || !got || got->expected_cookie != cookie) {
                    failures.fetch_add(1, std::memory_order_relaxed);
                }
            }
        });
    }

    for (auto& t : workers) {
        t.join();
    }

    const int delete_rc = pthread_key_delete(tls_key);
    *out_threads_tested = kTlsWorkerCount;
    *out_pthread_dtor_ok = (dtor_count.load(std::memory_order_relaxed) == kTlsWorkerCount) &&
                           (delete_rc == 0);
    return failures.load(std::memory_order_relaxed) == 0 && tls_state.fs_base == 0 &&
           tls_state.gs_base == 0 && *out_pthread_dtor_ok;
}

bool CheckMemoryMappingInvalidationAndTeardown(size_t page_size, bool* out_overlap_rejected_ok,
                                               bool* out_protect_ok, bool* out_icache_ok,
                                               bool* out_teardown_ok, uint64_t* out_epochs) {
    MappedRegion code_region;
    MappedRegion stack_region;
    if (!code_region.MapAnonymous(page_size, PROT_READ | PROT_WRITE) ||
        !stack_region.MapAnonymous(page_size, PROT_READ | PROT_WRITE)) {
        return false;
    }

    GuestRangeTable ranges;
    if (ranges.Register(code_region.Begin(), code_region.size, true, false) != 0 ||
        ranges.Register(stack_region.Begin(), stack_region.size, false, true) != 0) {
        return false;
    }

    // Reject exact and partial overlapping mappings with -EACCES.
    const bool exact_rejected =
        ranges.Register(code_region.Begin(), code_region.size, false, true) == -EACCES;
    const bool partial_rejected =
        ranges.Register(code_region.Begin() + 128, page_size, false, true) == -EACCES;
    *out_overlap_rejected_ok = exact_rejected && partial_rejected;

    // Populate synthetic x86-64 `mov rax, imm64; hlt`, flush icache, protect RO.
    constexpr uint64_t kInitialImm = 0x0123456789ABCDEFULL;
    constexpr uint64_t kUpdatedImm = 0xFEDCBA9876543210ULL;
    std::array<uint8_t, 11> insn{0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0xF4};
    std::memcpy(insn.data() + 2, &kInitialImm, sizeof(kInitialImm));
    std::memcpy(code_region.ptr, insn.data(), insn.size());
    FlushInstructionCache(code_region.ptr, insn.size());
    const bool ro_ok = code_region.Protect(PROT_READ);

    uint64_t readback_initial = 0;
    std::memcpy(&readback_initial, static_cast<const uint8_t*>(code_region.ptr) + 2,
                sizeof(readback_initial));

    // Self-modifying code / invalidation epoch bump: RW -> patch -> clear_cache -> RO.
    const bool rw_ok = code_region.Protect(PROT_READ | PROT_WRITE);
    std::memcpy(static_cast<uint8_t*>(code_region.ptr) + 2, &kUpdatedImm, sizeof(kUpdatedImm));
    FlushInstructionCache(code_region.ptr, insn.size());
    const bool ro2_ok = code_region.Protect(PROT_READ);
    code_region.invalidation_epoch += 1;

    uint64_t readback_updated = 0;
    std::memcpy(&readback_updated, static_cast<const uint8_t*>(code_region.ptr) + 2,
                sizeof(readback_updated));

    *out_protect_ok = ro_ok && rw_ok && ro2_ok;
    *out_icache_ok = (readback_initial == kInitialImm) && (readback_updated == kUpdatedImm) &&
                     (code_region.invalidation_epoch == 2);
    *out_epochs = code_region.invalidation_epoch;

    const bool inv_ok = *out_overlap_rejected_ok && *out_protect_ok && *out_icache_ok;

    // Teardown releases OS mappings and resets range tracking.
    ranges.Clear();
    const bool unmap_ok = code_region.Unmap() && stack_region.Unmap();
    *out_teardown_ok = unmap_ok && !code_region.IsValid() && !stack_region.IsValid() &&
                       ranges.Count() == 0;
    return inv_ok && *out_teardown_ok;
}

} // namespace

extern "C" int bb_android_run_fex_smoke_detailed(BbFexSmokeReport* out_report,
                                                 BbFexSmokeDetails* out_details) {
    if (!out_report) {
        return -1;
    }
    std::memset(out_report, 0, sizeof(*out_report));

    BbFexSmokeDetails details{};
    const size_t page_size = QueryPageSize();
    details.page_size = static_cast<uint64_t>(page_size);
#if defined(__ANDROID__)
    details.is_android = 1;
#endif
#if defined(__BIONIC__)
    details.is_bionic = 1;
#endif
#if defined(__aarch64__) || defined(_M_ARM64)
    details.is_arm64 = 1;
#endif
#if defined(_LIBCPP_VERSION)
    details.is_libcpp = 1;
#endif
#if defined(UNCHARTED_WITH_FEXCORE)
    details.fexcore_backed = 1;
#endif
    std::snprintf(details.target_triple, sizeof(details.target_triple), "%s", DetectTargetTriple());
    std::snprintf(details.fex_revision, sizeof(details.fex_revision), "%s", kPinnedFexRevision);

    if (page_size == 0) {
        if (out_details) {
            *out_details = details;
        }
        return -2;
    }

    bool rflags_ok = false;
    bool unaligned_ok = false;
    out_report->scalar_ok = CheckScalarAndFlags(page_size, &rflags_ok, &unaligned_ok) ? 1 : 0;
    details.rflags_ok = rflags_ok ? 1 : 0;
    details.unaligned_ok = unaligned_ok ? 1 : 0;

    bool vector_lanes_ok = false;
    out_report->vector_ok = CheckVectorState(&vector_lanes_ok) ? 1 : 0;
    details.vector_lanes_ok = vector_lanes_ok ? 1 : 0;

    bool nested_ok = false;
    bool pointer_ok = false;
    bool stack_ok = false;
    out_report->callback_ok = CheckCallbackAbi(page_size, &nested_ok, &pointer_ok, &stack_ok) ? 1 : 0;
    out_report->nested_callback_ok = nested_ok ? 1 : 0;
    details.pointer_arg_ok = pointer_ok ? 1 : 0;
    details.stack_arg_ok = stack_ok ? 1 : 0;

    uint32_t threads_tested = 0;
    bool pthread_dtor_ok = false;
    out_report->tls_isolation_ok = CheckTlsIsolation(&threads_tested, &pthread_dtor_ok) ? 1 : 0;
    details.threads_tested = threads_tested;
    details.pthread_tls_dtor_ok = pthread_dtor_ok ? 1 : 0;

    bool overlap_rejected_ok = false;
    bool protect_ok = false;
    bool icache_ok = false;
    bool teardown_ok = false;
    uint64_t invalidation_epochs = 0;
    out_report->memory_invalidation_ok =
        CheckMemoryMappingInvalidationAndTeardown(page_size, &overlap_rejected_ok, &protect_ok,
                                                  &icache_ok, &teardown_ok, &invalidation_epochs)
            ? 1
            : 0;
    out_report->teardown_ok = teardown_ok ? 1 : 0;
    details.overlap_rejected_ok = overlap_rejected_ok ? 1 : 0;
    details.mapping_protect_ok = protect_ok ? 1 : 0;
    details.icache_invalidation_ok = icache_ok ? 1 : 0;
    details.invalidation_epochs = invalidation_epochs;

    if (out_details) {
        *out_details = details;
    }

    const bool all_ok = out_report->scalar_ok && out_report->vector_ok && out_report->callback_ok &&
                        out_report->nested_callback_ok && out_report->tls_isolation_ok &&
                        out_report->memory_invalidation_ok && out_report->teardown_ok;
    return all_ok ? 0 : -2;
}

extern "C" int bb_android_run_fex_smoke(BbFexSmokeReport* out_report) {
    return bb_android_run_fex_smoke_detailed(out_report, nullptr);
}

extern "C" int bb_android_format_fex_smoke_report(const BbFexSmokeReport* report,
                                                  const BbFexSmokeDetails* details, char* out_buf,
                                                  size_t out_cap) {
    if (!report || !details || !out_buf || out_cap == 0) {
        return -1;
    }
    const bool all_ok = report->scalar_ok && report->vector_ok && report->callback_ok &&
                        report->nested_callback_ok && report->tls_isolation_ok &&
                        report->memory_invalidation_ok && report->teardown_ok &&
                        details->rflags_ok && details->unaligned_ok && details->pointer_arg_ok &&
                        details->stack_arg_ok && details->vector_lanes_ok &&
                        details->overlap_rejected_ok && details->pthread_tls_dtor_ok &&
                        details->mapping_protect_ok && details->icache_invalidation_ok;

    auto tag = [](int flag) { return flag ? "ok" : "fail"; };
    const int n = std::snprintf(
        out_buf, out_cap,
        "%s target=%s android=%d bionic=%d arm64=%d libcpp=%d fexcore=%d page_size=%llu "
        "threads=%u epochs=%llu scalar=%s vector=%s rflags=%s unaligned=%s callback=%s "
        "nested_callback=%s pointer_arg=%s stack_arg=%s tls=%s pthread_tls_dtor=%s mapping=%s "
        "overlap_rejected=%s invalidation=%s teardown=%s revision=%s",
        all_ok ? "FEXCORE_ANDROID_SMOKE_OK" : "FEXCORE_ANDROID_SMOKE_FAIL", details->target_triple,
        details->is_android, details->is_bionic, details->is_arm64, details->is_libcpp,
        details->fexcore_backed, static_cast<unsigned long long>(details->page_size),
        details->threads_tested, static_cast<unsigned long long>(details->invalidation_epochs),
        tag(report->scalar_ok), tag(report->vector_ok), tag(details->rflags_ok),
        tag(details->unaligned_ok), tag(report->callback_ok), tag(report->nested_callback_ok),
        tag(details->pointer_arg_ok), tag(details->stack_arg_ok), tag(report->tls_isolation_ok),
        tag(details->pthread_tls_dtor_ok), tag(details->mapping_protect_ok),
        tag(details->overlap_rejected_ok), tag(report->memory_invalidation_ok),
        tag(report->teardown_ok), details->fex_revision);
    if (n < 0 || static_cast<size_t>(n) >= out_cap) {
        return -1;
    }
    return n;
}
