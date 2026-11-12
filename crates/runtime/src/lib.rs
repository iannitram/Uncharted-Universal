//! uncharted_runtime: the host side of the port, replacing `src/*.c` module by module
//! (ADR 0002, docs/code-map.md "Modules: crates/runtime"). Built as `rlib` and `staticlib`; the
//! static library is linked into the C loader until the loader itself is Rust.
//!
//! [`abi`] is the first ported module (piece 3.01): the NID table, `runtime_resolve` and the libc
//! contracts of the deleted `src/runtime.c`, exporting its C symbols (declared for C in
//! `include/uncharted_runtime.h`, written by `cbindgen`; `tests/header.rs` keeps it current).
//! [`savedata`] has the local save-location metadata commit (#168); the guest savedata ABI
//! and loader are still pending.
//! [`crash`] emits the bounded single-line format consumed by `crates/core::logs::crash_line`;
//! wiring it into the fault/assertion handlers remains part of the loader port.
//! [`os`] has the open file description whose positioned reads and writes never touch the
//! shared position (#39, from `compat_win.c`); the guest file layer does not use it yet.
//! [`probe`] is `uncharted-run --probe`: the hardware report `crates/core::tune` reads (#171),
//! from Vulkan loaded at run time. The `uncharted-run` binary has no game loader yet.
//! [`content`] is the system module and app content ABI of the deleted `src/runtime_content.c`
//! (piece 3.04, x86-64 only: its guest functions use the System V ABI); [`display`] is the
//! `BB_GAME_RES` patch of the loaded executable from the deleted `src/runtime_display.c`.
//!
//! # Remaining planned modules
//!
//! Each module arrives with its own piece (ROADMAP stage 3), in this order: `time` and
//! `rtc`, `sync`, `trophy`, the rest of `savedata`,
//! `audio`, `ajm`, `file`, `pad`, `services` and `kernel`, `memory`, `fiber` and `thread`,
//! `cpu_patch` and `red_zone`, the rest of `os`, `loader` (in the `uncharted-run` binary).
//! `build.rs` builds `gpu/` with CMake when `build.sh` asks for it (`UNCHARTED_GPU_BUILD`).

// Skeleton only: stubs ignore their inputs. Remove this line when the last stub is built.
#![allow(unused_variables)]

pub mod abi;
pub mod benchmark;
pub mod benchmark_run;
#[cfg(target_arch = "x86_64")]
pub mod content;
pub mod crash;
pub mod display;
pub mod os;
// `cbindgen:ignore`: their constants are Rust-only, not part of the C header.
/// cbindgen:ignore
pub mod pad;
/// cbindgen:ignore
pub mod probe;
pub mod savedata;
