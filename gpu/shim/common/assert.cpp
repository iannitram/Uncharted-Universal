// bbport: GPU-side assertion failures stop the port with exit code 23.
#include <cstdio>
#include <cstdlib>
#include <stdexcept>
#ifdef _WIN32
#include <windows.h>
#else
#include <csignal>
#endif
#include "bbport_toggles.h"
#include "common/assert.h"
#include "common/logging/log.h"

__thread sigjmp_buf* runtime_fault_recover = nullptr;

// A draw-preparation worker translates shaders and builds pipelines ahead of the GPU thread, from
// guest memory the game may already have reused. Where it armed a recovery point (the same one a
// faulting read uses), an assertion on such data only abandons the speculation; the GPU thread
// prepares the draw itself, from memory as it is when the draw executes.
static void abandon_speculation() {
    if (!runtime_fault_recover) {
        return;
    }
    sigjmp_buf* recover = runtime_fault_recover;
    runtime_fault_recover = nullptr;
    std::fputs("GPU assertion while preparing a draw ahead of the GPU thread: leaving the draw to the GPU thread\n",
               stderr);
#ifdef _WIN32
    bb_longjmp(*recover, 1);
#else
    siglongjmp(*recover, 1);
#endif
}

void assert_fail_impl() {
    abandon_speculation();
    std::fflush(stdout);
    std::fputs("STOP: GPU library assertion failed (see GPU log above)\n", stderr);
    std::_Exit(23);
}

[[noreturn]] void unreachable_impl() {
    assert_fail_impl();
    throw std::runtime_error("Unreachable code");
}

void assert_fail_debug_msg(const char* msg) {
    LOG_CRITICAL(Debug, "Assertion Failed!\n{}", msg);
    assert_fail_impl();
}

extern "C" int bbgpu_fault_recover(int sig_or_code, void* context) {
#ifdef _WIN32
    if (static_cast<DWORD>(sig_or_code) == EXCEPTION_ACCESS_VIOLATION && runtime_fault_recover) {
        unsigned long long* buffer = *runtime_fault_recover;
        runtime_fault_recover = nullptr;
        auto* info = static_cast<EXCEPTION_POINTERS*>(context);
        CONTEXT* c = info->ContextRecord;
        c->Rsp = ((c->Rsp - 128) & ~static_cast<DWORD64>(15)) - 40;
        c->Rcx = reinterpret_cast<DWORD64>(buffer);
        c->Rdx = 1;
        c->Rip = reinterpret_cast<DWORD64>(&bb_longjmp);
        return 1;
    }
#else
    (void)context;
    if ((sig_or_code == SIGSEGV || sig_or_code == SIGBUS) && runtime_fault_recover) {
        sigjmp_buf* recover = runtime_fault_recover;
        runtime_fault_recover = nullptr;
        sigset_t unblock;
        sigemptyset(&unblock);
        sigaddset(&unblock, sig_or_code);
        pthread_sigmask(SIG_UNBLOCK, &unblock, nullptr);
        siglongjmp(*recover, 1);
    }
#endif
    return 0;
}
