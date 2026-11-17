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
| macOS host, interpreter forced (`BB_JIT=0`) | `sh tests/apple_nojit/run.sh host report.json` | arm64-apple-darwin | 0 | 7 | 0 | 1 |
| iOS no-JIT build, in the simulator (`xcrun simctl spawn`) | `sh tests/apple_nojit/run.sh sim report.json` | arm64-apple-ios17.0-simulator | 1 (`TARGET_OS_IPHONE` sets `BB_NO_JIT`) | 7 | 0 | 1 |

Passing families: integer-flags, shift-rotate, wide-multiply-divide, bit-scan, sse-integer, and
`div_by_zero` (bbcpu stops the guest with "division by zero"; the harness reads that stop from a
forked child as #DE). Unsupported: `ud2` (family `fault`): bbcpu reports "unsupported instruction"
and stops cleanly; it does not model #UD. Not authored yet (listed in each report's `not_authored`):
x87, avx-avx2, atomics-ordering, string-ops, unaligned-memory, host-call-callback, tls, sse-float,
bmi-lzcnt-movbe.

Limits: rsp is not modelled (the stub does not load or store it); faults are observed as bbcpu's
process stop, not delivered to the guest.

CI: the `apple-nojit-synthetic` job runs the macOS host case. The simulator run is local-only: I
could not verify from here that the hosted runner has an iOS Simulator runtime that boots
reliably, so CI does not claim it.
