//! The player's own `BB_*` settings, as far as the after-crash screen needs them: a known
//! issue's toggle is saved here and every game the app starts gets these variables, which win
//! over the defaults (`run_env::build_env`, `Settings::extra`). The file is
//! `<config>/uncharted-universal/settings.json`, an object whose `extra` object maps a variable
//! name to its value; other keys are kept as they are. Piece 2.06 (Saves and Settings) adds
//! the rest of the settings page to the same file.

use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

/// The file's name in the launcher's config folder (`AppDirs::config`).
pub const SETTINGS_FILE: &str = "settings.json";

/// The whole file as JSON; a missing, unreadable or damaged file (or one that is not an
/// object) is `Null`.
pub fn read(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or(Value::Null)
}

/// The `extra` variables: each text value of the `extra` object (other values are skipped).
pub fn extra(path: &Path) -> BTreeMap<String, String> {
    read(path)
        .get("extra")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Set `extra.<key>` to `value`, keeping everything else in the file, and write the file
/// whole (creating its folder). A damaged file is replaced by one with only this setting.
pub fn set_extra(path: &Path, key: &str, value: &str) -> io::Result<()> {
    let mut root = match read(path) {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let extra = root
        .entry("extra")
        .or_insert_with(|| Value::Object(Map::new()));
    if !extra.is_object() {
        *extra = Value::Object(Map::new());
    }
    if let Value::Object(map) = extra {
        map.insert(key.to_string(), Value::String(value.to_string()));
    }
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(root)).map_err(io::Error::other)?;
    fs::write(path, text + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("uu-app-settings-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn set_extra_keeps_the_other_settings() {
        let dir = temp("keep");
        let path = dir.join("config").join(SETTINGS_FILE);
        assert_eq!(extra(&path), BTreeMap::new());
        set_extra(&path, "BB_A", "1").unwrap();
        fs::write(
            &path,
            r#"{"language": "de", "extra": {"BB_A": "1", "BB_N": 5}}"#,
        )
        .unwrap();
        set_extra(&path, "BB_B", "2").unwrap();
        let got = read(&path);
        assert_eq!(got["language"], "de");
        assert_eq!(got["extra"]["BB_N"], 5);
        let want: BTreeMap<String, String> = [("BB_A", "1"), ("BB_B", "2")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(extra(&path), want);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_damaged_file_is_replaced() {
        let dir = temp("damaged");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(SETTINGS_FILE);
        fs::write(&path, "{not json").unwrap();
        assert_eq!(read(&path), Value::Null);
        set_extra(&path, "BB_X", "on").unwrap();
        assert_eq!(extra(&path).get("BB_X").map(String::as_str), Some("on"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
