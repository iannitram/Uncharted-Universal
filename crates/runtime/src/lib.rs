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
//! [`os`] is the host OS shims (piece 3.15, from `src/compat_win.c`): the open file description
//! whose positioned reads and writes never touch the shared position (#39), the Win32 errno map,
//! the sleep, the UTC offset, OS random bytes, `remove_tree` and CPU times; [`file`] keeps one
//! `os::Description` per open guest file.
//! [`file`] is the guest file system (piece 3.09): mounts, the mod overlay, the descriptor table
//! and the file calls of the deleted `src/runtime_file.c`, with its C symbols.
//! [`time`] and [`rtc`] (piece 3.02) are the guest's `gettimeofday` and `libSceRtc`, from
//! `src/runtime_time.c` and `src/runtime_rtc.c`; `abi`'s resolver chain asks them in the C order.
//! [`probe`] is `uncharted-run --probe`: the hardware report `crates/core::tune` reads (#171),
//! from Vulkan loaded at run time. The `uncharted-run` binary has no game loader yet.
//! [`sync`] is the guest's mutexes, condition variables, semaphores and rwlocks (piece 3.03),
//! exporting the C symbols of the deleted `src/runtime_mutex.c`, `runtime_sema.c` and
//! `runtime_rwlock.c`.
//!
//! [`content`] is the system module and app content ABI of the deleted `src/runtime_content.c`
//! (piece 3.04, x86-64 only: its guest functions use the System V ABI); [`display`] is the
//! `BB_GAME_RES` patch of the loaded executable from the deleted `src/runtime_display.c`.
//! [`trophy`] is libSceNpTrophy (3.05), the port of the deleted `src/runtime_trophy.c`. The
//! loader links this library and binds the user folder and the trophy notice into it.
//! [`audio`] is libSceAudioOut (piece 3.07, from the deleted `src/runtime_audio.c` and
//! `src/runtime_speakers.h`): ports paced like hardware on SDL3 audio streams in the loader
//! build, the speaker layout and the 7.1 fold-down; every other build plays on a silent timer.
//! [`ajm`] is libSceAjm (piece 3.08, from the deleted `src/runtime_ajm.c`): decoder batches, ATRAC9
//! through LibAtrac9 and MP3 through a pure-Rust decoder in place of FFmpeg.
//! [`kernel`] and [`services`] (piece 3.11) are libkernel/libScePosix process services and the
//! offline system services, from the deleted `src/runtime_kernel.c` and
//! `src/runtime_services.c`; they export their resolvers and the process clock the GPU library
//! shares, and keep their places in `abi`'s resolver chain.
//!
//! [`pad`] is libScePad (piece 3.10, from the deleted `src/runtime_pad.c`): SDL3 gamepads with gyro
//! and rumble, keyboard and mouse binds, the mouse camera hooks and the guest callback table, and
//! the `BB_BENCHMARK` scripted path; it exports `runtime_pad_resolve`, `runtime_pad_report`,
//! `runtime_pad_tool`, `runtime_pad_watch` and `runtime_guest_callbacks`.
//! [`cpu_patch`] and [`red_zone`] (piece 3.14) are the guest code patchers, ports of
//! `src/cpu_patch.c` and `src/red_zone.c`: instructions the host CPU lacks (BMI1, LZCNT, MOVBE)
//! run from thunks, and guest leaf functions that keep data below rsp run their faulting accesses
//! with rsp 128 bytes lower. Each exports its C symbols; the loader links these, the C files
//! stay until 3.17. The x86 decoder is iced-x86 (Zydis in the C).
//!
//! # Remaining planned modules
//!
//! Each module arrives with its own piece (ROADMAP stage 3), in this order: the rest of `savedata`,
//! `fiber` and `thread`,
//! `loader` (in the `uncharted-run` binary).
//! `build.rs` builds `gpu/` with CMake when `build.sh` asks for it (`UNCHARTED_GPU_BUILD`).

// Skeleton only: stubs ignore their inputs. Remove this line when the last stub is built.
#![allow(unused_variables)]

pub mod abi;
// `cbindgen:ignore`: only the Rust loader side calls its resolver and report; its constants and
// structs are Rust-only, not part of the C header.
/// cbindgen:ignore
pub mod ajm;
/// cbindgen:ignore
pub mod android_fex;
pub mod audio;
pub mod benchmark;
pub mod benchmark_run;
#[cfg(target_arch = "x86_64")]
pub mod content;
/// Piece 3.14: exports the C symbols of `src/cpu_patch.h` (declared there, not in the generated
/// header). Linux and Windows on x86-64; other hosts have no guest code to patch.
/// cbindgen:ignore
#[cfg(all(target_arch = "x86_64", any(target_os = "linux", windows)))]
pub mod cpu_patch;
/// cbindgen:ignore
pub mod crash;
pub mod display;
#[cfg(all(
    any(target_os = "linux", target_os = "windows"),
    target_arch = "x86_64"
))]
pub mod fiber;
pub mod file;
pub mod kernel;
/// cbindgen:ignore
pub mod loader;
// Left out of the header: src/runtime.h declares memory's symbols (src/runtime_memory.c is
// still the Windows build), and its constants are Rust's.
/// cbindgen:ignore
pub mod memory;
pub mod os;
// `cbindgen:ignore`: their constants are Rust-only, not part of the C header.
/// cbindgen:ignore
pub mod pad;
#[cfg(all(target_arch = "x86_64", any(target_os = "linux", windows)))]
mod patch_util;
/// cbindgen:ignore
pub mod probe;
/// Piece 3.14: exports the C symbols of `src/red_zone.h` (declared there, not in the generated
/// header).
/// cbindgen:ignore
#[cfg(all(target_arch = "x86_64", any(target_os = "linux", windows)))]
pub mod red_zone;
pub mod rtc;
pub mod savedata;
pub mod services;
#[cfg(target_arch = "x86_64")]
pub mod sync;
#[cfg(all(
    any(target_os = "linux", target_os = "windows"),
    target_arch = "x86_64"
))]
pub mod thread;
pub mod time;
pub mod trophy;
