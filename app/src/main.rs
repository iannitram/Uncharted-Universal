//! The launcher (ADR 0003). So far: the Continue screen's place held, a language button that
//! steps through the seven interface languages, and About (version, patches, licences, the
//! Slint notice), all reachable with keyboard and controller focus.

mod focus;
mod input;
mod lang;
mod vm;

use focus::{Effect, Launcher, Nav};
use input::{Action, Controller};
use lang::Texts;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use uncharted_core::language::{self, Language};
use uncharted_core::patches::{Patch, Selection, Target};
use uncharted_core::platform::{AppDirs, Os, Vars};

slint::include_modules!();

/// The launcher's state outside the window.
struct App {
    launcher: Launcher,
    /// `ui_language`: "" is Automatic.
    setting: String,
    /// Where the setting is saved; `None` when the platform folders are not set.
    settings_file: Option<PathBuf>,
    system_locale: Option<String>,
    texts: Texts,
    /// The chosen game's patches and identity, for About. The Continue screen (piece 2.04)
    /// picks the game; until then the list is empty.
    patches: Vec<Patch>,
    target: Target,
    selection: Selection,
}

impl App {
    fn new(setting: String, settings_file: Option<PathBuf>, system_locale: Option<String>) -> App {
        let language = language::resolve(&setting, system_locale.as_deref());
        App {
            launcher: Launcher::default(),
            setting,
            settings_file,
            system_locale,
            texts: Texts::new(language),
            patches: Vec::new(),
            target: Target {
                title_id: String::new(),
                executable: String::new(),
                app_ver: String::new(),
            },
            selection: Selection::default(),
        }
    }

    fn language(&self) -> Language {
        self.texts.language()
    }

    /// The language button's value.
    fn language_name(&self) -> String {
        let name = self.language().native_name();
        if self.setting.is_empty() {
            self.texts.tr1("Automatic ({})", name)
        } else {
            name.to_owned()
        }
    }

    /// Step to the next choice, save it, and switch the texts.
    fn next_language(&mut self) {
        self.setting = language::next_setting(&self.setting).to_owned();
        if let Some(file) = &self.settings_file {
            if let Err(err) = language::write_setting(file, &self.setting) {
                eprintln!("could not save the interface language: {err}");
            }
        }
        let language = language::resolve(&self.setting, self.system_locale.as_deref());
        self.texts = Texts::new(language);
    }

    /// Push everything the window shows that Rust builds, in the current language.
    fn show(&self, window: &AppWindow) {
        if let Err(err) = slint::select_bundled_translation(self.language().code()) {
            eprintln!("could not select the interface language: {err}");
        }
        window.set_page(self.launcher.screen() as i32);
        window.set_focused_index(self.launcher.focused() as i32);
        window.set_language_name(self.language_name().into());
        window.set_version(env!("CARGO_PKG_VERSION").into());
        let patches: Vec<PatchRow> =
            vm::about::patch_rows(&self.patches, &self.target, &self.selection, &self.texts)
                .into_iter()
                .map(|row| PatchRow {
                    name: row.name.into(),
                    author: row.author.into(),
                    status: row.status.into(),
                    available: row.available,
                })
                .collect();
        window.set_patches(ModelRc::new(VecModel::from(patches)));
        window.set_shipped_licences(licence_model(vm::about::SHIPPED, &self.texts));
        window.set_learned_licences(licence_model(vm::about::LEARNED_FROM, &self.texts));
    }

    /// Carry out what an input means.
    fn apply(&mut self, effect: Effect, window: &AppWindow) {
        match effect {
            Effect::None => {}
            Effect::Focus(index) => window.set_focused_index(index as i32),
            Effect::Show(screen) => {
                window.set_page(screen as i32);
                window.set_focused_index(self.launcher.focused() as i32);
            }
            Effect::Scroll(steps) => window.invoke_scroll_about(steps),
            Effect::NextLanguage => {
                self.next_language();
                self.show(window);
            }
        }
    }
}

fn licence_model(list: &[vm::about::Licence], texts: &Texts) -> ModelRc<LicenceRow> {
    let rows: Vec<LicenceRow> = vm::about::licence_rows(list, texts)
        .into_iter()
        .map(|row| LicenceRow {
            name: row.name.into(),
            licence: row.licence.into(),
            role: row.role.into(),
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

/// Wire the window's callbacks to `app`.
fn connect(window: &AppWindow, app: &Rc<RefCell<App>>) {
    let input = |app: &Rc<RefCell<App>>, window: &AppWindow, f: &dyn Fn(&mut App) -> Effect| {
        let mut app = app.borrow_mut();
        let effect = f(&mut app);
        app.apply(effect, window);
    };
    let (weak, state) = (window.as_weak(), app.clone());
    window.on_nav(move |code| {
        if let (Some(nav), Some(window)) = (Nav::from_code(code), weak.upgrade()) {
            input(&state, &window, &|app| app.launcher.nav(nav));
        }
    });
    let (weak, state) = (window.as_weak(), app.clone());
    window.on_clicked(move |index| {
        if let Some(window) = weak.upgrade() {
            let index = usize::try_from(index).unwrap_or(0);
            input(&state, &window, &|app| app.launcher.click(index));
        }
    });
    let (weak, state) = (window.as_weak(), app.clone());
    window.on_activate(move || {
        if let Some(window) = weak.upgrade() {
            input(&state, &window, &|app| app.launcher.activate());
        }
    });
    let (weak, state) = (window.as_weak(), app.clone());
    window.on_back(move || {
        if let Some(window) = weak.upgrade() {
            input(&state, &window, &|app| app.launcher.back());
        }
    });
}

fn main() -> Result<(), slint::PlatformError> {
    let settings_file = Vars::from_lookup(Os::current(), |name| std::env::var(name).ok())
        .map(|vars| AppDirs::new(&vars).settings())
        .ok();
    let setting = settings_file
        .as_deref()
        .map(language::read_setting)
        .unwrap_or_default();
    let app = Rc::new(RefCell::new(App::new(
        setting,
        settings_file,
        sys_locale::get_locale(),
    )));

    let window = AppWindow::new()?;
    app.borrow().show(&window);
    connect(&window, &app);

    // Poll the controller on the UI thread; gilrs events are cheap to drain.
    let mut controller = Controller::new();
    let timer = slint::Timer::default();
    let weak = window.as_weak();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(16),
        move || {
            let (Some(controller), Some(window)) = (controller.as_mut(), weak.upgrade()) else {
                return;
            };
            for action in controller.poll() {
                match action {
                    Action::Move(nav) => window.invoke_nav(nav_code(nav)),
                    Action::Activate => window.invoke_activate(),
                    Action::Back => window.invoke_back(),
                }
            }
        },
    );

    window.run()
}

/// The `nav` callback's integer for `nav` (the inverse of `Nav::from_code`).
fn nav_code(nav: Nav) -> i32 {
    match nav {
        Nav::Up => 0,
        Nav::Down => 1,
        Nav::Left => 2,
        Nav::Right => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use focus::Screen;
    use i_slint_backend_testing::ElementHandle;
    use slint::Model;

    fn window_with(app: &Rc<RefCell<App>>) -> AppWindow {
        i_slint_backend_testing::init_no_event_loop();
        let window = AppWindow::new().unwrap();
        app.borrow().show(&window);
        connect(&window, app);
        window
    }

    fn shows(window: &AppWindow, label: &str) -> bool {
        !ElementHandle::find_by_accessible_label(window, label)
            .collect::<Vec<_>>()
            .is_empty()
    }

    #[test]
    fn nav_codes_round_trip() {
        for nav in [Nav::Up, Nav::Down, Nav::Left, Nav::Right] {
            assert_eq!(Nav::from_code(nav_code(nav)), Some(nav));
        }
    }

    // One test builds the window: the testing backend is per thread and Slint's translation
    // choice is global to it.
    #[test]
    fn about_and_the_language_button_in_the_window() {
        let dir = std::env::temp_dir().join(format!("uncharted-app-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("settings.json");
        let app = Rc::new(RefCell::new(App::new(
            String::new(),
            Some(file.clone()),
            Some("ko_KR".into()),
        )));
        let window = window_with(&app);

        // Automatic with an unsupported locale is English.
        assert_eq!(window.get_language_name(), "Automatic (English)");
        assert!(shows(&window, "About"));
        assert!(shows(&window, "Continue screen goes here"));
        // The Slint notice is on About only, and the licences are filled in.
        assert_eq!(window.get_page(), 0);
        assert!(!shows(&window, "Built with Slint"));
        assert_eq!(
            window.get_shipped_licences().row_count(),
            vm::about::SHIPPED.len()
        );

        // Right, A: About. Down scrolls, B goes back with the focus on About.
        window.invoke_nav(nav_code(Nav::Right));
        assert_eq!(window.get_focused_index(), 1);
        window.invoke_activate();
        assert_eq!(window.get_page(), Screen::About as i32);
        // Only what is on screen is found: the top of About, not yet its end.
        assert!(shows(&window, "Patches"));
        assert!(!shows(&window, "Built with Slint"));
        // Down scrolls to the end (and stops there); the Slint notice is at the bottom.
        for _ in 0..40 {
            window.invoke_nav(nav_code(Nav::Down));
        }
        assert!(shows(&window, "Built with Slint"));
        assert!(!shows(&window, "Patches"));
        window.invoke_nav(nav_code(Nav::Up));
        assert!(shows(&window, "Back"), "the Back button stays on screen");
        window.invoke_back();
        assert_eq!(window.get_page(), Screen::Home as i32);
        assert_eq!(window.get_focused_index(), 1);

        // The language button steps English -> Spanish; Slint's own texts follow, and the
        // choice is saved.
        window.invoke_clicked(0);
        assert_eq!(window.get_language_name(), "English");
        assert_eq!(language::read_setting(&file), "en");
        window.invoke_activate();
        assert_eq!(window.get_language_name(), "Español");
        assert_eq!(language::read_setting(&file), "es");
        assert!(shows(&window, "Acerca de"), "Slint texts are in Spanish");
        assert!(!shows(&window, "About"));
        let roles = window.get_shipped_licences();
        assert_eq!(
            roles.row_data(0).unwrap().role,
            "Motor gráfico (nuestro fork, en gpu/)"
        );

        // Every language switches without an error.
        for _ in 0..6 {
            window.invoke_activate();
        }
        assert_eq!(language::read_setting(&file), "");
        assert_eq!(window.get_language_name(), "Automatic (English)");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
