/* C thread-locals other C and C++ code reads directly. Rust cannot export a thread-local, so
 * they stay here while the modules that set them move to crates/runtime (ADR 0002). */
#include "runtime.h"

/* Speculative readers of guest memory (GPU draw-preparation workers) register a recovery
 * point: a fault on that thread jumps back to it instead of terminating (probe.c). Here since
 * the memory module became Rust on Linux (piece 3.12) and src/runtime.c went (piece 3.01). */
__thread sigjmp_buf *runtime_fault_recover;

/* bbport (Mevlock): set while a render command context is open on this thread (runtime_pad.c
 * hooks, the job wait patch in probe.c). A fiber leaving the thread takes its context along, so
 * sceFiberSwitch and sceFiberReturnToThread clear it through runtime_ndgi_leave: from
 * runtime_fiber.c on Windows, from crates/runtime fiber on Linux (piece 3.13). */
_Thread_local int runtime_ndgi_open;
void runtime_ndgi_leave(void) { runtime_ndgi_open=0; }
