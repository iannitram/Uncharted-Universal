/* Backend that runs each synthetic case through bbcpu's interpreter (linked, not vendored; #212).
 * Authored for this repo against bbcpu's public header only. The case bytes are wrapped in an
 * authored x86-64 stub that loads the input state from memory and stores the final state back,
 * because bbcpu_call only passes six integer arguments. Each case runs in a forked child:
 * bbcpu stops the process (exit 30, "STOP: bbcpu: ..." on stderr) instead of delivering faults,
 * so the parent classifies that stop. rsp is not modelled (the stub does not load or store it). */
#include "nojit_abi.h"
#include "cpu/bbcpu.h"
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <unistd.h>

uint64_t bb_image_base = 0;

void *runtime_low_map(size_t size, int prot) {
    void *p = mmap(NULL, size, prot, MAP_ANON | MAP_PRIVATE, -1, 0);
    return p == MAP_FAILED ? NULL : p;
}

#define OFF_FLAGS 128
#define OFF_XMM 136
#define CODE_SIZE 0x4000

static size_t put(uint8_t *b, size_t n, const void *s, size_t k) { memcpy(b + n, s, k); return n + k; }
static size_t put32(uint8_t *b, size_t n, uint32_t v) { return put(b, n, &v, 4); }
/* mov r64,[rdi+disp32] (load) or mov [rdi+disp32],r64 (store) */
static size_t movgpr(uint8_t *b, size_t n, int r, uint32_t d, int store) {
    b[n++] = (uint8_t)(0x48 | (r >= 8 ? 4 : 0));
    b[n++] = store ? 0x89 : 0x8B;
    b[n++] = (uint8_t)(0x80 | ((r & 7) << 3) | 7);
    return put32(b, n, d);
}
/* movups xmmN,[rdi+disp32] (0F 10) or [rdi+disp32],xmmN (0F 11) */
static size_t movxmm(uint8_t *b, size_t n, int r, uint32_t d, int store) {
    if (r >= 8) b[n++] = 0x44;
    b[n++] = 0x0F;
    b[n++] = store ? 0x11 : 0x10;
    b[n++] = (uint8_t)(0x80 | ((r & 7) << 3) | 7);
    return put32(b, n, d);
}

/* stub(rdi = in, rsi = out): load state, run case bytes, store state. */
static size_t assemble(uint8_t *b, const uint8_t *code, size_t len) {
    size_t n = 0;
    b[n++] = 0x56; /* push rsi */
    for (int i = 0; i < 16; i++) n = movxmm(b, n, i, OFF_XMM + 16u * (uint32_t)i, 0);
    { static const uint8_t pf[] = {0xFF, 0xB7, 0x80, 0, 0, 0, 0x9D}; n = put(b, n, pf, sizeof pf); }
    for (int i = 0; i < 16; i++) if (i != 4 && i != 7) n = movgpr(b, n, i, 8u * (uint32_t)i, 0);
    n = movgpr(b, n, 7, 56, 0); /* rdi last */
    n = put(b, n, code, len);
    { static const uint8_t e[] = {0x57, 0x48, 0x8B, 0x7C, 0x24, 0x08, 0x9C, 0x8F, 0x87, 0x80, 0, 0, 0};
      n = put(b, n, e, sizeof e); }
    for (int i = 0; i < 16; i++) if (i != 4 && i != 7) n = movgpr(b, n, i, 8u * (uint32_t)i, 1);
    for (int i = 0; i < 16; i++) n = movxmm(b, n, i, OFF_XMM + 16u * (uint32_t)i, 1);
    { static const uint8_t t[] = {0x58}; n = put(b, n, t, 1); }
    n = movgpr(b, n, 0, 56, 1); /* rdi's value, popped into rax */
    { static const uint8_t r[] = {0x48, 0x83, 0xC4, 0x08, 0xC3}; n = put(b, n, r, sizeof r); }
    return n;
}

static void child(const uint8_t *code, size_t len, const nojit_state *in, int out_fd) {
    uint8_t *mem = mmap(NULL, CODE_SIZE, PROT_READ | PROT_WRITE, MAP_ANON | MAP_PRIVATE, -1, 0);
    if (mem == MAP_FAILED || len > 256) _exit(40);
    assemble(mem, code, len);
    bbcpu_add_guest_code((uintptr_t)mem, CODE_SIZE);
    static nojit_state in_copy, out_state;
    in_copy = *in;
    const uint64_t args[2] = {(uint64_t)(uintptr_t)&in_copy, (uint64_t)(uintptr_t)&out_state};
    bbcpu_call((uintptr_t)mem, 2, args);
    if (write(out_fd, &out_state, sizeof out_state) != (ssize_t)sizeof out_state) _exit(41);
    _exit(0);
}

int nojit_run(const uint8_t *code, size_t len, const nojit_state *in, nojit_state *out,
              int *fault_vector) {
    int po[2], pe[2];
    if (pipe(po) || pipe(pe)) return NOJIT_FAULT;
    fflush(NULL);
    pid_t pid = fork();
    if (pid < 0) { *fault_vector = 255; return NOJIT_FAULT; }
    if (pid == 0) {
        close(po[0]); close(pe[0]);
        dup2(pe[1], 2);
        dup2(pe[1], 1); /* bbcpu banners must not reach the report */
        child(code, len, in, po[1]);
    }
    close(po[1]); close(pe[1]);
    nojit_state st; size_t got = 0; char msg[512]; size_t mlen = 0;
    ssize_t k;
    while ((k = read(po[0], (char *)&st + got, sizeof st - got)) > 0) got += (size_t)k;
    while ((k = read(pe[0], msg + mlen, sizeof msg - 1 - mlen)) > 0) mlen += (size_t)k;
    msg[mlen] = 0;
    close(po[0]); close(pe[0]);
    int status = 0;
    waitpid(pid, &status, 0);
    if (WIFEXITED(status) && WEXITSTATUS(status) == 0 && got == sizeof st) {
        *out = st;
        out->gpr[4] = in->gpr[4];
        return NOJIT_OK;
    }
    if (WIFEXITED(status) && WEXITSTATUS(status) == 30) {
        if (strstr(msg, "division by zero")) { *fault_vector = 0; return NOJIT_FAULT; }
        if (strstr(msg, "unsupported instruction")) return NOJIT_UNSUPPORTED;
    }
    *fault_vector = 255; /* anything else: a crash, reported as a failure */
    return NOJIT_FAULT;
}

const char *nojit_backend_name(void) { return "bbcpu"; }
