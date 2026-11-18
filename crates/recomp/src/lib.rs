//! The AOT recompiler build step (roadmap 8.03, #396), first piece: the generator's frame.
//!
//! What is here: guest ranges as configuration (`config`), the work folder outside the clone
//! (`workdir`), a generator for a small authored x86-64 subset that refuses anything else
//! (`lift`), the aggregate coverage numbers (`coverage`) and the C ABI the generated library
//! exports (`abi`). Pure Rust, no unsafe.
//!
//! What is not here yet: no GPL-2.0 recompiler source is vendored, so there is no full decoder,
//! no ELF reader and no function discovery; the dump is never read. Nothing here has run on a
//! game, and no coverage figure for any title exists.
#![forbid(unsafe_code)]

pub mod abi;
pub mod config;
pub mod coverage;
pub mod lift;
pub mod workdir;
