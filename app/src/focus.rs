//! Focus on the Continue screen (docs/continue-screen.md, "Controller path"): two rows of
//! buttons, the action row on top and the page row below; on About, its two buttons; on
//! Fleet (2.08), its rows and Back; and on the after-crash screen (2.07), its buttons. Plain
//! Rust, so it is tested without a window.

use crate::vm::FleetVm;

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

/// Which of `len` buttons has focus. Moves wrap around.
#[derive(Debug)]
pub struct FocusRing {
    len: usize,
    index: usize,
}

impl FocusRing {
    pub fn new(len: usize) -> Self {
        assert!(len > 0, "a focus ring needs at least one button");
        FocusRing { len, index: 0 }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// Focus button `index` (a click); `false`, and no change, when out of range.
    pub fn set(&mut self, index: usize) -> bool {
        if index < self.len {
            self.index = index;
        }
        index < self.len
    }

    /// Up/Left go to the previous button, Down/Right to the next.
    pub fn step(&mut self, nav: Nav) {
        self.index = match nav {
            Nav::Up | Nav::Left => (self.index + self.len - 1) % self.len,
            Nav::Down | Nav::Right => (self.index + 1) % self.len,
        };
    }
}

/// About's buttons, in focus order. The language button is on About until Settings (2.06)
/// exists (owner ruling 2026-11-12).
pub const LANGUAGE_BUTTON: usize = 0;
pub const BACK_BUTTON: usize = 1;
const ABOUT_BUTTONS: usize = 2;

/// What the window must do after an input on About, Fleet or the after-crash screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Draw the focus on this About button.
    Focus(usize),
    /// Draw the focus on this Fleet item (a row, or Back last).
    FleetFocus(usize),
    /// Scroll About by this many steps (positive is towards the top).
    Scroll(i32),
    /// Step the interface language to the next choice.
    NextLanguage,
    /// Draw the focus on this after-crash button.
    CrashFocus(usize),
    /// Press after-crash button `index` (`AfterCrash::buttons`).
    Press(usize),
    /// Back to the Continue screen.
    Close,
}

/// Focus on About: left and right (and Tab) move between its buttons, up and down scroll.
#[derive(Debug)]
pub struct AboutFocus {
    ring: FocusRing,
}

impl Default for AboutFocus {
    fn default() -> Self {
        AboutFocus {
            ring: FocusRing::new(ABOUT_BUTTONS),
        }
    }
}

impl AboutFocus {
    pub fn focused(&self) -> usize {
        self.ring.index()
    }

    pub fn nav(&mut self, nav: Nav) -> Effect {
        match nav {
            Nav::Up => Effect::Scroll(1),
            Nav::Down => Effect::Scroll(-1),
            Nav::Left | Nav::Right => {
                self.ring.step(nav);
                Effect::Focus(self.ring.index())
            }
        }
    }

    /// A or Enter on the focused button.
    pub fn activate(&self) -> Effect {
        match self.ring.index() {
            LANGUAGE_BUTTON => Effect::NextLanguage,
            BACK_BUTTON => Effect::Close,
            _ => Effect::None,
        }
    }

    /// A click or tap on button `index`: focus it, then activate it.
    pub fn click(&mut self, index: usize) -> Effect {
        self.ring.set(index);
        self.activate()
    }
}

/// Fleet (2.08), opened from the page row over the Continue screen: up and down (and Tab) move
/// over the rows and Back; A on Back, or B, closes it; A on a row does nothing yet.
#[derive(Debug)]
pub struct FleetFocus {
    fleet: FleetVm,
    open: bool,
}

impl Default for FleetFocus {
    fn default() -> Self {
        FleetFocus::new(FleetVm::empty())
    }
}

impl FleetFocus {
    pub fn new(fleet: FleetVm) -> Self {
        FleetFocus { fleet, open: false }
    }

    pub fn fleet(&self) -> &FleetVm {
        &self.fleet
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open Fleet with the focus on its first row (on Back when there are no records).
    pub fn open(&mut self) {
        self.fleet.reset_focus();
        self.open = true;
    }

    pub fn nav(&mut self, nav: Nav) -> Effect {
        self.fleet.on_nav(nav);
        Effect::FleetFocus(self.fleet.focused())
    }

    /// A or Enter on the focused item.
    pub fn activate(&mut self) -> Effect {
        if self.fleet.focused() == self.fleet.back_index() {
            self.back()
        } else {
            Effect::None
        }
    }

    /// A click or tap on item `index`: focus it, then activate it.
    pub fn click(&mut self, index: usize) -> Effect {
        if self.fleet.focus_on(index) {
            self.activate()
        } else {
            Effect::None
        }
    }

    /// B, Escape: back to the Continue screen.
    pub fn back(&mut self) -> Effect {
        self.open = false;
        Effect::Close
    }
}

/// The after-crash screen (2.07), over the Continue screen: the d-pad (and Tab) moves over its
/// buttons and wraps; A presses one; B goes back to the Continue screen.
#[derive(Debug)]
pub struct CrashFocus {
    ring: FocusRing,
}

impl CrashFocus {
    /// `buttons` buttons, the first focused.
    pub fn new(buttons: usize) -> Self {
        CrashFocus {
            ring: FocusRing::new(buttons.max(1)),
        }
    }

    pub fn focused(&self) -> usize {
        self.ring.index()
    }

    pub fn nav(&mut self, nav: Nav) -> Effect {
        self.ring.step(nav);
        Effect::CrashFocus(self.ring.index())
    }

    /// A or Enter on the focused button.
    pub fn activate(&self) -> Effect {
        Effect::Press(self.ring.index())
    }

    /// A click or tap on button `index`: focus it, then press it.
    pub fn click(&mut self, index: usize) -> Effect {
        if self.ring.set(index) {
            self.activate()
        } else {
            Effect::None
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

    #[test]
    fn ring_moves_and_wraps() {
        let mut ring = FocusRing::new(2);
        assert_eq!(ring.index(), 0);
        ring.step(Nav::Right);
        assert_eq!(ring.index(), 1);
        ring.step(Nav::Down);
        assert_eq!(ring.index(), 0);
        ring.step(Nav::Left);
        assert_eq!(ring.index(), 1);
        ring.step(Nav::Up);
        assert_eq!(ring.index(), 0);
        ring.set(1);
        assert_eq!(ring.index(), 1);
        ring.set(7);
        assert_eq!(ring.index(), 1);
    }

    #[test]
    fn about_buttons_and_scrolling() {
        let mut about = AboutFocus::default();
        assert_eq!(about.focused(), LANGUAGE_BUTTON);
        assert_eq!(about.activate(), Effect::NextLanguage);
        assert_eq!(about.nav(Nav::Down), Effect::Scroll(-1));
        assert_eq!(about.nav(Nav::Up), Effect::Scroll(1));
        assert_eq!(about.nav(Nav::Right), Effect::Focus(BACK_BUTTON));
        assert_eq!(about.activate(), Effect::Close);
        assert_eq!(about.nav(Nav::Right), Effect::Focus(LANGUAGE_BUTTON));
        assert_eq!(about.click(BACK_BUTTON), Effect::Close);
        assert_eq!(about.click(LANGUAGE_BUTTON), Effect::NextLanguage);
    }

    fn with_fleet() -> FleetFocus {
        FleetFocus::new(FleetVm::from_text(include_str!("../fixtures/fleet.tsv")).unwrap())
    }

    #[test]
    fn fleet_moves_over_its_rows_and_back_closes_it() {
        let mut fleet = with_fleet();
        assert!(!fleet.is_open());
        fleet.open();
        assert!(fleet.is_open());
        assert_eq!(fleet.fleet().focused(), 0);

        // Down through the three rows to Back; A on a row does nothing, A on Back closes.
        assert_eq!(fleet.activate(), Effect::None);
        for row in 1..=3 {
            assert_eq!(fleet.nav(Nav::Down), Effect::FleetFocus(row));
        }
        assert_eq!(fleet.activate(), Effect::Close);
        assert!(!fleet.is_open());
    }

    #[test]
    fn b_closes_fleet_and_opening_it_again_starts_on_the_first_row() {
        let mut fleet = with_fleet();
        fleet.open();
        fleet.nav(Nav::Down);
        assert_eq!(fleet.back(), Effect::Close);
        assert!(!fleet.is_open());
        fleet.open();
        assert_eq!(fleet.fleet().focused(), 0);
    }

    #[test]
    fn a_fleet_click_focuses_and_activates() {
        let mut fleet = with_fleet();
        fleet.open();
        assert_eq!(fleet.click(1), Effect::None);
        assert_eq!(fleet.fleet().focused(), 1);
        assert_eq!(fleet.click(99), Effect::None);
        assert_eq!(fleet.fleet().focused(), 1);
        let back = fleet.fleet().back_index();
        assert_eq!(fleet.click(back), Effect::Close);
    }

    #[test]
    fn with_no_records_fleet_opens_on_back() {
        let mut fleet = FleetFocus::default();
        fleet.open();
        assert_eq!(fleet.fleet().focused(), fleet.fleet().back_index());
        assert_eq!(fleet.activate(), Effect::Close);
    }

    #[test]
    fn the_after_crash_screen_with_the_controller() {
        let mut crash = CrashFocus::new(3);
        assert_eq!(crash.focused(), 0);
        assert_eq!(crash.nav(Nav::Up), Effect::CrashFocus(2));
        assert_eq!(crash.activate(), Effect::Press(2));
        assert_eq!(crash.click(1), Effect::Press(1));
        assert_eq!(crash.focused(), 1);
        assert_eq!(crash.click(9), Effect::None);
        assert_eq!(crash.nav(Nav::Down), Effect::CrashFocus(2));
        assert_eq!(crash.nav(Nav::Right), Effect::CrashFocus(0));
    }
}
