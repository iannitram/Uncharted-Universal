# 0001: One Rust workspace for our code

**Decided:** 2026-11-01, by the owner in the Project Manager session's question tool: "Ours to Rust, GPU stays (Recommended)".

## Decision

Our code becomes one Cargo workspace: `crates/core` (`uncharted_core`, the engine: dump checks, prepare, patches, saves, Steam, hubs), `crates/runtime` (`uncharted_runtime`, the host runtime that replaces `src/`), `crates/cli` (`uncharted-universal`) and `app/` (`uncharted-app`, the Slint launcher). The vendored shadPS4 renderer and the port's C++ shim in `gpu/` stay C++, built by CMake from the runtime's build script and reached through `gpu/bbgpu.h`. Code we adopt for Android and Apple stays in its own language too.

## Why

- One language and one build (`cargo`) for everything of ours, tested the same way on Linux, Windows and macOS.
- shadPS4 is a large, moving upstream; keeping it in C++ keeps its fixes back-portable.

## Cost

Upstream's changes to `src/`, `launcher/` and `scripts/` no longer merge once a module is Rust; each becomes an `upstream-sync` issue that ports the change by hand.
