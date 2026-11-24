// bbport (Windows): the recovery point of speculative guest memory reads, for the Rust loader's
// vectored exception handler (crates/runtime/src/loader/fault.rs, roadmap 3.17c).
//
// A draw-preparation worker arms runtime_fault_recover (a thread-local sigjmp_buf pointer, see
// bbport_toggles.h) before it reads guest memory the game may already have reused. If the read
// faults, the thread resumes in bb_longjmp at that recovery point, outside the exception
// dispatcher (no unwinding through the faulting frames). src/probe.c did this inline in its
// handler; the Rust handler calls this function instead, because the thread-local belongs to
// this library.
#ifdef _WIN32
#include <windows.h>
#include "bbport_toggles.h"

extern "C" int bbgpu_fault_recover(int code, void* exception_pointers) {
    (void)code;
    if (!runtime_fault_recover || !exception_pointers) {
        return 0;
    }
    auto* info = static_cast<EXCEPTION_POINTERS*>(exception_pointers);
    unsigned long long* buffer = *runtime_fault_recover;
    runtime_fault_recover = nullptr;
    CONTEXT* c = info->ContextRecord;
    // Leave the red zone, align, and reserve the shadow space a call expects.
    c->Rsp = ((c->Rsp - 128) & ~static_cast<DWORD64>(15)) - 40;
    c->Rcx = reinterpret_cast<DWORD64>(buffer);
    c->Rdx = 1;
    c->Rip = reinterpret_cast<DWORD64>(&bb_longjmp);
    return 1;
}
#else
// Linux and the other hosts: the C loader (src/probe.c) still owns this recovery there.
extern "C" int bbgpu_fault_recover(int, void*) {
    return 0;
}
#endif
