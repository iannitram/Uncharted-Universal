use crate::focus::Grid;
use crate::screen::Input;

const LAYOUTS: [&[&str]; 3] = [
    &["1234567890", "qwertyuiop", "asdfghjkl ", "zxcvbnm.-/"],
    &["1234567890", "QWERTYUIOP", "ASDFGHJKL ", "ZXCVBNM_:\\"],
    &["!\"#$%&'()*", "+,-./:;<=>", "?@[\\]^_`{|", "}~"],
];
const MAX_LENGTH: usize = 4096;

pub(super) enum Edit {
    None,
    Cancel,
    Submit,
}

pub struct EditorView {
    pub text: String,
    pub rows: Vec<Vec<String>>,
    pub focus: Grid,
}

pub(super) struct Editor {
    pub text: String,
    layout: usize,
    focus: Grid,
}

impl Editor {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            layout: 0,
            focus: Grid::default(),
        }
    }

    fn rows(&self) -> &'static [&'static str] {
        LAYOUTS[self.layout]
    }

    fn lens(&self) -> Vec<usize> {
        let mut lens: Vec<_> = self.rows().iter().map(|row| row.len()).collect();
        lens.push(2); // Erase and Done.
        lens
    }

    pub fn handle(&mut self, input: Input) -> Edit {
        match input {
            Input::Move(nav) => self.focus = self.focus.step(nav, &self.lens()),
            Input::Tab { back } => self.focus = self.focus.tab(back, &self.lens()),
            Input::Back => return Edit::Cancel,
            Input::Activate => return self.press(self.focus),
            Input::PrevGame | Input::NextGame => {
                let by = if input == Input::NextGame {
                    1
                } else {
                    LAYOUTS.len() - 1
                };
                self.layout = (self.layout + by) % LAYOUTS.len();
                self.focus = self.focus.clamp(&self.lens());
            }
            Input::OtherSaves => {}
        }
        Edit::None
    }

    pub fn press(&mut self, key: Grid) -> Edit {
        let rows = self.rows();
        if let Some(row) = rows.get(key.row) {
            if let Some(ch) = row.chars().nth(key.col) {
                self.focus = key;
                if self.text.len() < MAX_LENGTH {
                    self.text.push(ch);
                }
            }
        } else if key.row == rows.len() {
            match key.col {
                0 => {
                    self.focus = key;
                    self.text.pop();
                }
                1 => {
                    self.focus = key;
                    return Edit::Submit;
                }
                _ => {}
            }
        }
        Edit::None
    }

    pub fn view_masked(&self, mask: bool) -> EditorView {
        EditorView {
            text: if mask {
                "•".repeat(self.text.chars().count())
            } else {
                self.text.clone()
            },
            rows: self
                .rows()
                .iter()
                .map(|row| row.chars().map(|ch| ch.to_string()).collect())
                .collect(),
            focus: self.focus,
        }
    }
}
