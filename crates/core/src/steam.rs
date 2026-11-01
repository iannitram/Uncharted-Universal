//! The Steam shortcut that opens the launcher from Game Mode (SPEC.md M13): a non-Steam game
//! entry in `shortcuts.vdf` whose target is `uncharted-universal play`, so save sync wraps every
//! start, plus the artwork files beside it. Steam rewrites the file when it quits, so the
//! launcher writes it only while Steam is closed. Replaces `launcher/bbport_steam.py`
//! (`shortcut_id`, `entry`, `add_shortcut`, `steam_folder`, `user_configs`, `steam_running`;
//! the Windows registry lookup becomes `userdata_roots`) and the file writing of
//! `launcher/uncharted_art.py`. Copied from iannitram/totk-native's `steam.rs` (MIT).

use crate::artwork::Art;
use crate::error::{not_built, Result};
use crate::platform::Vars;
use crate::vdf::Value;
use std::path::{Path, PathBuf};

/// CRC-32 (IEEE 802.3, reflected, polynomial 0xEDB88320, initial and final XOR 0xFFFFFFFF):
/// `crc32(b"123456789") == 0xCBF43926`.
pub fn crc32(bytes: &[u8]) -> u32 {
    not_built!("steam::crc32")
}

/// The shortcut's app id: `crc32(exe + name) | 0x8000_0000`, with `exe` exactly as stored in the
/// shortcut (quoted). Same as `shortcut_id` in `bbport_steam.py`.
pub fn shortcut_app_id(exe: &str, name: &str) -> u32 {
    not_built!("steam::shortcut_app_id")
}

/// A shortcut as the launcher writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub name: String,
    /// The program, unquoted.
    pub exe: String,
    pub start_dir: String,
    pub launch_options: String,
    pub icon: String,
}

impl Shortcut {
    /// `exe` in double quotes, as Steam stores it.
    pub fn quoted_exe(&self) -> String {
        format!("\"{}\"", self.exe)
    }

    /// `shortcut_app_id(&self.quoted_exe(), &self.name)`.
    pub fn app_id(&self) -> u32 {
        shortcut_app_id(&self.quoted_exe(), &self.name)
    }
}

/// The shortcut's map, items in this order (the order of `entry` in `bbport_steam.py`): "appid"
/// Int(app_id), "AppName", "Exe" (quoted), "StartDir" (quoted), "icon", "ShortcutPath" (""),
/// "LaunchOptions", "IsHidden" 0, "AllowDesktopConfig" 1, "AllowOverlay" 1, "OpenVR" 0,
/// "Devkit" 0, "DevkitGameID" "", "DevkitOverrideAppID" 0, "LastPlayTime" 0, "FlatpakAppID" "",
/// "tags" (empty map).
pub fn shortcut_entry(s: &Shortcut) -> Value {
    not_built!("steam::shortcut_entry")
}

/// Add `s` to a shortcuts.vdf document, or replace the entry with the same "appid", and return its
/// app id. An empty document gets the "shortcuts" map first. Entries keep their order (a new one
/// goes last) and are renumbered "0", "1", ... A document whose first item is not the map
/// "shortcuts" is `Error::Steam("shortcuts.vdf has no shortcuts list")`.
// Skeleton: the stub never grows the Vec, so clippy asks for a slice; the real one pushes.
#[allow(clippy::ptr_arg)]
pub fn upsert(doc: &mut Vec<(String, Value)>, s: &Shortcut) -> Result<u32> {
    not_built("steam::upsert")
}

/// Where Steam keeps its user folders on the OS this is built for, in the order to try: Linux
/// `<home>/.local/share/Steam/userdata`, `<home>/.steam/steam/userdata`, and the Flatpak
/// `<home>/.var/app/com.valvesoftware.Steam/.local/share/Steam/userdata`; Windows
/// `C:\Program Files (x86)\Steam\userdata`; Mac `<home>/Library/Application Support/Steam/userdata`.
pub fn userdata_roots(vars: &Vars) -> Vec<PathBuf> {
    not_built!("steam::userdata_roots")
}

/// Write the shortcut into every Steam user's `config/shortcuts.vdf` under `userdata` (keeping a
/// `.bak` of the old file), only when Steam is not running (`steam_running`); else
/// `Error::Steam("close Steam first")`. Returns the files written.
pub fn write_shortcut(
    userdata: &Path,
    s: &Shortcut,
    steam_running: &dyn Fn() -> bool,
) -> Result<Vec<PathBuf>> {
    not_built("steam::write_shortcut")
}

/// Write the library images for shortcut `app_id` into one Steam user folder's `config/grid`
/// (`artwork::paths`), creating it. `images` are PNG bytes already sized by `artwork`. Returns
/// the files written.
pub fn write_artwork(
    user_dir: &Path,
    app_id: u32,
    images: &[(Art, Vec<u8>)],
) -> Result<Vec<PathBuf>> {
    not_built("steam::write_artwork")
}
