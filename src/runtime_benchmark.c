/* Benchmark mode glue (BB_BENCHMARK=<seconds>, #173/#242). All the logic is Rust
 * (crates/runtime/src/benchmark_run.rs: the pad schedule, the samples, the report); this file is
 * the one place the C loader and the GPU library reach it from. Builds that do not link the Rust
 * runtime (the C unit tests) get no-ops, and a BB_BENCHMARK request there fails loudly instead
 * of playing a normal session. */
#include "runtime.h"
#include <stdio.h>
#include <stdlib.h>

#ifdef BB_RUST_BENCHMARK
typedef RuntimeBenchmarkPad UnchartedBenchmarkPad; /* the same #[repr(C)] layout */
int uncharted_benchmark_start(const char *saves_dir);
int uncharted_benchmark_active(void);
int uncharted_benchmark_pad(UnchartedBenchmarkPad *out);
int uncharted_benchmark_frame(uint64_t vram_mib);
void uncharted_benchmark_crash(void);

int runtime_benchmark_start(const char *saves_dir) { return uncharted_benchmark_start(saves_dir); }
int runtime_benchmark_active(void) { return uncharted_benchmark_active(); }
int runtime_benchmark_pad(RuntimeBenchmarkPad *out) { return uncharted_benchmark_pad(out); }
void runtime_benchmark_frame(uint64_t vram_mib) {
    const int state = uncharted_benchmark_frame(vram_mib);
    if (state) runtime_quit(state == 1 ? 0 : 3); /* the report is written, or the run failed */
}
void runtime_benchmark_crash(void) { uncharted_benchmark_crash(); }
#else
int runtime_benchmark_start(const char *saves_dir) {
    (void)saves_dir;
    if (!getenv("BB_BENCHMARK")) return 0;
    fputs("ERROR: BB_BENCHMARK is set, but this build has no benchmark mode (it links the Rust runtime)\n", stderr);
    return -1;
}
int runtime_benchmark_active(void) { return 0; }
int runtime_benchmark_pad(RuntimeBenchmarkPad *out) { (void)out; return 0; }
void runtime_benchmark_frame(uint64_t vram_mib) { (void)vram_mib; }
void runtime_benchmark_crash(void) {}
#endif
