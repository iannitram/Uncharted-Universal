//! The Settings page's values in `<config>/settings.json` (roadmap 2.06): one JSON object, the
//! display keys at the top (`device_profile`, `frame_rate`, `render_resolution`,
//! `anti_aliasing`, `upscale_preset`, `frame_generation`, the Python launcher's names). Every
//! other key in the file, the `extra` object of `BB_*` variables among them, is kept as it was:
//! this page owns only its own keys.

use crate::vm::settings::Stored;
use serde_json::{Map, Value};
use std::fs;
use std::io;
use std::path::Path;

/// The file in the config folder.
pub const FILE: &str = "settings.json";

/// The file as a JSON object; missing, unreadable or not an object is empty.
fn read(config: &Path) -> Map<String, Value> {
    fs::read_to_string(config.join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

/// The stored values; a key that is missing or not a string is empty (the device default).
pub fn load(config: &Path) -> Stored {
    let map = read(config);
    let get = |key: &str| {
        map.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    Stored {
        device_profile: get("device_profile"),
        frame_rate: get("frame_rate"),
        render_resolution: get("render_resolution"),
        anti_aliasing: get("anti_aliasing"),
        upscale_preset: get("upscale_preset"),
        frame_generation: get("frame_generation"),
    }
}

/// Write the values, keeping every other key of the file. Written to a temporary file first,
/// then renamed over the old one, so a crash never leaves half a file.
pub fn save(config: &Path, stored: &Stored) -> io::Result<()> {
    let mut map = read(config);
    for (key, value) in [
        ("device_profile", &stored.device_profile),
        ("frame_rate", &stored.frame_rate),
        ("render_resolution", &stored.render_resolution),
        ("anti_aliasing", &stored.anti_aliasing),
        ("upscale_preset", &stored.upscale_preset),
        ("frame_generation", &stored.frame_generation),
    ] {
        map.insert(key.to_owned(), Value::from(value.as_str()));
    }
    fs::create_dir_all(config)?;
    let text = serde_json::to_string_pretty(&Value::Object(map)).map_err(io::Error::other)?;
    let temp = config.join("settings.json.tmp");
    fs::write(&temp, text)?;
    fs::rename(&temp, config.join(FILE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::tests::Temp;

    #[test]
    fn a_missing_file_is_all_defaults() {
        let temp = Temp::new();
        assert_eq!(load(&temp.0), Stored::default());
    }

    #[test]
    fn save_then_load_keeps_other_keys() {
        let temp = Temp::new();
        fs::write(
            temp.0.join(FILE),
            r#"{"close_on_play": true, "extra": {"BB_TOGGLE2": "12"}, "frame_rate": 30}"#,
        )
        .unwrap();
        // A number where a string belongs is not set.
        assert_eq!(load(&temp.0).frame_rate, "");
        let stored = Stored {
            device_profile: "auto".into(),
            frame_rate: "60".into(),
            render_resolution: "2560x1440".into(),
            anti_aliasing: "fsr3".into(),
            upscale_preset: "balanced".into(),
            frame_generation: "off".into(),
        };
        save(&temp.0, &stored).unwrap();
        assert_eq!(load(&temp.0), stored);
        let text = fs::read_to_string(temp.0.join(FILE)).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["close_on_play"], true);
        assert_eq!(value["extra"]["BB_TOGGLE2"], "12");
        assert!(!temp.0.join("settings.json.tmp").exists());
    }

    #[test]
    fn a_damaged_file_is_replaced_by_the_values() {
        let temp = Temp::new();
        fs::write(temp.0.join(FILE), "not json").unwrap();
        assert_eq!(load(&temp.0), Stored::default());
        let stored = Stored {
            frame_rate: "30".into(),
            ..Stored::default()
        };
        save(&temp.0, &stored).unwrap();
        assert_eq!(load(&temp.0).frame_rate, "30");
    }
}
