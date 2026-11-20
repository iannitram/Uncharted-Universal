/* Speculative readers of guest memory (GPU draw-preparation workers) register a recovery point:
 * a fault on that thread jumps back to it instead of terminating (probe.c). A C file of its own
 * since src/runtime.c moved to crates/runtime (piece 3.01): Rust cannot export a C thread-local,
 * and src/runtime_memory.c is Windows only (piece 3.12). */
#include "runtime.h"

__thread sigjmp_buf *runtime_fault_recover;

#ifndef _WIN32
/* Linux builds fiber in Rust (crates/runtime/src/fiber.rs, piece 3.13); keep this C
 * thread-local and setter in C for probe.c's job_lock_wait and pad/hooks.rs. */
_Thread_local int runtime_ndgi_open;
void runtime_ndgi_set(int open) { runtime_ndgi_open = open; }
#endif
