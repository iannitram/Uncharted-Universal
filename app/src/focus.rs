//! Focus on the Continue screen (docs/continue-screen.md, "Controller path"): two rows of
//! buttons, the action row on top and the page row below. Plain Rust, so it is tested without a
//! window. The first run (2.05) uses `list_step`, `list_tab` and the `Digits` code boxes below.

/// A direction from the keyboard or the controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
}

/// A row of buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// Continue, Play, Other saves (or Set up).
    Actions,
    /// Saves, Settings, Fleet, About.
    Pages,
}

impl Row {
    /// The number the `.slint` file compares with `focus-row`.
    pub fn code(self) -> i32 {
        match self {
            Row::Actions => 0,
            Row::Pages => 1,
        }
    }
}

/// Which button has focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Focus {
    pub row: Row,
    pub index: usize,
}

impl Focus {
    pub fn actions(index: usize) -> Focus {
        Focus {
            row: Row::Actions,
            index,
        }
    }

    pub fn pages(index: usize) -> Focus {
        Focus {
            row: Row::Pages,
            index,
        }
    }

    /// The focus after `nav`, with `actions` and `pages` buttons in the two rows. Left and Right
    /// move along a row and stop at its ends; Down goes from the action row to the page row and
    /// Up comes back, each keeping the column as far as the other row reaches.
    pub fn step(self, nav: Nav, actions: usize, pages: usize) -> Focus {
        let len = |row| match row {
            Row::Actions => actions,
            Row::Pages => pages,
        };
        let clamp = |row, index: usize| Focus {
            row,
            index: index.min(len(row).saturating_sub(1)),
        };
        match nav {
            Nav::Left => clamp(self.row, self.index.saturating_sub(1)),
            Nav::Right => clamp(self.row, self.index + 1),
            Nav::Down if self.row == Row::Actions && pages > 0 => clamp(Row::Pages, self.index),
            Nav::Up if self.row == Row::Pages && actions > 0 => clamp(Row::Actions, self.index),
            Nav::Up | Nav::Down => self,
        }
    }

    /// Tab order: along the action row, then the page row, then round again. `back` is
    /// Shift+Tab.
    pub fn tab(self, back: bool, actions: usize, pages: usize) -> Focus {
        let total = actions + pages;
        if total == 0 {
            return self;
        }
        let at = match self.row {
            Row::Actions => self.index,
            Row::Pages => actions + self.index,
        };
        let next = if back {
            (at + total - 1) % total
        } else {
            (at + 1) % total
        };
        if next < actions {
            Focus::actions(next)
        } else {
            Focus::pages(next - actions)
        }
    }
}

/// Focus in a vertical list of `len` rows (the first-run page): Up and Down move and stop at
/// the ends; Left and Right stay.
pub fn list_step(index: usize, nav: Nav, len: usize) -> usize {
    let last = len.saturating_sub(1);
    match nav {
        Nav::Up => index.saturating_sub(1).min(last),
        Nav::Down => (index + 1).min(last),
        Nav::Left | Nav::Right => index.min(last),
    }
}

/// Tab order in a vertical list: down the rows, then round again. `back` is Shift+Tab.
pub fn list_tab(index: usize, back: bool, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    if back {
        (index + len - 1) % len
    } else {
        (index + 1) % len
    }
}

/// The digit entry of a pairing code: six boxes, one cursor. Left and Right move the cursor
/// and stop at the ends; Up and Down turn the digit under it (an empty box starts at 0 going
/// up and at 9 going down, then wraps).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Digits {
    pub digits: [Option<u8>; 6],
    pub cursor: usize,
}

impl Digits {
    pub fn step(mut self, nav: Nav) -> Digits {
        let at = self.cursor;
        match nav {
            Nav::Left => self.cursor = at.saturating_sub(1),
            Nav::Right => self.cursor = (at + 1).min(5),
            Nav::Up => self.digits[at] = Some(self.digits[at].map_or(0, |d| (d + 1) % 10)),
            Nav::Down => self.digits[at] = Some(self.digits[at].map_or(9, |d| (d + 9) % 10)),
        }
        self
    }

    /// A digit typed (keyboard 0-9): written under the cursor, which moves on.
    pub fn type_digit(mut self, digit: u8) -> Digits {
        self.digits[self.cursor] = Some(digit % 10);
        self.cursor = (self.cursor + 1).min(5);
        self
    }

    /// Backspace: clears the box under the cursor, or the one before it when that is empty.
    pub fn backspace(mut self) -> Digits {
        if self.digits[self.cursor].is_none() {
            self.cursor = self.cursor.saturating_sub(1);
        }
        self.digits[self.cursor] = None;
        self
    }

    /// The code once all six boxes hold a digit.
    pub fn code(&self) -> Option<String> {
        self.digits
            .iter()
            .map(|d| d.map(|d| char::from(b'0' + d)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_and_right_stop_at_the_ends_of_a_row() {
        let f = Focus::actions(0);
        assert_eq!(f.step(Nav::Left, 3, 4), Focus::actions(0));
        assert_eq!(f.step(Nav::Right, 3, 4), Focus::actions(1));
        assert_eq!(Focus::actions(2).step(Nav::Right, 3, 4), Focus::actions(2));
        assert_eq!(Focus::pages(3).step(Nav::Right, 3, 4), Focus::pages(3));
    }

    #[test]
    fn down_goes_to_the_pages_and_up_comes_back() {
        assert_eq!(Focus::actions(1).step(Nav::Down, 2, 4), Focus::pages(1));
        assert_eq!(Focus::pages(1).step(Nav::Down, 2, 4), Focus::pages(1));
        assert_eq!(Focus::pages(3).step(Nav::Up, 2, 4), Focus::actions(1));
        assert_eq!(Focus::actions(0).step(Nav::Up, 2, 4), Focus::actions(0));
    }

    #[test]
    fn tab_runs_through_both_rows_and_wraps() {
        let mut f = Focus::actions(0);
        let mut seen = vec![f];
        for _ in 0..5 {
            f = f.tab(false, 2, 4);
            seen.push(f);
        }
        assert_eq!(
            seen,
            [
                Focus::actions(0),
                Focus::actions(1),
                Focus::pages(0),
                Focus::pages(1),
                Focus::pages(2),
                Focus::pages(3),
            ]
        );
        assert_eq!(f.tab(false, 2, 4), Focus::actions(0));
        assert_eq!(Focus::actions(0).tab(true, 2, 4), Focus::pages(3));
    }

    #[test]
    fn a_list_stops_at_its_ends_and_tab_wraps() {
        assert_eq!(list_step(0, Nav::Up, 3), 0);
        assert_eq!(list_step(0, Nav::Down, 3), 1);
        assert_eq!(list_step(2, Nav::Down, 3), 2);
        assert_eq!(list_step(1, Nav::Left, 3), 1);
        assert_eq!(list_step(4, Nav::Right, 2), 1);
        assert_eq!(list_step(0, Nav::Down, 0), 0);
        assert_eq!(list_tab(2, false, 3), 0);
        assert_eq!(list_tab(0, true, 3), 2);
        assert_eq!(list_tab(0, false, 0), 0);
    }

    #[test]
    fn the_code_boxes_turn_type_and_clear() {
        let d = Digits::default().step(Nav::Up).step(Nav::Up);
        assert_eq!(d.digits[0], Some(1));
        let d = d.step(Nav::Right).step(Nav::Down);
        assert_eq!((d.cursor, d.digits[1]), (1, Some(9)));
        assert_eq!(d.step(Nav::Up).digits[1], Some(0));
        assert_eq!(Digits::default().step(Nav::Left).cursor, 0);
        let mut d = Digits::default();
        for digit in [4, 8, 1, 5, 2, 6, 7] {
            d = d.type_digit(digit);
        }
        assert_eq!(d.code().as_deref(), Some("481527"));
        assert_eq!(d.cursor, 5);
        let d = d.backspace();
        assert_eq!((d.cursor, d.digits[5], d.code()), (5, None, None));
        let d = d.backspace();
        assert_eq!((d.cursor, d.digits[4]), (4, None));
        assert_eq!(Digits::default().step(Nav::Right).backspace().cursor, 0);
    }

    #[test]
    fn row_codes() {
        assert_eq!(Row::Actions.code(), 0);
        assert_eq!(Row::Pages.code(), 1);
    }
}
