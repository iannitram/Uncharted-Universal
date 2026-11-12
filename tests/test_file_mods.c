/* The C side's link check of crates/runtime's file module (piece 3.09; src/runtime_file.c is
 * gone): a linked mod overlay and writable saves, through the exported C helpers and the guest
 * entry points runtime_file_resolve gives. Errors are FreeBSD errno values; the ones below have
 * the same numbers on Linux. The Rust tests (crates/runtime/src/file/tests.rs) cover the rest.
 *
 * Links against the runtime static library: the loader build (build.sh --test, with the C
 * runtime) or the plain one (.github/workflows/check.yml). */
#include "../crates/runtime/include/uncharted_runtime.h"
#include <assert.h>
#include "portable.h"
#ifndef _WIN32
#include <sys/stat.h>
#endif

/* With the C runtime linked (build.sh), the GPU library's settings menu restarts through
 * probe.c and every exit ends through it: neither happens here. */
void runtime_restart(void);
void runtime_restart(void) { abort(); }
void runtime_quit(int code);
void runtime_quit(int code) { fflush(NULL); _Exit(code); }

enum { GUEST_ENOENT = 2, GUEST_EACCES = 13, GUEST_EROFS = 30 };
#define SCE_ERROR(e) ((int64_t)(int32_t)(UINT32_C(0x80020000) | (e)))

static int64_t read_all(const char *path, char *content, size_t size) {
    int64_t fd = runtime_file_open(path, 0, 0);
    assert(fd >= 3);
    int64_t n = runtime_file_read((int)fd, content, size);
    assert(runtime_file_close((int)fd) == 0);
    return n;
}

#if defined(__x86_64__)
typedef int64_t (__attribute__((sysv_abi)) *PathCall)(const char *);
typedef int64_t (__attribute__((sysv_abi)) *TruncateCall)(const char *, int64_t);
typedef int64_t (__attribute__((sysv_abi)) *RenameCall)(const char *, const char *);
/* The guest's own calls on a game path: read-only. */
static void guest_calls_refuse_game_paths(const char *path) {
    TruncateCall truncate_call = (TruncateCall)runtime_file_resolve("WlyEA-sLDf0#p#J");
    PathCall unlink_call = (PathCall)runtime_file_resolve("AUXVxWeJU-A#p#J");
    RenameCall rename_call = (RenameCall)runtime_file_resolve("52NcYU9+lEo#p#J");
    assert(truncate_call && unlink_call && rename_call);
    assert(truncate_call(path, 0) == SCE_ERROR(GUEST_EROFS));
    assert(unlink_call(path) == SCE_ERROR(GUEST_EROFS));
    assert(rename_call(path, "/data/moved") == SCE_ERROR(GUEST_EROFS));
}
#else
static void guest_calls_refuse_game_paths(const char *path) { (void)path; }
#endif

int main(void) {
    /* bb_mkdtemp picks a scratch directory on both platforms (tests/portable.h). */
    char root[BB_TEST_PATH_MAX] = "/tmp/bbport-mod-files-XXXXXX"; /* mkdtemp's template */
    assert(bb_mkdtemp(root));
    char game[BB_TEST_PATH_MAX + 64], user[BB_TEST_PATH_MAX + 64];
    char source[BB_TEST_PATH_MAX + 64], link[BB_TEST_PATH_MAX + 64];
    snprintf(game, sizeof(game), "%s/game", root);
    snprintf(user, sizeof(user), "%s/user", root);
    snprintf(source, sizeof(source), "%s/mod.dcx", root);
    snprintf(link, sizeof(link), "%s/game/asset.dcx", root);
    assert(!mkdir(game, 0755));
    FILE *f = fopen(source, "w");
    assert(f && fputs("modded", f) >= 0 && !fclose(f));
    assert(!bb_test_link(source, link));
    runtime_file_configure(game, user);
    assert(!strcmp(runtime_file_user_dir(), user));

    for (int i = 0; i < 3; ++i) {
        const char *path = i == 0 ? "/app0/asset.dcx" : i == 1 ? "/hostapp/asset.dcx" : "asset.dcx";
        char content[8] = {0};
        assert(read_all(path, content, 6) == 6 && !strcmp(content, "modded"));
        unsigned char info[120];
        assert(runtime_file_stat(path, info) == 0);
        int64_t size;
        memcpy(&size, info + 72, sizeof(size));
        assert(size == 6);
        assert(runtime_file_open(path, 2, 0) == -GUEST_EROFS);
        assert(runtime_file_open(path, 0x400, 0) == -GUEST_EROFS);
        guest_calls_refuse_game_paths(path);
    }

    int64_t dir = runtime_file_open("/app0", 0x20000, 0);
    assert(dir >= 3);
    char entries[1024];
    int64_t base = -1;
    int64_t count = runtime_file_getdents((int)dir, entries, sizeof(entries), &base);
    assert(count > 0 && base == 0);
    int found = 0;
    for (int64_t p = 0; p < count;) {
        uint16_t length;
        memcpy(&length, entries + p + 4, 2);
        assert(length);
        if (!strcmp(entries + p + 8, "asset.dcx")) { assert(entries[p + 6] == 8); found = 1; }
        p += length;
    }
    assert(found && runtime_file_close((int)dir) == 0);

    int64_t save = runtime_file_open("/data/test-save", 0x202, 0644);
    assert(save >= 3);
    assert(runtime_file_write((int)save, "save", 4) == 4);
    assert(runtime_file_lseek((int)save, 1, 0) == 1);
    char back[4] = {0};
    assert(runtime_file_pread((int)save, back, 3, 0) == 3 && !memcmp(back, "sav", 3));
    assert(runtime_file_read((int)save, back, 3) == 3 && !memcmp(back, "ave", 3));
    unsigned char info[120];
    assert(runtime_file_fstat((int)save, info) == 0);
    assert(runtime_file_close((int)save) == 0);
    assert(runtime_file_close((int)save) == -9); /* EBADF */

    /* ".." hidden behind Windows separators must not escape a mount ... */
    char host[1024];
    assert(runtime_file_translate("/data/..\\..\\escape", host, sizeof(host)) == GUEST_EACCES);
    assert(runtime_file_open("..\\..\\escape", 0, 0) == -GUEST_EACCES);
    /* ... while ".." inside a single name is a normal, allowed component. */
    assert(runtime_file_open("/data/a..b", 0, 0) == -GUEST_ENOENT);
    assert(runtime_file_translate("/data/test-save", host, sizeof(host)) == 0);
    assert(!remove(host));

    assert(runtime_file_mount("/extra", game) == 0);
    char content[8] = {0};
    assert(read_all("/extra/asset.dcx", content, 6) == 6 && !strcmp(content, "modded"));
    runtime_file_unmount("/extra");
    assert(runtime_file_open("/extra/asset.dcx", 0, 0) == -GUEST_ENOENT);

    assert(!remove(link) && !remove(source) && !rmdir(game));
    const char *dirs[] = {"temp0", "download0", "data"};
    for (int i = 0; i < 3; ++i) {
        char p[1024];
        snprintf(p, sizeof(p), "%s/%s", user, dirs[i]);
        assert(!rmdir(p));
    }
    assert(!rmdir(user) && !rmdir(root));
    runtime_file_report();
    puts("Guest mod files (crates/runtime file module): reads, stat, merged listing, readonly assets and writable saves PASS");
    return 0;
}
