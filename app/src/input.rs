//! Controller input from gilrs (desktop). Android key events will join here later.
//! The buttons are those of docs/continue-screen.md, "Controller path": the D-pad and left
//! stick move focus, A (Cross) acts, B (Circle) goes back, Y (Triangle) opens Other saves and
//! LB / RB (L1 / R1) change game.

use crate::focus::Nav;
use crate::screen::Input;
use gilrs::{Axis, Button, Event, EventType, Gilrs};

/// Stick deflection that counts as a push.
const STICK_THRESHOLD: f32 = 0.6;

/// Reads gilrs events and turns them into [`Input`]s. A stick push fires once until it
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
    pub fn poll(&mut self) -> Vec<Input> {
        let mut inputs = Vec::new();
        while let Some(Event { event, .. }) = self.gilrs.next_event() {
            match event {
                EventType::ButtonPressed(button, _) => {
                    if let Some(input) = button_input(button) {
                        inputs.push(input);
                    }
                }
                EventType::AxisChanged(axis, value, _) => {
                    let held = match axis {
                        Axis::LeftStickX => &mut self.stick_x_held,
                        Axis::LeftStickY => &mut self.stick_y_held,
                        _ => continue,
                    };
                    if let Some(nav) = stick_push(held, axis, value) {
                        inputs.push(Input::Move(nav));
                    }
                }
                _ => {}
            }
        }
        inputs
    }
}

/// A stick movement: a direction when the stick crosses the threshold, then nothing until it
/// comes back under it. gilrs reports stick Y positive when pushed up.
fn stick_push(held: &mut bool, axis: Axis, value: f32) -> Option<Nav> {
    let (was, now) = (*held, value.abs() >= STICK_THRESHOLD);
    *held = now;
    if !now || was {
        return None;
    }
    Some(match (axis, value > 0.0) {
        (Axis::LeftStickX, true) => Nav::Right,
        (Axis::LeftStickX, false) => Nav::Left,
        (_, true) => Nav::Up,
        (_, false) => Nav::Down,
    })
}

fn button_input(button: Button) -> Option<Input> {
    match button {
        Button::DPadUp => Some(Input::Move(Nav::Up)),
        Button::DPadDown => Some(Input::Move(Nav::Down)),
        Button::DPadLeft => Some(Input::Move(Nav::Left)),
        Button::DPadRight => Some(Input::Move(Nav::Right)),
        // gilrs names buttons by position: South is A on Xbox pads and Cross on PlayStation
        // pads, East is B / Circle, North is Y / Triangle.
        Button::South => Some(Input::Activate),
        Button::East => Some(Input::Back),
        Button::North => Some(Input::OtherSaves),
        // LeftTrigger / RightTrigger are the bumpers (LB / RB, L1 / R1); the analogue
        // triggers are LeftTrigger2 / RightTrigger2.
        Button::LeftTrigger => Some(Input::PrevGame),
        Button::RightTrigger => Some(Input::NextGame),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_map() {
        assert_eq!(button_input(Button::DPadLeft), Some(Input::Move(Nav::Left)));
        assert_eq!(button_input(Button::DPadDown), Some(Input::Move(Nav::Down)));
        assert_eq!(button_input(Button::South), Some(Input::Activate));
        assert_eq!(button_input(Button::East), Some(Input::Back));
        assert_eq!(button_input(Button::North), Some(Input::OtherSaves));
        assert_eq!(button_input(Button::LeftTrigger), Some(Input::PrevGame));
        assert_eq!(button_input(Button::RightTrigger), Some(Input::NextGame));
        assert_eq!(button_input(Button::West), None);
        assert_eq!(button_input(Button::LeftTrigger2), None);
    }

    #[test]
    fn a_stick_push_fires_once_until_it_returns() {
        let mut held = false;
        assert_eq!(stick_push(&mut held, Axis::LeftStickX, 0.3), None);
        assert_eq!(
            stick_push(&mut held, Axis::LeftStickX, 0.9),
            Some(Nav::Right)
        );
        assert_eq!(stick_push(&mut held, Axis::LeftStickX, 1.0), None);
        assert_eq!(stick_push(&mut held, Axis::LeftStickX, 0.1), None);
        assert_eq!(
            stick_push(&mut held, Axis::LeftStickX, -0.7),
            Some(Nav::Left)
        );
        let mut held = false;
        assert_eq!(stick_push(&mut held, Axis::LeftStickY, 0.8), Some(Nav::Up));
        held = false;
        assert_eq!(
            stick_push(&mut held, Axis::LeftStickY, -0.8),
            Some(Nav::Down)
        );
    }
}
