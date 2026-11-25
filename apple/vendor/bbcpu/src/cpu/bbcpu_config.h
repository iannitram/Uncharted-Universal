/* SPDX-License-Identifier: GPL-2.0-or-later */
/* uu-local addition (see apple/vendor/bbcpu/VENDOR.txt): the addresses bbcpu uses as markers.
 *
 * Upstream hard-codes them for Bloodborne's address layout. They are never mapped: each is a
 * return address that is neither guest code nor a host function, so a host whose guest memory
 * covers one of them (the feasibility gate's "configuration, not constants" rule, gate 2) sets
 * another with -D when it builds libbbcpu. The defaults are upstream's values.
 *
 * Every marker is a 16-byte-aligned address; the per-depth ones need depth * 16 of room above. */
#ifndef BBCPU_CONFIG_H
#define BBCPU_CONFIG_H

#include <stdint.h>

/* Return addresses of host -> guest calls (cpu.c): base + 16 * call depth. */
#ifndef BBCPU_SENTINEL_BASE
#define BBCPU_SENTINEL_BASE UINT64_C(0x0000700000000000)
#endif
/* The return address of the calls a recorded function makes (record.c). */
#ifndef BBCPU_RECORD_SENTINEL
#define BBCPU_RECORD_SENTINEL UINT64_C(0x0000700000000f00)
#endif
/* Where translated code stops when a recompiled function runs off its thread (jit_arm64.c). */
#ifndef BBCPU_THREAD_SENTINEL
#define BBCPU_THREAD_SENTINEL UINT64_C(0x0000700000000d00)
#endif
/* The runtime-call frames of recompiled code (recomp.c): base + 16 * frame depth. */
#ifndef BBCPU_FRAME_BASE
#define BBCPU_FRAME_BASE UINT64_C(0x0000700100000000)
#endif

#endif
