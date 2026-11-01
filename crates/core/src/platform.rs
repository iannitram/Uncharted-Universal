//! Which OS this is, and the folders the launcher and the runtime use there. Values come from
//! environment variables through a lookup function, so the rules are tested on every OS
//! without touching the real environment (docs/code-map.md, `platform`). Replaces the path
//! logic in `launcher/bbport_assets.py` and the launcher's data-folder code.
//!
//! `Os`, `Vars` and `AppDirs` are copied from iannitram/totk-native (MIT) and renamed.

use crate::error::{not_built, Result};
use std::path::PathBuf;

/// A device family the launcher builds a port for (SPEC.md platforms).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Os {
    /// SteamOS on the AMD handheld and the Strix Halo tablet, and other Linux.
    Linux,
    /// The RTX 3090 Ti desktop.
    Windows,
    /// macOS on Apple silicon.
    Mac,
    /// iPhone and iPad (built on the Mac, installed by cable).
    Ios,
    /// Android arm64 with an Adreno GPU.
    Android,
}

impl Os {
    /// Every OS, in the order above.
    pub const ALL: [Os; 5] = [Os::Linux, Os::Windows, Os::Mac, Os::Ios, Os::Android];

    /// The OS this binary was built for.
    pub fn current() -> Os {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(target_os = "ios") {
            Os::Ios
        } else if cfg!(target_os = "android") {
            Os::Android
        } else {
            Os::Linux
        }
    }

    /// "linux", "windows", "mac", "ios", "android".
    pub fn key(self) -> &'static str {
        match self {
            Os::Linux => "linux",
            Os::Windows => "windows",
            Os::Mac => "mac",
            Os::Ios => "ios",
            Os::Android => "android",
        }
    }

    /// The inverse of [`Os::key`].
    pub fn from_key(key: &str) -> Option<Os> {
        Os::ALL.into_iter().find(|os| os.key() == key)
    }
}

/// The user's folders on a desktop OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vars {
    pub home: PathBuf,
    /// Per-user application data (XDG data home, `%LOCALAPPDATA%`, `~/Library/Application Support`).
    pub data: PathBuf,
    /// Per-user configuration (XDG config home, `%APPDATA%`, `~/Library/Application Support`).
    pub config: PathBuf,
}

impl Vars {
    /// Resolve the folders for `os` from environment variables read through `var`
    /// (pass `|k| std::env::var(k).ok()` for the real environment). An empty value counts as
    /// unset.
    ///
    /// - Linux: home `HOME`; data `XDG_DATA_HOME`, else `<home>/.local/share`; config
    ///   `XDG_CONFIG_HOME`, else `<home>/.config`.
    /// - Windows: home `USERPROFILE`; data `LOCALAPPDATA`; config `APPDATA`.
    /// - Mac: home `HOME`; data and config both `<home>/Library/Application Support`.
    /// - Ios, Android: `Error::Unsupported("the app supplies its folders on <key>")`.
    ///
    /// A missing required variable is `Error::Env("<NAME> is not set")`, first one missing
    /// in the order above.
    pub fn from_lookup(os: Os, var: impl Fn(&str) -> Option<String>) -> Result<Vars> {
        not_built("platform::Vars::from_lookup")
    }
}

/// The launcher's own folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    /// `<config>/uncharted-universal`: settings, `peers.json`, `mods.json`, `patches.json`.
    pub config: PathBuf,
    /// `<data>/uncharted-universal`: installs, backups, logs.
    pub data: PathBuf,
}

impl AppDirs {
    pub fn new(vars: &Vars) -> AppDirs {
        AppDirs {
            config: vars.config.join("uncharted-universal"),
            data: vars.data.join("uncharted-universal"),
        }
    }

    /// `<data>/installs/<game key>`: one prepared install (`Game::key`).
    pub fn install(&self, game_key: &str) -> PathBuf {
        self.data.join("installs").join(game_key)
    }

    /// `<data>/backups`: save backups before an update (SPEC.md M19).
    pub fn backups(&self) -> PathBuf {
        self.data.join("backups")
    }

    /// `<data>/logs`: `launcher.log`, `playthrough.jsonl`.
    pub fn logs(&self) -> PathBuf {
        self.data.join("logs")
    }
}
