//! Focus order for the placeholder buttons, plain Rust so it is tested without a window.

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

    /// Up/Left go to the previous button, Down/Right to the next.
    pub fn step(&mut self, nav: Nav) {
        self.index = match nav {
            Nav::Up | Nav::Left => (self.index + self.len - 1) % self.len,
            Nav::Down | Nav::Right => (self.index + 1) % self.len,
        };
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
    }

    #[test]
    fn nav_codes() {
        assert_eq!(Nav::from_code(0), Some(Nav::Up));
        assert_eq!(Nav::from_code(3), Some(Nav::Right));
        assert_eq!(Nav::from_code(9), None);
    }
}
