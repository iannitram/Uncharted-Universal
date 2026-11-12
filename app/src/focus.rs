//! Focus on the Continue screen (docs/continue-screen.md, "Controller path"): two rows of
//! buttons, the action row on top and the page row below. Plain Rust, so it is tested without a
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
    fn row_codes() {
        assert_eq!(Row::Actions.code(), 0);
        assert_eq!(Row::Pages.code(), 1);
    }
}
