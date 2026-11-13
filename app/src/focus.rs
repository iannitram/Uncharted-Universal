//! Screens and focus, plain Rust so they are tested without a window. The `.slint` file only
//! draws what this decides (ADR 0003).

use crate::vm::FleetVm;

/// A direction from the keyboard or the controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
}

impl Nav {
    /// The integer the `.slint` file passes through its `nav` callback.
    pub fn from_code(code: i32) -> Option<Nav> {
        match code {
            0 => Some(Nav::Up),
            1 => Some(Nav::Down),
            2 => Some(Nav::Left),
            3 => Some(Nav::Right),
            _ => None,
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

/// The launcher's screens. The number is the `.slint` file's `page` property.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// The Continue screen's place (piece 2.04) and its buttons.
    Home = 0,
    /// Version, patches, licences, the Slint notice.
    About = 1,
    /// The paired devices (2.08).
    Fleet = 2,
}

/// The Home buttons, in focus order.
pub const LANGUAGE_BUTTON: usize = 0;
pub const ABOUT_BUTTON: usize = 1;
pub const FLEET_BUTTON: usize = 2;
const HOME_BUTTONS: usize = 3;

/// What the window must do after an input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Draw the focus on this Home button.
    Focus(usize),
    /// Draw the focus on this Fleet item (a row, or Back last).
    FleetFocus(usize),
    /// Switch to this screen.
    Show(Screen),
    /// Scroll About by this many steps (positive is towards the top).
    Scroll(i32),
    /// Step the interface language to the next choice.
    NextLanguage,
}

/// The screen and the focus on it.
#[derive(Debug)]
pub struct Launcher {
    screen: Screen,
    home: FocusRing,
    fleet: FleetVm,
}

impl Default for Launcher {
    fn default() -> Self {
        Launcher::new(FleetVm::empty())
    }
}

impl Launcher {
    pub fn new(fleet: FleetVm) -> Self {
        Launcher {
            screen: Screen::Home,
            home: FocusRing::new(HOME_BUTTONS),
            fleet,
        }
    }

    pub fn fleet(&self) -> &FleetVm {
        &self.fleet
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn focused(&self) -> usize {
        self.home.index()
    }

    /// A direction: moves the Home focus; on About, up and down scroll (its one button, Back,
    /// keeps the focus).
    pub fn nav(&mut self, nav: Nav) -> Effect {
        match self.screen {
            Screen::Home => {
                self.home.step(nav);
                Effect::Focus(self.home.index())
            }
            Screen::About => match nav {
                Nav::Up => Effect::Scroll(1),
                Nav::Down => Effect::Scroll(-1),
                Nav::Left | Nav::Right => Effect::None,
            },
            Screen::Fleet => {
                self.fleet.on_nav(nav);
                Effect::FleetFocus(self.fleet.focused())
            }
        }
    }

    /// A or Enter on the focused button.
    pub fn activate(&mut self) -> Effect {
        match self.screen {
            Screen::Home => match self.home.index() {
                LANGUAGE_BUTTON => Effect::NextLanguage,
                ABOUT_BUTTON => self.show(Screen::About),
                FLEET_BUTTON => {
                    self.fleet.reset_focus();
                    self.show(Screen::Fleet)
                }
                _ => Effect::None,
            },
            Screen::About => self.show(Screen::Home),
            // Back is Fleet's last item; the rows do nothing yet.
            Screen::Fleet if self.fleet.focused() == self.fleet.back_index() => {
                self.show(Screen::Home)
            }
            Screen::Fleet => Effect::None,
        }
    }

    /// A click or tap on button `index` of the current screen: focus it, then activate it.
    pub fn click(&mut self, index: usize) -> Effect {
        let focused = match self.screen {
            Screen::Home => self.home.set(index),
            Screen::About => true,
            Screen::Fleet => self.fleet.focus_on(index),
        };
        if focused {
            self.activate()
        } else {
            Effect::None
        }
    }

    /// B, Escape or Backspace: About and Fleet go back Home, with the focus on their button.
    pub fn back(&mut self) -> Effect {
        match self.screen {
            Screen::Home => Effect::None,
            Screen::About | Screen::Fleet => self.show(Screen::Home),
        }
    }

    fn show(&mut self, screen: Screen) -> Effect {
        self.screen = screen;
        Effect::Show(screen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_and_wraps() {
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
    fn nav_codes() {
        assert_eq!(Nav::from_code(0), Some(Nav::Up));
        assert_eq!(Nav::from_code(3), Some(Nav::Right));
        assert_eq!(Nav::from_code(9), None);
    }

    #[test]
    fn about_is_reachable_with_the_controller_alone() {
        let mut launcher = Launcher::default();
        assert_eq!(launcher.screen(), Screen::Home);
        assert_eq!(launcher.focused(), LANGUAGE_BUTTON);
        assert_eq!(launcher.nav(Nav::Right), Effect::Focus(ABOUT_BUTTON));
        assert_eq!(launcher.activate(), Effect::Show(Screen::About));
        assert_eq!(launcher.screen(), Screen::About);

        // On About the stick scrolls; left and right do nothing.
        assert_eq!(launcher.nav(Nav::Down), Effect::Scroll(-1));
        assert_eq!(launcher.nav(Nav::Up), Effect::Scroll(1));
        assert_eq!(launcher.nav(Nav::Left), Effect::None);

        // A on Back, or B, returns Home with the focus where it was.
        assert_eq!(launcher.activate(), Effect::Show(Screen::Home));
        assert_eq!(launcher.focused(), ABOUT_BUTTON);
        launcher.activate();
        assert_eq!(launcher.back(), Effect::Show(Screen::Home));
        assert_eq!(launcher.back(), Effect::None);
    }

    #[test]
    fn the_language_button_steps_the_language() {
        let mut launcher = Launcher::default();
        assert_eq!(launcher.activate(), Effect::NextLanguage);
        assert_eq!(launcher.screen(), Screen::Home);
        assert_eq!(launcher.click(ABOUT_BUTTON), Effect::Show(Screen::About));
        assert_eq!(launcher.click(0), Effect::Show(Screen::Home));
        assert_eq!(launcher.click(LANGUAGE_BUTTON), Effect::NextLanguage);
    }

    fn with_fleet() -> Launcher {
        Launcher::new(FleetVm::from_text(include_str!("../fixtures/fleet.tsv")).unwrap())
    }

    #[test]
    fn the_controller_reaches_fleet_and_comes_back() {
        let mut launcher = with_fleet();
        launcher.nav(Nav::Right);
        assert_eq!(launcher.nav(Nav::Right), Effect::Focus(FLEET_BUTTON));
        assert_eq!(launcher.activate(), Effect::Show(Screen::Fleet));
        assert_eq!(launcher.fleet().focused(), 0);

        // Down through the three rows to Back; A on a row does nothing, A on Back returns
        // Home on the Fleet button.
        assert_eq!(launcher.activate(), Effect::None);
        for row in 1..=3 {
            assert_eq!(launcher.nav(Nav::Down), Effect::FleetFocus(row));
        }
        assert_eq!(launcher.activate(), Effect::Show(Screen::Home));
        assert_eq!(launcher.focused(), FLEET_BUTTON);
    }

    #[test]
    fn b_returns_home_from_fleet_and_opening_it_again_starts_on_the_first_row() {
        let mut launcher = with_fleet();
        launcher.click(FLEET_BUTTON);
        launcher.nav(Nav::Down);
        assert_eq!(launcher.back(), Effect::Show(Screen::Home));
        assert_eq!(launcher.focused(), FLEET_BUTTON);
        assert_eq!(launcher.activate(), Effect::Show(Screen::Fleet));
        assert_eq!(launcher.fleet().focused(), 0);
    }

    #[test]
    fn a_fleet_click_focuses_and_activates() {
        let mut launcher = with_fleet();
        assert_eq!(launcher.click(FLEET_BUTTON), Effect::Show(Screen::Fleet));
        assert_eq!(launcher.click(1), Effect::None);
        assert_eq!(launcher.fleet().focused(), 1);
        assert_eq!(launcher.click(99), Effect::None);
        assert_eq!(launcher.fleet().focused(), 1);
        let back = launcher.fleet().back_index();
        assert_eq!(launcher.click(back), Effect::Show(Screen::Home));
    }

    #[test]
    fn with_no_records_fleet_opens_on_back() {
        let mut launcher = Launcher::default();
        launcher.click(FLEET_BUTTON);
        assert_eq!(launcher.screen(), Screen::Fleet);
        assert_eq!(launcher.fleet().focused(), launcher.fleet().back_index());
        assert_eq!(launcher.activate(), Effect::Show(Screen::Home));
    }
}
