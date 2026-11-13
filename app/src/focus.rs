//! Screens and focus, plain Rust so they are tested without a window. The `.slint` file only
//! draws what this decides (ADR 0003).

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

    /// Focus button `index` (a click); out of range is ignored.
    pub fn set(&mut self, index: usize) {
        if index < self.len {
            self.index = index;
        }
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
}

/// The Home buttons, in focus order.
pub const LANGUAGE_BUTTON: usize = 0;
pub const ABOUT_BUTTON: usize = 1;
const HOME_BUTTONS: usize = 2;

/// What the window must do after an input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Draw the focus on this Home button.
    Focus(usize),
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
}

impl Default for Launcher {
    fn default() -> Self {
        Launcher {
            screen: Screen::Home,
            home: FocusRing::new(HOME_BUTTONS),
        }
    }
}

impl Launcher {
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
        }
    }

    /// A or Enter on the focused button.
    pub fn activate(&mut self) -> Effect {
        match self.screen {
            Screen::Home => match self.home.index() {
                LANGUAGE_BUTTON => Effect::NextLanguage,
                ABOUT_BUTTON => self.show(Screen::About),
                _ => Effect::None,
            },
            Screen::About => self.show(Screen::Home),
        }
    }

    /// A click or tap on button `index` of the current screen: focus it, then activate it.
    pub fn click(&mut self, index: usize) -> Effect {
        if self.screen == Screen::Home {
            self.home.set(index);
        }
        self.activate()
    }

    /// B, Escape or Backspace: About goes back Home, with the focus on the About button.
    pub fn back(&mut self) -> Effect {
        match self.screen {
            Screen::Home => Effect::None,
            Screen::About => self.show(Screen::Home),
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
}
