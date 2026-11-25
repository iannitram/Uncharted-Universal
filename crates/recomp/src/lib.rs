//! The Apple AOT recompiler foundation (ADR 0006, #396).
//!
//! `build.rs` compiles the vendored bbcpu (the x86-64 interpreter, the arm64 translator and the
//! loader of recompiled libraries) and Zydis for Apple arm64 targets, and builds the host tools
//! `uu-scan` (function discovery), `uu-bbrecomp` (the C generator) and `uu-fuzz-recomp` on a Mac.
//! `uu-recomp` (`main.rs`, [`pipeline`]) runs the owner's executable through them:
//!
//! ```text
//! load_image -> normalized ELF -> uu-scan -> imports.tsv, pointers.txt -> uu-bbrecomp -> clang -dynamiclib
//! ```
//!
//! Everything is written outside any git checkout ([`guard`]) and only counts are reported
//! ([`pipeline::Summary`]). Nothing here ever reads a game file in a test: the tests run on
//! authored synthetic code (`tests/recomp/`).
//!
//! On every other target the crate is empty, so Linux, Windows and Android builds do not see it.
#![cfg(all(target_vendor = "apple", target_arch = "aarch64"))]

pub mod guard;
pub mod pipeline;
pub mod tools;

/// The `BB_RECOMP_API_VERSION` of the vendored bbcpu and generator (`build.rs` checks it).
pub const API_VERSION: u32 = 4;
