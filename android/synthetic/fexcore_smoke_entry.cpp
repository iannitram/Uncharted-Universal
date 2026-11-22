// SPDX-License-Identifier: MIT
// Self-contained FEXCore guest smoke probe for Android NDK/bionic execution against the
// pinned FEXCore revision (f2b679f6028ce1c38875233aecfcf5d3f8ebecec).
// Adapted from zenithblue-oss/shadps4-arm64 (be6bc2e9c60799e071dd2fafa6216e8d80ec619c)
// for dynamic Android page sizes (4 KiB / 16 KiB) and W^X-safe guest code mappings.

#include "Common/Config.h"
#include "Common/HostFeatures.h"
#include <FEXCore/Config/Config.h>
#include <FEXCore/Core/Context.h>
#include <FEXCore/Core/CoreState.h>
#include <FEXCore/Core/SignalDelegator.h>
#include <FEXCore/Core/X86Enums.h>
#include <FEXCore/Debug/InternalThreadState.h>
#include <FEXCore/HLE/SyscallHandler.h>

#include <sys/mman.h>
#include <unistd.h>

#include <array>
#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <mutex>
#include <string_view>
#include <thread>
#include <vector>

#ifndef MAP_ANONYMOUS
#define MAP_ANONYMOUS MAP_ANON
#endif

namespace {

constexpr uint64_t kAddLeft = 0x1122334455667788ULL;
constexpr uint64_t kAddRight = 0x0102030405060708ULL;
constexpr uint64_t kXorLeft = 0xFEDCBA9876543210ULL;
constexpr uint64_t kXorRight = 0x0F0F0F0F0F0F0F0FULL;
constexpr uint64_t kStackSentinel = 0xCAFEBABEDEADBEEFULL;
constexpr double kFpLeft = 1.5;
constexpr double kFpRight = 2.25;
constexpr double kFpResult = 3.75;
constexpr uint64_t kThreadSentinelA = 0x13579BDF2468ACE0ULL;
constexpr uint64_t kThreadSentinelB = 0x02468ACE13579BDFULL;
constexpr uint64_t kCallbackInput = 0x1020304050607080ULL;
constexpr uint64_t kInvalidationInitial = 0x0123456789ABCDEFULL;
constexpr uint64_t kInvalidationUpdated = 0xFEDCBA9876543210ULL;
constexpr std::string_view kFexRevision = "f2b679f6028ce1c38875233aecfcf5d3f8ebecec";

int Fail(std::string_view check) {
    std::fprintf(stderr, "FEXCORE_GUEST_SMOKE_FAIL check=%.*s\n", static_cast<int>(check.size()),
                 check.data());
    return 1;
}

void FlushICache(void* addr, size_t len) {
    __builtin___clear_cache(static_cast<char*>(addr), static_cast<char*>(addr) + len);
}

size_t QueryPageSize() {
    const long ps = sysconf(_SC_PAGESIZE);
    if (ps < 4096) {
        return 0;
    }
    const size_t size = static_cast<size_t>(ps);
    return (size & (size - 1)) == 0 ? size : 0;
}

class Mapping final {
public:
    Mapping(size_t size, int protection)
        : size_(size),
          address_(mmap(nullptr, size, protection, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0)) {}

    Mapping(const Mapping&) = delete;
    Mapping& operator=(const Mapping&) = delete;

    ~Mapping() {
        if (address_ != MAP_FAILED && munmap(address_, size_) != 0) {
            std::fprintf(stderr, "FEXCORE_GUEST_SMOKE_FATAL check=release-mapping errno=%d\n",
                         errno);
            std::abort();
        }
    }

    bool IsValid() const {
        return address_ != MAP_FAILED;
    }

    void* Get() const {
        return address_;
    }

    bool Protect(int prot) const {
        return address_ != MAP_FAILED && mprotect(address_, size_, prot) == 0;
    }

private:
    size_t size_;
    void* address_;
};

class CallRetStack final {
public:
    explicit CallRetStack(size_t page_size)
        : page_size_(page_size),
          alloc_size_(FEXCore::Core::InternalThreadState::CALLRET_STACK_SIZE + 2 * page_size),
          address_(mmap(nullptr, alloc_size_, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0)) {}

    CallRetStack(const CallRetStack&) = delete;
    CallRetStack& operator=(const CallRetStack&) = delete;

    ~CallRetStack() {
        if (IsReserved() && munmap(address_, alloc_size_) != 0) {
            std::fprintf(stderr, "FEXCORE_GUEST_SMOKE_FATAL check=release-callret-stack errno=%d\n",
                         errno);
            std::abort();
        }
    }

    bool IsReserved() const {
        return address_ != MAP_FAILED;
    }

    bool MakeWritable() const {
        return mprotect(StackBase(), FEXCore::Core::InternalThreadState::CALLRET_STACK_SIZE,
                        PROT_READ | PROT_WRITE) == 0;
    }

    void Initialize(FEXCore::Core::InternalThreadState* thread) const {
        thread->CallRetStackBase = StackBase();
        thread->CurrentFrame->State.callret_sp =
            reinterpret_cast<uint64_t>(StackBase()) +
            FEXCore::Core::InternalThreadState::CALLRET_STACK_SIZE / 4;
    }

private:
    void* StackBase() const {
        return static_cast<uint8_t*>(address_) + page_size_;
    }

    size_t page_size_;
    size_t alloc_size_;
    void* address_;
};

class ConfigScope final {
public:
    ConfigScope() {
        FEX::Config::InitializeConfigs(FEX::Config::PortableInformation{});
        FEXCore::Config::Initialize();
        FEXCore::Config::Load();
        FEXCore::Config::Set(FEXCore::Config::CONFIG_IS64BIT_MODE, "1");
        FEXCore::Config::Set(FEXCore::Config::CONFIG_DISABLETELEMETRY, "1");
    }

    ConfigScope(const ConfigScope&) = delete;
    ConfigScope& operator=(const ConfigScope&) = delete;

    ~ConfigScope() {
        FEXCore::Config::Shutdown();
    }
};

class SmokeSyscallHandler final : public FEXCore::HLE::SyscallHandler {
public:
    SmokeSyscallHandler() {
        OSABI = FEXCore::HLE::SyscallOSABI::OS_GENERIC;
    }

    uint64_t HandleSyscall(FEXCore::Core::CpuStateFrame*, FEXCore::HLE::SyscallArguments*) override {
        return 0;
    }

    FEXCore::HLE::ExecutableRangeInfo QueryGuestExecutableRange(
        FEXCore::Core::InternalThreadState*, uint64_t) override {
        return {0, std::numeric_limits<uint64_t>::max(), true};
    }

    std::optional<FEXCore::ExecutableFileSectionInfo> LookupExecutableFileSection(
        FEXCore::Core::InternalThreadState*, uint64_t) override {
        return std::nullopt;
    }
};

class SmokeSignalDelegator final : public FEXCore::SignalDelegator {
public:
    explicit SmokeSignalDelegator(uintptr_t callback_return) : callback_return_(callback_return) {}

    uintptr_t GetThunkCallbackRET() const override {
        return callback_return_;
    }

private:
    uintptr_t callback_return_;
};

class ThreadScope final {
public:
    ThreadScope(FEXCore::Context::Context* context, FEXCore::Core::InternalThreadState* thread)
        : context_(context), thread_(thread) {}

    ThreadScope(const ThreadScope&) = delete;
    ThreadScope& operator=(const ThreadScope&) = delete;

    ~ThreadScope() {
        if (thread_ != nullptr) {
            context_->DestroyThread(thread_);
        }
    }

private:
    FEXCore::Context::Context* context_;
    FEXCore::Core::InternalThreadState* thread_;
};

class GuestSegmentState final {
public:
    void Initialize(FEXCore::Core::CPUState& state) {
        state.segment_arrays[FEXCore::Core::CPUState::SEGMENT_ARRAY_INDEX_GDT] = gdt_.data();
        state.segment_arrays[FEXCore::Core::CPUState::SEGMENT_ARRAY_INDEX_LDT] = gdt_.data();
        state.cs_idx = FEXCore::Core::CPUState::DEFAULT_USER_CS << 3;
        auto* code_segment = FEXCore::Core::CPUState::GetSegmentFromIndex(state, state.cs_idx);
        FEXCore::Core::CPUState::SetGDTBase(code_segment, 0);
        FEXCore::Core::CPUState::SetGDTLimit(code_segment, 0xFFFFFU);
        state.cs_cached = FEXCore::Core::CPUState::CalculateGDTBase(*code_segment);
        code_segment->L = 1;
        code_segment->D = 0;
    }

private:
    std::array<FEXCore::Core::CPUState::gdt_segment, 32> gdt_{};
};

void AppendImmediate(std::vector<uint8_t>& code, uint64_t value) {
    const auto offset = code.size();
    code.resize(offset + sizeof(value));
    std::memcpy(code.data() + offset, &value, sizeof(value));
}

struct GuestCode final {
    std::vector<uint8_t> bytes;
    size_t callback_offset = 0;
    size_t callback_return_offset = 0;
    size_t invalidation_offset = 0;
    size_t invalidation_immediate_offset = 0;
    size_t thread_offset = 0;
};

struct ThreadProof final {
    bool created = false;
    bool tls_read = false;
};

GuestCode BuildGuestCode() {
    GuestCode result;
    auto& code = result.bytes;

    // 64-bit GPR arithmetic + flags + SSE double add + call/push/ret + hlt
    code.insert(code.end(), {0x48, 0xB8}); // mov rax, kAddLeft
    AppendImmediate(code, kAddLeft);
    code.insert(code.end(), {0x48, 0xBB}); // mov rbx, kAddRight
    AppendImmediate(code, kAddRight);
    code.insert(code.end(), {0x48, 0x01, 0xD8}); // add rax, rbx

    code.insert(code.end(), {0x48, 0xB9}); // mov rcx, kXorLeft
    AppendImmediate(code, kXorLeft);
    code.insert(code.end(), {0x48, 0xBA}); // mov rdx, kXorRight
    AppendImmediate(code, kXorRight);
    code.insert(code.end(), {0x48, 0x31, 0xD1}); // xor rcx, rdx (clears CF/OF, updates ZF/SF/PF)

    code.insert(code.end(), {0xF2, 0x0F, 0x58, 0xC1}); // addsd xmm0, xmm1

    code.push_back(0xE8); // call stack_test
    const size_t call_disp_offset = code.size();
    code.resize(code.size() + sizeof(int32_t));
    code.push_back(0xF4); // hlt

    const size_t stack_test_offset = code.size();
    code.insert(code.end(), {0x48, 0xBA}); // mov rdx, kStackSentinel
    AppendImmediate(code, kStackSentinel);
    code.push_back(0x52);                           // push rdx
    code.insert(code.end(), {0x48, 0x8B, 0x34, 0x24}); // mov rsi, [rsp]
    code.push_back(0x5F);                           // pop rdi
    code.push_back(0xC3);                           // ret

    const auto displacement =
        static_cast<int32_t>(stack_test_offset - (call_disp_offset + sizeof(int32_t)));
    std::memcpy(code.data() + call_disp_offset, &displacement, sizeof(displacement));

    result.callback_offset = code.size();
    code.insert(code.end(), {0x48, 0x8D, 0x47, 0x05, 0xC3}); // lea rax, [rdi + 5]; ret

    result.callback_return_offset = code.size();
    code.insert(code.end(), {0x0F, 0x3E}); // FEX CALLBACKRET

    result.invalidation_offset = code.size();
    code.insert(code.end(), {0x48, 0xB8}); // mov rax, kInvalidationInitial
    result.invalidation_immediate_offset = code.size();
    AppendImmediate(code, kInvalidationInitial);
    code.push_back(0xF4); // hlt

    result.thread_offset = code.size();
    code.insert(code.end(),
                {0x64, 0x48, 0x8B, 0x04, 0x25, 0x00, 0x00, 0x00, 0x00}); // mov rax, fs:[0]
    code.push_back(0xF4);                                                 // hlt
    return result;
}

ThreadProof ExecuteThreadTlsCheck(FEXCore::Context::Context* context, uint64_t rip,
                                  size_t page_size, uint64_t sentinel) {
    ThreadProof proof;
    Mapping stack_page{page_size, PROT_READ | PROT_WRITE};
    Mapping tls_page{page_size, PROT_READ | PROT_WRITE};
    if (!stack_page.IsValid() || !tls_page.IsValid()) {
        return proof;
    }
    std::memcpy(tls_page.Get(), &sentinel, sizeof(sentinel));

    GuestSegmentState segment_state;
    CallRetStack callret_stack{page_size};
    if (!callret_stack.IsReserved() || !callret_stack.MakeWritable()) {
        return proof;
    }

    const uint64_t initial_rsp = reinterpret_cast<uint64_t>(stack_page.Get()) + page_size - 16;
    auto* thread = context->CreateThread(rip, initial_rsp);
    if (thread == nullptr) {
        return proof;
    }
    callret_stack.Initialize(thread);
    ThreadScope thread_scope{context, thread};
    segment_state.Initialize(thread->CurrentFrame->State);
    thread->CurrentFrame->State.fs_cached = reinterpret_cast<uint64_t>(tls_page.Get());

    context->ExecuteThread(thread);
    proof.created = true;
    proof.tls_read = thread->CurrentFrame->State.gregs[FEXCore::X86State::REG_RAX] == sentinel &&
                     thread->CurrentFrame->State.gregs[FEXCore::X86State::REG_RSP] == initial_rsp;
    return proof;
}

uint64_t DoubleBits(double value) {
    uint64_t bits = 0;
    std::memcpy(&bits, &value, sizeof(bits));
    return bits;
}

} // namespace

int main() {
    const size_t page_size = QueryPageSize();
    if (page_size == 0) {
        return Fail("page-size");
    }

    Mapping code_page{page_size, PROT_READ | PROT_WRITE};
    Mapping stack_page{page_size, PROT_READ | PROT_WRITE};
    Mapping tls_page{page_size, PROT_READ | PROT_WRITE};
    if (!code_page.IsValid() || !stack_page.IsValid() || !tls_page.IsValid()) {
        return Fail("mapping");
    }

    const auto guest_code = BuildGuestCode();
    if (guest_code.bytes.size() > page_size) {
        return Fail("guest-code-size");
    }
    std::memcpy(code_page.Get(), guest_code.bytes.data(), guest_code.bytes.size());
    FlushICache(code_page.Get(), guest_code.bytes.size());
    if (!code_page.Protect(PROT_READ)) {
        return Fail("protect-code-ro");
    }
    const uint64_t initial_rip = reinterpret_cast<uint64_t>(code_page.Get());

    ConfigScope config;
    auto host_features = FEX::FetchHostFeatures();
    auto context = FEXCore::Context::Context::CreateNewContext(host_features);
    if (!context) {
        return Fail("create-context");
    }

    SmokeSignalDelegator signal_delegator{initial_rip + guest_code.callback_return_offset};
    auto syscall_handler = fextl::make_unique<SmokeSyscallHandler>();
    context->SetSignalDelegator(&signal_delegator);
    context->SetSyscallHandler(syscall_handler.get());
    context->EnableExitOnHLT();
    if (!context->InitCore()) {
        return Fail("init-core");
    }

    const uint64_t initial_rsp = reinterpret_cast<uint64_t>(stack_page.Get()) + page_size - 16;
    GuestSegmentState segment_state;
    CallRetStack callret_stack{page_size};
    if (!callret_stack.IsReserved() || !callret_stack.MakeWritable()) {
        return Fail("callret-stack");
    }

    auto* thread = context->CreateThread(initial_rip, initial_rsp);
    if (thread == nullptr) {
        return Fail("create-thread");
    }
    {
        callret_stack.Initialize(thread);
        ThreadScope thread_scope{context.get(), thread};
        segment_state.Initialize(thread->CurrentFrame->State);
        thread->CurrentFrame->State.fs_cached = reinterpret_cast<uint64_t>(tls_page.Get());
        context->SetFlagsFromCompactedEFLAGS(thread, 0x202U);

        std::array<__uint128_t, FEXCore::Core::CPUState::NUM_XMMS> initial_xmm{};
        std::array<__uint128_t, FEXCore::Core::CPUState::NUM_XMMS> initial_ymm_high{};
        const uint64_t fp_left_bits = DoubleBits(kFpLeft);
        const uint64_t fp_right_bits = DoubleBits(kFpRight);
        std::memcpy(&initial_xmm[0], &fp_left_bits, sizeof(fp_left_bits));
        std::memcpy(&initial_xmm[1], &fp_right_bits, sizeof(fp_right_bits));
        context->SetXMMRegistersFromState(thread, initial_xmm.data(),
                                          host_features.SupportsAVX ? initial_ymm_high.data()
                                                                    : nullptr);

        context->ExecuteThread(thread);

        auto& state = thread->CurrentFrame->State;
        if (state.gregs[FEXCore::X86State::REG_RAX] != kAddLeft + kAddRight ||
            state.gregs[FEXCore::X86State::REG_RCX] != (kXorLeft ^ kXorRight)) {
            return Fail("gpr");
        }
        const uint32_t eflags = context->ReconstructCompactedEFLAGS(thread, false, nullptr, 0);
        if ((eflags & 0x881U) != 0x080U) { // SF set after xor of negative 0xF1F3..., CF/OF cleared
            return Fail("rflags");
        }
        if (state.gregs[FEXCore::X86State::REG_RSI] != kStackSentinel ||
            state.gregs[FEXCore::X86State::REG_RDI] != kStackSentinel ||
            state.gregs[FEXCore::X86State::REG_RSP] != initial_rsp) {
            return Fail("stack");
        }

        std::array<__uint128_t, FEXCore::Core::CPUState::NUM_XMMS> final_xmm{};
        std::array<__uint128_t, FEXCore::Core::CPUState::NUM_XMMS> final_ymm_high{};
        context->ReconstructXMMRegisters(thread, final_xmm.data(),
                                         host_features.SupportsAVX ? final_ymm_high.data() : nullptr);
        uint64_t fp_result_bits = 0;
        std::memcpy(&fp_result_bits, &final_xmm[0], sizeof(fp_result_bits));
        if (fp_result_bits != DoubleBits(kFpResult)) {
            return Fail("xmm");
        }

        ThreadProof proof_a;
        ThreadProof proof_b;
        const uint64_t thread_rip = initial_rip + guest_code.thread_offset;
        std::thread host_a{[&] {
            proof_a = ExecuteThreadTlsCheck(context.get(), thread_rip, page_size, kThreadSentinelA);
        }};
        std::thread host_b{[&] {
            proof_b = ExecuteThreadTlsCheck(context.get(), thread_rip, page_size, kThreadSentinelB);
        }};
        host_a.join();
        host_b.join();
        if (!proof_a.created || !proof_b.created) {
            return Fail("threads");
        }
        if (!proof_a.tls_read || !proof_b.tls_read) {
            return Fail("tls");
        }

        state.gregs[FEXCore::X86State::REG_RDI] = kCallbackInput;
        state.gregs[FEXCore::X86State::REG_RSP] = initial_rsp;
        context->HandleCallback(thread, initial_rip + guest_code.callback_offset);
        if (state.gregs[FEXCore::X86State::REG_RAX] != kCallbackInput + 5 ||
            state.gregs[FEXCore::X86State::REG_RSP] != initial_rsp) {
            return Fail("callback");
        }

        const uint64_t inv_rip = initial_rip + guest_code.invalidation_offset;
        state.rip = inv_rip;
        state.gregs[FEXCore::X86State::REG_RSP] = initial_rsp;
        context->ExecuteThread(thread);
        if (state.gregs[FEXCore::X86State::REG_RAX] != kInvalidationInitial) {
            return Fail("invalidation-initial");
        }

        if (!code_page.Protect(PROT_READ | PROT_WRITE)) {
            return Fail("protect-code-rw");
        }
        auto* inv_imm =
            static_cast<uint8_t*>(code_page.Get()) + guest_code.invalidation_immediate_offset;
        std::memcpy(inv_imm, &kInvalidationUpdated, sizeof(kInvalidationUpdated));
        FlushICache(inv_imm, sizeof(kInvalidationUpdated));
        if (!code_page.Protect(PROT_READ)) {
            return Fail("protect-code-ro2");
        }
        {
            std::scoped_lock lock{context->GetCodeInvalidationMutex()};
            context->InvalidateCodeBuffersCodeRange(inv_rip, 11);
            context->InvalidateThreadCachedCodeRange(thread, inv_rip, 11);
        }

        state.rip = inv_rip;
        state.gregs[FEXCore::X86State::REG_RSP] = initial_rsp;
        context->ExecuteThread(thread);
        if (state.gregs[FEXCore::X86State::REG_RAX] != kInvalidationUpdated) {
            return Fail("invalidation");
        }
    }

    std::printf(
        "FEXCORE_GUEST_SMOKE_OK revision=%.*s page_size=%zu gpr=ok rflags=ok xmm=ok stack=ok "
        "threads=ok tls=ok callback=ok invalidation=ok teardown=ok\n",
        static_cast<int>(kFexRevision.size()), kFexRevision.data(), page_size);
    return 0;
}
