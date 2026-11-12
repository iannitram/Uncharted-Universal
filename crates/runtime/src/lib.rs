//! uncharted_runtime: the host side of the port, replacing `src/*.c` module by module
//! (ADR 0002, docs/code-map.md "Modules: crates/runtime"). Built as `rlib` and `staticlib`; the
//! static library is linked into the C loader until the loader itself is Rust.
//!
//! [`abi`] has one exported symbol, plus stubs for `runtime_resolve` and the NID table.
//! [`savedata`] has the local save-location metadata commit (#168); the guest savedata ABI
//! and loader are still pending, so no C symbols have moved yet.
//! [`crash`] emits the bounded single-line format consumed by `crates/core::logs::crash_line`;
//! wiring it into the fault/assertion handlers remains part of the loader port.
//!
//! # Remaining planned modules
//!
//! Each module arrives with its own piece (ROADMAP stage 3), in this order: `time` and
//! `rtc`, `sync`, `content` and `display`, `trophy`, the rest of `savedata`,
//! `audio`, `ajm`, `file`, `pad`, `services` and `kernel`, `memory`, `fiber` and `thread`,
//! `cpu_patch` and `red_zone`, `os`, `loader` (and the `uncharted-run` binary). The `gpu`
//! build script (CMake over `gpu/`, piece 3.01) and the `cbindgen` header come with the
//! scaffold piece, not here.

// Skeleton only: stubs ignore their inputs. Remove this line when the last stub is built.
#![allow(unused_variables)]

pub mod abi;
pub mod benchmark;
pub mod crash;
pub mod pad;
pub mod savedata;
