//! uncharted_runtime: the host side of the port, replacing `src/*.c` module by module
//! (ADR 0002, docs/code-map.md "Modules: crates/runtime"). Built as `rlib` and `staticlib`; the
//! static library is linked into the C loader until the loader itself is Rust.
//!
//! Skeleton (2026-11-01): only [`abi`] exists, and it exports one symbol.
//!
//! # Planned modules (none are in the tree yet)
//!
//! The list is empty on purpose; each module arrives with its own piece (ROADMAP stage 3),
//! in this order: `time` and `rtc`, `sync`, `content` and `display`, `trophy`, `savedata`,
//! `audio`, `ajm`, `file`, `pad`, `services` and `kernel`, `memory`, `fiber` and `thread`,
//! `cpu_patch` and `red_zone`, `os`, `loader` (and the `uncharted-run` binary). The `gpu`
//! build script (CMake over `gpu/`, piece 3.01) and the `cbindgen` header come with the
//! scaffold piece, not here.

pub mod abi;
