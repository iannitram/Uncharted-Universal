//! Controller input from gilrs (desktop). Android key events will join here later.

use crate::focus::Nav;
use gilrs::{Axis, Button, Event, EventType, Gilrs};

/// Stick deflection that counts as a push.
const STICK_THRESHOLD: f32 = 0.6;

/// What a controller event means to the launcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Move(Nav),
    Activate,
    /// Leave the page (B on Xbox pads, Circle on PlayStation pads).
    Back,
}

/// Reads gilrs events and turns them into [`Action`]s. A stick push fires once until it
/// returns to centre.
pub struct Controller {
    gilrs: Gilrs,
    stick_x_held: bool,
    stick_y_held: bool,
}

impl Controller {
    /// `None` when the platform has no gamepad backend (the keyboard still works).
    pub fn new() -> Option<Controller> {
        match Gilrs::new() {
            Ok(gilrs) => Some(Controller {
                gilrs,
                stick_x_held: false,
                stick_y_held: false,
            }),
            Err(err) => {
                eprintln!("controller input unavailable: {err}");
                None
            }
        }
    }

    /// Drain pending events.
    pub fn poll(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        while let Some(Event { event, .. }) = self.gilrs.next_event() {
            match event {
                EventType::ButtonPressed(button, _) => {
                    if let Some(action) = button_action(button) {
                        actions.push(action);
                    }
                }
                EventType::AxisChanged(axis, value, _) => {
                    let held = match axis {
                        Axis::LeftStickX => &mut self.stick_x_held,
                        Axis::LeftStickY => &mut self.stick_y_held,
                        _ => continue,
                    };
                    let (was, now) = (*held, value.abs() >= STICK_THRESHOLD);
                    *held = now;
                    if now && !was {
                        // gilrs reports stick Y positive when pushed up.
                        let nav = match (axis, value > 0.0) {
                            (Axis::LeftStickX, true) => Nav::Right,
                            (Axis::LeftStickX, false) => Nav::Left,
                            (_, true) => Nav::Up,
                            (_, false) => Nav::Down,
                        };
                        actions.push(Action::Move(nav));
                    }
                }
                _ => {}
            }
        }
        actions
    }
}

fn button_action(button: Button) -> Option<Action> {
    match button {
        Button::DPadUp => Some(Action::Move(Nav::Up)),
        Button::DPadDown => Some(Action::Move(Nav::Down)),
        Button::DPadLeft => Some(Action::Move(Nav::Left)),
        Button::DPadRight => Some(Action::Move(Nav::Right)),
        // "South" is A on Xbox pads and Cross on PlayStation pads.
        Button::South => Some(Action::Activate),
        // "East" is B on Xbox pads and Circle on PlayStation pads.
        Button::East => Some(Action::Back),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpad_south_and_east_map() {
        assert_eq!(
            button_action(Button::DPadLeft),
            Some(Action::Move(Nav::Left))
        );
        assert_eq!(button_action(Button::South), Some(Action::Activate));
        assert_eq!(button_action(Button::East), Some(Action::Back));
        assert_eq!(button_action(Button::North), None);
    }
}
