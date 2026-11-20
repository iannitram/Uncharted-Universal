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
