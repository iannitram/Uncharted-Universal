//! Controller input from gilrs (desktop). Android key events will join here later.
//! The buttons are those of docs/continue-screen.md, "Controller path": the D-pad and left
//! stick move focus, A (Cross) acts, B (Circle) goes back, Y (Triangle) opens Other saves and
//! LB / RB (L1 / R1) change game.

use crate::focus::Nav;
use crate::prompts::Style;
use crate::screen::Input;
use gilrs::{Axis, Button, Event, EventType, Gamepad, Gilrs};
use std::collections::HashMap;

/// Stick deflection that counts as a push.
const STICK_THRESHOLD: f32 = 0.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PadInput {
    pub input: Input,
    pub style: Style,
}

struct Pad {
    style: Style,
    stick_x_held: bool,
    stick_y_held: bool,
}

/// Track the normalized events separately from the backend so tests can hot-plug a fake pad.
#[derive(Default)]
pub struct Pads(HashMap<usize, Pad>);

impl Pads {
    pub fn connect(&mut self, id: usize, style: Style) {
        self.0.insert(
            id,
            Pad {
                style,
                stick_x_held: false,
                stick_y_held: false,
            },
        );
    }

    pub fn disconnect(&mut self, id: usize) {
        self.0.remove(&id);
    }

    pub fn button(&self, id: usize, button: Button) -> Option<PadInput> {
        let pad = self.0.get(&id)?;
        Some(PadInput {
            input: button_input(button)?,
            style: pad.style,
        })
    }

    pub fn axis(&mut self, id: usize, axis: Axis, value: f32) -> Option<PadInput> {
        let pad = self.0.get_mut(&id)?;
        let held = match axis {
            Axis::LeftStickX => &mut pad.stick_x_held,
            Axis::LeftStickY => &mut pad.stick_y_held,
            _ => return None,
        };
        Some(PadInput {
            input: Input::Move(stick_push(held, axis, value)?),
            style: pad.style,
        })
    }
}

/// Reads mapped gilrs events for the connected pads, including pads attached while running.
pub struct Controller {
    gilrs: Gilrs,
    pads: Pads,
}

impl Controller {
    /// The keyboard still works if a platform has no gamepad backend.
    pub fn new() -> Option<Self> {
        match Gilrs::new() {
            Ok(gilrs) => {
                let mut pads = Pads::default();
                for (id, gamepad) in gilrs.gamepads() {
                    pads.connect(id.into(), gamepad_style(&gamepad));
                }
                Some(Self { gilrs, pads })
            }
            Err(err) => {
                eprintln!("controller input unavailable: {err}");
                None
            }
        }
    }

    /// Drain pending events. Connection and axis state alone do not switch the hints.
    pub fn poll(&mut self) -> Vec<PadInput> {
        let mut inputs = Vec::new();
        while let Some(Event { id, event, .. }) = self.gilrs.next_event() {
            let index = id.into();
            let input = match event {
                EventType::Connected => {
                    if let Some(gamepad) = self.gilrs.connected_gamepad(id) {
                        self.pads.connect(index, gamepad_style(&gamepad));
                    }
                    None
                }
                EventType::Disconnected => {
                    self.pads.disconnect(index);
                    None
                }
                EventType::ButtonPressed(button, _) => self.pads.button(index, button),
                EventType::AxisChanged(axis, value, _) => self.pads.axis(index, axis, value),
                _ => None,
            };
            inputs.extend(input);
        }
        inputs
    }
}

fn gamepad_style(gamepad: &Gamepad<'_>) -> Style {
    pad_style(gamepad.vendor_id(), gamepad.name(), gamepad.os_name())
}

fn pad_style(vendor: Option<u16>, mapped_name: &str, os_name: &str) -> Style {
    match vendor {
        Some(0x054c) => return Style::PlayStation,
        Some(0x057e) => return Style::Nintendo,
        Some(0x045e) => return Style::Xbox,
        _ => {}
    }
    // Some backends omit hardware IDs. The mapping and operating system names may differ.
    for name in [mapped_name, os_name] {
        let name = name.to_ascii_lowercase();
        if [
            "playstation",
            "dualshock",
            "dualsense",
            "ps4 controller",
            "ps5 controller",
        ]
        .iter()
        .any(|part| name.contains(part))
        {
            return Style::PlayStation;
        }
        if ["nintendo", "joy-con", "joycon", "switch pro"]
            .iter()
            .any(|part| name.contains(part))
        {
            return Style::Nintendo;
        }
    }
    Style::Xbox
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
    fn gilrs_identities_choose_prompt_styles() {
        for (vendor, mapped, os, style) in [
            (
                Some(0x045e),
                "Xbox One Controller",
                "Xbox Controller",
                Style::Xbox,
            ),
            (
                Some(0x054c),
                "PS4 Controller",
                "Wireless Controller",
                Style::PlayStation,
            ),
            (
                Some(0x054c),
                "Wireless Controller",
                "DualSense Wireless Controller",
                Style::PlayStation,
            ),
            (
                Some(0x057e),
                "Nintendo Switch Pro Controller",
                "Pro Controller",
                Style::Nintendo,
            ),
            (Some(0x057e), "Joy-Con (L)", "Joy-Con (L)", Style::Nintendo),
            (None, "Wireless Controller", "DualSense", Style::PlayStation),
            (None, "Unknown", "Joy-Con (R)", Style::Nintendo),
            (None, "Generic USB Gamepad", "Unknown", Style::Xbox),
            (
                Some(0x045e),
                "Nintendo emulating Xbox",
                "Xbox Controller",
                Style::Xbox,
            ),
        ] {
            assert_eq!(pad_style(vendor, mapped, os), style);
        }
    }

    #[test]
    fn hotplug_and_stick_state_are_per_pad() {
        let mut pads = Pads::default();
        assert_eq!(pads.button(1, Button::South), None);
        pads.connect(1, Style::PlayStation);
        pads.connect(2, Style::Nintendo);
        let right = Input::Move(Nav::Right);
        assert_eq!(
            pads.axis(1, Axis::LeftStickX, 0.9),
            Some(PadInput {
                input: right,
                style: Style::PlayStation,
            })
        );
        assert_eq!(
            pads.axis(2, Axis::LeftStickX, 0.9),
            Some(PadInput {
                input: right,
                style: Style::Nintendo,
            })
        );
        assert_eq!(pads.axis(1, Axis::LeftStickX, 1.0), None);
        pads.disconnect(1);
        assert_eq!(pads.axis(1, Axis::LeftStickX, 0.9), None);
        assert_eq!(pads.button(1, Button::South), None);
        assert_eq!(pads.axis(2, Axis::LeftStickX, 1.0), None);
        pads.connect(1, Style::PlayStation);
        assert_eq!(pads.axis(1, Axis::LeftStickX, 0.9).unwrap().input, right);
    }

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
