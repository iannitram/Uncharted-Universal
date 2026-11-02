# 0002: The runtime moves to Rust one module at a time

**Decided:** 2026-11-01, following ADR 0001 (Project Manager, R03).

## Decision

`crates/runtime` builds as a static library linked with the remaining C. Each `src/runtime_*.c` is replaced by a Rust module that exports the same `extern "C"` symbols (header from `cbindgen`), and the C file is deleted in the same PR. Its C test becomes a Rust test. Leaves first (time, sync, content, display), then trophy, savedata, audio, AJM, file, pad, services, memory, threads, code patching, the Windows shims, and the loader (`probe.c`) last. After the loader, `src/` is gone and `cargo build` makes `uncharted-run`.

## Rules

- The game builds and reaches its first frame at every step: a boot smoke test runs in CI, and every step also gets a device check on the AMD handheld, not only the six that touch savedata, audio, AJM, memory, threads and the loader (owner ruling, 2026-11-01; SPEC.md says a device check per step).
- No behaviour change in a port PR; a bug found while porting is its own issue.
- Every `unsafe` block has a `// SAFETY:` line.

## Why

A rewrite in one go would leave the game unplayable for months and make the campaign bugs (M1-M3) impossible to bisect.
