/* C thread-locals other C and C++ code reads directly. Rust cannot export a thread-local, so
 * they stay here while the modules that set them move to crates/runtime (ADR 0002). */
#include "runtime.h"

/* Speculative readers of guest memory (GPU draw-preparation workers) register a recovery
 * point: a fault on that thread jumps back to it instead of terminating (probe.c). Here since
 * the memory module became Rust on Linux (piece 3.12) and src/runtime.c went (piece 3.01). */
__thread sigjmp_buf *runtime_fault_recover;
