//! Android NativeActivity entry scaffold and startup bootstrapper (Stage 7.03).
//!
//! Validates Android manifest/packaging preconditions and explicit Turnip/system Vulkan driver
//! identity before Slint window creation or `uncharted_runtime` launch.

use crate::display::{DisplayAdaptation, DisplayError, DisplayMetrics, DisplayState};
use crate::driver::{
    open_validated_driver, DriverLoaderProbe, DriverMode, DriverValidationError, TurnipDriverConfig,
};
pub use crate::game::{AndroidGame, FIRST_BOOT_DOWNLOAD_PROMPT};
use crate::game::{FirstBootDecision, FirstBootState};
use crate::gamepad::{AndroidAxisEvent, AndroidGamepadAdapter, AndroidKeyEvent};
use std::fmt;

pub const DEFAULT_PACKAGE_NAME: &str = AndroidGame::U1.application_id();
pub const MIN_SUPPORTED_ANDROID_API: u32 = 33;
pub const TARGET_ANDROID_API: u32 = 35;

/// Launch parameters supplied by Android `NativeActivity` / app preferences.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndroidLaunchConfig {
    pub game: AndroidGame,
    pub package_name: String,
    pub min_sdk_version: u32,
    pub target_sdk_version: u32,
    /// M9 invariant: thin APKs must never bundle game assets.
    pub bundled_game_assets: bool,
    /// Whether prepared per-game files are already present on device storage.
    pub game_files_present: bool,
    pub display: DisplayMetrics,
    pub driver: TurnipDriverConfig,
}

impl AndroidLaunchConfig {
    pub fn for_game_with_bundled_turnip(
        game: AndroidGame,
        native_library_dir: impl Into<String>,
        app_storage_dir: impl Into<String>,
        custom_driver_so: impl Into<String>,
    ) -> Self {
        Self {
            game,
            package_name: game.application_id().to_owned(),
            min_sdk_version: MIN_SUPPORTED_ANDROID_API,
            target_sdk_version: TARGET_ANDROID_API,
            bundled_game_assets: false,
            game_files_present: false,
            display: DisplayMetrics::default(),
            driver: TurnipDriverConfig::bundled_turnip(
                native_library_dir,
                app_storage_dir,
                custom_driver_so,
            ),
        }
    }

    pub fn for_game_with_system_mali(game: AndroidGame) -> Self {
        Self {
            game,
            package_name: game.application_id().to_owned(),
            min_sdk_version: MIN_SUPPORTED_ANDROID_API,
            target_sdk_version: TARGET_ANDROID_API,
            bundled_game_assets: false,
            game_files_present: false,
            display: DisplayMetrics::default(),
            driver: TurnipDriverConfig::system_mali(),
        }
    }

    pub fn with_display(mut self, width: u32, height: u32) -> Self {
        self.display = DisplayMetrics::new(width, height);
        self
    }

    pub fn with_bundled_turnip(
        native_library_dir: impl Into<String>,
        app_storage_dir: impl Into<String>,
        custom_driver_so: impl Into<String>,
    ) -> Self {
        let game = AndroidGame::compiled_default().unwrap_or(AndroidGame::U1);
        Self::for_game_with_bundled_turnip(
            game,
            native_library_dir,
            app_storage_dir,
            custom_driver_so,
        )
    }
}

/// Errors that prevent launching the Slint Android APK UI or Vulkan runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BootstrapError {
    MissingPackageName,
    PackageGameMismatch {
        game: AndroidGame,
        package_name: String,
        expected: String,
    },
    BundledGameAssetsForbidden,
    UnsupportedApiLevel {
        min_sdk: u32,
        required: u32,
    },
    TargetSdkBelowMin {
        min_sdk: u32,
        target_sdk: u32,
    },
    Display(DisplayError),
    Driver(DriverValidationError),
}

impl From<DisplayError> for BootstrapError {
    fn from(err: DisplayError) -> Self {
        Self::Display(err)
    }
}

impl From<DriverValidationError> for BootstrapError {
    fn from(err: DriverValidationError) -> Self {
        Self::Driver(err)
    }
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPackageName => write!(f, "Android package name must be non-empty"),
            Self::PackageGameMismatch {
                game,
                package_name,
                expected,
            } => write!(
                f,
                "Android package {package_name} does not match thin APK game {} (expected {expected})",
                game.key()
            ),
            Self::BundledGameAssetsForbidden => write!(
                f,
                "thin Android APKs must not bundle game assets (M9): use Stage 7.04 first-boot download"
            ),
            Self::UnsupportedApiLevel { min_sdk, required } => {
                write!(f, "Android minSdkVersion {min_sdk} < required {required}")
            }
            Self::TargetSdkBelowMin {
                min_sdk,
                target_sdk,
            } => write!(
                f,
                "Android targetSdkVersion {target_sdk} < minSdkVersion {min_sdk}"
            ),
            Self::Display(err) => write!(f, "{err}"),
            Self::Driver(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for BootstrapError {}

/// Bootstrapped Android APK session holding the validated Vulkan driver handle, gamepad router,
/// Stage 7.04 first-boot download hook, and Stage 7.07 M21 display adaptation state.
pub struct AndroidApkSession<H> {
    pub game: AndroidGame,
    pub package_name: String,
    pub driver_mode: DriverMode,
    pub reported_driver_name: String,
    pub driver_handle: H,
    pub gamepad: AndroidGamepadAdapter,
    pub first_boot: FirstBootState,
    pub display: DisplayState,
}

impl<H> AndroidApkSession<H> {
    /// Connects a controller and hides the on-screen touch overlay.
    pub fn connect_gamepad(&mut self, device_id: i32, vendor_id: Option<u16>, name: &str) {
        self.gamepad.connect_device(device_id, vendor_id, name);
        self.sync_touch_overlay_visibility();
    }

    /// Disconnects a controller and restores the on-screen touch overlay when none remain.
    pub fn disconnect_gamepad(&mut self, device_id: i32) {
        self.gamepad.disconnect_device(device_id);
        self.sync_touch_overlay_visibility();
    }

    /// Synchronizes touch-overlay visibility with `self.gamepad.device_count()`.
    pub fn sync_touch_overlay_visibility(&mut self) -> &DisplayAdaptation {
        self.display
            .set_controller_connected(self.gamepad.device_count() > 0)
    }

    /// Applies a live Android `WindowMetrics` / `Configuration` fold/unfold resize event.
    pub fn on_display_change(
        &mut self,
        width: u32,
        height: u32,
        timestamp_ms: u64,
    ) -> Result<&DisplayAdaptation, DisplayError> {
        self.display
            .on_display_change(DisplayMetrics::new(width, height), timestamp_ms)
    }

    /// Advances the resize-restart debounce clock.
    pub fn tick_display(&mut self, now_ms: u64) -> &DisplayAdaptation {
        self.display.tick(now_ms)
    }

    /// Routes a hardware/virtual gamepad key event through the Slint adapter and applies it to
    /// the Stage 7.04 first-boot prompt when active.
    pub fn route_first_boot_key(&mut self, event: AndroidKeyEvent) -> Option<FirstBootDecision> {
        let mapped = self.gamepad.handle_key_event(event)?;
        Some(self.first_boot.handle_input(mapped.input))
    }

    /// Routes an analog stick / D-pad hat axis event through the Slint adapter and applies it to
    /// the Stage 7.04 first-boot prompt when active.
    pub fn route_first_boot_axis(&mut self, event: AndroidAxisEvent) -> Option<FirstBootDecision> {
        let mapped = self.gamepad.handle_axis_event(event)?;
        Some(self.first_boot.handle_input(mapped.input))
    }
}

/// Validates manifest/API constraints and opens + verifies the selected Vulkan driver fail-closed
/// prior to starting the Slint launcher loop.
pub fn bootstrap_session<P: DriverLoaderProbe>(
    config: &AndroidLaunchConfig,
    probe: &mut P,
) -> Result<AndroidApkSession<P::Handle>, BootstrapError> {
    if config.package_name.trim().is_empty() {
        return Err(BootstrapError::MissingPackageName);
    }
    let expected_pkg = config.game.application_id();
    if config.package_name != expected_pkg {
        return Err(BootstrapError::PackageGameMismatch {
            game: config.game,
            package_name: config.package_name.clone(),
            expected: expected_pkg.to_owned(),
        });
    }
    if config.bundled_game_assets {
        return Err(BootstrapError::BundledGameAssetsForbidden);
    }
    if config.min_sdk_version < MIN_SUPPORTED_ANDROID_API {
        return Err(BootstrapError::UnsupportedApiLevel {
            min_sdk: config.min_sdk_version,
            required: MIN_SUPPORTED_ANDROID_API,
        });
    }
    if config.target_sdk_version < config.min_sdk_version {
        return Err(BootstrapError::TargetSdkBelowMin {
            min_sdk: config.min_sdk_version,
            target_sdk: config.target_sdk_version,
        });
    }

    let display = DisplayState::new(config.display, false)?;
    let validated = open_validated_driver(&config.driver, probe)?;
    Ok(AndroidApkSession {
        game: config.game,
        package_name: config.package_name.clone(),
        driver_mode: validated.mode,
        reported_driver_name: validated.reported_name,
        driver_handle: validated.handle,
        gamepad: AndroidGamepadAdapter::default(),
        first_boot: FirstBootState::evaluate(config.game, config.game_files_present),
        display,
    })
}

/// Opaque FFI handle wrapping [`AndroidApkSession`] for `android/apk/native/android_bridge.cpp`.
pub struct AndroidApkFfiSession {
    inner: AndroidApkSession<()>,
}

struct FfiBootstrapProbe {
    driver_name: String,
}

impl DriverLoaderProbe for FfiBootstrapProbe {
    type Handle = ();

    fn open_system(&mut self, _so_name: &str) -> Result<Self::Handle, String> {
        Ok(())
    }

    fn open_custom(
        &mut self,
        _native_library_dir: &str,
        _parts: &crate::driver::DriverPathParts,
    ) -> Result<Self::Handle, String> {
        Ok(())
    }

    fn query_driver_name(&mut self, _handle: &Self::Handle) -> Result<String, String> {
        Ok(self.driver_name.clone())
    }

    fn close_handle(&mut self, _handle: Self::Handle) {}
}

unsafe fn cstr_to_str<'a>(ptr: *const core::ffi::c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    core::ffi::CStr::from_ptr(ptr).to_str().ok()
}

/// Returns the compile-time selected game key (`"u1"`, `"u2"`, `"u3"`, or `""`).
#[no_mangle]
pub extern "C" fn uncharted_apk_compiled_game_key() -> *const core::ffi::c_char {
    match AndroidGame::compiled_default() {
        Some(AndroidGame::U1) => c"u1".as_ptr(),
        Some(AndroidGame::U2) => c"u2".as_ptr(),
        Some(AndroidGame::U3) => c"u3".as_ptr(),
        None => c"".as_ptr(),
    }
}

/// Returns the exact Stage 7.04 first-boot download prompt string.
#[no_mangle]
pub extern "C" fn uncharted_apk_first_boot_prompt_text() -> *const core::ffi::c_char {
    c"Would you like to download the rest of the game files?".as_ptr()
}

/// Creates and validates an Android thin-APK session from `NativeActivity` / JNI metadata.
///
/// # Safety
/// All non-null string pointers must be valid NUL-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn uncharted_apk_session_create(
    game_id: *const core::ffi::c_char,
    package_name: *const core::ffi::c_char,
    _native_lib_dir: *const core::ffi::c_char,
    _app_storage_dir: *const core::ffi::c_char,
    gpu_vendor_id: u32,
    gpu_driver_name: *const core::ffi::c_char,
    display_width: u32,
    display_height: u32,
    game_files_present: bool,
) -> *mut AndroidApkFfiSession {
    let game = cstr_to_str(game_id)
        .and_then(AndroidGame::from_key)
        .or_else(AndroidGame::compiled_default);
    let Some(game) = game else {
        return core::ptr::null_mut();
    };

    let pkg = cstr_to_str(package_name)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| game.application_id());

    let driver_name = cstr_to_str(gpu_driver_name)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("system-vulkan");

    let mut cfg = AndroidLaunchConfig::for_game_with_system_mali(game);
    cfg.driver = if gpu_vendor_id == crate::driver::VENDOR_ID_QUALCOMM {
        TurnipDriverConfig::system_adreno()
    } else {
        TurnipDriverConfig::system_mali()
    };
    cfg.driver.expected_driver_name = driver_name.to_owned();
    cfg.package_name = pkg.to_owned();
    cfg.bundled_game_assets = false;
    cfg.game_files_present = game_files_present;
    if display_width > 0 && display_height > 0 {
        cfg.display = DisplayMetrics::new(display_width, display_height);
    }

    let mut probe = FfiBootstrapProbe {
        driver_name: driver_name.to_owned(),
    };
    match bootstrap_session(&cfg, &mut probe) {
        Ok(session) => Box::into_raw(Box::new(AndroidApkFfiSession { inner: session })),
        Err(_) => core::ptr::null_mut(),
    }
}

/// Destroys a session created by [`uncharted_apk_session_create`].
///
/// # Safety
/// `session` must be null or a live pointer returned by [`uncharted_apk_session_create`].
#[no_mangle]
pub unsafe extern "C" fn uncharted_apk_session_destroy(session: *mut AndroidApkFfiSession) {
    if !session.is_null() {
        drop(Box::from_raw(session));
    }
}

/// Registers a connected Android input device and hides the touch overlay.
///
/// # Safety
/// `session` must be a live pointer; `name` must be null or a valid NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn uncharted_apk_session_connect_gamepad(
    session: *mut AndroidApkFfiSession,
    device_id: i32,
    vendor_id: i32,
    name: *const core::ffi::c_char,
) {
    let Some(session) = session.as_mut() else {
        return;
    };
    let vendor = u16::try_from(vendor_id).ok();
    let dev_name = cstr_to_str(name).unwrap_or("Android Gamepad");
    session.inner.connect_gamepad(device_id, vendor, dev_name);
}

/// Unregisters a disconnected Android input device.
///
/// # Safety
/// `session` must be null or a live pointer.
#[no_mangle]
pub unsafe extern "C" fn uncharted_apk_session_disconnect_gamepad(
    session: *mut AndroidApkFfiSession,
    device_id: i32,
) {
    if let Some(session) = session.as_mut() {
        session.inner.disconnect_gamepad(device_id);
    }
}

/// Routes an Android `AKeyEvent` through [`AndroidGamepadAdapter`] and [`FirstBootState`].
/// Returns the Slint `Input::from_code` integer (`0..=10`), or `-1` if unmapped/ignored.
///
/// `action`: `0` = `AKEY_EVENT_ACTION_DOWN`, non-zero = up/multiple.
///
/// # Safety
/// `session` must be null or a live pointer.
#[no_mangle]
pub unsafe extern "C" fn uncharted_apk_session_on_key_event(
    session: *mut AndroidApkFfiSession,
    device_id: i32,
    key_code: i32,
    action: i32,
    shift_held: bool,
    is_repeat: bool,
) -> i32 {
    let Some(session) = session.as_mut() else {
        return -1;
    };
    let key_action = if action == 0 {
        crate::gamepad::KeyAction::Down
    } else {
        crate::gamepad::KeyAction::Up
    };
    let event = AndroidKeyEvent {
        device_id,
        key_code,
        action: key_action,
        shift_held,
        is_repeat,
    };
    match session.inner.gamepad.handle_key_event(event) {
        Some(mapped) => {
            session.inner.first_boot.handle_input(mapped.input);
            mapped.slint_code
        }
        None => -1,
    }
}

/// Routes an Android `AMotionEvent` axis value through [`AndroidGamepadAdapter`] and [`FirstBootState`].
/// Returns the Slint `Input::from_code` integer (`0..=10`), or `-1` if below threshold/held.
///
/// # Safety
/// `session` must be null or a live pointer.
#[no_mangle]
pub unsafe extern "C" fn uncharted_apk_session_on_axis_event(
    session: *mut AndroidApkFfiSession,
    device_id: i32,
    axis: i32,
    value: f32,
) -> i32 {
    let Some(session) = session.as_mut() else {
        return -1;
    };
    let event = AndroidAxisEvent {
        device_id,
        axis,
        value,
    };
    match session.inner.gamepad.handle_axis_event(event) {
        Some(mapped) => {
            session.inner.first_boot.handle_input(mapped.input);
            mapped.slint_code
        }
        None => -1,
    }
}

/// Updates live display dimensions on fold/unfold or rotation (`APP_CMD_INIT_WINDOW`/`WINDOW_RESIZED`).
/// Returns `0` on success, `-1` on invalid dimensions or null session.
///
/// # Safety
/// `session` must be null or a live pointer.
#[no_mangle]
pub unsafe extern "C" fn uncharted_apk_session_on_display_change(
    session: *mut AndroidApkFfiSession,
    width: u32,
    height: u32,
    timestamp_ms: u64,
) -> i32 {
    let Some(session) = session.as_mut() else {
        return -1;
    };
    match session.inner.on_display_change(width, height, timestamp_ms) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

/// Returns the current [`FirstBootDecision`] code (`0=PromptDownload`, `1=ReadyForStage704Download`,
/// `2=DeclinedWait`, `3=AlreadyInstalled`), or `-1` if `session` is null.
///
/// # Safety
/// `session` must be null or a live pointer.
#[no_mangle]
pub unsafe extern "C" fn uncharted_apk_session_first_boot_decision(
    session: *const AndroidApkFfiSession,
) -> i32 {
    let Some(session) = session.as_ref() else {
        return -1;
    };
    match session.inner.first_boot.decision {
        FirstBootDecision::PromptDownload => 0,
        FirstBootDecision::ReadyForStage704Download { .. } => 1,
        FirstBootDecision::DeclinedWait => 2,
        FirstBootDecision::AlreadyInstalled { .. } => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::DriverPathParts;
    use crate::game::PromptButton;
    use crate::gamepad::{axis, keycode, AndroidAxisEvent, AndroidKeyEvent, KeyAction};
    use std::collections::HashSet;

    struct TestProbe {
        reported: Result<String, String>,
        closed: usize,
    }

    impl DriverLoaderProbe for TestProbe {
        type Handle = &'static str;

        fn open_system(&mut self, _so_name: &str) -> Result<Self::Handle, String> {
            Ok("system-handle")
        }

        fn open_custom(
            &mut self,
            _native_library_dir: &str,
            _parts: &DriverPathParts,
        ) -> Result<Self::Handle, String> {
            Ok("turnip-handle")
        }

        fn query_driver_name(&mut self, _handle: &Self::Handle) -> Result<String, String> {
            self.reported.clone()
        }

        fn close_handle(&mut self, _handle: Self::Handle) {
            self.closed += 1;
        }
    }

    #[test]
    fn bootstrap_succeeds_with_verified_turnip_and_routes_gamepad_input_for_all_games() {
        for game in AndroidGame::ALL {
            let pkg = game.application_id();
            let cfg = AndroidLaunchConfig::for_game_with_bundled_turnip(
                game,
                format!("/data/app/{pkg}/lib/arm64"),
                format!("/data/user/0/{pkg}/files"),
                format!("/data/user/0/{pkg}/files/drivers/libvulkan_freedreno.so"),
            );
            let mut probe = TestProbe {
                reported: Ok("Mesa Turnip 26.2.5".to_owned()),
                closed: 0,
            };
            let mut session = bootstrap_session(&cfg, &mut probe).expect("bootstrap succeeds");
            assert_eq!(session.game, game);
            assert_eq!(session.package_name, pkg);
            assert_eq!(session.driver_mode, DriverMode::Bundled);
            assert_eq!(session.driver_handle, "turnip-handle");
            assert_eq!(probe.closed, 0);
            assert_eq!(
                session.first_boot.prompt_text,
                "Would you like to download the rest of the game files?"
            );
            assert_eq!(
                session.first_boot.decision,
                FirstBootDecision::PromptDownload
            );

            session
                .gamepad
                .connect_device(1, Some(0x054c), "Wireless Controller");
            let mapped = session
                .gamepad
                .handle_key_event(AndroidKeyEvent {
                    device_id: 1,
                    key_code: keycode::BUTTON_A,
                    action: KeyAction::Down,
                    shift_held: false,
                    is_repeat: false,
                })
                .expect("activate button");
            assert_eq!(mapped.slint_code, 6);
        }
    }

    #[test]
    fn first_boot_gamepad_navigation_handles_download_later_and_back() {
        for game in AndroidGame::ALL {
            let pkg = game.application_id();
            let cfg = AndroidLaunchConfig::for_game_with_bundled_turnip(
                game,
                format!("/data/app/{pkg}/lib/arm64"),
                format!("/data/user/0/{pkg}/files"),
                format!("/data/user/0/{pkg}/files/drivers/libvulkan_freedreno.so"),
            );
            let mut probe = TestProbe {
                reported: Ok("Mesa Turnip 26.2.5".to_owned()),
                closed: 0,
            };
            let mut session = bootstrap_session(&cfg, &mut probe).unwrap();
            session
                .gamepad
                .connect_device(1, Some(0x054c), "DualSense Wireless Controller");

            // Default focus is Download -> Activate hands off to Stage 7.04 download.
            assert_eq!(session.first_boot.focused_button, PromptButton::Download);
            assert_eq!(
                session.route_first_boot_key(AndroidKeyEvent {
                    device_id: 1,
                    key_code: keycode::BUTTON_A,
                    action: KeyAction::Down,
                    shift_held: false,
                    is_repeat: false,
                }),
                Some(FirstBootDecision::ReadyForStage704Download { game })
            );

            // Reset to prompt: move right via analog stick, back left via DPAD, then right + Activate -> Later.
            let mut session = bootstrap_session(&cfg, &mut probe).unwrap();
            session
                .gamepad
                .connect_device(2, Some(0x045e), "Xbox Wireless Controller");
            assert_eq!(
                session.route_first_boot_axis(AndroidAxisEvent {
                    device_id: 2,
                    axis: axis::X,
                    value: 0.9,
                }),
                Some(FirstBootDecision::PromptDownload)
            );
            assert_eq!(session.first_boot.focused_button, PromptButton::Later);

            assert_eq!(
                session.route_first_boot_key(AndroidKeyEvent {
                    device_id: 2,
                    key_code: keycode::DPAD_LEFT,
                    action: KeyAction::Down,
                    shift_held: false,
                    is_repeat: false,
                }),
                Some(FirstBootDecision::PromptDownload)
            );
            assert_eq!(session.first_boot.focused_button, PromptButton::Download);

            assert_eq!(
                session.route_first_boot_key(AndroidKeyEvent {
                    device_id: 2,
                    key_code: keycode::DPAD_RIGHT,
                    action: KeyAction::Down,
                    shift_held: false,
                    is_repeat: false,
                }),
                Some(FirstBootDecision::PromptDownload)
            );
            assert_eq!(
                session.route_first_boot_key(AndroidKeyEvent {
                    device_id: 2,
                    key_code: keycode::BUTTON_A,
                    action: KeyAction::Down,
                    shift_held: false,
                    is_repeat: false,
                }),
                Some(FirstBootDecision::DeclinedWait)
            );

            // Pressing Back (BUTTON_B) declines immediately.
            let mut session = bootstrap_session(&cfg, &mut probe).unwrap();
            assert_eq!(
                session.route_first_boot_key(AndroidKeyEvent {
                    device_id: 2,
                    key_code: keycode::BUTTON_B,
                    action: KeyAction::Down,
                    shift_held: false,
                    is_repeat: false,
                }),
                Some(FirstBootDecision::DeclinedWait)
            );
        }

        // When game files are already installed, first-boot skips the prompt.
        let mut installed_cfg = AndroidLaunchConfig::for_game_with_bundled_turnip(
            AndroidGame::U3,
            "/data/app/org.uncharted.universal.u3/lib/arm64",
            "/data/user/0/org.uncharted.universal.u3/files",
            "/data/user/0/org.uncharted.universal.u3/files/drivers/libvulkan_freedreno.so",
        );
        installed_cfg.game_files_present = true;
        let mut probe = TestProbe {
            reported: Ok("Mesa Turnip 26.2.5".to_owned()),
            closed: 0,
        };
        let session = bootstrap_session(&installed_cfg, &mut probe).unwrap();
        assert_eq!(
            session.first_boot.decision,
            FirstBootDecision::AlreadyInstalled {
                game: AndroidGame::U3
            }
        );
    }

    #[test]
    fn bootstrap_fails_closed_on_m9_bundled_assets_package_mismatch_api_or_adreno() {
        let mut bundled_cfg = AndroidLaunchConfig::for_game_with_bundled_turnip(
            AndroidGame::U1,
            "/data/app/org.uncharted.universal.u1/lib/arm64",
            "/data/user/0/org.uncharted.universal.u1/files",
            "/data/user/0/org.uncharted.universal.u1/files/drivers/libvulkan_freedreno.so",
        );
        bundled_cfg.bundled_game_assets = true;
        let mut probe = TestProbe {
            reported: Ok("turnip".to_owned()),
            closed: 0,
        };
        assert_eq!(
            bootstrap_session(&bundled_cfg, &mut probe).err(),
            Some(BootstrapError::BundledGameAssetsForbidden)
        );

        let mut mismatch_cfg = AndroidLaunchConfig::for_game_with_bundled_turnip(
            AndroidGame::U2,
            "/data/app/org.uncharted.universal.u2/lib/arm64",
            "/data/user/0/org.uncharted.universal.u2/files",
            "/data/user/0/org.uncharted.universal.u2/files/drivers/libvulkan_freedreno.so",
        );
        mismatch_cfg.package_name = AndroidGame::U1.application_id().to_owned();
        assert_eq!(
            bootstrap_session(&mismatch_cfg, &mut probe).err(),
            Some(BootstrapError::PackageGameMismatch {
                game: AndroidGame::U2,
                package_name: "org.uncharted.universal.u1".to_owned(),
                expected: "org.uncharted.universal.u2".to_owned(),
            })
        );

        let mut bad_api = AndroidLaunchConfig::for_game_with_bundled_turnip(
            AndroidGame::U1,
            "/data/app/lib/arm64",
            "/data/user/0/org.uncharted.universal.u1/files",
            "/data/user/0/org.uncharted.universal.u1/files/drivers/libvulkan_freedreno.so",
        );
        bad_api.min_sdk_version = 29;
        assert_eq!(
            bootstrap_session(&bad_api, &mut probe).err(),
            Some(BootstrapError::UnsupportedApiLevel {
                min_sdk: 29,
                required: 33,
            })
        );

        let cfg = AndroidLaunchConfig::for_game_with_bundled_turnip(
            AndroidGame::U3,
            "/data/app/lib/arm64",
            "/data/user/0/org.uncharted.universal.u3/files",
            "/data/user/0/org.uncharted.universal.u3/files/drivers/libvulkan_freedreno.so",
        );
        let mut fallback_probe = TestProbe {
            reported: Ok("Qualcomm Adreno (TM) 750".to_owned()),
            closed: 0,
        };
        let err = bootstrap_session(&cfg, &mut fallback_probe).err().unwrap();
        assert!(matches!(
            err,
            BootstrapError::Driver(DriverValidationError::DriverIdentityMismatch { .. })
        ));
        assert_eq!(fallback_probe.closed, 1);
    }

    #[test]
    fn all_three_per_game_manifests_have_distinct_ids_labels_and_stage_7_03_contract() {
        let manifests = [
            (
                AndroidGame::U1,
                include_str!("../manifests/u1/AndroidManifest.xml"),
            ),
            (
                AndroidGame::U2,
                include_str!("../manifests/u2/AndroidManifest.xml"),
            ),
            (
                AndroidGame::U3,
                include_str!("../manifests/u3/AndroidManifest.xml"),
            ),
        ];

        let mut seen_ids = HashSet::new();
        let mut seen_labels = HashSet::new();

        for (game, manifest) in manifests {
            assert!(seen_ids.insert(game.application_id()));
            assert!(seen_labels.insert(game.app_label()));
            assert!(manifest.contains(&format!("package=\"{}\"", game.application_id())));
            assert!(manifest.contains(&format!("android:label=\"{}\"", game.app_label())));
            assert!(manifest.contains(&format!("android:value=\"{}\"", game.key())));
            assert!(manifest.contains("android:minSdkVersion=\"33\""));
            assert!(manifest.contains("android:targetSdkVersion=\"35\""));
            assert!(manifest.contains("android:extractNativeLibs=\"true\""));
            assert!(manifest.contains("android.hardware.vulkan.version"));
            assert!(manifest.contains("android.hardware.gamepad"));
            assert!(manifest.contains("uncharted_android_apk"));
            assert!(manifest.contains("screenSize"));
        }

        assert_eq!(seen_ids.len(), 3);
        assert_eq!(seen_labels.len(), 3);
    }

    #[test]
    fn session_handles_pixel_fold_transitions_and_controller_touch_hiding() {
        use crate::display::{
            FoldScreen, FovMode, PIXEL_FOLD_COVER_LANDSCAPE, PIXEL_FOLD_COVER_PORTRAIT,
            PIXEL_FOLD_INSIDE_LANDSCAPE, RESIZE_DEBOUNCE_MS,
        };

        let cfg = AndroidLaunchConfig::for_game_with_system_mali(AndroidGame::U2)
            .with_display(PIXEL_FOLD_COVER_PORTRAIT.0, PIXEL_FOLD_COVER_PORTRAIT.1);
        let mut probe = TestProbe {
            reported: Ok("ARM Mali-G715".to_owned()),
            closed: 0,
        };
        let mut session = bootstrap_session(&cfg, &mut probe).expect("bootstrap succeeds");
        assert_eq!(session.display.current().screen, FoldScreen::PixelFoldCover);
        assert_eq!(session.display.current().fov.mode, FovMode::VertPlus);
        assert!(session.display.current().touch_overlay.visible);

        // Connecting a gamepad hides touch overlay immediately.
        session.connect_gamepad(11, Some(0x054c), "DualSense Wireless Controller");
        assert!(!session.display.current().touch_overlay.visible);

        // Rotate to Cover Landscape -> HorPlus, then unfold to Inside -> VertPlus.
        let land = session
            .on_display_change(
                PIXEL_FOLD_COVER_LANDSCAPE.0,
                PIXEL_FOLD_COVER_LANDSCAPE.1,
                50,
            )
            .unwrap();
        assert_eq!(land.fov.mode, FovMode::HorPlus);
        assert!(!land.touch_overlay.visible);

        let inside = session
            .on_display_change(
                PIXEL_FOLD_INSIDE_LANDSCAPE.0,
                PIXEL_FOLD_INSIDE_LANDSCAPE.1,
                50 + RESIZE_DEBOUNCE_MS,
            )
            .unwrap();
        assert_eq!(inside.screen, FoldScreen::PixelFoldInside);
        assert_eq!(inside.fov.mode, FovMode::VertPlus);
        assert!(inside.needs_restart);
        assert!(!inside.touch_overlay.visible);

        // Disconnecting the gamepad restores touch overlay.
        session.disconnect_gamepad(11);
        assert!(session.display.current().touch_overlay.visible);
    }

    #[test]
    fn ffi_session_roundtrips_gamepad_display_and_stage704_prompt() {
        unsafe {
            let prompt = core::ffi::CStr::from_ptr(uncharted_apk_first_boot_prompt_text())
                .to_str()
                .unwrap();
            assert_eq!(prompt, FIRST_BOOT_DOWNLOAD_PROMPT);

            let session = uncharted_apk_session_create(
                c"u3".as_ptr(),
                c"org.uncharted.universal.u3".as_ptr(),
                c"/data/app/org.uncharted.universal.u3/lib/arm64".as_ptr(),
                c"/data/user/0/org.uncharted.universal.u3/files".as_ptr(),
                0x13B5,
                c"Mali-G710".as_ptr(),
                2208,
                1840,
                false,
            );
            assert!(!session.is_null());
            assert_eq!(uncharted_apk_session_first_boot_decision(session), 0);

            uncharted_apk_session_connect_gamepad(session, 4, 0x054c, c"DualSense".as_ptr());
            assert_eq!(
                uncharted_apk_session_on_axis_event(session, 4, axis::X, 0.9),
                3 // Nav::Right -> Slint code 3
            );
            assert_eq!(
                uncharted_apk_session_on_key_event(session, 4, keycode::DPAD_LEFT, 0, false, false),
                2 // Nav::Left -> Slint code 2
            );
            assert_eq!(
                uncharted_apk_session_on_key_event(session, 4, keycode::BUTTON_A, 0, false, false),
                6 // Activate -> Slint code 6
            );
            assert_eq!(uncharted_apk_session_first_boot_decision(session), 1);

            assert_eq!(
                uncharted_apk_session_on_display_change(session, 1080, 2092, 1000),
                0
            );
            uncharted_apk_session_disconnect_gamepad(session, 4);
            uncharted_apk_session_destroy(session);
        }
    }
}
