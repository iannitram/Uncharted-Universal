/* SPDX-License-Identifier: GPL-2.0-only */
/* tests/recomp/host.c: a minimal bbcpu host for the recompiler tests (crates/recomp/tests/e2e.rs).
 *
 *   uu-recomp-host ELF [--patch OFFSET:BYTE]... [--import OFFSET]... -- CALL...
 *
 * Maps the PT_LOAD segments of the authored x86-64 ELF anywhere, applies the patches (so a
 * library generated from the original no longer matches), fills each --import slot with a host
 * function, loads the library BB_RECOMP_LIB names (if any), then makes each CALL:
 * OFFSET:ARG,ARG,... (up to 6 integer arguments, numbers or @scratch, a zeroed 4 KiB buffer
 * shared by all calls). Output, one line each:
 *   recompiled N            how many functions the library replaced (bbcpu_recomp_functions)
 *   call OFFSET rax=HEX     the result
 *   scratch HEX             the first 32 bytes of the buffer after that call
 * A guest the interpreter refuses ends the process through bbcpu (STOP: ..., status 30). */
#include "cpu/bbcpu.h"
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>

uint64_t bb_image_base;

void *runtime_low_map(size_t size, int prot) {
    void *p = mmap(NULL, size, prot, MAP_PRIVATE | MAP_ANON, -1, 0);
    return p == MAP_FAILED ? NULL : p;
}

/* What an import slot points at: the guest calls it as a host function (SysV arguments). */
static uint64_t host_import(uint64_t a, uint64_t b) { return a * 1000 + b + 7; }

typedef struct { unsigned char e_ident[16]; uint16_t e_type, e_machine; uint32_t e_version;
    uint64_t e_entry, e_phoff, e_shoff; uint32_t e_flags; uint16_t e_ehsize, e_phentsize, e_phnum,
    e_shentsize, e_shnum, e_shstrndx; } Elf64_Ehdr;
typedef struct { uint32_t p_type, p_flags; uint64_t p_offset, p_vaddr, p_paddr, p_filesz, p_memsz,
    p_align; } Elf64_Phdr;

static uint8_t *map_image(const char *path) {
    FILE *f = fopen(path, "rb");
    if (!f) { perror(path); exit(2); }
    fseek(f, 0, SEEK_END);
    const long size = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t *data = malloc((size_t)size);
    if (!data || fread(data, 1, (size_t)size, f) != (size_t)size) exit(2);
    fclose(f);
    const Elf64_Ehdr *eh = (const Elf64_Ehdr *)data;
    uint64_t end = 0;
    for (int i = 0; i < eh->e_phnum; ++i) {
        const Elf64_Phdr *ph = (const Elf64_Phdr *)(data + eh->e_phoff + (uint64_t)i * eh->e_phentsize);
        if (ph->p_type == 1 && ph->p_vaddr + ph->p_memsz > end) end = ph->p_vaddr + ph->p_memsz;
    }
    uint8_t *image = mmap(NULL, (end + 16383) & ~UINT64_C(16383), PROT_READ | PROT_WRITE,
                          MAP_PRIVATE | MAP_ANON, -1, 0);
    if (image == MAP_FAILED) exit(2);
    for (int i = 0; i < eh->e_phnum; ++i) {
        const Elf64_Phdr *ph = (const Elf64_Phdr *)(data + eh->e_phoff + (uint64_t)i * eh->e_phentsize);
        if (ph->p_type != 1) continue;
        memcpy(image + ph->p_vaddr, data + ph->p_offset, ph->p_filesz);
        if (ph->p_flags & 1) bbcpu_add_guest_code((uintptr_t)image + ph->p_vaddr, ph->p_memsz);
    }
    free(data);
    return image;
}

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: %s ELF [--patch OFF:BYTE] [--import OFF] -- CALL...\n", argv[0]); return 2; }
    uint8_t *image = map_image(argv[1]);
    bb_image_base = (uint64_t)(uintptr_t)image;
    int i = 2;
    for (; i < argc && strcmp(argv[i], "--"); ++i) {
        if (!strcmp(argv[i], "--patch") && i + 1 < argc) {
            char *colon;
            const uint64_t offset = strtoull(argv[++i], &colon, 0);
            image[offset] = (uint8_t)strtoul(colon + 1, NULL, 0);
        } else if (!strcmp(argv[i], "--import") && i + 1 < argc) {
            const uint64_t pointer = (uint64_t)(uintptr_t)&host_import;
            memcpy(image + strtoull(argv[++i], NULL, 0), &pointer, 8);
        } else {
            fprintf(stderr, "%s: unknown argument\n", argv[i]);
            return 2;
        }
    }
    bbcpu_recomp_load(bb_image_base); /* a no-op without BB_RECOMP_LIB */
    printf("recompiled %zu\n", bbcpu_recomp_functions());
    static uint8_t scratch[4096];
    for (++i; i < argc; ++i) {
        char *colon;
        const uint64_t offset = strtoull(argv[i], &colon, 0);
        uint64_t args[6];
        int count = 0;
        for (char *p = *colon ? colon + 1 : colon; *p && count < 6;) {
            args[count++] = !strncmp(p, "@scratch", 8) ? (uint64_t)(uintptr_t)scratch : strtoull(p, &p, 0);
            if (*p == '@') p += 8;
            if (*p == ',') ++p;
        }
        const uint64_t rax = bbcpu_call((uintptr_t)image + offset, count, args);
        printf("call %#" PRIx64 " rax=%" PRIx64 "\nscratch ", offset, rax);
        for (int b = 0; b < 32; ++b) printf("%02x", scratch[b]);
        printf("\n");
        fflush(stdout);
    }
    return 0;
}
