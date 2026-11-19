/* Windows implementations of compat_win.h. */
#ifdef _WIN32
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <io.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "compat_win.h"
/* compat_errno_from_win32 is Rust now (crates/runtime/src/os.rs). */
#include "../crates/runtime/include/uncharted_runtime.h"

/* bb_jmp_buf: rbx rbp rdi rsi r12-r15 (0-56), rsp after return (64), return address (72),
 * xmm6-15 (80-239), mxcsr (240), x87 control word (244). */
__asm__(".text\n"
        ".globl bb_setjmp\n"
        ".def bb_setjmp; .scl 2; .type 32; .endef\n"
        "bb_setjmp:\n"
        "    mov %rbx,0(%rcx)\n    mov %rbp,8(%rcx)\n    mov %rdi,16(%rcx)\n    mov %rsi,24(%rcx)\n"
        "    mov %r12,32(%rcx)\n    mov %r13,40(%rcx)\n    mov %r14,48(%rcx)\n    mov %r15,56(%rcx)\n"
        "    lea 8(%rsp),%rdx\n    mov %rdx,64(%rcx)\n    mov (%rsp),%rdx\n    mov %rdx,72(%rcx)\n"
        "    movups %xmm6,80(%rcx)\n    movups %xmm7,96(%rcx)\n    movups %xmm8,112(%rcx)\n"
        "    movups %xmm9,128(%rcx)\n    movups %xmm10,144(%rcx)\n    movups %xmm11,160(%rcx)\n"
        "    movups %xmm12,176(%rcx)\n    movups %xmm13,192(%rcx)\n    movups %xmm14,208(%rcx)\n"
        "    movups %xmm15,224(%rcx)\n    stmxcsr 240(%rcx)\n    fnstcw 244(%rcx)\n"
        "    xor %eax,%eax\n    ret\n"
        ".globl bb_longjmp\n"
        ".def bb_longjmp; .scl 2; .type 32; .endef\n"
        "bb_longjmp:\n"
        "    mov %edx,%eax\n    test %eax,%eax\n    jnz 1f\n    mov $1,%eax\n"
        "1:  mov 0(%rcx),%rbx\n    mov 8(%rcx),%rbp\n    mov 16(%rcx),%rdi\n    mov 24(%rcx),%rsi\n"
        "    mov 32(%rcx),%r12\n    mov 40(%rcx),%r13\n    mov 48(%rcx),%r14\n    mov 56(%rcx),%r15\n"
        "    movups 80(%rcx),%xmm6\n    movups 96(%rcx),%xmm7\n    movups 112(%rcx),%xmm8\n"
        "    movups 128(%rcx),%xmm9\n    movups 144(%rcx),%xmm10\n    movups 160(%rcx),%xmm11\n"
        "    movups 176(%rcx),%xmm12\n    movups 192(%rcx),%xmm13\n    movups 208(%rcx),%xmm14\n"
        "    movups 224(%rcx),%xmm15\n    ldmxcsr 240(%rcx)\n    fldcw 244(%rcx)\n"
        "    mov 64(%rcx),%rsp\n    jmp *72(%rcx)\n");

/* Positioned I/O on a synchronous handle moves the file pointer: it is restored afterwards.
 * A concurrent read() on the same descriptor would race, as with a shared offset anyway. */
static ssize_t positioned(int fd, void *buffer, size_t size, int64_t offset, int write) {
    HANDLE h = (HANDLE)_get_osfhandle(fd);
    if (h == INVALID_HANDLE_VALUE) { errno = EBADF; return -1; }
    if (offset < 0) { errno = EINVAL; return -1; }
    LARGE_INTEGER zero = {0}, saved;
    if (!SetFilePointerEx(h, zero, &saved, FILE_CURRENT)) { errno = compat_errno_from_win32(GetLastError()); return -1; }
    size_t done = 0;
    while (done < size) {
        DWORD chunk = size - done > 0x40000000u ? 0x40000000u : (DWORD)(size - done), n = 0;
        OVERLAPPED at = {0};
        uint64_t position = (uint64_t)offset + done;
        at.Offset = (DWORD)position; at.OffsetHigh = (DWORD)(position >> 32);
        BOOL ok = write ? WriteFile(h, (const char *)buffer + done, chunk, &n, &at)
                        : ReadFile(h, (char *)buffer + done, chunk, &n, &at);
        if (!ok) {
            DWORD e = GetLastError();
            if (e == ERROR_HANDLE_EOF) break;
            SetFilePointerEx(h, saved, NULL, FILE_BEGIN);
            if (done) return (ssize_t)done;
            errno = compat_errno_from_win32(e);
            return -1;
        }
        done += n;
        if (n < chunk) break;
    }
    SetFilePointerEx(h, saved, NULL, FILE_BEGIN);
    return (ssize_t)done;
}
/* Positioned reads on a second handle of the file (ReOpenFile), one per descriptor, made at the first pread: its file
 * pointer is nobody's, so reads never move the descriptor's own and need no save and restore. With the restore, two
 * preads on one descriptor at once could leave the pointer at the other's position, and a read() at the same time took
 * its data from the wrong place (the idea from iannitram's Uncharted Universal, 2026-11). Each slot keeps the handle it
 * was made from: a descriptor number used again for another file gets a new second handle. compat_close drops it.
 * BB_PREAD_SIDE=0, or a file ReOpenFile refuses: the restore as before. */
#define SIDE_SLOTS 4096
static struct { HANDLE main, side; SRWLOCK lock; } side_slots[SIDE_SLOTS];
static int side_allowed(void) {
    static int allowed = -1;
    if (allowed < 0) { const char *e = getenv("BB_PREAD_SIDE"); allowed = !(e && e[0] == '0'); }
    return allowed;
}
static HANDLE side_handle(int fd, HANDLE main) {
    if (fd < 0 || fd >= SIDE_SLOTS || !side_allowed()) return NULL;
    HANDLE found = NULL;
    AcquireSRWLockShared(&side_slots[fd].lock);
    if (side_slots[fd].main == main) found = side_slots[fd].side;
    ReleaseSRWLockShared(&side_slots[fd].lock);
    if (found) return found == INVALID_HANDLE_VALUE ? NULL : found;
    AcquireSRWLockExclusive(&side_slots[fd].lock);
    if (side_slots[fd].main != main) {
        if (side_slots[fd].side && side_slots[fd].side != INVALID_HANDLE_VALUE) CloseHandle(side_slots[fd].side);
        HANDLE side = ReOpenFile(main, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, 0);
        side_slots[fd].main = main;
        side_slots[fd].side = side; /* INVALID_HANDLE_VALUE: refused, remembered for this file */
    }
    found = side_slots[fd].side;
    ReleaseSRWLockExclusive(&side_slots[fd].lock);
    return found == INVALID_HANDLE_VALUE ? NULL : found;
}
int compat_close(int fd) {
    if (fd >= 0 && fd < SIDE_SLOTS) {
        AcquireSRWLockExclusive(&side_slots[fd].lock);
        if (side_slots[fd].side && side_slots[fd].side != INVALID_HANDLE_VALUE) CloseHandle(side_slots[fd].side);
        side_slots[fd].main = side_slots[fd].side = NULL;
        ReleaseSRWLockExclusive(&side_slots[fd].lock);
    }
    return _close(fd);
}
static ssize_t side_read(HANDLE side, void *buffer, size_t size, int64_t offset) {
    size_t done = 0;
    while (done < size) {
        DWORD chunk = size - done > 0x40000000u ? 0x40000000u : (DWORD)(size - done), n = 0;
        OVERLAPPED at = {0};
        uint64_t position = (uint64_t)offset + done;
        at.Offset = (DWORD)position; at.OffsetHigh = (DWORD)(position >> 32);
        if (!ReadFile(side, (char *)buffer + done, chunk, &n, &at)) {
            DWORD e = GetLastError();
            if (e == ERROR_HANDLE_EOF) break;
            if (done) return (ssize_t)done;
            errno = compat_errno_from_win32(e);
            return -1;
        }
        done += n;
        if (n < chunk) break;
    }
    return (ssize_t)done;
}
ssize_t compat_pread(int fd, void *buffer, size_t size, int64_t offset) {
    HANDLE h = (HANDLE)_get_osfhandle(fd);
    if (h == INVALID_HANDLE_VALUE) { errno = EBADF; return -1; }
    if (offset < 0) { errno = EINVAL; return -1; }
    HANDLE side = side_handle(fd, h);
    return side ? side_read(side, buffer, size, offset) : positioned(fd, buffer, size, offset, 0);
}
ssize_t compat_pwrite(int fd, const void *buffer, size_t size, int64_t offset) { return positioned(fd, (void *)buffer, size, offset, 1); }

#endif
