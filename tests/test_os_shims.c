/* The Windows shims from C, as the runtime calls them: the compat_* functions and
 * bb_setjmp/bb_longjmp are Rust since piece 3.15 (crates/runtime/src/os), linked from the
 * crate's static library. The Rust tests cover each shim's behaviour; this checks the C side:
 * the declarations in uncharted_runtime.h, the pread/pwrite macros of compat_win.h, and the
 * jump pair, which Rust never calls. Windows x86-64 (MSYS2 CLANG64), run by build.sh --test. */
#include "runtime.h"
#include <immintrin.h>
#include <windows.h>

#define PATH_SIZE 512

static int failures;
#define CHECK(condition) do { if (!(condition)) { \
    fprintf(stderr, "%s:%d: CHECK(%s) failed\n", __FILE__, __LINE__, #condition); ++failures; } } while (0)

/* Values the compiler is free to keep in callee-saved registers across bb_setjmp. */
static __attribute__((noinline)) uint64_t mix(uint64_t x) { return x * 0x9e3779b97f4a7c15ull + 7; }
static __attribute__((noinline)) double scaled(int x) { return x * 1.25 + 0.5; }

/* Leaves every callee-saved register but rbp (the frame pointer) and the SSE control word
 * changed, then jumps: bb_longjmp must put back what bb_setjmp saved. */
static __attribute__((noinline, noreturn)) void trash_and_jump(bb_jmp_buf buffer, int value) {
    __asm__ volatile(
        "mov $-1,%%rbx\n mov $-1,%%rsi\n mov $-1,%%rdi\n"
        "mov $-1,%%r12\n mov $-1,%%r13\n mov $-1,%%r14\n mov $-1,%%r15\n"
        "pcmpeqd %%xmm6,%%xmm6\n pcmpeqd %%xmm7,%%xmm7\n pcmpeqd %%xmm8,%%xmm8\n"
        "pcmpeqd %%xmm9,%%xmm9\n pcmpeqd %%xmm10,%%xmm10\n pcmpeqd %%xmm11,%%xmm11\n"
        "pcmpeqd %%xmm12,%%xmm12\n pcmpeqd %%xmm13,%%xmm13\n pcmpeqd %%xmm14,%%xmm14\n"
        "pcmpeqd %%xmm15,%%xmm15\n"
        ::: "rbx", "rsi", "rdi", "r12", "r13", "r14", "r15", "xmm6", "xmm7", "xmm8", "xmm9",
            "xmm10", "xmm11", "xmm12", "xmm13", "xmm14", "xmm15", "memory");
    _mm_setcsr((_mm_getcsr() & ~0x6000u) | 0x6000u); /* round toward zero */
    bb_longjmp(buffer, value);
}

static void check_jump(int value, int expected) {
    static bb_jmp_buf buffer;
    uint64_t a = mix(1), b = mix(2), c = mix(3), d = mix(4), e = mix(5), f = mix(6), g = mix(7);
    double x0 = scaled(0), x1 = scaled(1), x2 = scaled(2), x3 = scaled(3), x4 = scaled(4);
    double x5 = scaled(5), x6 = scaled(6), x7 = scaled(7), x8 = scaled(8), x9 = scaled(9);
    unsigned csr = _mm_getcsr();
    volatile int jumps = 0;
    int result = bb_setjmp(buffer);
    if (result == 0) {
        ++jumps;
        trash_and_jump(buffer, value);
    }
    CHECK(jumps == 1);
    CHECK(result == expected);
    CHECK(a == mix(1) && b == mix(2) && c == mix(3) && d == mix(4));
    CHECK(e == mix(5) && f == mix(6) && g == mix(7));
    CHECK(x0 == scaled(0) && x1 == scaled(1) && x2 == scaled(2) && x3 == scaled(3));
    CHECK(x4 == scaled(4) && x5 == scaled(5) && x6 == scaled(6) && x7 == scaled(7));
    CHECK(x8 == scaled(8) && x9 == scaled(9));
    CHECK(_mm_getcsr() == csr);
}

static void check_positioned_io(const char *dir) {
    char path[PATH_SIZE + 32];
    snprintf(path, sizeof(path), "%s\\positioned.bin", dir);
    int fd = _open(path, _O_RDWR | _O_CREAT | _O_TRUNC | _O_BINARY, _S_IREAD | _S_IWRITE);
    CHECK(fd >= 0);
    CHECK(_write(fd, "0123456789", 10) == 10);
    CHECK(_lseeki64(fd, 3, SEEK_SET) == 3);
    /* The macros of compat_win.h: pread/pwrite are compat_pread/compat_pwrite. */
    CHECK(pwrite(fd, "AB", 2, 6) == 2);
    char got[8] = {0};
    CHECK(pread(fd, got, 4, 5) == 4 && !memcmp(got, "5AB8", 4));
    CHECK(pread(fd, got, 4, 10) == 0);
    CHECK(_lseeki64(fd, 0, SEEK_CUR) == 3);
    errno = 0;
    CHECK(pread(fd, got, 1, -1) == -1 && errno == EINVAL);
    CHECK(!_close(fd));
}

int main(void) {
    check_jump(7, 7);
    check_jump(0, 1); /* a value of 0 comes back as 1, as with longjmp */
    check_jump(-3, -3);

    CHECK(compat_errno_from_win32(ERROR_SUCCESS) == 0);
    CHECK(compat_errno_from_win32(ERROR_FILE_NOT_FOUND) == ENOENT);
    CHECK(compat_errno_from_win32(ERROR_SHARING_VIOLATION) == EACCES);
    CHECK(compat_errno_from_win32(ERROR_DIR_NOT_EMPTY) == ENOTEMPTY);
    CHECK(compat_errno_from_win32(ERROR_NOT_SUPPORTED) == EIO);

    /* Seconds east of UTC: local broken-down time minus UTC broken-down time. */
    time_t now = time(NULL);
    struct tm local, utc;
    CHECK(!localtime_s(&local, &now) && !gmtime_s(&utc, &now));
    long expected = (long)(local.tm_hour - utc.tm_hour) * 3600 + (local.tm_min - utc.tm_min) * 60;
    int days = local.tm_yday - utc.tm_yday;
    if (days > 1) days = -1; /* across a year end */
    if (days < -1) days = 1;
    expected += days * 86400L;
    CHECK(compat_utc_offset(now) == expected);

    unsigned char first[32] = {0}, second[32] = {0}, zero[32] = {0};
    CHECK(compat_random(first, sizeof(first)) == 0);
    CHECK(compat_random(second, sizeof(second)) == 0);
    CHECK(memcmp(first, second, sizeof(first)) && memcmp(first, zero, sizeof(first)));
    CHECK(compat_random(NULL, 0) == 0);

    LARGE_INTEGER frequency, before, after;
    QueryPerformanceFrequency(&frequency);
    QueryPerformanceCounter(&before);
    compat_sleep_ns(2000000);
    QueryPerformanceCounter(&after);
    double slept_ms = (double)(after.QuadPart - before.QuadPart) * 1000.0 / (double)frequency.QuadPart;
    CHECK(slept_ms >= 1.9);
    compat_sleep_ns(0);

    int64_t user = -1, kernel = -1;
    CHECK(compat_cpu_times(0, &user, &kernel) == 0 && user >= 0 && kernel >= 0);
    user = kernel = -1;
    CHECK(compat_cpu_times(1, &user, &kernel) == 0 && user >= 0 && kernel >= 0);

    char root[PATH_SIZE];
    DWORD base = GetTempPathA(sizeof(root) - 64, root);
    CHECK(base > 0 && base < sizeof(root) - 64);
    snprintf(root + base, sizeof(root) - base, "uu-os-shims-%lu", GetCurrentProcessId());
    CHECK(!mkdir(root, 0755));
    check_positioned_io(root);
    char nested[PATH_SIZE + 32], file[PATH_SIZE + 64];
    snprintf(nested, sizeof(nested), "%s\\slot", root);
    CHECK(!mkdir(nested, 0755));
    snprintf(file, sizeof(file), "%s\\slot\\made-up.bin", root);
    FILE *out = fopen(file, "wb");
    CHECK(out && fputs("made up", out) >= 0 && !fclose(out));
    CHECK(SetFileAttributesA(file, FILE_ATTRIBUTE_READONLY));
    CHECK(compat_remove_tree(root) == 0);
    CHECK(GetFileAttributesA(root) == INVALID_FILE_ATTRIBUTES);
    CHECK(compat_remove_tree(root) == 0);
    CHECK(compat_remove_tree(NULL) == 0);

    if (failures) return 1;
    puts("os shims: ok");
    return 0;
}
