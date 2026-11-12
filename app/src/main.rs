//! The launcher (ADR 0003). Scaffold: a home page with the Continue screen's place held, a
//! placeholder button and the Fleet button, and the Fleet page (2.08). Keyboard and controller
//! focus move between the items; the page state lives in `pages` and `vm`, plain Rust.

mod focus;
mod input;
mod pages;
mod vm;

use focus::Nav;
use input::{Action, Controller};
use pages::{Pages, HOME_BUTTONS};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use vm::FleetVm;

slint::include_modules!();

/// A file of fleet records (`fleet::to_line` lines, this device first) the Fleet page reads
/// until the hub keeps real ones (roadmap 4.04); `app/fixtures/fleet.tsv` is an example.
const FLEET_RECORDS_VAR: &str = "UNCHARTED_FLEET_RECORDS";

fn load_fleet() -> FleetVm {
    let Some(path) = std::env::var_os(FLEET_RECORDS_VAR) else {
        return FleetVm::empty();
    };
    let read = std::fs::read_to_string(&path)
        .map_err(|err| err.to_string())
        .and_then(|text| FleetVm::from_text(&text).map_err(|err| err.to_string()));
    read.unwrap_or_else(|err| {
        eprintln!(
            "fleet records unreadable ({}): {err}",
            path.to_string_lossy()
        );
        FleetVm::empty()
    })
}

/// Copy the Fleet page's rows and texts into the window (once: they do not change while it
/// runs until hub sync lands).
fn show_fleet(window: &AppWindow, fleet: &FleetVm) {
    let rows: Vec<FleetRow> = fleet
        .rows()
        .iter()
        .map(|r| FleetRow {
            name: r.name.as_str().into(),
            detail: r.detail.as_str().into(),
            sync: r.sync.as_str().into(),
            free: r.free.as_str().into(),
        })
        .collect();
    window.set_fleet_rows(ModelRc::new(VecModel::from(rows)));
    window.set_fleet_warning(fleet.warning().unwrap_or_default().into());
    window.set_fleet_empty(fleet.empty_text().unwrap_or_default().into());
    window.set_fleet_back_index(fleet.back_index() as i32);
}

/// Copy the page and focus into the window after every press.
fn show_focus(window: &AppWindow, pages: &Pages) {
    window.set_page(pages.page() as i32);
    window.set_focused_index(pages.home_focus() as i32);
    window.set_fleet_focus(pages.fleet().focused() as i32);
}

fn main() -> Result<(), slint::PlatformError> {
    let window = AppWindow::new()?;
    let pages = Rc::new(RefCell::new(Pages::new(load_fleet())));
    window.set_home_buttons(ModelRc::new(VecModel::from(
        HOME_BUTTONS
            .iter()
            .map(|&label| SharedString::from(label))
            .collect::<Vec<_>>(),
    )));
    show_fleet(&window, pages.borrow().fleet());
    show_focus(&window, &pages.borrow());

    // Every input, from the keyboard, the mouse or the controller, goes through here.
    let apply = {
        let pages = pages.clone();
        let weak = window.as_weak();
        Rc::new(move |change: &dyn Fn(&mut Pages)| {
            change(&mut pages.borrow_mut());
            if let Some(window) = weak.upgrade() {
                show_focus(&window, &pages.borrow());
            }
        })
    };

    window.on_nav({
        let apply = apply.clone();
        move |code| {
            if let Some(nav) = Nav::from_code(code) {
                apply(&|p| p.on_nav(nav));
            }
        }
    });
    window.on_activate({
        let apply = apply.clone();
        move || apply(&|p| p.on_activate())
    });
    window.on_clicked({
        let apply = apply.clone();
        move |index| {
            if let Ok(index) = usize::try_from(index) {
                apply(&|p| p.on_click(index));
            }
        }
    });
    window.on_back({
        let apply = apply.clone();
        move || apply(&|p| p.on_back())
    });

    // Poll the controller on the UI thread; gilrs events are cheap to drain.
    let mut controller = Controller::new();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(16),
        move || {
            let Some(controller) = controller.as_mut() else {
                return;
            };
            for action in controller.poll() {
                match action {
                    Action::Move(nav) => apply(&|p| p.on_nav(nav)),
                    Action::Activate => apply(&|p| p.on_activate()),
                    Action::Back => apply(&|p| p.on_back()),
                }
            }
        },
    );

    window.run()
}
