//! The launcher (ADR 0003). Scaffold: an empty window with the Continue screen's place held
//! and two placeholder buttons that keyboard and controller focus move between.

mod focus;
mod input;

use focus::{FocusRing, Nav};
use input::{Action, Controller};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

slint::include_modules!();

const BUTTONS: usize = 2;

fn main() -> Result<(), slint::PlatformError> {
    let window = AppWindow::new()?;
    let ring = Rc::new(RefCell::new(FocusRing::new(BUTTONS)));

    let apply_nav = {
        let ring = ring.clone();
        let weak = window.as_weak();
        Rc::new(move |nav: Nav| {
            ring.borrow_mut().step(nav);
            if let Some(window) = weak.upgrade() {
                window.set_focused_index(ring.borrow().index() as i32);
            }
        })
    };

    window.on_nav({
        let apply_nav = apply_nav.clone();
        move |code| {
            if let Some(nav) = Nav::from_code(code) {
                apply_nav(nav);
            }
        }
    });
    window.on_activate(|index| println!("activated placeholder button {index}"));

    // Poll the controller on the UI thread; gilrs events are cheap to drain.
    let mut controller = Controller::new();
    let timer = slint::Timer::default();
    let weak = window.as_weak();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(16),
        move || {
            let Some(controller) = controller.as_mut() else {
                return;
            };
            for action in controller.poll() {
                match action {
                    Action::Move(nav) => apply_nav(nav),
                    Action::Activate => {
                        if let Some(window) = weak.upgrade() {
                            window.invoke_activate(window.get_focused_index());
                        }
                    }
                }
            }
        },
    );

    window.run()
}
