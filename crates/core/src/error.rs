//! The one error type. Every variant has a stable `code()` (the launcher's UI and the CLI's
//! `--json` match on it) and a plain message (`Display`) a player can read. Replaces the
//! scripts' `ValueError`, `PkgError`, `TrophyError` and `SystemExit(message)` calls. Built in
//! the skeleton: every module uses it.
//!
//! No message ever holds a key value or game data (SPEC.md M9).
//!
//! The shape follows iannitram/totk-native's `error.rs` (MIT).

use std::fmt;

/// The one error type; see the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A stub (skeleton only); the string names the function, `"<module>::<fn>"`.
    NotBuilt(&'static str),
    /// A folder the rules need is not set (an environment variable, the install folder).
    Env(String),
    /// Nothing is built or run this way on this device.
    Unsupported(String),
    /// The dump is not usable: missing files, still encrypted, damaged `param.sfo`.
    BadDump(String),
    /// The game's executable revision (`APP_VER`) is not one the port supports.
    WrongRevision {
        /// Which game, as `Game::key()`.
        game: String,
        found: String,
    },
    /// A `.pkg` container is damaged, encrypted, a delta patch, or unsafe to extract.
    Pkg(String),
    /// Preparing an install: the boot image, linked modules, the content profile.
    Prepare(String),
    /// A patch file or applying a patch failed.
    Patch(String),
    /// A mod folder has an unsupported layout or content.
    Mods(String),
    /// Cheats or trophies.
    Trophies(String),
    /// The render-size patch cannot fit the size asked for.
    RenderSize(String),
    /// The save folders: the save root, a backup, an update.
    Saves(String),
    /// Steam's files: shortcuts.vdf unreadable, no Steam user found, Steam still running.
    Steam(String),
    /// A build hub, its bundles, the fleet records or pairing (ADR 0004).
    Hub(String),
    /// Starting the game, or save-relay.
    Launch(String),
    /// A local file operation failed.
    Io(String),
}

/// `Result` with the crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// The stable code. Never rename one.
    pub fn code(&self) -> &'static str {
        match self {
            Error::NotBuilt(_) => "not_built",
            Error::Env(_) => "env",
            Error::Unsupported(_) => "unsupported",
            Error::BadDump(_) => "bad_dump",
            Error::WrongRevision { .. } => "wrong_revision",
            Error::Pkg(_) => "pkg",
            Error::Prepare(_) => "prepare",
            Error::Patch(_) => "patch",
            Error::Mods(_) => "mods",
            Error::Trophies(_) => "trophies",
            Error::RenderSize(_) => "render_size",
            Error::Saves(_) => "saves",
            Error::Steam(_) => "steam",
            Error::Hub(_) => "hub",
            Error::Launch(_) => "launch",
            Error::Io(_) => "io",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotBuilt(what) => write!(f, "not built yet: {what}"),
            Error::Env(m) => write!(f, "A folder is not set: {m}"),
            Error::Unsupported(m) => write!(f, "Not on this device: {m}"),
            Error::BadDump(m) => write!(f, "This dump is not usable: {m}"),
            Error::WrongRevision { game, found } => write!(
                f,
                "This copy of {game} is revision {found}, which the port does not support yet. \
                 Dump the revision the About page lists."
            ),
            Error::Pkg(m) => write!(f, "This .pkg cannot be unpacked: {m}"),
            Error::Prepare(m) => write!(f, "Preparing the game failed: {m}"),
            Error::Patch(m) => write!(f, "Patch: {m}"),
            Error::Mods(m) => write!(f, "Mods: {m}"),
            Error::Trophies(m) => write!(f, "Trophies: {m}"),
            Error::RenderSize(m) => write!(f, "Render size: {m}"),
            Error::Saves(m) => write!(f, "Saves: {m}"),
            Error::Steam(m) => write!(f, "Steam: {m}"),
            Error::Hub(m) => write!(f, "Build hub: {m}"),
            Error::Launch(m) => write!(f, "Starting the game failed: {m}"),
            Error::Io(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e.to_string())
    }
}

/// The body of a stub whose signature returns `Result`: `not_built("sfo::parse")`.
pub fn not_built<T>(what: &'static str) -> Result<T> {
    Err(Error::NotBuilt(what))
}
