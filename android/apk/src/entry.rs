//! Android NativeActivity entry scaffold and startup bootstrapper (Stage 7.03).
//!
//! Validates Android manifest/packaging preconditions and explicit Turnip/system Vulkan driver
//! identity before Slint window creation or `uncharted_runtime` launch.

use crate::driver::{
    open_validated_driver, DriverLoaderProbe, DriverMode, DriverValidationError, TurnipDriverConfig,
};
use crate::gamepad::AndroidGamepadAdapter;
use std::fmt;

pub const DEFAULT_PACKAGE_NAME: &str = "org.uncharted.universal";
pub const MIN_SUPPORTED_ANDROID_API: u32 = 33;
pub const TARGET_ANDROID_API: u32 = 35;

/// Launch parameters supplied by Android `NativeActivity` / app preferences.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndroidLaunchConfig {
    pub package_name: String,
    pub min_sdk_version: u32,
    pub target_sdk_version: u32,
    pub driver: TurnipDriverConfig,
}

impl AndroidLaunchConfig {
    pub fn with_bundled_turnip(
        native_library_dir: impl Into<String>,
        app_storage_dir: impl Into<String>,
        custom_driver_so: impl Into<String>,
    ) -> Self {
        Self {
            package_name: DEFAULT_PACKAGE_NAME.to_owned(),
            min_sdk_version: MIN_SUPPORTED_ANDROID_API,
            target_sdk_version: TARGET_ANDROID_API,
            driver: TurnipDriverConfig::bundled_turnip(
                native_library_dir,
                app_storage_dir,
                custom_driver_so,
            ),
        }
    }
}

/// Errors that prevent launching the Slint Android APK UI or Vulkan runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BootstrapError {
    MissingPackageName,
    UnsupportedApiLevel { min_sdk: u32, required: u32 },
    TargetSdkBelowMin { min_sdk: u32, target_sdk: u32 },
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

/// Bootstrapped Android APK session holding the validated Vulkan driver handle and gamepad router.
pub struct AndroidApkSession<H> {
    pub package_name: String,
    pub driver_mode: DriverMode,
    pub reported_driver_name: String,
    pub driver_handle: H,
    pub gamepad: AndroidGamepadAdapter,
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
        package_name: config.package_name.clone(),
        driver_mode: validated.mode,
        reported_driver_name: validated.reported_name,
        driver_handle: validated.handle,
        gamepad: AndroidGamepadAdapter::default(),
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
    use crate::gamepad::{keycode, AndroidKeyEvent, KeyAction};

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
    fn bootstrap_succeeds_with_verified_turnip_and_routes_gamepad_input() {
        let cfg = AndroidLaunchConfig::with_bundled_turnip(
            "/data/app/org.uncharted.universal/lib/arm64",
            "/data/user/0/org.uncharted.universal/files",
            "/data/user/0/org.uncharted.universal/files/drivers/libvulkan_freedreno.so",
        );
        let mut probe = TestProbe {
            reported: Ok("Mesa Turnip 26.2.5".to_owned()),
            closed: 0,
        };
        let mut session = bootstrap_session(&cfg, &mut probe).expect("bootstrap succeeds");
        assert_eq!(session.package_name, DEFAULT_PACKAGE_NAME);
        assert_eq!(session.driver_mode, DriverMode::Bundled);
        assert_eq!(session.driver_handle, "turnip-handle");
        assert_eq!(probe.closed, 0);

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

    #[test]
    fn bootstrap_fails_closed_on_api_mismatch_or_stock_adreno_fallback() {
        let mut bad_api = AndroidLaunchConfig::with_bundled_turnip(
            "/data/app/lib/arm64",
            "/data/user/0/org.uncharted.universal/files",
            "/data/user/0/org.uncharted.universal/files/drivers/libvulkan_freedreno.so",
        );
        bad_api.min_sdk_version = 29;
        let mut probe = TestProbe {
            reported: Ok("turnip".to_owned()),
            closed: 0,
        };
        assert_eq!(
            bootstrap_session(&bad_api, &mut probe).err(),
            Some(BootstrapError::UnsupportedApiLevel {
                min_sdk: 29,
                required: 33,
            })
        );

        let cfg = AndroidLaunchConfig::with_bundled_turnip(
            "/data/app/lib/arm64",
            "/data/user/0/org.uncharted.universal/files",
            "/data/user/0/org.uncharted.universal/files/drivers/libvulkan_freedreno.so",
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
    fn manifest_matches_stage_7_03_packaging_and_api_contract() {
        let manifest = include_str!("../AndroidManifest.xml");
        assert!(manifest.contains(DEFAULT_PACKAGE_NAME));
        assert!(manifest.contains("android:minSdkVersion=\"33\""));
        assert!(manifest.contains("android:targetSdkVersion=\"35\""));
        assert!(manifest.contains("android:extractNativeLibs=\"true\""));
        assert!(manifest.contains("android.hardware.vulkan.version"));
        assert!(manifest.contains("android.hardware.gamepad"));
        assert!(manifest.contains("uncharted_android_apk"));
        assert!(manifest.contains("screenSize"));
    }
}
