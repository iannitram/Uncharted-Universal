//! The launcher's seven interface languages and the setting that picks one. Replaces
//! `LANGUAGES` and `language_code` in `launcher/uncharted_i18n.py`; the texts themselves are
//! the app's gettext catalogues (`app/lang/<code>/LC_MESSAGES/uncharted-app.po`), not this
//! crate's. The setting is `ui_language` in the launcher's `settings.json`, the key the Python
//! launcher used: "" is Automatic (the system locale, English when the launcher has no
//! translation for it), else a language code.

use crate::error::{Error, Result};
use std::path::Path;

/// The settings key, as in the Python launcher's `APP_DEFAULTS`.
pub const SETTING_KEY: &str = "ui_language";

/// One interface language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    English,
    Spanish,
    Portuguese,
    French,
    German,
    Italian,
    Japanese,
}

impl Language {
    /// Every language, in the order the launcher offers them.
    pub const ALL: [Language; 7] = [
        Language::English,
        Language::Spanish,
        Language::Portuguese,
        Language::French,
        Language::German,
        Language::Italian,
        Language::Japanese,
    ];

    /// The ISO 639-1 code; also the catalogue folder name under `app/lang/`.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Spanish => "es",
            Language::Portuguese => "pt",
            Language::French => "fr",
            Language::German => "de",
            Language::Italian => "it",
            Language::Japanese => "ja",
        }
    }

    /// The language's name in itself, as the launcher lists it (never translated).
    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Spanish => "Español",
            Language::Portuguese => "Português",
            Language::French => "Français",
            Language::German => "Deutsch",
            Language::Italian => "Italiano",
            Language::Japanese => "日本語",
        }
    }

    pub fn from_code(code: &str) -> Option<Language> {
        Language::ALL
            .into_iter()
            .find(|language| language.code() == code)
    }

    /// The language of a system locale ("es_ES.UTF-8", "pt-BR", "ja"), by its language part;
    /// `None` for a language the launcher has no texts for.
    pub fn from_locale(locale: &str) -> Option<Language> {
        let base = locale
            .split(['_', '-', '.', '@'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        Language::from_code(&base)
    }
}

/// The setting values the launcher offers, in order: "" (Automatic), then each language code.
pub fn setting_choices() -> Vec<&'static str> {
    std::iter::once("")
        .chain(Language::ALL.iter().map(|language| language.code()))
        .collect()
}

/// The choice after `setting` (wrapping), for a button that steps through the languages.
pub fn next_setting(setting: &str) -> &'static str {
    let choices = setting_choices();
    let index = choices
        .iter()
        .position(|choice| *choice == setting)
        .unwrap_or(0);
    choices[(index + 1) % choices.len()]
}

/// The language to show: the setting when it names one, else the system locale's language,
/// else English (`language_code` in `uncharted_i18n.py`).
pub fn resolve(setting: &str, system_locale: Option<&str>) -> Language {
    Language::from_code(setting)
        .or_else(|| system_locale.and_then(Language::from_locale))
        .unwrap_or(Language::English)
}

/// The saved setting, "" (Automatic) when the file is missing, unreadable or holds no known
/// language code.
pub fn read_setting(settings_file: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(settings_file) else {
        return String::new();
    };
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(&text)
    else {
        return String::new();
    };
    match map.get(SETTING_KEY).and_then(|value| value.as_str()) {
        Some(code) if Language::from_code(code).is_some() => code.to_owned(),
        _ => String::new(),
    }
}

/// Save the setting, keeping the file's other keys. `setting` is "" or a language code. The
/// file is written next to itself and renamed over, so a crash never leaves half a file.
pub fn write_setting(settings_file: &Path, setting: &str) -> Result<()> {
    if !setting.is_empty() && Language::from_code(setting).is_none() {
        return Err(Error::Unsupported(format!(
            "The launcher has no interface language \"{setting}\"."
        )));
    }
    let mut map = match std::fs::read_to_string(settings_file)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    map.insert(
        SETTING_KEY.to_owned(),
        serde_json::Value::String(setting.to_owned()),
    );
    if let Some(parent) = settings_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .expect("serializing a JSON map is infallible");
    let mut temporary = settings_file.as_os_str().to_owned();
    temporary.push(".tmp");
    std::fs::write(&temporary, text + "\n")?;
    std::fs::rename(&temporary, settings_file)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("uncharted-language-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn launcher_offers_the_seven_languages() {
        // `test_launcher_offers_selected_languages` in tests/test_uncharted_i18n.py.
        assert_eq!(
            setting_choices(),
            ["", "en", "es", "pt", "fr", "de", "it", "ja"]
        );
        let names: Vec<_> = Language::ALL.iter().map(|l| l.native_name()).collect();
        assert_eq!(
            names,
            [
                "English",
                "Español",
                "Português",
                "Français",
                "Deutsch",
                "Italiano",
                "日本語"
            ]
        );
    }

    #[test]
    fn automatic_language_uses_the_system_locale() {
        // `test_automatic_language_uses_windows_locale`: "<code>_XX" picks the code, an
        // unsupported locale (Korean) falls back to English.
        for code in ["es", "pt", "fr", "de", "it", "ja"] {
            let locale = format!("{code}_XX");
            assert_eq!(resolve("", Some(&locale)).code(), code);
        }
        assert_eq!(resolve("", Some("ko_KR")), Language::English);
        assert_eq!(resolve("", None), Language::English);
        assert_eq!(resolve("", Some("pt-BR")), Language::Portuguese);
        assert_eq!(resolve("", Some("de_DE.UTF-8")), Language::German);
        assert_eq!(resolve("", Some("FR")), Language::French);
        assert_eq!(resolve("", Some("")), Language::English);
    }

    #[test]
    fn a_chosen_language_wins_over_the_locale() {
        assert_eq!(resolve("ja", Some("de_DE")), Language::Japanese);
        assert_eq!(resolve("en", Some("es_ES")), Language::English);
        // An unknown saved value counts as Automatic.
        assert_eq!(resolve("ru", Some("it_IT")), Language::Italian);
    }

    #[test]
    fn next_setting_steps_through_every_choice_and_wraps() {
        let mut setting = "";
        let mut seen = vec![setting];
        for _ in 0..7 {
            setting = next_setting(setting);
            seen.push(setting);
        }
        assert_eq!(seen, setting_choices());
        assert_eq!(next_setting("ja"), "");
        assert_eq!(next_setting("unknown"), "en");
    }

    #[test]
    fn setting_round_trips_and_keeps_other_keys() {
        let dir = scratch("round-trip");
        let file = dir.join("settings.json");
        assert_eq!(read_setting(&file), "");
        write_setting(&file, "fr").unwrap();
        assert_eq!(read_setting(&file), "fr");

        std::fs::write(&file, r#"{"start": "u2", "ui_language": "de"}"#).unwrap();
        write_setting(&file, "ja").unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(saved["start"], "u2");
        assert_eq!(saved["ui_language"], "ja");
        write_setting(&file, "").unwrap();
        assert_eq!(read_setting(&file), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_settings_read_as_automatic_and_bad_codes_are_refused() {
        let dir = scratch("bad");
        let file = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        for text in [
            "not json",
            "[1, 2]",
            r#"{"ui_language": 3}"#,
            r#"{"ui_language": "ru"}"#,
        ] {
            std::fs::write(&file, text).unwrap();
            assert_eq!(read_setting(&file), "", "{text}");
        }
        let error = write_setting(&file, "ru").unwrap_err();
        assert_eq!(error.code(), "unsupported");
        // A damaged file is replaced, not kept.
        std::fs::write(&file, "not json").unwrap();
        write_setting(&file, "it").unwrap();
        assert_eq!(read_setting(&file), "it");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
