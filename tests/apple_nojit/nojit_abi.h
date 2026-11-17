/* Narrow C ABI between the synthetic no-JIT test and an interpreter backend (#212).
 * Authored for this repo; no bbcpu code is copied. A backend implements nojit_run(). */
#ifndef NOJIT_ABI_H
#define NOJIT_ABI_H
#include <stddef.h>
#include <stdint.h>

typedef struct {
    uint64_t gpr[16];     /* rax rcx rdx rbx rsp rbp rsi rdi r8..r15 */
    uint64_t rflags;
    uint8_t xmm[16][16];
} nojit_state;

enum { NOJIT_OK = 0, NOJIT_FAULT = 1, NOJIT_UNSUPPORTED = 2 };
/* x86 vector numbers for fault_vector: 0 = #DE, 6 = #UD. */

/* Run `len` bytes of x86-64 code from `in` and write the final state to `out`.
 * Return NOJIT_OK, NOJIT_FAULT (set *fault_vector), or NOJIT_UNSUPPORTED (clean refusal). */
int nojit_run(const uint8_t *code, size_t len, const nojit_state *in, nojit_state *out,
              int *fault_vector);
/* Backend name for the report, e.g. "none" or the interpreter revision. */
const char *nojit_backend_name(void);
#endif
