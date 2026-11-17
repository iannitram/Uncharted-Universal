# Synthetic no-JIT execution results (#212)

Authored x86-64 cases, hand-checked expected registers, flags, xmm state and fault behaviour, run
through bbcpu's interpreter (revision `3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca`, GPL-2.0, linked
not vendored; Zydis v4.1.1, MIT). No game data. This is synthetic execution on a Mac and its iOS
Simulator only: no device run, no game readiness claim.

Date: 2026-11-17. Machine: 8 GB Apple-silicon Mac mini, macOS 27.0.1. Toolchain: Apple clang
21.0.0 (`xcrun --sdk macosx|iphonesimulator clang`). Simulator runtime: iOS 27.2 (iPhone 17,
booted by the script and shut down afterwards).

| Run | Command | Target | no_jit_build | pass | fail | unsupported |
| --- | --- | --- | --- | --- | --- | --- |
| macOS host, interpreter forced (`BB_JIT=0`) | `sh tests/apple_nojit/run.sh host report.json` | arm64-apple-darwin | 0 | 63 | 0 | 1 |
| iOS no-JIT build, in the simulator (`xcrun simctl spawn`) | `sh tests/apple_nojit/run.sh sim report.json` | arm64-apple-ios17.0-simulator | 1 (`TARGET_OS_IPHONE` sets `BB_NO_JIT`) | 63 | 0 | 1 |

Both runs have identical per-family numbers (pass / fail / unsupported), and `not_authored` is
empty in both reports:

| Family | pass | fail | unsupported |
| --- | --- | --- | --- |
| integer-flags | 1 | 0 | 0 |
| shift-rotate | 1 | 0 | 0 |
| wide-multiply-divide | 1 | 0 | 0 |
| bit-scan | 1 | 0 | 0 |
| sse-integer | 2 | 0 | 0 |
| sse-float | 8 | 0 | 0 |
| avx-avx2 | 3 | 0 | 0 |
| x87 | 6 | 0 | 0 |
| string-ops | 7 | 0 | 0 |
| atomics-ordering | 8 | 0 | 0 |
| unaligned-memory | 8 | 0 | 0 |
| bmi-lzcnt-movbe | 9 | 0 | 0 |
| host-call-callback | 3 | 0 | 0 |
| tls | 4 | 0 | 0 |
| fault | 1 | 0 | 1 |

`fault`: `div_by_zero` passes (bbcpu stops the guest with "division by zero"; the harness reads
that stop from a forked child as #DE). `ud2` is unsupported: bbcpu reports "unsupported
instruction" and stops cleanly; it does not model #UD. Every expected value is worked by hand
from the Intel SDM (comment on each case in `cases_more.inc` and `test_nojit.c`), never taken from
the interpreter.

Memory-using cases use a 256-byte scratch area in the state (`NOJIT_MEM + off` in a register is
rewritten to its real address and mapped back on output). The scratch area is also the guest fs
base (`bbcpu_set_thread_pointer`), which is what the tls cases read and write. host-call-callback
runs `mov rax,imm64; call rax` into three host functions (integer args, a double in xmm0, and one
that calls a guest function back through `bbcpu_call`); after a host call only rax, xmm0 and the
callee-saved registers are compared. `lock cmpxchg16b` needs the 16-byte alignment the scratch
area now has (an earlier unaligned run crashed bbcpu; real hardware would #GP too).

Limits: rsp is not modelled (the stub does not load or store it); faults are observed as bbcpu's
process stop, not delivered to the guest.

CI: the `apple-nojit-synthetic` job runs the macOS host case. The simulator run is local-only: I
could not verify from here that the hosted runner has an iOS Simulator runtime that boots
reliably, so CI does not claim it.
