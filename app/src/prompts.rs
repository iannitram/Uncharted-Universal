use crate::focus::Nav;
use crate::screen::Input;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    #[default]
    Xbox,
    PlayStation,
    Nintendo,
    Keyboard,
}

impl Style {
    pub fn button(self, input: Input) -> &'static str {
        match input {
            Input::Activate => match self {
                Self::Xbox => "A",
                Self::PlayStation => "×",
                Self::Nintendo => "B",
                Self::Keyboard => "Enter",
            },
            Input::Back => match self {
                Self::Xbox => "B",
                Self::PlayStation => "○",
                Self::Nintendo => "A",
                Self::Keyboard => "Esc",
            },
            Input::PrevGame => match self {
                Self::Xbox => "LB",
                Self::PlayStation => "L1",
                Self::Nintendo => "L",
                Self::Keyboard => "PgUp",
            },
            Input::NextGame => match self {
                Self::Xbox => "RB",
                Self::PlayStation => "R1",
                Self::Nintendo => "R",
                Self::Keyboard => "PgDn",
            },
            Input::OtherSaves => match self {
                Self::Xbox => "Y",
                Self::PlayStation => "△",
                Self::Nintendo => "X",
                Self::Keyboard => "O",
            },
            Input::Move(Nav::Up) => "↑",
            Input::Move(Nav::Down) => "↓",
            Input::Move(Nav::Left) => "←",
            Input::Move(Nav::Right) => "→",
            Input::Tab { .. } => "",
        }
    }
}

/// Stored in settings.json independently of the pad used most recently.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Choice {
    #[default]
    Automatic,
    Fixed(Style),
}

impl Choice {
    pub const ALL: [Self; 5] = [
        Self::Automatic,
        Self::Fixed(Style::Xbox),
        Self::Fixed(Style::PlayStation),
        Self::Fixed(Style::Nintendo),
        Self::Fixed(Style::Keyboard),
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Fixed(Style::Xbox) => "xbox",
            Self::Fixed(Style::PlayStation) => "playstation",
            Self::Fixed(Style::Nintendo) => "nintendo",
            Self::Fixed(Style::Keyboard) => "keyboard",
        }
    }

    pub fn from_id(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|choice| choice.id() == value)
            .unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Fixed(Style::Xbox) => "Xbox",
            Self::Fixed(Style::PlayStation) => "PlayStation",
            Self::Fixed(Style::Nintendo) => "Nintendo",
            Self::Fixed(Style::Keyboard) => "Keyboard",
        }
    }

    pub fn resolve(self, last_pad: Style) -> Style {
        match self {
            Self::Automatic => last_pad,
            Self::Fixed(style) => style,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_styles_map_every_input_distinctly() {
        for style in [
            Style::Xbox,
            Style::PlayStation,
            Style::Nintendo,
            Style::Keyboard,
        ] {
            assert_ne!(style.button(Input::Activate), style.button(Input::Back));
            assert_ne!(style.button(Input::PrevGame), style.button(Input::NextGame));
            assert!(!style.button(Input::OtherSaves).is_empty());
            assert_eq!(style.button(Input::Move(Nav::Up)), "↑");
            assert_eq!(style.button(Input::Move(Nav::Down)), "↓");
            assert_eq!(style.button(Input::Move(Nav::Left)), "←");
            assert_eq!(style.button(Input::Move(Nav::Right)), "→");
            assert_eq!(style.button(Input::Tab { back: false }), "");
        }
        assert_eq!(Style::Nintendo.button(Input::Activate), "B");
        assert_eq!(Style::Nintendo.button(Input::Back), "A");
        assert_eq!(Style::Nintendo.button(Input::OtherSaves), "X");
        assert_eq!(Style::Nintendo.button(Input::PrevGame), "L");
        assert_eq!(Style::Nintendo.button(Input::NextGame), "R");
        assert_eq!(Style::PlayStation.button(Input::Activate), "×");
        assert_eq!(Style::PlayStation.button(Input::Back), "○");
        assert_eq!(Style::PlayStation.button(Input::OtherSaves), "△");
    }

    #[test]
    fn choice_round_trips_and_unknown_falls_back_to_automatic() {
        for choice in Choice::ALL {
            assert_eq!(Choice::from_id(choice.id()), choice);
            assert!(!choice.label().is_empty());
        }
        assert_eq!(Choice::from_id(""), Choice::Automatic);
        assert_eq!(Choice::from_id("steamdeck"), Choice::Automatic);
        assert_eq!(Choice::from_id("Xbox"), Choice::Automatic);
    }

    #[test]
    fn resolve_follows_last_pad_only_when_automatic() {
        for pad in [
            Style::Xbox,
            Style::PlayStation,
            Style::Nintendo,
            Style::Keyboard,
        ] {
            assert_eq!(Choice::Automatic.resolve(pad), pad);
            assert_eq!(Choice::Fixed(Style::Nintendo).resolve(pad), Style::Nintendo);
            assert_eq!(Choice::Fixed(Style::Xbox).resolve(pad), Style::Xbox);
        }
    }
}
