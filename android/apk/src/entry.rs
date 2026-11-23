//! Android NativeActivity entry scaffold and startup bootstrapper (Stage 7.03).
//!
//! Validates Android manifest/packaging preconditions and explicit Turnip/system Vulkan driver
//! identity before Slint window creation or `uncharted_runtime` launch.

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
            driver: TurnipDriverConfig::bundled_turnip(
                native_library_dir,
                app_storage_dir,
                custom_driver_so,
            ),
        }
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
    Driver(DriverValidationError),
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
            Self::Driver(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for BootstrapError {}

/// Bootstrapped Android APK session holding the validated Vulkan driver handle, gamepad router,
/// and Stage 7.04 first-boot download hook.
pub struct AndroidApkSession<H> {
    pub game: AndroidGame,
    pub package_name: String,
    pub driver_mode: DriverMode,
    pub reported_driver_name: String,
    pub driver_handle: H,
    pub gamepad: AndroidGamepadAdapter,
    pub first_boot: FirstBootState,
}

impl<H> AndroidApkSession<H> {
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

    let validated = open_validated_driver(&config.driver, probe)?;
    Ok(AndroidApkSession {
        game: config.game,
        package_name: config.package_name.clone(),
        driver_mode: validated.mode,
        reported_driver_name: validated.reported_name,
        driver_handle: validated.handle,
        gamepad: AndroidGamepadAdapter::default(),
        first_boot: FirstBootState::evaluate(config.game, config.game_files_present),
    })
}

/// NativeActivity entrypoint symbol for the Android `aarch64-linux-android` APK library.
///
/// Stage 7.03 scaffold: Stages 7.04–7.06 wire `android-activity` (`AndroidApp`) and
/// `slint::android::init` + `uncharted_runtime` launch here.
///
/// # Safety
/// Called by Android's `NativeActivity` loader with a valid `ANativeActivity*` / `android_app*`.
#[cfg(target_os = "android")]
#[no_mangle]
pub unsafe extern "C" fn android_main(_app: *mut core::ffi::c_void) {}

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
}
