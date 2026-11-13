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
//! [`memory`] is the guest address space (piece 3.12): direct and flexible memory, the region
//! table and the GPU library's page hooks. On Linux and Android it exports the C symbols of
//! `src/runtime_memory.c`, which Windows still builds until piece 3.15.
//! [`os`] has the open file description whose positioned reads and writes never touch the
//! shared position (#39, from `compat_win.c`); the guest file layer does not use it yet.
//! [`time`] and [`rtc`] (piece 3.02) are the guest's `gettimeofday` and `libSceRtc`, from
//! `src/runtime_time.c` and `src/runtime_rtc.c`; `abi`'s resolver chain asks them in the C order.
//! [`probe`] is `uncharted-run --probe`: the hardware report `crates/core::tune` reads (#171),
//! from Vulkan loaded at run time. The `uncharted-run` binary has no game loader yet.
//! [`content`] is the system module and app content ABI of the deleted `src/runtime_content.c`
//! (piece 3.04, x86-64 only: its guest functions use the System V ABI); [`display`] is the
//! `BB_GAME_RES` patch of the loaded executable from the deleted `src/runtime_display.c`.
//! [`trophy`] is libSceNpTrophy (3.05), the port of the deleted `src/runtime_trophy.c`. The
//! loader links this library and binds the user folder and the trophy notice into it.
//!
//! # Remaining planned modules
//!
//! Each module arrives with its own piece (ROADMAP stage 3), in this order: `sync`,
//! the rest of `savedata`,
//! `audio`, `ajm`, `file`, `pad`, `services` and `kernel`, `fiber` and `thread`,
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
// Left out of the header: src/runtime.h declares memory's symbols (src/runtime_memory.c is
// still the Windows build), and its constants are Rust's.
/// cbindgen:ignore
pub mod memory;
pub mod os;
// `cbindgen:ignore`: their constants are Rust-only, not part of the C header.
/// cbindgen:ignore
pub mod pad;
/// cbindgen:ignore
pub mod probe;
pub mod rtc;
pub mod savedata;
pub mod time;
pub mod trophy;
