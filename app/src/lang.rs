//! The interface texts in seven languages. English is the source: every text is written in
//! English in `ui/app.slint` (`@tr("...")`) or in Rust (`texts.tr("...")`), and each other
//! language has one gettext catalogue, `lang/<code>/LC_MESSAGES/uncharted-app.po`. Slint bundles
//! the catalogues for its own texts (build.rs); this module reads the same files for the texts
//! Rust builds (licence uses, patch states), so there is one catalogue per language. Replaces
//! `launcher/uncharted_i18n.py` and `uncharted_i18n_extra.py`; the tests replace
//! `tests/test_uncharted_i18n.py`.

use std::collections::HashMap;
use uncharted_core::language::Language;

/// The catalogue of every language but English, embedded at build time.
fn catalogue(language: Language) -> Option<&'static str> {
    Some(match language {
        Language::English => return None,
        Language::Spanish => include_str!("../lang/es/LC_MESSAGES/uncharted-app.po"),
        Language::Portuguese => include_str!("../lang/pt/LC_MESSAGES/uncharted-app.po"),
        Language::French => include_str!("../lang/fr/LC_MESSAGES/uncharted-app.po"),
        Language::German => include_str!("../lang/de/LC_MESSAGES/uncharted-app.po"),
        Language::Italian => include_str!("../lang/it/LC_MESSAGES/uncharted-app.po"),
        Language::Japanese => include_str!("../lang/ja/LC_MESSAGES/uncharted-app.po"),
    })
}

/// The texts of one language. A text with no translation shows in English.
#[derive(Debug)]
pub struct Texts {
    language: Language,
    table: HashMap<String, String>,
}

impl Texts {
    pub fn new(language: Language) -> Texts {
        let table = catalogue(language)
            .map(|po| {
                parse_po(po)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|(_, translated)| !translated.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Texts { language, table }
    }

    pub fn language(&self) -> Language {
        self.language
    }

    /// `english` in this language.
    pub fn tr(&self, english: &str) -> String {
        self.table
            .get(english)
            .cloned()
            .unwrap_or_else(|| english.to_owned())
    }

    /// `english` in this language, with its `{}` replaced by `value`.
    pub fn tr1(&self, english: &str, value: &str) -> String {
        self.tr(english).replacen("{}", value, 1)
    }
}

/// The entries of a `.po` file as (msgid, msgstr), without the header entry. Only what our
/// catalogues use: `#` comments, `msgid`/`msgstr` with continuation lines, and the escapes
/// `\"`, `\\`, `\n` and `\t`.
pub fn parse_po(text: &str) -> Result<Vec<(String, String)>, String> {
    enum Field {
        None,
        Id,
        Str,
    }
    let mut entries = Vec::new();
    let (mut id, mut translated, mut field) = (String::new(), String::new(), Field::None);
    let mut finish = |id: &mut String, translated: &mut String| {
        if !id.is_empty() {
            entries.push((std::mem::take(id), std::mem::take(translated)));
        }
        id.clear();
        translated.clear();
    };
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        let error = |what: &str| format!("line {}: {what}", number + 1);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let rest = if let Some(rest) = line.strip_prefix("msgid ") {
            finish(&mut id, &mut translated);
            field = Field::Id;
            rest
        } else if let Some(rest) = line.strip_prefix("msgstr ") {
            field = Field::Str;
            rest
        } else if line.starts_with('"') {
            line
        } else {
            return Err(error("not a msgid, msgstr or string line"));
        };
        let value = unquote(rest).ok_or_else(|| error("bad string"))?;
        match field {
            Field::Id => id.push_str(&value),
            Field::Str => translated.push_str(&value),
            Field::None => return Err(error("string before any msgid")),
        }
    }
    finish(&mut id, &mut translated);
    Ok(entries)
}

/// The text of one `"..."` string with its escapes resolved; `None` when it is not one string.
fn unquote(quoted: &str) -> Option<String> {
    let inner = quoted.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(match chars.next()? {
                'n' => '\n',
                't' => '\t',
                other @ ('"' | '\\') => other,
                _ => return None,
            }),
            '"' => return None,
            c => out.push(c),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    fn app_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// The string literals after each `marker` in `source` (`@tr("`, `.tr("`).
    fn literals_after(source: &str, marker: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut rest = source;
        while let Some(start) = rest.find(marker) {
            rest = &rest[start + marker.len() - 1..];
            let mut end = 1;
            let bytes = rest.as_bytes();
            while bytes[end] != b'"' {
                end += if bytes[end] == b'\\' { 2 } else { 1 };
            }
            found.push(unquote(&rest[..=end]).expect("a plain string literal"));
            rest = &rest[end + 1..];
        }
        found
    }

    fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_sources(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    /// Every text the app shows: the `.slint` file's `@tr` texts and Rust's `tr`/`tr1` texts.
    fn used_texts() -> BTreeSet<String> {
        let slint = std::fs::read_to_string(app_dir().join("ui/app.slint")).unwrap();
        let mut texts: BTreeSet<String> = literals_after(&slint, "@tr(\"").into_iter().collect();
        let mut files = Vec::new();
        rust_sources(&app_dir().join("src"), &mut files);
        for file in files {
            if file.ends_with("lang.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&file).unwrap();
            texts.extend(literals_after(&source, ".tr(\""));
            texts.extend(literals_after(&source, ".tr1(\""));
        }
        // The licence uses are data, translated with `texts.tr(item.role)`.
        let about = &crate::vm::about::SHIPPED;
        let learned = &crate::vm::about::LEARNED_FROM;
        texts.extend(
            about
                .iter()
                .chain(learned.iter())
                .map(|item| item.role.to_owned()),
        );
        texts
    }

    fn translated() -> impl Iterator<Item = Language> {
        Language::ALL
            .into_iter()
            .filter(|language| *language != Language::English)
    }

    #[test]
    fn every_text_is_translated_in_every_language() {
        // `test_each_translation_covers_every_launcher_text`, plus the reverse: a catalogue
        // holds no text the app no longer shows, and placeholders match (`{}` counts).
        let used = used_texts();
        assert!(used.len() > 20, "found only {} texts", used.len());
        for language in translated() {
            let entries = parse_po(catalogue(language).unwrap()).unwrap();
            let ids: BTreeSet<String> = entries.iter().map(|(id, _)| id.clone()).collect();
            assert_eq!(ids.len(), entries.len(), "{language:?}: duplicate msgid");
            let missing: Vec<_> = used.difference(&ids).collect();
            let stale: Vec<_> = ids.difference(&used).collect();
            assert!(missing.is_empty(), "{language:?} lacks {missing:?}");
            assert!(stale.is_empty(), "{language:?} has unused {stale:?}");
            for (id, text) in &entries {
                assert!(!text.trim().is_empty(), "{language:?}: {id:?} is empty");
                assert_eq!(
                    text.matches("{}").count(),
                    id.matches("{}").count(),
                    "{language:?}: {id:?}"
                );
            }
        }
    }

    #[test]
    fn catalogues_declare_utf8_and_their_language() {
        for language in translated() {
            let po = catalogue(language).unwrap();
            assert!(po.contains("charset=UTF-8"), "{language:?}");
            assert!(
                po.contains(&format!("\"Language: {}\\n\"", language.code())),
                "{language:?}"
            );
        }
    }

    #[test]
    fn the_slint_file_shows_no_untranslated_text() {
        let slint = std::fs::read_to_string(app_dir().join("ui/app.slint")).unwrap();
        // The Continue screen (2.04) in `AppWindow` stays English until its texts are in the
        // catalogues (#379, owner ruling 2026-11-12). Only these literals there are let through;
        // any other literal, and any literal before `AppWindow`, needs @tr.
        const CONTINUE_ENGLISH: &str = "|LB|RB|B  Quit|Quit the launcher?|Quit|Stay|B  Back|Back|\
            A  Continue this save|Newer|A  Keep this save|B  Decide later|Play anyway";
        let window = slint
            .lines()
            .position(|l| l.starts_with("export component AppWindow"));
        for (number, line) in slint.lines().enumerate() {
            let in_window = number >= window.unwrap();
            let code = line.split("//").next().unwrap();
            for property in ["text: \"", "label: \""] {
                let Some(at) = code.find(property) else {
                    continue;
                };
                let literal = code[at + property.len()..].split('"').next().unwrap();
                assert!(
                    in_window && CONTINUE_ENGLISH.split('|').any(|t| t == literal),
                    "ui/app.slint line {}: {property}{literal}\" needs @tr",
                    number + 1
                );
            }
        }
    }

    #[test]
    fn each_language_translates_back() {
        // `test_each_language_translates_play`, for a text this app has.
        let expected = [
            (Language::Spanish, "Volver"),
            (Language::Portuguese, "Voltar"),
            (Language::French, "Retour"),
            (Language::German, "Zurück"),
            (Language::Italian, "Indietro"),
            (Language::Japanese, "戻る"),
        ];
        for (language, back) in expected {
            assert_eq!(Texts::new(language).tr("Back"), back, "{language:?}");
        }
    }

    #[test]
    fn missing_text_falls_back_to_english() {
        assert_eq!(
            Texts::new(Language::Spanish).tr("Untranslated text"),
            "Untranslated text"
        );
        assert_eq!(Texts::new(Language::English).tr("Back"), "Back");
        assert_eq!(
            Texts::new(Language::English).tr1("Automatic ({})", "English"),
            "Automatic (English)"
        );
        assert_eq!(
            Texts::new(Language::German).tr1("Automatic ({})", "Deutsch"),
            "Automatisch (Deutsch)"
        );
    }

    #[test]
    fn long_texts_are_really_translated() {
        // `test_interface_language_help_is_translated`.
        let text = "No patches yet: the patches of a prepared game are listed here.";
        for language in translated() {
            assert_ne!(Texts::new(language).tr(text), text, "{language:?}");
        }
    }

    #[test]
    fn po_parser_reads_continuations_and_escapes() {
        let po = "# comment\nmsgid \"\"\nmsgstr \"Language: es\\n\"\n\n\
                  msgid \"Say \\\"hi\\\"\"\nmsgstr \"\"\n\"Di \\\"hola\\\"\"\n";
        assert_eq!(
            parse_po(po).unwrap(),
            [("Say \"hi\"".to_owned(), "Di \"hola\"".to_owned())]
        );
        assert!(parse_po("msgid \"open").is_err());
        assert!(parse_po("bogus").is_err());
    }
}
