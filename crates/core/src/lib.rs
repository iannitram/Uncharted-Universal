//! uncharted_core: the launcher's engine (docs/code-map.md). The Slint app (`app/`, planned)
//! and the `uncharted-universal` command line both drive it; neither has logic of its own.
//!
//! Skeleton (2026-11-01): every module has its final public types and signatures, one file
//! per module of the code map's `crates/core` table. A function that is not built yet returns
//! `Err(Error::NotBuilt("<module>::<fn>"))`, or panics with "not built yet" when its signature
//! cannot fail; each one is filled in by its own issue (ROADMAP stage 1). Pure Rust, no
//! dependencies. `relay` (save-relay adapter, feature `relay`) is planned and not in the tree.
//!
//! Never in this crate: game data, keys, firmware, or code that copies, uploads or logs them
//! (SPEC.md M9, Won'ts).

// Skeleton only: stubs ignore their inputs. Remove this line when the last stub is built.
#![allow(unused_variables)]
#![forbid(unsafe_code)]

/// Panics with "not built yet: <what>"; the body of a stub whose signature cannot fail.
/// Kept (allowed unused) until the last stub is built, then delete it with `error::not_built`.
#[allow(unused_macros)]
macro_rules! not_built {
    ($what:expr) => {
        unimplemented!("not built yet: {}", $what)
    };
}

pub mod artwork;
pub mod aspect;
pub mod backup;
pub mod bundle;
pub mod cheats;
pub mod content_profile;
pub mod dump;
pub mod error;
pub mod fleet;
pub mod games;
pub mod host_env;
#[cfg(feature = "hub")]
pub mod hub;
pub mod language;
pub mod launch;
pub mod link;
pub mod logs;
pub mod mods;
pub mod patches;
pub mod pkg;
pub mod platform;
pub mod prepare;
pub mod profile;
pub mod recipe;
pub mod render_size;
pub mod run_env;
pub mod savecard;
pub mod saves;
pub mod sfo;
pub mod steam;
pub mod trophies;
pub mod tune;
pub mod update;
pub mod vdf;

pub use error::{Error, Result};
