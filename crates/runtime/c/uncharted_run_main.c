/* `main` of the Windows CLANG64 `uncharted-run.exe` (roadmap 3.17d, #457).
 *
 * Everything the runner does is Rust (crates/runtime/src/loader/cli.rs). CMake links this file
 * with libuncharted_runtime.a and the static GPU library (gpu/CMakeLists.txt), because the GPU
 * library is C++ with its own dependencies and a Rust binary cannot name them. This file only
 * hands the GPU library's entry points to the Rust runner and gives the GPU library and the C
 * modules that remain the two process-control functions they call, `runtime_quit` and
 * `runtime_restart`, which are Rust (loader/entry.rs) and are not exported under those names so
 * that bb-probe (src/probe.c, which has its own) can link the same library. */
#include <stdint.h>
#include <stdio.h>
#include "../include/uncharted_runtime.h"
#include "../../../gpu/bbgpu.h"

int bbgpu_fault_recover(int code, void *context); /* gpu/shim/bbport_fault_recover.cpp */

/* Keep in step with UnchartedRunHooks in crates/runtime/src/loader/cli.rs. */
typedef struct {
    uint32_t version;
    void (*bbgpu_register_kernel)(void);
    void (*bbnet_configure)(const char *, const char *, const char *, const char *);
    int (*bbgpu_init)(const BbGpuConfig *);
    void (*bbgpu_prepare_exit)(void);
    unsigned (*bbgpu_symbol_count)(void);
    void (*bbgpu_trophy_notify)(int, const char *, const char *, const void *, uint64_t);
    int (*gpu_handle_fault)(void *, void *);
    void (*gpu_dump_writes)(void *);
    int (*fault_recover)(int, void *);
} UnchartedRunHooks;
#define UNCHARTED_RUN_HOOKS_VERSION 1u

int uncharted_run_main(const UnchartedRunHooks *hooks);
__attribute__((noreturn)) void uncharted_run_quit(int code);
__attribute__((noreturn)) void uncharted_run_restart(void);

__attribute__((noreturn)) void runtime_quit(int code) { uncharted_run_quit(code); }
void runtime_restart(void) { uncharted_run_restart(); }

int main(void) {
    /* The guest's output and the GPU library's log lines share the pipe a launcher reads. */
    setvbuf(stdout, NULL, _IONBF, 0);
    /* A library built from other sources than this header's would misbehave quietly. */
    if (uncharted_runtime_version() != UNCHARTED_RUNTIME_ABI_VERSION) {
        fprintf(stderr, "ERROR: crates/runtime has ABI version %u, this loader needs %u: rebuild both\n",
                (unsigned)uncharted_runtime_version(), (unsigned)UNCHARTED_RUNTIME_ABI_VERSION);
        return 1;
    }
    const UnchartedRunHooks hooks = {
        .version = UNCHARTED_RUN_HOOKS_VERSION,
        .bbgpu_register_kernel = bbgpu_register_kernel,
        .bbnet_configure = bbnet_configure,
        .bbgpu_init = bbgpu_init,
        .bbgpu_prepare_exit = bbgpu_prepare_exit,
        .bbgpu_symbol_count = bbgpu_symbol_count,
        .bbgpu_trophy_notify = bbgpu_trophy_notify,
        .gpu_handle_fault = bbgpu_handle_fault,
        .gpu_dump_writes = bbgpu_dump_guest_writes,
        .fault_recover = bbgpu_fault_recover,
    };
    return uncharted_run_main(&hooks);
}
