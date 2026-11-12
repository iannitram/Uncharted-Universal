//! uncharted_runtime: the host side of the port, replacing `src/*.c` module by module
//! (ADR 0002, docs/code-map.md "Modules: crates/runtime"). Built as `rlib` and `staticlib`; the
//! static library is linked into the C loader until the loader itself is Rust.
//!
//! [`abi`] has one exported symbol, plus stubs for `runtime_resolve` and the NID table.
//! [`savedata`] has the local save-location metadata commit (#168); the guest savedata ABI
//! and loader are still pending.
//! [`crash`] emits the bounded single-line format consumed by `crates/core::logs::crash_line`;
//! wiring it into the fault/assertion handlers remains part of the loader port.
//! [`memory`] is the guest address space (piece 3.12): direct and flexible memory, the region
//! table and the GPU library's page hooks. On Linux and Android it exports the C symbols of
//! `src/runtime_memory.c`, which Windows still builds until piece 3.15.
//! [`os`] has the open file description whose positioned reads and writes never touch the
//! shared position (#39, from `compat_win.c`); the guest file layer does not use it yet.
//! [`probe`] is `uncharted-run --probe`: the hardware report `crates/core::tune` reads (#171),
//! from Vulkan loaded at run time. The `uncharted-run` binary has no game loader yet.
//!
//! # Remaining planned modules
//!
//! Each module arrives with its own piece (ROADMAP stage 3), in this order: `time` and
//! `rtc`, `sync`, `content` and `display`, `trophy`, the rest of `savedata`,
//! `audio`, `ajm`, `file`, `pad`, `services` and `kernel`, `fiber` and `thread`,
//! `cpu_patch` and `red_zone`, the rest of `os`, `loader` (in the `uncharted-run` binary).
//! The `gpu` build script (CMake over `gpu/`, piece 3.01) and the `cbindgen` header come
//! with the scaffold piece, not here.

// Skeleton only: stubs ignore their inputs. Remove this line when the last stub is built.
#![allow(unused_variables)]

pub mod abi;
pub mod benchmark;
pub mod benchmark_run;
pub mod crash;
pub mod memory;
pub mod os;
pub mod pad;
pub mod probe;
pub mod savedata;
