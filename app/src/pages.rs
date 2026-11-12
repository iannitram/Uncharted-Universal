//! Which page the window shows and where a press goes, plain Rust so the controller route to
//! each page is tested without a window. The home page holds the scaffold's buttons until the
//! Continue screen (2.04) replaces it; its second button opens Fleet (2.08).

use crate::focus::{FocusRing, Nav};
use crate::vm::FleetVm;

/// A page of the window. The integer is the `.slint` file's `page` property.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Home = 0,
    Fleet = 1,
}

/// The home page's buttons, in focus order.
pub const HOME_BUTTONS: [&str; 2] = ["Placeholder", "Fleet"];
/// The index of the home page's Fleet button.
pub const HOME_FLEET: usize = 1;

/// The window's state: the page shown, the home focus and the Fleet page.
#[derive(Debug)]
pub struct Pages {
    page: Page,
    home: FocusRing,
    fleet: FleetVm,
}

impl Pages {
    /// Opens on the home page with focus on its first button.
    pub fn new(fleet: FleetVm) -> Self {
        Pages {
            page: Page::Home,
            home: FocusRing::new(HOME_BUTTONS.len()),
            fleet,
        }
    }

    pub fn page(&self) -> Page {
        self.page
    }

    /// The focused button on the home page.
    pub fn home_focus(&self) -> usize {
        self.home.index()
    }

    pub fn fleet(&self) -> &FleetVm {
        &self.fleet
    }

    /// A direction moves focus on the page shown.
    pub fn on_nav(&mut self, nav: Nav) {
        match self.page {
            Page::Home => self.home.step(nav),
            Page::Fleet => self.fleet.on_nav(nav),
        }
    }

    /// A (South) or Return on the focused item: Fleet on the home page opens the Fleet page;
    /// Back on the Fleet page returns home. Other items do nothing yet.
    pub fn on_activate(&mut self) {
        match self.page {
            Page::Home if self.home.index() == HOME_FLEET => {
                self.fleet.reset_focus();
                self.page = Page::Fleet;
            }
            Page::Fleet if self.fleet.focused() == self.fleet.back_index() => self.on_back(),
            _ => {}
        }
    }

    /// Focus `index` on the page shown and activate it (a mouse click or touch). An index past
    /// the page's items is ignored.
    pub fn on_click(&mut self, index: usize) {
        let focused = match self.page {
            Page::Home => self.home.focus_on(index),
            Page::Fleet => self.fleet.focus_on(index),
        };
        if focused {
            self.on_activate();
        }
    }

    /// B (East) or Escape: the Fleet page returns home with focus on its Fleet button.
    pub fn on_back(&mut self) {
        if self.page == Page::Fleet {
            self.page = Page::Home;
            self.home = FocusRing::new(HOME_BUTTONS.len());
            self.home.step(Nav::Down);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages() -> Pages {
        Pages::new(FleetVm::from_text(include_str!("../fixtures/fleet.tsv")).unwrap())
    }

    #[test]
    fn the_controller_reaches_fleet_and_comes_back() {
        let mut p = pages();
        assert_eq!(p.page(), Page::Home);
        assert_eq!(p.home_focus(), 0);
        p.on_activate();
        assert_eq!(p.page(), Page::Home, "the placeholder does nothing");

        p.on_nav(Nav::Right);
        assert_eq!(p.home_focus(), HOME_FLEET);
        p.on_activate();
        assert_eq!(p.page(), Page::Fleet);
        assert_eq!(p.fleet().focused(), 0);

        // Down through the three rows to Back, then A returns home on the Fleet button.
        for _ in 0..3 {
            p.on_nav(Nav::Down);
        }
        p.on_activate();
        assert_eq!(p.page(), Page::Home);
        assert_eq!(p.home_focus(), HOME_FLEET);
    }

    #[test]
    fn b_returns_home_from_fleet_and_does_nothing_at_home() {
        let mut p = pages();
        p.on_back();
        assert_eq!(p.page(), Page::Home);
        assert_eq!(p.home_focus(), 0);

        p.on_nav(Nav::Down);
        p.on_activate();
        p.on_nav(Nav::Down);
        p.on_back();
        assert_eq!(p.page(), Page::Home);
        assert_eq!(p.home_focus(), HOME_FLEET);
    }

    #[test]
    fn opening_fleet_again_starts_on_the_first_row() {
        let mut p = pages();
        p.on_nav(Nav::Down);
        p.on_activate();
        p.on_nav(Nav::Down);
        p.on_back();
        p.on_activate();
        assert_eq!(p.page(), Page::Fleet);
        assert_eq!(p.fleet().focused(), 0);
    }

    #[test]
    fn a_click_focuses_and_activates() {
        let mut p = pages();
        p.on_click(HOME_FLEET);
        assert_eq!(p.page(), Page::Fleet);
        p.on_click(1);
        assert_eq!(p.page(), Page::Fleet);
        assert_eq!(p.fleet().focused(), 1);
        p.on_click(99);
        assert_eq!(p.fleet().focused(), 1);
        p.on_click(p.fleet().back_index());
        assert_eq!(p.page(), Page::Home);
    }

    #[test]
    fn with_no_records_fleet_opens_on_back() {
        let mut p = Pages::new(FleetVm::empty());
        p.on_click(HOME_FLEET);
        assert_eq!(p.page(), Page::Fleet);
        assert_eq!(p.fleet().focused(), p.fleet().back_index());
        p.on_activate();
        assert_eq!(p.page(), Page::Home);
    }
}
