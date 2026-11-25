//! Apple GameController model (#410, #441): which pad a GameController class and product
//! category describe, which button prompts that pad gets, and which of the game's pad
//! elements it can reach.
//!
//! On Apple the runtime reads pads through SDL3. The element map models SDL's GameController
//! backend (iOS/iPadOS, or macOS with `SDL_HINT_JOYSTICK_HIDAPI` off). On macOS the HIDAPI driver
//! usually takes Xbox, PS4, PS5, Switch Pro and Joy-Con pads first and sets its own element map and
//! quirks (SDL_mfijoystick.m skips those devices); the SDL type, so the prompt family, comes out
//! the same. This table models the wrapping so it can be tested in CI on every OS; it holds names
//! and numbers only and links nothing from Apple. It is pinned to SDL 3.4.20 (`sdl3-sys 0.7.3`,
//! `src/joystick/apple/SDL_mfijoystick.m`): an SDL bump must re-check it. It checks our model of
//! SDL. Whether a real pad reports these categories and elements is checked on the owner's device.
//!
//! SDL decides a pad's type from `GCController.productCategory` alone. [`layout`] also falls back
//! to the profile class, so a pad whose category SDL does not know can still be placed; that is the
//! one place the two differ (see the tests).

use super::host::{prompts_for_gamepad_type, PROMPTS_NINTENDO, PROMPTS_PLAYSTATION, PROMPTS_XBOX};
use super::input::ACTION_TABLE;

/// The prompt family a pad gets: which button pictures are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptFamily {
    Xbox,
    PlayStation,
    Nintendo,
}

impl PromptFamily {
    /// The `Host::prompts_controller` value.
    pub const fn value(self) -> i32 {
        match self {
            PromptFamily::Xbox => PROMPTS_XBOX,
            PromptFamily::PlayStation => PROMPTS_PLAYSTATION,
            PromptFamily::Nintendo => PROMPTS_NINTENDO,
        }
    }

    /// The family of a `Host::prompts_controller` value (the keyboard, 0, is not a pad).
    pub const fn from_value(value: i32) -> Option<PromptFamily> {
        match value {
            PROMPTS_XBOX => Some(PromptFamily::Xbox),
            PROMPTS_PLAYSTATION => Some(PromptFamily::PlayStation),
            PROMPTS_NINTENDO => Some(PromptFamily::Nintendo),
            _ => None,
        }
    }
}

/// The GameController profile class of a controller (`GCController.physicalInputProfile`'s
/// concrete class, or the extended profile the controller offers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GcClass {
    Xbox,
    DualShock,
    DualSense,
    Extended,
    /// Any other class (micro gamepad, remotes, a name this table does not know).
    Other,
}

impl GcClass {
    pub fn from_name(name: &str) -> GcClass {
        match name {
            "GCXboxGamepad" => GcClass::Xbox,
            "GCDualShockGamepad" => GcClass::DualShock,
            "GCDualSenseGamepad" => GcClass::DualSense,
            "GCExtendedGamepad" => GcClass::Extended,
            _ => GcClass::Other,
        }
    }
}

/// The physical shape of a pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Xbox,
    DualShock,
    DualSense,
    SwitchPro,
    JoyConPair,
    JoyConL,
    JoyConR,
    /// The MFi extended profile: face buttons, shoulders, triggers, d-pad, two sticks, Menu; thumbstick
    /// buttons, Options and Home when the pad has them.
    Mfi,
    /// An extended gamepad of unknown make (the extra buttons may or may not exist).
    GenericExtended,
}

impl Layout {
    pub const ALL: [Layout; 9] = [
        Layout::Xbox,
        Layout::DualShock,
        Layout::DualSense,
        Layout::SwitchPro,
        Layout::JoyConPair,
        Layout::JoyConL,
        Layout::JoyConR,
        Layout::Mfi,
        Layout::GenericExtended,
    ];
}

/// The `productCategory` strings SDL 3.4.20 matches (`IsControllerPS4` and its siblings).
pub const CATEGORY_DUALSHOCK: &str = "DualShock 4";
pub const CATEGORY_DUALSENSE: &str = "DualSense";
pub const CATEGORY_XBOX: &str = "Xbox One";
pub const CATEGORY_SWITCH_PRO: &str = "Switch Pro Controller";
pub const CATEGORY_JOYCON_L: &str = "Nintendo Switch Joy-Con (L)";
pub const CATEGORY_JOYCON_R: &str = "Nintendo Switch Joy-Con (R)";
pub const CATEGORY_JOYCON_PAIR: &str = "Nintendo Switch Joy-Con (L/R)";
/// Apple's category for an original MFi pad (SDL does not match it; it is the generic case).
pub const CATEGORY_MFI: &str = "MFi";

/// The pad a class and product category describe. The category decides (it is what SDL reads);
/// the class places a pad whose category is empty or unknown.
pub fn layout(class: GcClass, category: &str) -> Layout {
    match category {
        CATEGORY_DUALSHOCK => Layout::DualShock,
        CATEGORY_DUALSENSE => Layout::DualSense,
        CATEGORY_XBOX => Layout::Xbox,
        CATEGORY_SWITCH_PRO => Layout::SwitchPro,
        CATEGORY_JOYCON_L => Layout::JoyConL,
        CATEGORY_JOYCON_R => Layout::JoyConR,
        CATEGORY_JOYCON_PAIR => Layout::JoyConPair,
        CATEGORY_MFI => Layout::Mfi,
        _ => match class {
            GcClass::Xbox => Layout::Xbox,
            GcClass::DualShock => Layout::DualShock,
            GcClass::DualSense => Layout::DualSense,
            GcClass::Extended | GcClass::Other => Layout::GenericExtended,
        },
    }
}

/// The `SDL_GamepadType` SDL 3.4.20 reports for a product category, from the USB ids it gives each
/// (`SDL_mfijoystick.m`). 0 (unknown) for anything it does not match: those get Apple's vendor id,
/// and SDL's type for them is unknown or standard (0 or 1); both give Xbox prompts.
pub const fn sdl_gamepad_type(category: &str) -> i32 {
    // `match` on `&str` is not const; compare by hand.
    const fn is(a: &str, b: &str) -> bool {
        let (a, b) = (a.as_bytes(), b.as_bytes());
        if a.len() != b.len() {
            return false;
        }
        let mut i = 0;
        while i < a.len() {
            if a[i] != b[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    if is(category, CATEGORY_XBOX) {
        SDL_TYPE_XBOXONE
    } else if is(category, CATEGORY_DUALSHOCK) {
        SDL_TYPE_PS4
    } else if is(category, CATEGORY_DUALSENSE) {
        SDL_TYPE_PS5
    } else if is(category, CATEGORY_SWITCH_PRO) {
        SDL_TYPE_SWITCH_PRO
    } else if is(category, CATEGORY_JOYCON_L) {
        SDL_TYPE_JOYCON_LEFT
    } else if is(category, CATEGORY_JOYCON_R) {
        SDL_TYPE_JOYCON_RIGHT
    } else if is(category, CATEGORY_JOYCON_PAIR) {
        SDL_TYPE_JOYCON_PAIR
    } else {
        SDL_TYPE_UNKNOWN
    }
}

// `SDL_GamepadType` values of SDL 3.4.20 `SDL_gamepad.h`; the tests compare them with `sdl3-sys`.
pub const SDL_TYPE_UNKNOWN: i32 = 0;
pub const SDL_TYPE_XBOXONE: i32 = 3;
pub const SDL_TYPE_PS4: i32 = 5;
pub const SDL_TYPE_PS5: i32 = 6;
pub const SDL_TYPE_SWITCH_PRO: i32 = 7;
pub const SDL_TYPE_JOYCON_LEFT: i32 = 8;
pub const SDL_TYPE_JOYCON_RIGHT: i32 = 9;
pub const SDL_TYPE_JOYCON_PAIR: i32 = 10;

/// The prompt family the player sees: what SDL decides from the product category alone, composed
/// with the host's SDL-type table. PlayStation for DualShock and DualSense, Nintendo for the Switch
/// Pro and Joy-Cons, Xbox for everything else (Xbox, MFi, generic, unknown categories).
pub fn prompt_family(category: &str) -> PromptFamily {
    match prompts_for_gamepad_type(sdl_gamepad_type(category), false) {
        PROMPTS_PLAYSTATION => PromptFamily::PlayStation,
        PROMPTS_NINTENDO => PromptFamily::Nintendo,
        _ => PromptFamily::Xbox,
    }
}

/// The family this table would pick if the owner decides the profile class should place a pad
/// whose category SDL does not know. It is NOT what the player sees today: a DualShock-class pad
/// with an unknown category gets PlayStation here and Xbox from [`prompt_family`].
pub fn prompt_family_by_class(class: GcClass, category: &str) -> PromptFamily {
    match layout(class, category) {
        Layout::DualShock | Layout::DualSense => PromptFamily::PlayStation,
        Layout::SwitchPro | Layout::JoyConPair | Layout::JoyConL | Layout::JoyConR => {
            PromptFamily::Nintendo
        }
        Layout::Xbox | Layout::Mfi | Layout::GenericExtended => PromptFamily::Xbox,
    }
}

/// A physical element GameController reports (`GCInput*` names).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GcElement {
    ButtonA,
    ButtonB,
    ButtonX,
    ButtonY,
    LeftShoulder,
    RightShoulder,
    LeftTrigger,
    RightTrigger,
    LeftThumbstickButton,
    RightThumbstickButton,
    /// The Menu button (Start, Plus, Options on a DualShock/DualSense).
    ButtonMenu,
    /// The Options button (View, Minus, Share on a DualShock, Create on a DualSense).
    ButtonOptions,
    ButtonHome,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    LeftThumbstick,
    RightThumbstick,
    /// The DualShock/DualSense touchpad click.
    TouchpadButton,
}

/// An element of the game's pad: what the guest reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameElement {
    Cross,
    Circle,
    Square,
    Triangle,
    L1,
    R1,
    L2,
    R2,
    L3,
    R3,
    Options,
    Touchpad,
    Up,
    Down,
    Left,
    Right,
    LeftStick,
    RightStick,
}

impl GameElement {
    pub const ALL: [GameElement; 18] = [
        GameElement::Cross,
        GameElement::Circle,
        GameElement::Square,
        GameElement::Triangle,
        GameElement::L1,
        GameElement::R1,
        GameElement::L2,
        GameElement::R2,
        GameElement::L3,
        GameElement::R3,
        GameElement::Options,
        GameElement::Touchpad,
        GameElement::Up,
        GameElement::Down,
        GameElement::Left,
        GameElement::Right,
        GameElement::LeftStick,
        GameElement::RightStick,
    ];

    fn from_action(name: &str) -> Option<GameElement> {
        Some(match name {
            "cross" => GameElement::Cross,
            "circle" => GameElement::Circle,
            "square" => GameElement::Square,
            "triangle" => GameElement::Triangle,
            "l1" => GameElement::L1,
            "r1" => GameElement::R1,
            "l2" => GameElement::L2,
            "r2" => GameElement::R2,
            "l3" => GameElement::L3,
            "r3" => GameElement::R3,
            "options" => GameElement::Options,
            "touchpad" => GameElement::Touchpad,
            "up" => GameElement::Up,
            "down" => GameElement::Down,
            "left" => GameElement::Left,
            "right" => GameElement::Right,
            _ => return None,
        })
    }
}

/// The game element an SDL gamepad input name drives, through the default bindings
/// ([`ACTION_TABLE`]). `leftx` and `rightx` stand for the two axes of each stick. None for a name
/// the default bindings leave unbound (`guide`, `touchpad`, `misc1`, the paddles).
pub fn game_element_for_sdl(name: &str) -> Option<GameElement> {
    match name {
        "leftx" => Some(GameElement::LeftStick),
        "rightx" => Some(GameElement::RightStick),
        _ => ACTION_TABLE
            .iter()
            .find(|row| row.pad == Some(name))
            .and_then(|row| GameElement::from_action(row.name)),
    }
}

/// Whether an element is always there or only on some pads of the layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Always,
    /// Present only when the controller reports it (checked at run time).
    Optional,
}

/// One element a layout reports and the SDL input it becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub gc: GcElement,
    /// The SDL gamepad input name SDL 3.4.20 gives it (`SDL_mfijoystick.m` `IOS_JoystickGetGamepadMapping`).
    pub sdl: &'static str,
    pub presence: Presence,
}

const fn m(gc: GcElement, sdl: &'static str) -> Mapping {
    Mapping {
        gc,
        sdl,
        presence: Presence::Always,
    }
}

const fn opt(gc: GcElement, sdl: &'static str) -> Mapping {
    Mapping {
        gc,
        sdl,
        presence: Presence::Optional,
    }
}

use GcElement::*;

// Positional faces: GameController names the faces by their label on a Nintendo pad (A is east),
// so SDL swaps A/B and X/Y back to positions (`has_nintendo_buttons`).
const COMMON_STICKS_DPAD: [Mapping; 6] = [
    m(DpadUp, "dpup"),
    m(DpadDown, "dpdown"),
    m(DpadLeft, "dpleft"),
    m(DpadRight, "dpright"),
    m(LeftThumbstick, "leftx"),
    m(RightThumbstick, "rightx"),
];

const XBOX: &[Mapping] = &[
    m(ButtonA, "a"),
    m(ButtonB, "b"),
    m(ButtonX, "x"),
    m(ButtonY, "y"),
    m(LeftShoulder, "leftshoulder"),
    m(RightShoulder, "rightshoulder"),
    m(LeftTrigger, "lefttrigger"),
    m(RightTrigger, "righttrigger"),
    m(LeftThumbstickButton, "leftstick"),
    m(RightThumbstickButton, "rightstick"),
    m(ButtonMenu, "start"),
    m(ButtonOptions, "back"),
    m(ButtonHome, "guide"),
];

const PLAYSTATION: &[Mapping] = &[
    m(ButtonA, "a"),
    m(ButtonB, "b"),
    m(ButtonX, "x"),
    m(ButtonY, "y"),
    m(LeftShoulder, "leftshoulder"),
    m(RightShoulder, "rightshoulder"),
    m(LeftTrigger, "lefttrigger"),
    m(RightTrigger, "righttrigger"),
    m(LeftThumbstickButton, "leftstick"),
    m(RightThumbstickButton, "rightstick"),
    m(ButtonMenu, "start"),
    m(ButtonOptions, "back"),
    m(ButtonHome, "guide"),
    m(TouchpadButton, "touchpad"),
];

const NINTENDO_FACES: [Mapping; 4] = [
    m(ButtonA, "b"),
    m(ButtonB, "a"),
    m(ButtonX, "y"),
    m(ButtonY, "x"),
];

const SWITCH_PRO: &[Mapping] = &[
    NINTENDO_FACES[0],
    NINTENDO_FACES[1],
    NINTENDO_FACES[2],
    NINTENDO_FACES[3],
    m(LeftShoulder, "leftshoulder"),
    m(RightShoulder, "rightshoulder"),
    m(LeftTrigger, "lefttrigger"),
    m(RightTrigger, "righttrigger"),
    m(LeftThumbstickButton, "leftstick"),
    m(RightThumbstickButton, "rightstick"),
    m(ButtonMenu, "start"),
    m(ButtonOptions, "back"),
    m(ButtonHome, "guide"),
];

// The pair has no Home button (the right Joy-Con's Home is not reported in the combined profile).
const JOYCON_PAIR: &[Mapping] = &[
    NINTENDO_FACES[0],
    NINTENDO_FACES[1],
    NINTENDO_FACES[2],
    NINTENDO_FACES[3],
    m(LeftShoulder, "leftshoulder"),
    m(RightShoulder, "rightshoulder"),
    m(LeftTrigger, "lefttrigger"),
    m(RightTrigger, "righttrigger"),
    m(LeftThumbstickButton, "leftstick"),
    m(RightThumbstickButton, "rightstick"),
    m(ButtonMenu, "start"),
    m(ButtonOptions, "back"),
];

// A single Joy-Con held sideways: SDL has no Nintendo swap for it (`has_nintendo_buttons` is off)
// but swaps B and X, because the buttons are rotated a quarter turn. One stick, SL and SR as the
// shoulders, no triggers and no d-pad. L has Minus, R has Plus and Home.
const JOYCON_L: &[Mapping] = &[
    m(ButtonA, "a"),
    m(ButtonB, "x"),
    m(ButtonX, "b"),
    m(ButtonY, "y"),
    m(LeftShoulder, "leftshoulder"),
    m(RightShoulder, "rightshoulder"),
    m(LeftThumbstickButton, "leftstick"),
    m(ButtonOptions, "back"),
    m(LeftThumbstick, "leftx"),
];

const JOYCON_R: &[Mapping] = &[
    m(ButtonA, "a"),
    m(ButtonB, "x"),
    m(ButtonX, "b"),
    m(ButtonY, "y"),
    m(LeftShoulder, "leftshoulder"),
    m(RightShoulder, "rightshoulder"),
    m(RightThumbstickButton, "rightstick"),
    m(ButtonMenu, "start"),
    m(ButtonHome, "guide"),
    m(RightThumbstick, "rightx"),
];

// The MFi extended profile. Thumbstick buttons, Options and Home are optional: SDL maps them when
// the pad's physical input profile has them, and many pads reporting category "MFi" do.
const MFI: &[Mapping] = &[
    m(ButtonA, "a"),
    m(ButtonB, "b"),
    m(ButtonX, "x"),
    m(ButtonY, "y"),
    m(LeftShoulder, "leftshoulder"),
    m(RightShoulder, "rightshoulder"),
    m(LeftTrigger, "lefttrigger"),
    m(RightTrigger, "righttrigger"),
    opt(LeftThumbstickButton, "leftstick"),
    opt(RightThumbstickButton, "rightstick"),
    m(ButtonMenu, "start"),
    opt(ButtonOptions, "back"),
    opt(ButtonHome, "guide"),
];

// Unknown make: the same elements as the MFi set (SDL maps an unknown pad the same way).
const GENERIC: &[Mapping] = MFI;

/// The elements of a layout that map to the game's buttons, with the SDL input each becomes:
/// buttons, triggers, both sticks and the d-pad where it has them. Elements the game does not bind
/// are left out (Share on Series X|S pads, the Elite paddles, the extra DualSense inputs). A single
/// Joy-Con lists one stick and no triggers or d-pad: the rest is in [`Coverage::missing`].
pub fn elements(layout: Layout) -> Vec<Mapping> {
    let (base, with_sticks_dpad): (&[Mapping], bool) = match layout {
        Layout::Xbox => (XBOX, true),
        Layout::DualShock | Layout::DualSense => (PLAYSTATION, true),
        Layout::SwitchPro => (SWITCH_PRO, true),
        Layout::JoyConPair => (JOYCON_PAIR, true),
        Layout::JoyConL => (JOYCON_L, false),
        Layout::JoyConR => (JOYCON_R, false),
        Layout::Mfi => (MFI, true),
        Layout::GenericExtended => (GENERIC, true),
    };
    let mut all = base.to_vec();
    if with_sticks_dpad {
        all.extend_from_slice(&COMMON_STICKS_DPAD);
    }
    all
}

/// What a layout can and cannot give the game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coverage {
    /// Game elements the layout always reaches.
    pub reachable: Vec<GameElement>,
    /// Game elements reached only if the controller reports the optional button.
    pub optional: Vec<GameElement>,
    /// Game elements the layout cannot reach at all: the fallback decision belongs to the caller.
    pub missing: Vec<GameElement>,
}

pub fn coverage(layout: Layout) -> Coverage {
    let list = elements(layout);
    let mut cov = Coverage {
        reachable: Vec::new(),
        optional: Vec::new(),
        missing: Vec::new(),
    };
    for want in GameElement::ALL {
        let mut best: Option<Presence> = None;
        for e in &list {
            if game_element_for_sdl(e.sdl) == Some(want) {
                best = match (best, e.presence) {
                    (Some(Presence::Always), _) | (_, Presence::Always) => Some(Presence::Always),
                    _ => Some(Presence::Optional),
                };
            }
        }
        match best {
            Some(Presence::Always) => cov.reachable.push(want),
            Some(Presence::Optional) => cov.optional.push(want),
            None => cov.missing.push(want),
        }
    }
    cov
}

#[cfg(test)]
mod tests {
    use super::super::host::{prompts_for_gamepad_type, PROMPTS_KEYBOARD};
    use super::*;
    use sdl3_sys::gamepad::SDL_GamepadType;

    const CATEGORIES: [(&str, Layout, PromptFamily, i32); 7] = [
        (CATEGORY_XBOX, Layout::Xbox, PromptFamily::Xbox, 3),
        (
            CATEGORY_DUALSHOCK,
            Layout::DualShock,
            PromptFamily::PlayStation,
            5,
        ),
        (
            CATEGORY_DUALSENSE,
            Layout::DualSense,
            PromptFamily::PlayStation,
            6,
        ),
        (
            CATEGORY_SWITCH_PRO,
            Layout::SwitchPro,
            PromptFamily::Nintendo,
            7,
        ),
        (
            CATEGORY_JOYCON_L,
            Layout::JoyConL,
            PromptFamily::Nintendo,
            8,
        ),
        (
            CATEGORY_JOYCON_R,
            Layout::JoyConR,
            PromptFamily::Nintendo,
            9,
        ),
        (
            CATEGORY_JOYCON_PAIR,
            Layout::JoyConPair,
            PromptFamily::Nintendo,
            10,
        ),
    ];

    #[test]
    fn sdl_type_constants_match_the_pinned_sdl() {
        assert_eq!(SDL_TYPE_UNKNOWN, SDL_GamepadType::UNKNOWN.0);
        assert_eq!(SDL_TYPE_XBOXONE, SDL_GamepadType::XBOXONE.0);
        assert_eq!(SDL_TYPE_PS4, SDL_GamepadType::PS4.0);
        assert_eq!(SDL_TYPE_PS5, SDL_GamepadType::PS5.0);
        assert_eq!(SDL_TYPE_SWITCH_PRO, SDL_GamepadType::NINTENDO_SWITCH_PRO.0);
        assert_eq!(
            SDL_TYPE_JOYCON_LEFT,
            SDL_GamepadType::NINTENDO_SWITCH_JOYCON_LEFT.0
        );
        assert_eq!(
            SDL_TYPE_JOYCON_RIGHT,
            SDL_GamepadType::NINTENDO_SWITCH_JOYCON_RIGHT.0
        );
        assert_eq!(
            SDL_TYPE_JOYCON_PAIR,
            SDL_GamepadType::NINTENDO_SWITCH_JOYCON_PAIR.0
        );
    }

    #[test]
    fn every_category_gives_its_layout_type_and_family() {
        for (category, want_layout, want_family, want_type) in CATEGORIES {
            assert_eq!(
                layout(GcClass::Extended, category),
                want_layout,
                "{category}"
            );
            assert_eq!(sdl_gamepad_type(category), want_type, "{category}");
            assert_eq!(prompt_family(category), want_family, "{category}");
            // Composed with the host's SDL-type table, the family is the same.
            assert_eq!(
                prompts_for_gamepad_type(sdl_gamepad_type(category), false),
                want_family.value(),
                "{category}"
            );
        }
    }

    #[test]
    fn category_wins_over_class() {
        let cases = [
            (CATEGORY_SWITCH_PRO, Layout::SwitchPro),
            (CATEGORY_JOYCON_PAIR, Layout::JoyConPair),
        ];
        for class in [
            GcClass::Xbox,
            GcClass::DualShock,
            GcClass::DualSense,
            GcClass::Extended,
            GcClass::Other,
        ] {
            for (category, want) in cases {
                assert_eq!(layout(class, category), want, "{class:?} {category}");
                assert_eq!(prompt_family(category), PromptFamily::Nintendo);
            }
        }
    }

    #[test]
    fn every_class_places_a_pad_with_no_known_category() {
        for category in ["", "HID", "Unknown Pad"] {
            assert_eq!(layout(GcClass::Xbox, category), Layout::Xbox);
            assert_eq!(layout(GcClass::DualShock, category), Layout::DualShock);
            assert_eq!(layout(GcClass::DualSense, category), Layout::DualSense);
            assert_eq!(layout(GcClass::Extended, category), Layout::GenericExtended);
            assert_eq!(layout(GcClass::Other, category), Layout::GenericExtended);
            assert_eq!(
                prompt_family_by_class(GcClass::Xbox, category),
                PromptFamily::Xbox
            );
            assert_eq!(
                prompt_family_by_class(GcClass::DualShock, category),
                PromptFamily::PlayStation
            );
            assert_eq!(
                prompt_family_by_class(GcClass::DualSense, category),
                PromptFamily::PlayStation
            );
            assert_eq!(
                prompt_family_by_class(GcClass::Extended, category),
                PromptFamily::Xbox
            );
            assert_eq!(
                prompt_family_by_class(GcClass::Other, category),
                PromptFamily::Xbox
            );
        }
    }

    #[test]
    fn class_names_parse() {
        assert_eq!(GcClass::from_name("GCXboxGamepad"), GcClass::Xbox);
        assert_eq!(GcClass::from_name("GCDualShockGamepad"), GcClass::DualShock);
        assert_eq!(GcClass::from_name("GCDualSenseGamepad"), GcClass::DualSense);
        assert_eq!(GcClass::from_name("GCExtendedGamepad"), GcClass::Extended);
        assert_eq!(GcClass::from_name("GCMicroGamepad"), GcClass::Other);
        assert_eq!(GcClass::from_name(""), GcClass::Other);
    }

    #[test]
    fn mfi_and_unknown_pads_get_xbox_prompts() {
        assert_eq!(layout(GcClass::Extended, CATEGORY_MFI), Layout::Mfi);
        assert_eq!(sdl_gamepad_type(CATEGORY_MFI), SDL_TYPE_UNKNOWN);
        assert_eq!(prompt_family(CATEGORY_MFI), PromptFamily::Xbox);
        assert_eq!(prompt_family("something new"), PromptFamily::Xbox);
        // SDL reports unknown (0) or standard (1) for them; both are Xbox.
        for kind in [0, 1] {
            assert_eq!(prompts_for_gamepad_type(kind, false), PROMPTS_XBOX);
        }
    }

    #[test]
    fn the_one_place_the_table_and_sdl_differ() {
        // SDL reads only the category: a DualShock-class pad with an unknown category is a generic
        // pad to it (Xbox prompts). This table places it by class (PlayStation prompts).
        assert_eq!(
            prompts_for_gamepad_type(sdl_gamepad_type("HID"), false),
            PROMPTS_XBOX
        );
        assert_eq!(
            prompt_family_by_class(GcClass::DualShock, "HID"),
            PromptFamily::PlayStation
        );
        // `prompt_family` follows SDL, so it does not predict the class fallback.
        assert_eq!(prompt_family("HID"), PromptFamily::Xbox);
    }

    #[test]
    fn family_values_round_trip() {
        for f in [
            PromptFamily::Xbox,
            PromptFamily::PlayStation,
            PromptFamily::Nintendo,
        ] {
            assert_eq!(PromptFamily::from_value(f.value()), Some(f));
        }
        assert_eq!(PromptFamily::from_value(PROMPTS_KEYBOARD), None);
    }

    fn sdl_of(layout: Layout, gc: GcElement) -> Option<&'static str> {
        elements(layout).iter().find(|e| e.gc == gc).map(|e| e.sdl)
    }

    #[test]
    fn every_element_of_every_layout_reaches_a_game_element_or_is_a_known_extra() {
        for layout in Layout::ALL {
            for e in elements(layout) {
                let reached = game_element_for_sdl(e.sdl);
                // The only reported elements that reach nothing: the Home button and the
                // DualShock/DualSense touchpad click, which the default bindings leave unbound.
                match e.gc {
                    ButtonHome | TouchpadButton => {
                        assert_eq!(reached, None, "{layout:?} {:?}", e.gc)
                    }
                    _ => assert!(reached.is_some(), "{layout:?} {:?} -> {}", e.gc, e.sdl),
                }
            }
        }
    }

    #[test]
    fn no_two_elements_of_a_layout_drive_the_same_sdl_input() {
        for layout in Layout::ALL {
            let list = elements(layout);
            for (i, a) in list.iter().enumerate() {
                for b in &list[i + 1..] {
                    assert_ne!(a.sdl, b.sdl, "{layout:?} {:?} and {:?}", a.gc, b.gc);
                    assert_ne!(a.gc, b.gc, "{layout:?}");
                }
            }
        }
    }

    #[test]
    fn faces_land_on_positions_for_every_layout() {
        // South is Cross, east Circle, west Square, north Triangle, whatever the label.
        for layout in [
            Layout::Xbox,
            Layout::DualShock,
            Layout::DualSense,
            Layout::Mfi,
            Layout::GenericExtended,
        ] {
            assert_eq!(sdl_of(layout, ButtonA), Some("a"), "{layout:?}");
            assert_eq!(sdl_of(layout, ButtonB), Some("b"), "{layout:?}");
            assert_eq!(sdl_of(layout, ButtonX), Some("x"), "{layout:?}");
            assert_eq!(sdl_of(layout, ButtonY), Some("y"), "{layout:?}");
        }
        // Nintendo labels: A is east, B south, X north, Y west.
        for layout in [Layout::SwitchPro, Layout::JoyConPair] {
            assert_eq!(sdl_of(layout, ButtonA), Some("b"), "{layout:?}");
            assert_eq!(sdl_of(layout, ButtonB), Some("a"), "{layout:?}");
            assert_eq!(sdl_of(layout, ButtonX), Some("y"), "{layout:?}");
            assert_eq!(sdl_of(layout, ButtonY), Some("x"), "{layout:?}");
        }
        // A single Joy-Con swaps only B and X (the quarter turn).
        for layout in [Layout::JoyConL, Layout::JoyConR] {
            assert_eq!(sdl_of(layout, ButtonA), Some("a"), "{layout:?}");
            assert_eq!(sdl_of(layout, ButtonB), Some("x"), "{layout:?}");
            assert_eq!(sdl_of(layout, ButtonX), Some("b"), "{layout:?}");
            assert_eq!(sdl_of(layout, ButtonY), Some("y"), "{layout:?}");
        }
        assert_eq!(game_element_for_sdl("a"), Some(GameElement::Cross));
        assert_eq!(game_element_for_sdl("b"), Some(GameElement::Circle));
        assert_eq!(game_element_for_sdl("x"), Some(GameElement::Square));
        assert_eq!(game_element_for_sdl("y"), Some(GameElement::Triangle));
    }

    #[test]
    fn menu_and_options_land_on_the_game_buttons() {
        assert_eq!(game_element_for_sdl("start"), Some(GameElement::Options));
        assert_eq!(game_element_for_sdl("back"), Some(GameElement::Touchpad));
        for layout in [
            Layout::Xbox,
            Layout::DualShock,
            Layout::DualSense,
            Layout::SwitchPro,
            Layout::JoyConPair,
        ] {
            assert_eq!(sdl_of(layout, ButtonMenu), Some("start"));
            assert_eq!(sdl_of(layout, ButtonOptions), Some("back"));
        }
        // The touchpad click and Home are not bound by default.
        assert_eq!(game_element_for_sdl("touchpad"), None);
        assert_eq!(game_element_for_sdl("guide"), None);
        assert_eq!(sdl_of(Layout::DualShock, TouchpadButton), Some("touchpad"));
        assert_eq!(sdl_of(Layout::DualSense, TouchpadButton), Some("touchpad"));
        assert_eq!(sdl_of(Layout::Xbox, TouchpadButton), None);
    }

    #[test]
    fn sticks_and_triggers_are_reached() {
        assert_eq!(game_element_for_sdl("leftx"), Some(GameElement::LeftStick));
        assert_eq!(
            game_element_for_sdl("rightx"),
            Some(GameElement::RightStick)
        );
        assert_eq!(game_element_for_sdl("lefttrigger"), Some(GameElement::L2));
        assert_eq!(game_element_for_sdl("righttrigger"), Some(GameElement::R2));
        assert_eq!(game_element_for_sdl("leftstick"), Some(GameElement::L3));
        assert_eq!(game_element_for_sdl("rightstick"), Some(GameElement::R3));
        assert_eq!(game_element_for_sdl("leftshoulder"), Some(GameElement::L1));
        assert_eq!(game_element_for_sdl("rightshoulder"), Some(GameElement::R1));
        assert_eq!(game_element_for_sdl("dpup"), Some(GameElement::Up));
        assert_eq!(game_element_for_sdl("dpdown"), Some(GameElement::Down));
        assert_eq!(game_element_for_sdl("dpleft"), Some(GameElement::Left));
        assert_eq!(game_element_for_sdl("dpright"), Some(GameElement::Right));
        assert_eq!(game_element_for_sdl("nonsense"), None);
    }

    fn all_of(except: &[GameElement]) -> Vec<GameElement> {
        GameElement::ALL
            .iter()
            .copied()
            .filter(|e| !except.contains(e))
            .collect()
    }

    #[test]
    fn full_pads_reach_every_game_element() {
        for layout in [
            Layout::Xbox,
            Layout::DualShock,
            Layout::DualSense,
            Layout::SwitchPro,
            Layout::JoyConPair,
        ] {
            let c = coverage(layout);
            assert_eq!(c.reachable, GameElement::ALL.to_vec(), "{layout:?}");
            assert!(c.optional.is_empty() && c.missing.is_empty(), "{layout:?}");
        }
    }

    #[test]
    fn mfi_thumbstick_clicks_and_options_are_optional() {
        let c = coverage(Layout::Mfi);
        let optional = [GameElement::L3, GameElement::R3, GameElement::Touchpad];
        assert_eq!(c.optional, optional.to_vec());
        assert_eq!(c.reachable, all_of(&optional));
        assert!(c.missing.is_empty());
    }

    #[test]
    fn generic_pad_has_optional_extras() {
        let c = coverage(Layout::GenericExtended);
        let optional = [GameElement::L3, GameElement::R3, GameElement::Touchpad];
        assert_eq!(c.optional, optional.to_vec());
        assert_eq!(c.reachable, all_of(&optional));
        assert!(c.missing.is_empty());
    }

    #[test]
    fn single_joycons_report_what_they_lack() {
        let no_dpad_triggers = [
            GameElement::L2,
            GameElement::R2,
            GameElement::Up,
            GameElement::Down,
            GameElement::Left,
            GameElement::Right,
        ];
        let left = coverage(Layout::JoyConL);
        let mut lacks = no_dpad_triggers.to_vec();
        lacks.extend([
            GameElement::R3,
            GameElement::Options,
            GameElement::RightStick,
        ]);
        lacks.sort_by_key(|e| GameElement::ALL.iter().position(|a| a == e));
        assert_eq!(left.missing, lacks);
        assert_eq!(left.reachable, all_of(&lacks));
        assert!(left.optional.is_empty());

        let right = coverage(Layout::JoyConR);
        let mut lacks = no_dpad_triggers.to_vec();
        lacks.extend([
            GameElement::L3,
            GameElement::Touchpad,
            GameElement::LeftStick,
        ]);
        lacks.sort_by_key(|e| GameElement::ALL.iter().position(|a| a == e));
        assert_eq!(right.missing, lacks);
        assert_eq!(right.reachable, all_of(&lacks));
        assert!(right.optional.is_empty());
    }

    #[test]
    fn every_game_element_is_classified_for_every_layout() {
        for layout in Layout::ALL {
            let c = coverage(layout);
            assert_eq!(
                c.reachable.len() + c.optional.len() + c.missing.len(),
                GameElement::ALL.len(),
                "{layout:?}"
            );
        }
    }

    #[test]
    fn game_elements_match_the_default_binding_table() {
        // Every pad-bound action in the default table maps to a game element, so a new action
        // cannot be added without this table learning of it.
        for row in ACTION_TABLE.iter().filter(|r| r.pad.is_some()) {
            assert!(GameElement::from_action(row.name).is_some(), "{}", row.name);
        }
    }
}
