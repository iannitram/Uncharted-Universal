//! The app's texts in the seven languages of the Python launcher (`LANGUAGES` in
//! `launcher/uncharted_i18n.py`), English first. Only the after-crash screen's texts live here
//! so far; piece 2.09 moves every page's texts to Slint's translation files. `{}` in a text is
//! filled with [`fill`].

/// A language the app speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Es,
    Pt,
    Fr,
    De,
    It,
    Ja,
}

impl Lang {
    /// Every language, English first (for the tests).
    #[cfg(test)]
    pub const ALL: [Lang; 7] = [
        Lang::En,
        Lang::Es,
        Lang::Pt,
        Lang::Fr,
        Lang::De,
        Lang::It,
        Lang::Ja,
    ];

    /// The language of a locale name such as `de_DE.UTF-8`, `pt-BR` or `ja`; English for one
    /// the app does not speak.
    pub fn from_locale(locale: &str) -> Lang {
        let code: String = locale
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_ascii_lowercase();
        match code.as_str() {
            "es" => Lang::Es,
            "pt" => Lang::Pt,
            "fr" => Lang::Fr,
            "de" => Lang::De,
            "it" => Lang::It,
            "ja" => Lang::Ja,
            _ => Lang::En,
        }
    }

    /// The language from the first set one of `UU_LANG`, `LC_ALL`, `LC_MESSAGES`, `LANG`
    /// (read through `var`); English when none is set.
    pub fn detect(var: impl Fn(&str) -> Option<String>) -> Lang {
        ["UU_LANG", "LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(var)
            .find(|value| !value.is_empty())
            .map_or(Lang::En, |value| Lang::from_locale(&value))
    }
}

/// One text of the after-crash screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Text {
    Heading,
    Chapter,
    NoChapter,
    Known,
    FixedIn,
    Unknown,
    Detail,
    PlayWithFix,
    PlayAgain,
    OpenIssue,
    Report,
    Back,
    Copied,
    SaveFailed,
    StartFailed,
    Running,
}

impl Text {
    /// Every text, for the tests.
    #[cfg(test)]
    pub const ALL: [Text; 16] = [
        Text::Heading,
        Text::Chapter,
        Text::NoChapter,
        Text::Known,
        Text::FixedIn,
        Text::Unknown,
        Text::Detail,
        Text::PlayWithFix,
        Text::PlayAgain,
        Text::OpenIssue,
        Text::Report,
        Text::Back,
        Text::Copied,
        Text::SaveFailed,
        Text::StartFailed,
        Text::Running,
    ];

    /// The text in `lang`: English, Spanish, Portuguese, French, German, Italian, Japanese.
    pub fn get(self, lang: Lang) -> &'static str {
        let all: [&'static str; 7] = match self {
            Text::Heading => [
                "The game stopped",
                "El juego se detuvo",
                "O jogo parou",
                "Le jeu s'est arrêté",
                "Das Spiel wurde unerwartet beendet",
                "Il gioco si è interrotto",
                "ゲームが停止しました",
            ],
            Text::Chapter => [
                "Chapter: {}",
                "Capítulo: {}",
                "Capítulo: {}",
                "Chapitre : {}",
                "Kapitel: {}",
                "Capitolo: {}",
                "チャプター: {}",
            ],
            Text::NoChapter => [
                "Chapter: not known yet (no save)",
                "Capítulo: aún desconocido (sin partida guardada)",
                "Capítulo: ainda desconhecido (sem jogo salvo)",
                "Chapitre : inconnu (aucune sauvegarde)",
                "Kapitel: noch unbekannt (kein Spielstand)",
                "Capitolo: ancora sconosciuto (nessun salvataggio)",
                "チャプター: 不明（セーブなし）",
            ],
            Text::Known => [
                "This is a known problem: {}",
                "Es un problema conocido: {}",
                "É um problema conhecido: {}",
                "C'est un problème connu : {}",
                "Das ist ein bekanntes Problem: {}",
                "È un problema noto: {}",
                "既知の問題です: {}",
            ],
            Text::FixedIn => [
                "Fixed in version {}; update the launcher.",
                "Corregido en la versión {}; actualiza el lanzador.",
                "Corrigido na versão {}; atualize o iniciador.",
                "Corrigé dans la version {} ; mettez le lanceur à jour.",
                "Behoben in Version {}; aktualisiere den Launcher.",
                "Risolto nella versione {}; aggiorna il launcher.",
                "バージョン {} で修正済みです。ランチャーを更新してください。",
            ],
            Text::Unknown => [
                "This crash is new to us. Report it and we can look into it.",
                "No conocemos este fallo. Infórmalo y lo investigaremos.",
                "Não conhecemos esta falha. Relate-a e vamos investigá-la.",
                "Ce plantage nous est inconnu. Signalez-le et nous l'examinerons.",
                "Dieser Absturz ist uns neu. Melde ihn, dann sehen wir ihn uns an.",
                "Questo arresto anomalo ci è nuovo. Segnalalo e lo esamineremo.",
                "これは未知のクラッシュです。報告していただければ調査します。",
            ],
            Text::Detail => [
                "What the game said: {}",
                "Lo que dijo el juego: {}",
                "O que o jogo disse: {}",
                "Message du jeu : {}",
                "Meldung des Spiels: {}",
                "Messaggio del gioco: {}",
                "ゲームのメッセージ: {}",
            ],
            Text::PlayWithFix => [
                "Turn on {} and play again",
                "Activar {} y volver a jugar",
                "Ativar {} e jogar de novo",
                "Activer {} et rejouer",
                "{} einschalten und erneut spielen",
                "Attiva {} e gioca di nuovo",
                "{} をオンにしてもう一度プレイ",
            ],
            Text::PlayAgain => [
                "Play again",
                "Volver a jugar",
                "Jogar de novo",
                "Rejouer",
                "Erneut spielen",
                "Gioca di nuovo",
                "もう一度プレイ",
            ],
            Text::OpenIssue => [
                "Open the issue page",
                "Abrir la página del problema",
                "Abrir a página do problema",
                "Ouvrir la page du problème",
                "Problemseite öffnen",
                "Apri la pagina del problema",
                "問題のページを開く",
            ],
            Text::Report => [
                "Report it",
                "Informar",
                "Relatar",
                "Signaler",
                "Melden",
                "Segnala",
                "報告する",
            ],
            Text::Back => [
                "Back", "Volver", "Voltar", "Retour", "Zurück", "Indietro", "戻る",
            ],
            Text::Copied => [
                "The log is copied. Paste it into the Log field of the form.",
                "El registro está copiado. Pégalo en el campo Log del formulario.",
                "O registro foi copiado. Cole-o no campo Log do formulário.",
                "Le journal est copié. Collez-le dans le champ Log du formulaire.",
                "Das Protokoll ist kopiert. Füge es im Feld Log des Formulars ein.",
                "Il registro è copiato. Incollalo nel campo Log del modulo.",
                "ログをコピーしました。フォームの Log 欄に貼り付けてください。",
            ],
            Text::SaveFailed => [
                "Could not save the setting: {}",
                "No se pudo guardar el ajuste: {}",
                "Não foi possível salvar a configuração: {}",
                "Impossible d'enregistrer le réglage : {}",
                "Die Einstellung konnte nicht gespeichert werden: {}",
                "Impossibile salvare l'impostazione: {}",
                "設定を保存できませんでした: {}",
            ],
            Text::StartFailed => [
                "Could not start the game: {}",
                "No se pudo iniciar el juego: {}",
                "Não foi possível iniciar o jogo: {}",
                "Impossible de lancer le jeu : {}",
                "Das Spiel konnte nicht gestartet werden: {}",
                "Impossibile avviare il gioco: {}",
                "ゲームを開始できませんでした: {}",
            ],
            Text::Running => [
                "{} is running",
                "{} está en marcha",
                "{} está em execução",
                "{} est en cours",
                "{} läuft",
                "{} è in esecuzione",
                "{} を実行中",
            ],
        };
        all[lang as usize]
    }
}

/// `text` with its first `{}` replaced by `value`.
pub fn fill(text: &str, value: &str) -> String {
    text.replacen("{}", value, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_exists_in_every_language_with_the_same_blanks() {
        for text in Text::ALL {
            let blanks = text.get(Lang::En).matches("{}").count();
            for lang in Lang::ALL {
                let got = text.get(lang);
                assert!(!got.trim().is_empty(), "{text:?} in {lang:?}");
                assert_eq!(got.matches("{}").count(), blanks, "{text:?} in {lang:?}");
            }
        }
    }

    #[test]
    fn languages_come_from_the_locale() {
        assert_eq!(Lang::from_locale("de_DE.UTF-8"), Lang::De);
        assert_eq!(Lang::from_locale("pt-BR"), Lang::Pt);
        assert_eq!(Lang::from_locale("JA"), Lang::Ja);
        assert_eq!(Lang::from_locale("C"), Lang::En);
        assert_eq!(Lang::from_locale("ru_RU"), Lang::En);
        let vars = |name: &str| match name {
            "LC_ALL" => Some(String::new()),
            "LANG" => Some("fr_FR.UTF-8".to_string()),
            _ => None,
        };
        assert_eq!(Lang::detect(vars), Lang::Fr);
        assert_eq!(Lang::detect(|_| None), Lang::En);
    }

    #[test]
    fn fill_replaces_the_first_blank() {
        assert_eq!(
            fill(Text::Chapter.get(Lang::En), "Train-wreck"),
            "Chapter: Train-wreck"
        );
    }
}
