//! Focus on the Continue screen (docs/continue-screen.md, "Controller path"): two rows of
//! buttons, the action row on top and the page row below. The Saves and Settings pages (2.06)
//! use [`Grid`]: any number of rows, each with its own buttons. The first run (2.05) uses
//! `list_step`, `list_tab` and the `Digits` code boxes. Plain Rust, so it is tested without a
//! window.

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

/// Focus on a page of rows (Saves, Settings): `row` and `col` index `lens`, the number of
/// buttons in each row. A row with no button is drawn but never focused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Grid {
    pub row: usize,
    pub col: usize,
}

impl Grid {
    pub fn at(row: usize, col: usize) -> Grid {
        Grid { row, col }
    }

    /// The first button of the first row that has one; (0, 0) when no row has one.
    pub fn first(lens: &[usize]) -> Grid {
        Grid::at(lens.iter().position(|n| *n > 0).unwrap_or(0), 0)
    }

    /// The focus after `nav`. Left and Right move along the row and stop at its ends; Up and
    /// Down go to the nearest row above or below that has a button, keeping the column as far
    /// as that row reaches, and stop at the first and last such row.
    pub fn step(self, nav: Nav, lens: &[usize]) -> Grid {
        let here = self.clamp(lens);
        let len = lens.get(here.row).copied().unwrap_or(0);
        let col = |row: usize| here.col.min(lens[row].saturating_sub(1));
        match nav {
            Nav::Left => Grid::at(here.row, here.col.saturating_sub(1)),
            Nav::Right => Grid::at(here.row, (here.col + 1).min(len.saturating_sub(1))),
            Nav::Up => (0..here.row)
                .rev()
                .find(|r| lens[*r] > 0)
                .map_or(here, |r| Grid::at(r, col(r))),
            Nav::Down => (here.row + 1..lens.len())
                .find(|r| lens[*r] > 0)
                .map_or(here, |r| Grid::at(r, col(r))),
        }
    }

    /// Tab order: every button, row by row, then round again. `back` is Shift+Tab.
    pub fn tab(self, back: bool, lens: &[usize]) -> Grid {
        let all: Vec<Grid> = lens
            .iter()
            .enumerate()
            .flat_map(|(row, n)| (0..*n).map(move |col| Grid::at(row, col)))
            .collect();
        if all.is_empty() {
            return self;
        }
        let here = self.clamp(lens);
        let at = all.iter().position(|g| *g == here).unwrap_or(0);
        let next = if back {
            (at + all.len() - 1) % all.len()
        } else {
            (at + 1) % all.len()
        };
        all[next]
    }

    /// The nearest real button after the rows changed (a reload): the same row when it still
    /// has buttons, else the nearest row above it that has one, else the first; the column is
    /// cut to the row's length.
    pub fn clamp(self, lens: &[usize]) -> Grid {
        let row = if lens.get(self.row).is_some_and(|n| *n > 0) {
            Some(self.row)
        } else {
            (0..self.row.min(lens.len())).rev().find(|r| lens[*r] > 0)
        };
        match row {
            Some(row) => Grid::at(row, self.col.min(lens[row] - 1)),
            None => Grid::first(lens),
        }
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

    #[test]
    fn grid_moves_along_rows_and_skips_empty_ones() {
        // Three buttons, then a row with none, then two, then one.
        let lens = [3, 0, 2, 1];
        let g = Grid::at(0, 2);
        assert_eq!(g.step(Nav::Right, &lens), Grid::at(0, 2));
        assert_eq!(g.step(Nav::Left, &lens), Grid::at(0, 1));
        assert_eq!(g.step(Nav::Down, &lens), Grid::at(2, 1));
        assert_eq!(Grid::at(2, 1).step(Nav::Down, &lens), Grid::at(3, 0));
        assert_eq!(Grid::at(3, 0).step(Nav::Down, &lens), Grid::at(3, 0));
        assert_eq!(Grid::at(2, 0).step(Nav::Up, &lens), Grid::at(0, 0));
        assert_eq!(Grid::at(0, 1).step(Nav::Up, &lens), Grid::at(0, 1));
    }

    #[test]
    fn grid_tab_runs_through_every_button_and_wraps() {
        let lens = [2, 0, 1];
        let mut g = Grid::first(&lens);
        let mut seen = vec![g];
        for _ in 0..3 {
            g = g.tab(false, &lens);
            seen.push(g);
        }
        assert_eq!(
            seen,
            [
                Grid::at(0, 0),
                Grid::at(0, 1),
                Grid::at(2, 0),
                Grid::at(0, 0)
            ]
        );
        assert_eq!(Grid::at(0, 0).tab(true, &lens), Grid::at(2, 0));
        assert_eq!(Grid::at(1, 0).tab(false, &[0, 0]), Grid::at(1, 0));
    }

    #[test]
    fn grid_clamps_after_rows_change() {
        assert_eq!(Grid::at(2, 3).clamp(&[2, 4, 2]), Grid::at(2, 1));
        assert_eq!(Grid::at(2, 0).clamp(&[2, 4, 0]), Grid::at(1, 0));
        assert_eq!(Grid::at(5, 0).clamp(&[0, 1]), Grid::at(1, 0));
        assert_eq!(Grid::at(0, 0).clamp(&[0, 0, 1]), Grid::at(2, 0));
        assert_eq!(Grid::at(1, 1).clamp(&[]), Grid::at(0, 0));
        assert_eq!(Grid::first(&[0, 0, 3]), Grid::at(2, 0));
    }
}
