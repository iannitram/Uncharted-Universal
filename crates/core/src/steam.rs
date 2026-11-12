//! The Steam shortcut that opens the launcher from Game Mode (SPEC.md M13): a non-Steam game
//! entry in `shortcuts.vdf` whose target is `uncharted-universal play`, so save sync wraps every
//! start, plus the artwork files beside it. Steam rewrites the file when it quits, so the
//! launcher writes it only while Steam is closed. Replaces `launcher/bbport_steam.py`
//! (`shortcut_id`, `entry`, `add_shortcut`, `steam_folder`, `user_configs`, `steam_running`;
//! the Windows registry lookup becomes `userdata_roots`) and the file writing of
//! `launcher/uncharted_art.py`. Copied from iannitram/totk-native's `steam.rs` (MIT).

use crate::artwork::{self, Art};
use crate::error::{Error, Result};
use crate::platform::{Os, Vars};
use crate::vdf::{self, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// CRC-32 (IEEE 802.3, reflected, polynomial 0xEDB88320, initial and final XOR 0xFFFFFFFF):
/// `crc32(b"123456789") == 0xCBF43926`.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// The shortcut's app id: `crc32(exe + name) | 0x8000_0000`, with `exe` exactly as stored in the
/// shortcut (quoted). Same as `shortcut_id` in `bbport_steam.py`.
pub fn shortcut_app_id(exe: &str, name: &str) -> u32 {
    crc32(format!("{exe}{name}").as_bytes()) | 0x8000_0000
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
    Value::Map(entry_fields(s))
}

/// The items of [`shortcut_entry`].
fn entry_fields(s: &Shortcut) -> Vec<(String, Value)> {
    let string = |value: &str| Value::Str(value.to_string());
    vec![
        ("appid".to_string(), Value::Int(s.app_id())),
        ("AppName".to_string(), string(&s.name)),
        ("Exe".to_string(), string(&s.quoted_exe())),
        (
            "StartDir".to_string(),
            string(&format!("\"{}\"", s.start_dir)),
        ),
        ("icon".to_string(), string(&s.icon)),
        ("ShortcutPath".to_string(), string("")),
        ("LaunchOptions".to_string(), string(&s.launch_options)),
        ("IsHidden".to_string(), Value::Int(0)),
        ("AllowDesktopConfig".to_string(), Value::Int(1)),
        ("AllowOverlay".to_string(), Value::Int(1)),
        ("OpenVR".to_string(), Value::Int(0)),
        ("Devkit".to_string(), Value::Int(0)),
        ("DevkitGameID".to_string(), string("")),
        ("DevkitOverrideAppID".to_string(), Value::Int(0)),
        ("LastPlayTime".to_string(), Value::Int(0)),
        ("FlatpakAppID".to_string(), string("")),
        ("tags".to_string(), Value::Map(vec![])),
    ]
}

/// What Steam or the player adds to an entry, kept when the launcher replaces it (as
/// `add_shortcut` in `bbport_steam.py` keeps them).
const KEPT: [&str; 2] = ["LastPlayTime", "tags"];

/// Add `s` to a shortcuts.vdf document, or replace the entry with the same "appid", and return its
/// app id. A replaced entry keeps its "LastPlayTime" and "tags" (play time and the player's
/// collections, as `add_shortcut` keeps them). An empty document gets the "shortcuts" map
/// first. Entries keep their order (a new one goes last) and are renumbered "0", "1", ... A
/// document whose first item is not the map "shortcuts" is
/// `Error::Steam("shortcuts.vdf has no shortcuts list")`.
pub fn upsert(doc: &mut Vec<(String, Value)>, s: &Shortcut) -> Result<u32> {
    if doc.is_empty() {
        doc.push(("shortcuts".to_string(), Value::Map(vec![])));
    }
    let entries = match doc.first_mut() {
        Some((key, Value::Map(entries))) if key == "shortcuts" => entries,
        _ => {
            return Err(Error::Steam(
                "shortcuts.vdf has no shortcuts list".to_string(),
            ))
        }
    };

    let id = s.app_id();
    let mut fields = entry_fields(s);
    let same_id = |value: &Value| match value {
        Value::Map(old) => old
            .iter()
            .any(|(key, value)| key == "appid" && *value == Value::Int(id)),
        _ => false,
    };
    match entries.iter_mut().find(|(_, value)| same_id(value)) {
        Some((_, old)) => {
            if let Value::Map(old_fields) = old {
                for (key, value) in fields.iter_mut() {
                    if !KEPT.contains(&key.as_str()) {
                        continue;
                    }
                    if let Some((_, kept)) = old_fields.iter().find(|(k, _)| k == key) {
                        *value = kept.clone();
                    }
                }
            }
            *old = Value::Map(fields);
        }
        None => entries.push((String::new(), Value::Map(fields))),
    }
    for (index, (key, _)) in entries.iter_mut().enumerate() {
        *key = index.to_string();
    }
    Ok(id)
}

/// Where Steam keeps its user folders on the OS this is built for, in the order to try: Linux
/// `<home>/.local/share/Steam/userdata`, `<home>/.steam/steam/userdata`, and the Flatpak
/// `<home>/.var/app/com.valvesoftware.Steam/.local/share/Steam/userdata`; Windows
/// `C:\Program Files (x86)\Steam\userdata`; Mac `<home>/Library/Application Support/Steam/userdata`.
pub fn userdata_roots(vars: &Vars) -> Vec<PathBuf> {
    userdata_roots_for(Os::current(), vars)
}

/// [`userdata_roots`] for `os`, so every OS's list is tested on any OS. iOS and Android have no
/// Steam: an empty list.
pub fn userdata_roots_for(os: Os, vars: &Vars) -> Vec<PathBuf> {
    let home = &vars.home;
    match os {
        Os::Linux => vec![
            home.join(".local/share/Steam/userdata"),
            home.join(".steam/steam/userdata"),
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam/userdata"),
        ],
        Os::Windows => vec![PathBuf::from(r"C:\Program Files (x86)\Steam\userdata")],
        Os::Mac => vec![home.join("Library/Application Support/Steam/userdata")],
        Os::Ios | Os::Android => vec![],
    }
}

/// The Steam user folders under `userdata`: `<id>` folders whose name is all digits, except
/// `0` (not a real account), sorted by name. As `user_configs` in `bbport_steam.py`.
fn user_dirs(userdata: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(userdata)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let is_user = !name.is_empty() && name != "0" && name.bytes().all(|b| b.is_ascii_digit());
        if is_user && entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// Write the shortcut into every Steam user's `config/shortcuts.vdf` under `userdata` (keeping a
/// `.bak` of the old file), only when Steam is not running (`steam_running`); else
/// `Error::Steam("close Steam first")`. Returns the files written.
///
/// Every user's file is read and updated before any is written, so a damaged shortcuts.vdf
/// (any `vdf::read` or `upsert` error) leaves all of them as they were. A user without the file
/// gets a new one. A missing `userdata` is `Error::Steam("no Steam user folder at <userdata>")`;
/// a `userdata` with no user folder in it writes nothing and returns an empty list.
pub fn write_shortcut(
    userdata: &Path,
    s: &Shortcut,
    steam_running: &dyn Fn() -> bool,
) -> Result<Vec<PathBuf>> {
    if steam_running() {
        return Err(Error::Steam("close Steam first".to_string()));
    }
    if !userdata.is_dir() {
        return Err(Error::Steam(format!(
            "no Steam user folder at {}",
            userdata.display()
        )));
    }

    let mut planned = Vec::new();
    for user in user_dirs(userdata)? {
        let path = user.join("config").join("shortcuts.vdf");
        let old = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let mut doc = match &old {
            Some(bytes) => vdf::read(bytes)?,
            None => Vec::new(),
        };
        upsert(&mut doc, s)?;
        planned.push((path, old, vdf::write(&doc)));
    }

    let mut written = Vec::new();
    for (path, old, new) in planned {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(old) = old {
            fs::write(path.with_file_name("shortcuts.vdf.bak"), old)?;
        }
        // Write beside it, then rename over it, so a failed write never leaves half a file.
        let tmp = path.with_file_name("shortcuts.vdf.tmp");
        fs::write(&tmp, new)?;
        fs::rename(&tmp, &path)?;
        written.push(path);
    }
    Ok(written)
}

/// Write the library images for shortcut `app_id` into one Steam user folder's `config/grid`
/// (`artwork::paths`), creating it. `images` are PNG bytes already sized by `artwork`. Returns
/// the files written, in `images` order.
pub fn write_artwork(
    user_dir: &Path,
    app_id: u32,
    images: &[(Art, Vec<u8>)],
) -> Result<Vec<PathBuf>> {
    let paths = artwork::paths(user_dir, app_id);
    let mut written = Vec::new();
    for (art, png) in images {
        let Some((_, path)) = paths.iter().find(|(a, _)| a == art) else {
            continue; // artwork::paths lists every Art
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, png)?;
        written.push(path.clone());
    }
    Ok(written)
}

#[cfg(test)]
mod tests;
