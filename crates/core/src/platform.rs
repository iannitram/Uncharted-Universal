//! Which OS this is, and the folders the launcher and the runtime use there. Values come from
//! environment variables through a lookup function, so the rules are tested on every OS
//! without touching the real environment (docs/code-map.md, `platform`). Replaces the path
//! logic in `launcher/bbport_assets.py` and the launcher's data-folder code.
//!
//! `Os`, `Vars` and `AppDirs` are copied from iannitram/totk-native (MIT) and renamed.

use crate::error::{Error, Result};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// Create a new per-user configuration file without following or replacing an existing path.
/// Unix permissions are owner-only (0600); Windows inherits the app-data directory ACL.
pub fn create_private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

/// Whether a file's permissions are suitable for storing a pairing key.
pub fn private_file_permissions(path: &Path) -> Result<bool> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(metadata.permissions().mode() & 0o077 == 0)
    }
    #[cfg(not(unix))]
    {
        // The file lives below the per-user application configuration directory, whose ACL is
        // inherited from APPDATA/Application Support on these platforms.
        Ok(true)
    }
}

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
        fn required(var: &impl Fn(&str) -> Option<String>, name: &str) -> Result<PathBuf> {
            var(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .ok_or_else(|| Error::Env(format!("{name} is not set")))
        }

        match os {
            Os::Linux => {
                let home = required(&var, "HOME")?;
                let data = var("XDG_DATA_HOME")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".local/share"));
                let config = var("XDG_CONFIG_HOME")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".config"));
                Ok(Vars { home, data, config })
            }
            Os::Windows => Ok(Vars {
                home: required(&var, "USERPROFILE")?,
                data: required(&var, "LOCALAPPDATA")?,
                config: required(&var, "APPDATA")?,
            }),
            Os::Mac => {
                let home = required(&var, "HOME")?;
                let support = home.join("Library/Application Support");
                Ok(Vars {
                    home,
                    data: support.clone(),
                    config: support,
                })
            }
            Os::Ios | Os::Android => Err(Error::Unsupported(format!(
                "the app supplies its folders on {}",
                os.key()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Os, Vars};
    use crate::error::Error;
    use std::{collections::HashMap, path::PathBuf};

    fn vars(os: Os, values: &[(&str, &str)]) -> crate::error::Result<Vars> {
        let values: HashMap<_, _> = values
            .iter()
            .map(|(key, value)| (*key, (*value).to_owned()))
            .collect();
        Vars::from_lookup(os, |key| values.get(key).cloned())
    }

    #[test]
    fn linux_uses_xdg_overrides_and_home_fallbacks() {
        let overridden = vars(
            Os::Linux,
            &[
                ("HOME", "/home/player"),
                ("XDG_DATA_HOME", "/data"),
                ("XDG_CONFIG_HOME", "/config"),
            ],
        )
        .unwrap();
        assert_eq!(overridden.home, PathBuf::from("/home/player"));
        assert_eq!(overridden.data, PathBuf::from("/data"));
        assert_eq!(overridden.config, PathBuf::from("/config"));

        let fallback = vars(
            Os::Linux,
            &[("HOME", "/home/player"), ("XDG_DATA_HOME", "")],
        )
        .unwrap();
        assert_eq!(fallback.data, PathBuf::from("/home/player/.local/share"));
        assert_eq!(fallback.config, PathBuf::from("/home/player/.config"));
    }

    #[test]
    fn windows_requires_each_standard_folder_variable() {
        let folders = vars(
            Os::Windows,
            &[
                ("USERPROFILE", "C:\\Users\\player"),
                ("LOCALAPPDATA", "C:\\Users\\player\\AppData\\Local"),
                ("APPDATA", "C:\\Users\\player\\AppData\\Roaming"),
            ],
        )
        .unwrap();
        assert_eq!(folders.home, PathBuf::from("C:\\Users\\player"));
        assert_eq!(
            folders.data,
            PathBuf::from("C:\\Users\\player\\AppData\\Local")
        );
        assert_eq!(
            folders.config,
            PathBuf::from("C:\\Users\\player\\AppData\\Roaming")
        );
    }

    #[test]
    fn mac_uses_application_support_for_data_and_config() {
        let folders = vars(Os::Mac, &[("HOME", "/Users/player")]).unwrap();
        let support = PathBuf::from("/Users/player/Library/Application Support");
        assert_eq!(folders.home, PathBuf::from("/Users/player"));
        assert_eq!(folders.data, support);
        assert_eq!(folders.config, folders.data);
    }

    #[test]
    fn empty_required_values_are_reported_in_contract_order() {
        assert_eq!(
            vars(Os::Linux, &[("HOME", "")]).unwrap_err(),
            Error::Env("HOME is not set".into())
        );
        assert_eq!(
            vars(
                Os::Windows,
                &[("USERPROFILE", "C:\\Users\\player"), ("LOCALAPPDATA", "")]
            )
            .unwrap_err(),
            Error::Env("LOCALAPPDATA is not set".into())
        );
        assert_eq!(
            vars(
                Os::Windows,
                &[
                    ("USERPROFILE", "C:\\Users\\player"),
                    ("LOCALAPPDATA", "C:\\data"),
                    ("APPDATA", "")
                ]
            )
            .unwrap_err(),
            Error::Env("APPDATA is not set".into())
        );
    }

    #[test]
    fn unset_required_values_match_empty_ones_and_empty_xdg_overrides_fall_back() {
        assert_eq!(
            vars(Os::Linux, &[]).unwrap_err(),
            Error::Env("HOME is not set".into())
        );
        assert_eq!(
            vars(Os::Mac, &[]).unwrap_err(),
            Error::Env("HOME is not set".into())
        );
        assert_eq!(
            vars(
                Os::Windows,
                &[("LOCALAPPDATA", "C:\\data"), ("APPDATA", "C:\\roaming")]
            )
            .unwrap_err(),
            Error::Env("USERPROFILE is not set".into())
        );
        let fallback = vars(
            Os::Linux,
            &[
                ("HOME", "/home/player"),
                ("XDG_DATA_HOME", ""),
                ("XDG_CONFIG_HOME", ""),
            ],
        )
        .unwrap();
        assert_eq!(fallback.data, PathBuf::from("/home/player/.local/share"));
        assert_eq!(fallback.config, PathBuf::from("/home/player/.config"));
    }

    #[test]
    fn mobile_folders_are_supplied_without_reading_environment() {
        for os in [Os::Ios, Os::Android] {
            let result =
                Vars::from_lookup(os, |_| panic!("mobile lookup must not read environment"));
            assert_eq!(
                result.unwrap_err(),
                Error::Unsupported(format!("the app supplies its folders on {}", os.key()))
            );
        }
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

    /// `<config>/settings.json`: the launcher's own settings (`language::SETTING_KEY`).
    pub fn settings(&self) -> PathBuf {
        self.config.join("settings.json")
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
