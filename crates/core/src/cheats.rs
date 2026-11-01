//! Cheat patches ("health" and "ammo") for the three executables. A cheat is written only when
//! the executable's SHA-256 is one of the known builds AND the bytes at its address are the
//! expected ones; anything else is reported and skipped, never an error. Addresses are image
//! offsets. Replaces `scripts/cheats.py`.
//!
//! "Cheat file parse" in the code map: the cheat table itself (hash -> cheat -> address,
//! original bytes, replacement) is compiled into the crate; piece 1.12 moves it here from
//! `PATCHES` in `cheats.py`.

use crate::error::{not_built, Result};
use crate::games::Game;
use crate::patches::ByteWrite;

/// A cheat the launcher offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cheat {
    Health,
    Ammo,
}

/// Executable SHA-256 of the 01.00 builds, `V100` in `cheats.py` (hex).
pub const SHA256_V100: [(Game, &str); 3] = [
    (
        Game::U1,
        "8953b6ed5cf4d1d26aa3aa4cd1e0c72817ea155f6a5774fb17f6fec7d787f953",
    ),
    (
        Game::U2,
        "61c0456314a8de2dbd32e1c426f7828092de7d25a58b83386ced1a48d7765ebe",
    ),
    (
        Game::U3,
        "acc952e5a9ac7062f8ecfc52700faa599d5f29deb1e24f9e81e6de832326d1d0",
    ),
];

/// Executable SHA-256 of the 01.02 builds, `V102` in `cheats.py` (hex).
pub const SHA256_V102: [(Game, &str); 3] = [
    (
        Game::U1,
        "dc017b45da8978bb11a0a11d940e96d98fa07f3c066a3debba043714432212f8",
    ),
    (
        Game::U2,
        "5047b693b1c6fd06ccb1eacecacee60b6183bedc18d023cac276e1ec6009dfe2",
    ),
    (
        Game::U3,
        "02c111aa7e73e445d71a1ad861702811f31d471abc61caf01732d55a77eb1c38",
    ),
];

/// What compiling cheats gave.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Compiled {
    pub writes: Vec<ByteWrite>,
    /// One plain sentence per cheat that was skipped (unknown build, bytes differ).
    pub skipped: Vec<String>,
}

/// Compile the `enabled` cheats for `executable` (the file's bytes, hashed here).
pub fn compile(executable: &[u8], enabled: &[Cheat]) -> Result<Compiled> {
    not_built("cheats::compile")
}

/// The `cheats.bin` for the runtime: the writes in the same layout as `patches::to_patches_bin`.
pub fn to_cheats_bin(writes: &[ByteWrite]) -> Vec<u8> {
    not_built!("cheats::to_cheats_bin")
}
