/* Synthetic x86-64 cases for the Apple ARM64 no-JIT interpreter path (#212). Instruction bytes
 * and expected results are hand-assembled and hand-checked; no game data. Writes a JSON coverage
 * report. Exit 1 on any wrong result; unsupported cases are reported, not failed, unless
 * --require-supported is given. Usage: test_nojit [--require-supported] [report.json] */
#include "nojit_abi.h"
#include <stdio.h>
#include <string.h>
#if defined(__APPLE__)
#include <TargetConditionals.h>
#endif
#if defined(__APPLE__) && TARGET_OS_IPHONE
#define NOJIT_BUILD 1 /* iOS: JIT is unavailable, interpreter only */
#else
#define NOJIT_BUILD 0
#endif

#define FLAGS_ARITH 0x8D5ull /* CF PF AF ZF SF OF */
#define F_CF 1ull
#define F_PF 4ull
#define F_AF 0x10ull
#define F_ZF 0x40ull
#define F_OF 0x800ull

typedef struct {
    const char *family, *name;
    uint8_t code[8];
    size_t len;
    nojit_state in, exp;
    uint64_t flag_mask;
    int fault; /* -1 none, else expected vector */
} tcase;

static void u32x4(uint8_t *x, uint32_t a, uint32_t b, uint32_t c, uint32_t d) {
    uint32_t v[4] = {a, b, c, d};
    memcpy(x, v, 16);
}

static size_t build(tcase *t) {
    size_t n = 0;
    memset(t, 0, sizeof t[0] * 8);
    /* add rax,rbx: -1 + 1 = 0; CF PF AF ZF set */
    t[n] = (tcase){"integer-flags", "add64_carry", {0x48, 0x01, 0xD8}, 3};
    t[n].in.gpr[0] = ~0ull; t[n].in.gpr[3] = 1;
    t[n].exp = t[n].in; t[n].exp.gpr[0] = 0;
    t[n].flag_mask = FLAGS_ARITH; t[n].exp.rflags = F_CF | F_PF | F_AF | F_ZF; t[n].fault = -1; n++;
    /* shl rax,4: 1 -> 0x10, CF clear, PF clear (one bit set); AF/OF undefined so not compared */
    t[n] = (tcase){"shift-rotate", "shl64_imm", {0x48, 0xC1, 0xE0, 0x04}, 4};
    t[n].in.gpr[0] = 1; t[n].exp = t[n].in; t[n].exp.gpr[0] = 0x10;
    t[n].flag_mask = F_CF | F_PF | F_ZF | 0x80; t[n].fault = -1; n++;
    /* mul rbx: 0x8000000000000000 * 4 = 2:0 (rdx:rax); CF OF set */
    t[n] = (tcase){"wide-multiply-divide", "mul64_wide", {0x48, 0xF7, 0xE3}, 3};
    t[n].in.gpr[0] = 1ull << 63; t[n].in.gpr[3] = 4;
    t[n].exp = t[n].in; t[n].exp.gpr[0] = 0; t[n].exp.gpr[2] = 2;
    t[n].flag_mask = F_CF | F_OF; t[n].exp.rflags = F_CF | F_OF; t[n].fault = -1; n++;
    /* div rbx with rbx = 0: #DE */
    t[n] = (tcase){"fault", "div_by_zero", {0x48, 0xF7, 0xF3}, 3};
    t[n].in.gpr[0] = 7; t[n].fault = 0; n++;
    /* ud2: #UD */
    t[n] = (tcase){"fault", "ud2", {0x0F, 0x0B}, 2}; t[n].fault = 6; n++;
    /* bsr rax,rcx: rcx = 0x100 -> 8 */
    t[n] = (tcase){"bit-scan", "bsr64", {0x48, 0x0F, 0xBD, 0xC1}, 4};
    t[n].in.gpr[1] = 0x100; t[n].exp = t[n].in; t[n].exp.gpr[0] = 8;
    t[n].flag_mask = F_ZF; t[n].fault = -1; n++;
    /* paddd xmm0,xmm1 lane-wise with wraparound */
    t[n] = (tcase){"sse-integer", "paddd", {0x66, 0x0F, 0xFE, 0xC1}, 4};
    u32x4(t[n].in.xmm[0], 1, 2, 3, 0xFFFFFFFFu); u32x4(t[n].in.xmm[1], 10, 20, 30, 1);
    t[n].exp = t[n].in; u32x4(t[n].exp.xmm[0], 11, 22, 33, 0); t[n].fault = -1; n++;
    /* pxor xmm0,xmm0 clears */
    t[n] = (tcase){"sse-integer", "pxor_zero", {0x66, 0x0F, 0xEF, 0xC0}, 4};
    u32x4(t[n].in.xmm[0], 5, 6, 7, 8); t[n].exp = t[n].in; memset(t[n].exp.xmm[0], 0, 16);
    t[n].fault = -1; n++;
    return n;
}

/* Families the planned AOT library needs but this file does not author yet. */
static const char *NOT_AUTHORED[] = {"x87", "avx-avx2", "atomics-ordering", "string-ops",
    "unaligned-memory", "host-call-callback", "tls", "sse-float", "bmi-lzcnt-movbe"};

int main(int argc, char **argv) {
    int require = 0;
    const char *path = NULL;
    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--require-supported")) require = 1; else path = argv[i];
    }
    static tcase t[8];
    size_t n = build(t);
    int pass = 0, fail = 0, unsup = 0;
    FILE *f = path ? fopen(path, "w") : stdout;
    if (!f) { perror("report"); return 2; }
    fprintf(f, "{\n  \"backend\": \"%s\",\n  \"no_jit_build\": %d,\n  \"cases\": [\n",
            nojit_backend_name(), NOJIT_BUILD);
    for (size_t i = 0; i < n; i++) {
        nojit_state out; memset(&out, 0, sizeof out);
        int vec = -1;
        int r = nojit_run(t[i].code, t[i].len, &t[i].in, &out, &vec);
        const char *res;
        if (r == NOJIT_UNSUPPORTED) { res = "unsupported"; unsup++; }
        else {
            int ok;
            if (t[i].fault >= 0) ok = (r == NOJIT_FAULT && vec == t[i].fault);
            else ok = (r == NOJIT_OK &&
                       !memcmp(out.gpr, t[i].exp.gpr, sizeof out.gpr) &&
                       !memcmp(out.xmm, t[i].exp.xmm, sizeof out.xmm) &&
                       (out.rflags & t[i].flag_mask) == (t[i].exp.rflags & t[i].flag_mask));
            res = ok ? "pass" : "fail"; if (ok) pass++; else fail++;
        }
        fprintf(f, "    {\"family\": \"%s\", \"case\": \"%s\", \"result\": \"%s\"}%s\n",
                t[i].family, t[i].name, res, i + 1 < n ? "," : "");
    }
    fprintf(f, "  ],\n  \"pass\": %d, \"fail\": %d, \"unsupported\": %d,\n  \"not_authored\": [",
            pass, fail, unsup);
    for (size_t i = 0; i < sizeof NOT_AUTHORED / sizeof *NOT_AUTHORED; i++)
        fprintf(f, "%s\"%s\"", i ? ", " : "", NOT_AUTHORED[i]);
    fprintf(f, "]\n}\n");
    if (path) fclose(f);
    return (fail || (require && unsup)) ? 1 : 0;
}
