/* Further synthetic cases (included by test_nojit.c). Every expected value is worked by hand from
 * the Intel SDM instruction pages (comment on each case); none is taken from running an
 * interpreter. Scratch memory mem[] is filled with mem[i] = i + 1 where a case says "seq".
 * Memory operands use NOJIT_MEM + off (see nojit_abi.h). */
#define M(off) (NOJIT_MEM + (off))
enum { RAX, RCX, RDX, RBX, RSP, RBP, RSI, RDI };
#define F_DF 0x400ull
#define F_SF 0x80ull

static tcase *cur;
#define CASE(fam, nm, ...) do { static const uint8_t c_[] = {__VA_ARGS__}; \
    cur = nxt++; cur->family = fam; cur->name = nm; cur->len = sizeof c_; \
    memcpy(cur->code, c_, sizeof c_); cur->fault = -1; } while (0)
#define SAME() (cur->exp = cur->in)
#define IN cur->in
#define EX cur->exp

static void seq(uint8_t *m) { for (int i = 0; i < 256; i++) m[i] = (uint8_t)(i + 1); }
static void p32(uint8_t *m, int o, uint32_t v) { memcpy(m + o, &v, 4); }
static void p64(uint8_t *m, int o, uint64_t v) { memcpy(m + o, &v, 8); }
static void pd(uint8_t *m, int o, double v) { memcpy(m + o, &v, 8); }
static void x64(uint8_t *x, uint64_t lo, uint64_t hi) { memcpy(x, &lo, 8); memcpy(x + 8, &hi, 8); }

static size_t build_more(tcase *nxt0) {
    tcase *nxt = nxt0;

    /* jz selects eax=2 when ZF=1, else eax=1; jmp skips the other arm; MOV/Jcc preserve flags. */
    CASE("branches", "jz_taken", 0x74, 0x07, 0xB8, 1, 0, 0, 0, 0xEB, 0x05, 0xB8, 2, 0, 0, 0);
    IN.rflags = F_ZF; SAME(); EX.gpr[RAX] = 2; cur->flag_mask = FLAGS_ARITH;
    CASE("branches", "jz_not_taken", 0x74, 0x07, 0xB8, 1, 0, 0, 0, 0xEB, 0x05, 0xB8, 2, 0, 0, 0);
    SAME(); EX.gpr[RAX] = 1; cur->flag_mask = FLAGS_ARITH;
    /* call +2 to lea rax,[rax+rax]; ret, then jmp +5 past the callee: 21*2=42, flags unchanged. */
    CASE("calls-returns", "guest_call_ret", 0xE8, 2, 0, 0, 0, 0xEB, 5, 0x48, 0x8D, 0x04, 0x00, 0xC3);
    IN.gpr[RAX] = 21; SAME(); EX.gpr[RAX] = 42; cur->flag_mask = FLAGS_ARITH;

    /* ---- string-ops (SDM: MOVS, STOS, LODS, CMPS, SCAS; REP/REPE/REPNE; DF) ---- */
    /* rep movsb, rcx=5: copies mem[0..5)=1..5 to mem[64..69); rsi/rdi advance 5, rcx=0 */
    CASE("string-ops", "rep_movsb", 0xF3, 0xA4);
    IN.gpr[RCX] = 5; IN.gpr[RSI] = M(0); IN.gpr[RDI] = M(64); seq(IN.mem); SAME();
    memcpy(EX.mem + 64, IN.mem, 5); EX.gpr[RCX] = 0; EX.gpr[RSI] = M(5); EX.gpr[RDI] = M(69);
    /* rep stosb: al=0xAB stored 4 times at mem[64..68) */
    CASE("string-ops", "rep_stosb", 0xF3, 0xAA);
    IN.gpr[RAX] = 0xAB; IN.gpr[RCX] = 4; IN.gpr[RDI] = M(64); seq(IN.mem); SAME();
    memset(EX.mem + 64, 0xAB, 4); EX.gpr[RCX] = 0; EX.gpr[RDI] = M(68);
    /* repe cmpsb on "ABCD" vs "ABXD": 3 compares, stops at C(43h)-X(58h): ZF=0, borrow CF=1 */
    CASE("string-ops", "repe_cmpsb", 0xF3, 0xA6);
    IN.gpr[RCX] = 4; IN.gpr[RSI] = M(0); IN.gpr[RDI] = M(64);
    memcpy(IN.mem, "ABCD", 4); memcpy(IN.mem + 64, "ABXD", 4); SAME();
    EX.gpr[RCX] = 1; EX.gpr[RSI] = M(3); EX.gpr[RDI] = M(67);
    cur->flag_mask = F_ZF | F_CF; EX.rflags = F_CF;
    /* rep movsq, rcx=2: copies 16 bytes mem[0..16) to mem[80..96) */
    CASE("string-ops", "rep_movsq", 0xF3, 0x48, 0xA5);
    IN.gpr[RCX] = 2; IN.gpr[RSI] = M(0); IN.gpr[RDI] = M(80); seq(IN.mem); SAME();
    memcpy(EX.mem + 80, IN.mem, 16); EX.gpr[RCX] = 0; EX.gpr[RSI] = M(16); EX.gpr[RDI] = M(96);
    /* repne scasb for al=33h in 11h 22h 33h ...: 3 compares, match: ZF=1, rdi=+3, rcx=10-3 */
    CASE("string-ops", "repne_scasb", 0xF2, 0xAE);
    IN.gpr[RAX] = 0x33; IN.gpr[RCX] = 10; IN.gpr[RDI] = M(0);
    IN.mem[0] = 0x11; IN.mem[1] = 0x22; IN.mem[2] = 0x33; IN.mem[3] = 0x44; SAME();
    EX.gpr[RCX] = 7; EX.gpr[RDI] = M(3); cur->flag_mask = F_ZF | F_CF; EX.rflags = F_ZF;
    /* lodsb: al=mem[2]=33h, rsi+1; upper rax bytes kept */
    CASE("string-ops", "lodsb", 0xAC);
    IN.gpr[RAX] = 0x1122334455667788ull; IN.gpr[RSI] = M(2); IN.mem[2] = 0x33; SAME();
    EX.gpr[RAX] = 0x1122334455667733ull; EX.gpr[RSI] = M(3);
    /* std; rep movsb; cld with rcx=3 from mem[3] down: mem[3],[2],[1] (seq 4,3,2) go to
     * mem[67],[66],[65]; rsi=M+0, rdi=M+64; DF cleared again */
    CASE("string-ops", "std_rep_movsb_backward", 0xFD, 0xF3, 0xA4, 0xFC);
    IN.gpr[RCX] = 3; IN.gpr[RSI] = M(3); IN.gpr[RDI] = M(67); seq(IN.mem); SAME();
    EX.mem[67] = 4; EX.mem[66] = 3; EX.mem[65] = 2; EX.gpr[RCX] = 0;
    EX.gpr[RSI] = M(0); EX.gpr[RDI] = M(64); cur->flag_mask = F_DF; EX.rflags = 0;

    /* ---- atomics-ordering (SDM: LOCK, CMPXCHG, CMPXCHG16B, XADD, XCHG, BTS, fences) ---- */
    /* lock cmpxchg [rbx],ecx: eax(5)==mem(5) -> mem=ecx=9, ZF=1, eax kept */
    CASE("atomics-ordering", "lock_cmpxchg_success", 0xF0, 0x0F, 0xB1, 0x0B);
    IN.gpr[RBX] = M(0); IN.gpr[RAX] = 5; IN.gpr[RCX] = 9; p32(IN.mem, 0, 5); SAME();
    p32(EX.mem, 0, 9); cur->flag_mask = F_ZF; EX.rflags = F_ZF;
    /* mem(6)!=eax(5): eax=6, mem unchanged, ZF=0 */
    CASE("atomics-ordering", "lock_cmpxchg_fail", 0xF0, 0x0F, 0xB1, 0x0B);
    IN.gpr[RBX] = M(0); IN.gpr[RAX] = 5; IN.gpr[RCX] = 9; p32(IN.mem, 0, 6); SAME();
    EX.gpr[RAX] = 6; cur->flag_mask = F_ZF; EX.rflags = 0;
    /* lock xadd [rbx],ecx: mem 10, ecx 3 -> ecx=10, mem=13 */
    CASE("atomics-ordering", "lock_xadd", 0xF0, 0x0F, 0xC1, 0x0B);
    IN.gpr[RBX] = M(0); IN.gpr[RCX] = 3; p32(IN.mem, 0, 10); SAME();
    EX.gpr[RCX] = 10; p32(EX.mem, 0, 13);
    /* xchg [rbx],rcx (implicitly locked): swap the two 64-bit values */
    CASE("atomics-ordering", "xchg_mem64", 0x48, 0x87, 0x0B);
    IN.gpr[RBX] = M(0); IN.gpr[RCX] = 0x3333333344444444ull; p64(IN.mem, 0, 0x1111111122222222ull); SAME();
    EX.gpr[RCX] = 0x1111111122222222ull; p64(EX.mem, 0, 0x3333333344444444ull);
    /* lock add [rbx],eax: 0xFFFFFFFF + 1 = 0; CF PF AF ZF set, SF OF clear */
    CASE("atomics-ordering", "lock_add_flags", 0xF0, 0x01, 0x03);
    IN.gpr[RBX] = M(0); IN.gpr[RAX] = 1; p32(IN.mem, 0, 0xFFFFFFFFu); SAME();
    p32(EX.mem, 0, 0); cur->flag_mask = FLAGS_ARITH; EX.rflags = F_CF | F_PF | F_AF | F_ZF;
    /* lock bts [rbx],eax: bit 5 of 0 set -> 0x20, CF = old bit = 0 */
    CASE("atomics-ordering", "lock_bts", 0xF0, 0x0F, 0xAB, 0x03);
    IN.gpr[RBX] = M(0); IN.gpr[RAX] = 5; SAME(); p32(EX.mem, 0, 0x20); cur->flag_mask = F_CF;
    /* lock cmpxchg16b [rsi]: rdx:rax = 2:1 equals mem -> mem = rcx:rbx = 8:7, ZF=1 */
    CASE("atomics-ordering", "lock_cmpxchg16b", 0xF0, 0x48, 0x0F, 0xC7, 0x0E);
    IN.gpr[RSI] = M(0); IN.gpr[RAX] = 1; IN.gpr[RDX] = 2; IN.gpr[RBX] = 7; IN.gpr[RCX] = 8;
    p64(IN.mem, 0, 1); p64(IN.mem, 8, 2); SAME();
    p64(EX.mem, 0, 7); p64(EX.mem, 8, 8); cur->flag_mask = F_ZF; EX.rflags = F_ZF;
    /* lfence; sfence; mfence: no architectural state change */
    CASE("atomics-ordering", "fences", 0x0F, 0xAE, 0xE8, 0x0F, 0xAE, 0xF8, 0x0F, 0xAE, 0xF0);
    IN.gpr[RAX] = 0x77; SAME();

    /* ---- unaligned-memory (x86 allows any alignment; mem[i] = i+1, little endian) ---- */
    /* mov ax,[rbx+1]: mem[1],mem[2] = 02,03 -> ax=0x0302, upper 48 bits kept */
    CASE("unaligned-memory", "load16_odd", 0x66, 0x8B, 0x43, 0x01);
    IN.gpr[RBX] = M(0); IN.gpr[RAX] = 0xAAAAAAAAAAAAAAAAull; seq(IN.mem); SAME();
    EX.gpr[RAX] = 0xAAAAAAAAAAAA0302ull;
    /* mov eax,[rbx+3]: bytes 04 05 06 07 -> 0x07060504, zero-extended */
    CASE("unaligned-memory", "load32_off3", 0x8B, 0x43, 0x03);
    IN.gpr[RBX] = M(0); IN.gpr[RAX] = ~0ull; seq(IN.mem); SAME(); EX.gpr[RAX] = 0x07060504u;
    /* mov rax,[rbx+5]: bytes 06..0D -> 0x0D0C0B0A09080706 */
    CASE("unaligned-memory", "load64_off5", 0x48, 0x8B, 0x43, 0x05);
    IN.gpr[RBX] = M(0); seq(IN.mem); SAME(); EX.gpr[RAX] = 0x0D0C0B0A09080706ull;
    /* movups xmm0,[rbx+7]: bytes mem[7..23) = 08..17h */
    CASE("unaligned-memory", "load128_off7", 0x0F, 0x10, 0x43, 0x07);
    IN.gpr[RBX] = M(0); seq(IN.mem); SAME();
    for (int j = 0; j < 16; j++) EX.xmm[0][j] = (uint8_t)(8 + j);
    /* mov [rbx+0x21],rcx: eight bytes 88 77 66 55 44 33 22 11 at mem[0x21..0x29) */
    CASE("unaligned-memory", "store64_off33", 0x48, 0x89, 0x4B, 0x21);
    IN.gpr[RBX] = M(0); IN.gpr[RCX] = 0x1122334455667788ull; seq(IN.mem); SAME();
    p64(EX.mem, 0x21, 0x1122334455667788ull);
    /* mov [rbx+0x29],ecx: DD CC BB AA at mem[0x29..0x2D) */
    CASE("unaligned-memory", "store32_off41", 0x89, 0x4B, 0x29);
    IN.gpr[RBX] = M(0); IN.gpr[RCX] = 0xAABBCCDDu; seq(IN.mem); SAME(); p32(EX.mem, 0x29, 0xAABBCCDDu);
    /* mov [rbx+0x31],cx: EF BE at mem[0x31..0x33) */
    CASE("unaligned-memory", "store16_off49", 0x66, 0x89, 0x4B, 0x31);
    IN.gpr[RBX] = M(0); IN.gpr[RCX] = 0xBEEF; seq(IN.mem); SAME(); EX.mem[0x31] = 0xEF; EX.mem[0x32] = 0xBE;
    /* movups [rbx+0x43],xmm1: bytes A0..AFh at mem[0x43..0x53) */
    CASE("unaligned-memory", "store128_off67", 0x0F, 0x11, 0x4B, 0x43);
    IN.gpr[RBX] = M(0); seq(IN.mem);
    for (int j = 0; j < 16; j++) IN.xmm[1][j] = (uint8_t)(0xA0 + j);
    SAME(); for (int j = 0; j < 16; j++) EX.mem[0x43 + j] = (uint8_t)(0xA0 + j);

    /* ---- x87 (default FCW 0x037F: extended precision, round to nearest even) ---- */
    /* fld qword [rbx]; fadd qword [rbx+8]; fstp qword [rbx+16]: 1.5 + 2.25 = 3.75 */
    CASE("x87", "fld_fadd_fstp", 0xDD, 0x03, 0xDC, 0x43, 0x08, 0xDD, 0x5B, 0x10);
    IN.gpr[RBX] = M(0); pd(IN.mem, 0, 1.5); pd(IN.mem, 8, 2.25); SAME(); pd(EX.mem, 16, 3.75);
    /* fmul qword: 1.5 * 2.25 = 3.375 */
    CASE("x87", "fld_fmul_fstp", 0xDD, 0x03, 0xDC, 0x4B, 0x08, 0xDD, 0x5B, 0x10);
    IN.gpr[RBX] = M(0); pd(IN.mem, 0, 1.5); pd(IN.mem, 8, 2.25); SAME(); pd(EX.mem, 16, 3.375);
    /* fsqrt: sqrt(2.25) = 1.5 exactly */
    CASE("x87", "fsqrt", 0xDD, 0x03, 0xD9, 0xFA, 0xDD, 0x5B, 0x10);
    IN.gpr[RBX] = M(0); pd(IN.mem, 0, 2.25); SAME(); pd(EX.mem, 16, 1.5);
    /* fld qword 2.5; fistp dword [rbx+16]: round to nearest even -> 2 */
    CASE("x87", "fistp_nearest_even", 0xDD, 0x03, 0xDB, 0x5B, 0x10);
    IN.gpr[RBX] = M(0); pd(IN.mem, 0, 2.5); SAME(); p32(EX.mem, 16, 2);
    /* fild dword [rbx] = -3; fstp qword [rbx+16] -> -3.0 */
    CASE("x87", "fild_fstp", 0xDB, 0x03, 0xDD, 0x5B, 0x10);
    IN.gpr[RBX] = M(0); p32(IN.mem, 0, (uint32_t)-3); SAME(); pd(EX.mem, 16, -3.0);
    /* fld [rbx+8]=2.25; fld [rbx]=1.5; fcomi st0,st1: 1.5 < 2.25 -> CF=1, ZF=0, PF=0; pop both */
    CASE("x87", "fcomi_less", 0xDD, 0x43, 0x08, 0xDD, 0x03, 0xDB, 0xF1, 0xDD, 0xD8, 0xDD, 0xD8);
    IN.gpr[RBX] = M(0); pd(IN.mem, 0, 1.5); pd(IN.mem, 8, 2.25); SAME();
    cur->flag_mask = F_CF | F_ZF | F_PF; EX.rflags = F_CF;

    /* ---- sse-float (IEEE single/double; scalar ops leave the upper lanes unchanged) ---- */
    /* addss: 1.5f(3FC00000) + 2.25f(40100000) = 3.75f(40700000) in lane 0 */
    CASE("sse-float", "addss", 0xF3, 0x0F, 0x58, 0xC1);
    u32x4(IN.xmm[0], 0x3FC00000, 0x11111111, 0x22222222, 0x33333333);
    u32x4(IN.xmm[1], 0x40100000, 0x44444444, 0x55555555, 0x66666666); SAME();
    u32x4(EX.xmm[0], 0x40700000, 0x11111111, 0x22222222, 0x33333333);
    /* mulsd: 1.5 * 2.25 = 3.375 = 400B000000000000; high qword kept */
    CASE("sse-float", "mulsd", 0xF2, 0x0F, 0x59, 0xC1);
    x64(IN.xmm[0], 0x3FF8000000000000ull, 0xAAAAAAAAAAAAAAAAull);
    x64(IN.xmm[1], 0x4002000000000000ull, 0xBBBBBBBBBBBBBBBBull); SAME();
    x64(EX.xmm[0], 0x400B000000000000ull, 0xAAAAAAAAAAAAAAAAull);
    /* cvtsi2sd xmm0,rax: -7 -> -7.0 = C01C000000000000; high qword kept */
    CASE("sse-float", "cvtsi2sd", 0xF2, 0x48, 0x0F, 0x2A, 0xC0);
    IN.gpr[RAX] = (uint64_t)-7; x64(IN.xmm[0], 0, 0xCCCCCCCCCCCCCCCCull); SAME();
    x64(EX.xmm[0], 0xC01C000000000000ull, 0xCCCCCCCCCCCCCCCCull);
    /* cvttsd2si rax,xmm0: -2.75 (C006000000000000) truncates toward zero -> -2 */
    CASE("sse-float", "cvttsd2si", 0xF2, 0x48, 0x0F, 0x2C, 0xC0);
    x64(IN.xmm[0], 0xC006000000000000ull, 0); SAME(); EX.gpr[RAX] = (uint64_t)-2;
    /* sqrtsd xmm0,xmm0: sqrt(2.25)=1.5 */
    CASE("sse-float", "sqrtsd", 0xF2, 0x0F, 0x51, 0xC0);
    x64(IN.xmm[0], 0x4002000000000000ull, 0x1234); SAME(); x64(EX.xmm[0], 0x3FF8000000000000ull, 0x1234);
    /* ucomisd xmm0,xmm1: 1.5 < 2.25 -> CF=1 ZF=0 PF=0 */
    CASE("sse-float", "ucomisd_less", 0x66, 0x0F, 0x2E, 0xC1);
    x64(IN.xmm[0], 0x3FF8000000000000ull, 0); x64(IN.xmm[1], 0x4002000000000000ull, 0); SAME();
    cur->flag_mask = F_CF | F_ZF | F_PF; EX.rflags = F_CF;
    /* cvtss2sd xmm0,xmm1: 2.25f -> 2.25 (4002000000000000); high qword of xmm0 kept */
    CASE("sse-float", "cvtss2sd", 0xF3, 0x0F, 0x5A, 0xC1);
    x64(IN.xmm[0], 0, 0xDDDDDDDDDDDDDDDDull); u32x4(IN.xmm[1], 0x40100000, 0, 0, 0); SAME();
    x64(EX.xmm[0], 0x4002000000000000ull, 0xDDDDDDDDDDDDDDDDull);
    /* addps: {1,2,3,4} + 0.5 = {1.5,2.5,3.5,4.5} = 3FC00000 40200000 40600000 40900000 */
    CASE("sse-float", "addps", 0x0F, 0x58, 0xC1);
    u32x4(IN.xmm[0], 0x3F800000, 0x40000000, 0x40400000, 0x40800000);
    u32x4(IN.xmm[1], 0x3F000000, 0x3F000000, 0x3F000000, 0x3F000000); SAME();
    u32x4(EX.xmm[0], 0x3FC00000, 0x40200000, 0x40600000, 0x40900000);

    /* ---- avx-avx2 (VEX; each case ends in vzeroall so all xmm are zero, results go to mem) ---- */
    /* vmovdqu ymm0,[rbx]; vmovdqu ymm1,[rbx+32]; vpaddd ymm2,ymm0,ymm1; vmovdqu [rbx+64],ymm2;
     * vzeroall. Lanes a={1..8}, b={10,20,..,80} except b[7]=1 with a[7]=0xFFFFFFFF wraps to 0 */
    CASE("avx-avx2", "vpaddd_ymm", 0xC5, 0xFE, 0x6F, 0x03, 0xC5, 0xFE, 0x6F, 0x4B, 0x20,
         0xC5, 0xFD, 0xFE, 0xD1, 0xC5, 0xFE, 0x7F, 0x53, 0x40, 0xC5, 0xFC, 0x77);
    IN.gpr[RBX] = M(0);
    for (int i = 0; i < 8; i++) { p32(IN.mem, 4 * i, (uint32_t)(i + 1)); p32(IN.mem, 32 + 4 * i, (uint32_t)(10 * (i + 1))); }
    p32(IN.mem, 28, 0xFFFFFFFFu); p32(IN.mem, 60, 1); SAME();
    for (int i = 0; i < 7; i++) p32(EX.mem, 64 + 4 * i, (uint32_t)(11 * (i + 1)));
    p32(EX.mem, 92, 0);
    /* vfmadd231ps ymm2,ymm0,ymm1 (ymm2 = ymm0*ymm1 + ymm2): a={1..8}, b=2.0, c=0.5 -> 2a+0.5
     * = 2.5 4.5 6.5 8.5 10.5 12.5 14.5 16.5 = 40200000 40900000 40D00000 41080000 41280000
     * 41480000 41680000 41840000 */
    CASE("avx-avx2", "vfmadd231ps_ymm", 0xC5, 0xFE, 0x6F, 0x03, 0xC5, 0xFE, 0x6F, 0x4B, 0x20,
         0xC5, 0xFE, 0x6F, 0x53, 0x40, 0xC4, 0xE2, 0x7D, 0xB8, 0xD1, 0xC5, 0xFE, 0x7F, 0x53, 0x60,
         0xC5, 0xFC, 0x77);
    IN.gpr[RBX] = M(0);
    { static const uint32_t a[8] = {0x3F800000, 0x40000000, 0x40400000, 0x40800000,
                                    0x40A00000, 0x40C00000, 0x40E00000, 0x41000000};
      static const uint32_t r[8] = {0x40200000, 0x40900000, 0x40D00000, 0x41080000,
                                    0x41280000, 0x41480000, 0x41680000, 0x41840000};
      for (int i = 0; i < 8; i++) { p32(IN.mem, 4 * i, a[i]); p32(IN.mem, 32 + 4 * i, 0x40000000);
                                    p32(IN.mem, 64 + 4 * i, 0x3F000000); }
      SAME(); for (int i = 0; i < 8; i++) p32(EX.mem, 96 + 4 * i, r[i]); }
    /* vpcmpeqd ymm2,ymm0,ymm1; vpmovmskb eax,ymm2: lanes 0 and 2 equal -> bytes 0-3 and 8-11
     * -> mask 0x0F0F */
    CASE("avx-avx2", "vpcmpeqd_vpmovmskb", 0xC5, 0xFE, 0x6F, 0x03, 0xC5, 0xFE, 0x6F, 0x4B, 0x20,
         0xC5, 0xFD, 0x76, 0xD1, 0xC5, 0xFD, 0xD7, 0xC2, 0xC5, 0xFC, 0x77);
    IN.gpr[RBX] = M(0); IN.gpr[RAX] = ~0ull;
    for (int i = 0; i < 8; i++) { p32(IN.mem, 4 * i, (uint32_t)i); p32(IN.mem, 32 + 4 * i, (uint32_t)(i == 0 || i == 2 ? i : 100 + i)); }
    SAME(); EX.gpr[RAX] = 0x0F0F;

    /* ---- bmi-lzcnt-movbe ---- */
    /* lzcnt rax,rcx: rcx=0x100 (bit 8) -> 63-8=55, CF=0 ZF=0 */
    CASE("bmi-lzcnt-movbe", "lzcnt64", 0xF3, 0x48, 0x0F, 0xBD, 0xC1);
    IN.gpr[RCX] = 0x100; SAME(); EX.gpr[RAX] = 55; cur->flag_mask = F_CF | F_ZF; EX.rflags = 0;
    /* lzcnt of 0 = operand size 64, CF=1 */
    CASE("bmi-lzcnt-movbe", "lzcnt64_zero", 0xF3, 0x48, 0x0F, 0xBD, 0xC1);
    IN.gpr[RAX] = 9; SAME(); EX.gpr[RAX] = 64; cur->flag_mask = F_CF | F_ZF; EX.rflags = F_CF;
    /* tzcnt rax,rcx: 0x100 -> 8, CF=0 ZF=0 */
    CASE("bmi-lzcnt-movbe", "tzcnt64", 0xF3, 0x48, 0x0F, 0xBC, 0xC1);
    IN.gpr[RCX] = 0x100; SAME(); EX.gpr[RAX] = 8; cur->flag_mask = F_CF | F_ZF; EX.rflags = 0;
    CASE("bmi-lzcnt-movbe", "tzcnt64_zero", 0xF3, 0x48, 0x0F, 0xBC, 0xC1);
    IN.gpr[RAX] = 9; SAME(); EX.gpr[RAX] = 64; cur->flag_mask = F_CF | F_ZF; EX.rflags = F_CF;
    /* popcnt rax,rcx: 0xFF00FF has 16 set bits, ZF=0 */
    CASE("bmi-lzcnt-movbe", "popcnt64", 0xF3, 0x48, 0x0F, 0xB8, 0xC1);
    IN.gpr[RCX] = 0xFF00FF; SAME(); EX.gpr[RAX] = 16; cur->flag_mask = F_ZF;
    /* andn rax,rbx,rcx = ~rbx & rcx = ~0xF0F0 & 0xFFFF = 0x0F0F */
    CASE("bmi-lzcnt-movbe", "andn64", 0xC4, 0xE2, 0xE0, 0xF2, 0xC1);
    IN.gpr[RBX] = 0xF0F0; IN.gpr[RCX] = 0xFFFF; SAME(); EX.gpr[RAX] = 0x0F0F;
    /* blsr rax,rcx: clears the lowest set bit: 0x28 -> 0x20 */
    CASE("bmi-lzcnt-movbe", "blsr64", 0xC4, 0xE2, 0xF8, 0xF3, 0xC9);
    IN.gpr[RCX] = 0x28; SAME(); EX.gpr[RAX] = 0x20;
    /* movbe eax,[rbx]: bytes 11 22 33 44 loaded byte-swapped -> 0x11223344 */
    CASE("bmi-lzcnt-movbe", "movbe_load32", 0x0F, 0x38, 0xF0, 0x03);
    IN.gpr[RBX] = M(0); IN.mem[0] = 0x11; IN.mem[1] = 0x22; IN.mem[2] = 0x33; IN.mem[3] = 0x44; SAME();
    EX.gpr[RAX] = 0x11223344u;
    /* movbe [rbx+8],ecx: 0xAABBCCDD stored big-endian: AA BB CC DD */
    CASE("bmi-lzcnt-movbe", "movbe_store32", 0x0F, 0x38, 0xF1, 0x4B, 0x08);
    IN.gpr[RBX] = M(0); IN.gpr[RCX] = 0xAABBCCDDu; SAME();
    EX.mem[8] = 0xAA; EX.mem[9] = 0xBB; EX.mem[10] = 0xCC; EX.mem[11] = 0xDD;

    /* ---- host-call-callback: guest `mov rax,imm64; call rax` to a host function and back.
     * After the call only rax/xmm0 and callee-saved rbx rbp r12-r15 are checked. ---- */
    /* host fn 1 = a*b+c: 6*7+3 = 45 in rax; rbx (pointer) and r12/r15 preserved */
    CASE("host-call-callback", "host_call_mix", 0x48, 0xB8, NOJIT_HOST(1), 0xFF, 0xD0);
    IN.gpr[RDI] = 6; IN.gpr[RSI] = 7; IN.gpr[RDX] = 3; IN.gpr[RBX] = M(0);
    IN.gpr[12] = 0x1234; IN.gpr[15] = 0xDEAD; SAME(); EX.gpr[RAX] = 45;
    cur->gpr_skip = 0xFC6; cur->xmm_skip = 0xFFFF;
    /* host fn 2 = x*0.5 on xmm0: 5.0 (4014000000000000) -> 2.5 (4004000000000000) */
    CASE("host-call-callback", "host_call_double", 0x48, 0xB8, NOJIT_HOST(2), 0xFF, 0xD0);
    x64(IN.xmm[0], 0x4014000000000000ull, 0); IN.gpr[RBP] = 0xB0B; SAME();
    x64(EX.xmm[0], 0x4004000000000000ull, 0);
    cur->gpr_skip = 0xFC7; cur->xmm_skip = 0xFFFE;
    /* host fn 3 calls the guest back (guest fn: rax = rdi+rdi) then adds 1: 2*21+1 = 43 */
    CASE("host-call-callback", "host_call_guest_callback", 0x48, 0xB8, NOJIT_HOST(3), 0xFF, 0xD0);
    IN.gpr[RDI] = 21; IN.gpr[13] = 0xC0DE; SAME(); EX.gpr[RAX] = 43;
    cur->gpr_skip = 0xFC6; cur->xmm_skip = 0xFFFF;

    /* ---- tls (guest fs base = mem, set through bbcpu_set_thread_pointer) ---- */
    /* mov rax,fs:[8] */
    CASE("tls", "fs_load64", 0x64, 0x48, 0x8B, 0x04, 0x25, 0x08, 0, 0, 0);
    p64(IN.mem, 8, 0x1122334455667788ull); SAME(); EX.gpr[RAX] = 0x1122334455667788ull;
    /* mov eax,fs:[4]: 4 bytes of seq at 4..7 = 05 06 07 08 -> 0x08070605 */
    CASE("tls", "fs_load32", 0x64, 0x8B, 0x04, 0x25, 0x04, 0, 0, 0);
    seq(IN.mem); IN.gpr[RAX] = ~0ull; SAME(); EX.gpr[RAX] = 0x08070605u;
    /* mov fs:[0x10],rcx stores into mem[0x10..0x18) */
    CASE("tls", "fs_store64", 0x64, 0x48, 0x89, 0x0C, 0x25, 0x10, 0, 0, 0);
    IN.gpr[RCX] = 0xCAFE; SAME(); p64(EX.mem, 0x10, 0xCAFE);
    /* mov rax,fs:[rbx] with rbx=0x18: indexed fs access reads mem[0x18..0x20) */
    CASE("tls", "fs_load_indexed", 0x64, 0x48, 0x8B, 0x03);
    IN.gpr[RBX] = 0x18; p64(IN.mem, 0x18, 0x0807060504030201ull); SAME(); EX.gpr[RAX] = 0x0807060504030201ull;

    return (size_t)(nxt - nxt0);
}
