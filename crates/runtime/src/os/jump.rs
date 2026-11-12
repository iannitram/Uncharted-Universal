//! `bb_setjmp` and `bb_longjmp` (Windows x86-64), moved from `compat_win.c` unchanged: a
//! `setjmp`/`longjmp` pair without SEH unwinding (guest frames have no unwind data) that saves
//! every Win64 callee-saved register, `xmm6`-`xmm15` included. clang's `__builtin_longjmp`
//! restores a wrong frame pointer on Win64. `bb_longjmp` may also be entered from the
//! exception handler (`probe.c` points the faulting thread's `Rip` at it).
//!
//! C declares them in `src/compat_win.h` (`bb_jmp_buf`, `returns_twice`), which `cbindgen`
//! cannot express; Rust never calls them. `tests/test_os_shims.c` checks them from C.
//!
//! `bb_jmp_buf`: rbx rbp rdi rsi r12-r15 (bytes 0-56), rsp after return (64), return address
//! (72), xmm6-15 (80-239), mxcsr (240), x87 control word (244).

std::arch::global_asm!(
    ".text",
    ".globl bb_setjmp",
    ".def bb_setjmp; .scl 2; .type 32; .endef",
    "bb_setjmp:",
    "    mov %rbx,0(%rcx)",
    "    mov %rbp,8(%rcx)",
    "    mov %rdi,16(%rcx)",
    "    mov %rsi,24(%rcx)",
    "    mov %r12,32(%rcx)",
    "    mov %r13,40(%rcx)",
    "    mov %r14,48(%rcx)",
    "    mov %r15,56(%rcx)",
    "    lea 8(%rsp),%rdx",
    "    mov %rdx,64(%rcx)",
    "    mov (%rsp),%rdx",
    "    mov %rdx,72(%rcx)",
    "    movups %xmm6,80(%rcx)",
    "    movups %xmm7,96(%rcx)",
    "    movups %xmm8,112(%rcx)",
    "    movups %xmm9,128(%rcx)",
    "    movups %xmm10,144(%rcx)",
    "    movups %xmm11,160(%rcx)",
    "    movups %xmm12,176(%rcx)",
    "    movups %xmm13,192(%rcx)",
    "    movups %xmm14,208(%rcx)",
    "    movups %xmm15,224(%rcx)",
    "    stmxcsr 240(%rcx)",
    "    fnstcw 244(%rcx)",
    "    xor %eax,%eax",
    "    ret",
    ".globl bb_longjmp",
    ".def bb_longjmp; .scl 2; .type 32; .endef",
    "bb_longjmp:",
    // A value of 0 returns 1 from `bb_setjmp`, as `longjmp` does.
    "    mov %edx,%eax",
    "    test %eax,%eax",
    "    jnz .Lbb_longjmp_restore",
    "    mov $1,%eax",
    ".Lbb_longjmp_restore:",
    "    mov 0(%rcx),%rbx",
    "    mov 8(%rcx),%rbp",
    "    mov 16(%rcx),%rdi",
    "    mov 24(%rcx),%rsi",
    "    mov 32(%rcx),%r12",
    "    mov 40(%rcx),%r13",
    "    mov 48(%rcx),%r14",
    "    mov 56(%rcx),%r15",
    "    movups 80(%rcx),%xmm6",
    "    movups 96(%rcx),%xmm7",
    "    movups 112(%rcx),%xmm8",
    "    movups 128(%rcx),%xmm9",
    "    movups 144(%rcx),%xmm10",
    "    movups 160(%rcx),%xmm11",
    "    movups 176(%rcx),%xmm12",
    "    movups 192(%rcx),%xmm13",
    "    movups 208(%rcx),%xmm14",
    "    movups 224(%rcx),%xmm15",
    "    ldmxcsr 240(%rcx)",
    "    fldcw 244(%rcx)",
    "    mov 64(%rcx),%rsp",
    "    jmp *72(%rcx)",
    options(att_syntax)
);
