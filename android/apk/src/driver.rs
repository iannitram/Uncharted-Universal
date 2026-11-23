//! Explicit, fail-closed Android Vulkan / Turnip driver-selection config mirroring
//! `android/driver_loader.h` (`#210`, Stage 7.03).
//!
//! Custom and bundled Turnip modes require `useLegacyPackaging=true` (`extractNativeLibs="true"`),
//! a driver `.so` inside `app_storage_dir`, and post-load driver identity verification. A silent
//! fallback to the stock Qualcomm Adreno driver is always refused.

use core::ffi::{c_char, c_void};
use std::fmt;

/// Qualcomm Adreno vendor ID (`0x5143`), matching `BB_ANDROID_VENDOR_QUALCOMM`.
pub const VENDOR_ID_QUALCOMM: u32 = 0x5143;
/// ARM Mali vendor ID (`0x13B5`), matching `BB_ANDROID_VENDOR_ARM` (e.g. Google Tensor G2 / Mali-G710).
pub const VENDOR_ID_ARM: u32 = 0x13B5;
/// Vulkan 1.3 packed API version (`VK_MAKE_API_VERSION(0, 1, 3, 0)`).
pub const VK_API_VERSION_1_3: u32 = (1 << 22) | (3 << 12);

/// GPU family classification for Android driver selection (`#536`).
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AndroidGpuFamily {
    Unknown = 0,
    Adreno = 1,
    Mali = 2,
}

/// Classifies an Android GPU from its Vulkan vendor ID and/or device/driver name (`#536`).
pub fn classify_android_gpu(vendor_id: u32, device_or_driver_name: &str) -> AndroidGpuFamily {
    if vendor_id == VENDOR_ID_QUALCOMM {
        return AndroidGpuFamily::Adreno;
    }
    if vendor_id == VENDOR_ID_ARM {
        return AndroidGpuFamily::Mali;
    }
    if contains_ascii_case_insensitive(device_or_driver_name, "adreno")
        || contains_ascii_case_insensitive(device_or_driver_name, "turnip")
        || contains_ascii_case_insensitive(device_or_driver_name, "freedreno")
    {
        return AndroidGpuFamily::Adreno;
    }
    if contains_ascii_case_insensitive(device_or_driver_name, "mali") {
        return AndroidGpuFamily::Mali;
    }
    AndroidGpuFamily::Unknown
}

/// Required Vulkan 1.3 / shadPS4 device capabilities for system Tensor/Mali drivers (`#536`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaliVulkanCaps {
    pub vendor_id: u32,
    pub api_version: u32,
    pub dynamic_rendering: bool,
    pub synchronization2: bool,
    pub timeline_semaphore: bool,
    pub buffer_device_address: bool,
    pub swapchain_extension: bool,
    pub driver_name: String,
}

impl MaliVulkanCaps {
    pub fn validate(&self) -> Result<(), DriverValidationError> {
        if classify_android_gpu(self.vendor_id, &self.driver_name) != AndroidGpuFamily::Mali {
            return Err(DriverValidationError::MaliUnsupportedCapability(
                "Vulkan driver is not ARM/Mali".to_owned(),
            ));
        }
        if self.api_version < VK_API_VERSION_1_3 {
            return Err(DriverValidationError::MaliUnsupportedCapability(
                "Tensor/Mali Vulkan driver requires Vulkan 1.3 or newer".to_owned(),
            ));
        }
        let missing = [
            (!self.dynamic_rendering, "dynamicRendering"),
            (!self.synchronization2, "synchronization2"),
            (!self.timeline_semaphore, "timelineSemaphore"),
            (!self.buffer_device_address, "bufferDeviceAddress"),
            (!self.swapchain_extension, "VK_KHR_swapchain"),
        ]
        .into_iter()
        .find_map(|(is_missing, name)| is_missing.then_some(name));

        if let Some(feature) = missing {
            return Err(DriverValidationError::MaliUnsupportedCapability(format!(
                "Tensor/Mali driver missing required {feature}"
            )));
        }
        Ok(())
    }
}

/// Mirrors `BbAndroidDriverMode` in `android/driver_loader.h`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverMode {
    System = 0,
    Custom = 1,
    Bundled = 2,
}

impl DriverMode {
    pub const fn as_u32(self) -> u32 {
        self as u32
    }

    pub const fn is_custom_or_bundled(self) -> bool {
        matches!(self, Self::Custom | Self::Bundled)
    }
}

impl TryFrom<u32> for DriverMode {
    type Error = DriverValidationError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::System),
            1 => Ok(Self::Custom),
            2 => Ok(Self::Bundled),
            other => Err(DriverValidationError::UnknownMode(other)),
        }
    }
}

/// C-ABI mirror of `BbAndroidDriverConfig` in `android/driver_loader.h`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CDriverConfig {
    pub mode: u32,
    pub use_legacy_packaging: u32,
    pub native_library_dir: *const c_char,
    pub app_storage_dir: *const c_char,
    pub custom_driver_so: *const c_char,
    pub expected_driver_name: *const c_char,
}

/// Split driver path components passed to `libadrenotools` (`adrenotools_open_libvulkan`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriverPathParts {
    pub driver_dir: String,
    pub driver_name: String,
    pub tmp_dir: String,
}

/// Safe Rust configuration for Android Vulkan driver bootstrap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnipDriverConfig {
    pub mode: DriverMode,
    pub use_legacy_packaging: bool,
    pub native_library_dir: String,
    pub app_storage_dir: String,
    pub custom_driver_so: String,
    pub expected_driver_name: String,
}

impl TurnipDriverConfig {
    pub fn bundled_turnip(
        native_library_dir: impl Into<String>,
        app_storage_dir: impl Into<String>,
        custom_driver_so: impl Into<String>,
    ) -> Self {
        Self {
            mode: DriverMode::Bundled,
            use_legacy_packaging: true,
            native_library_dir: native_library_dir.into(),
            app_storage_dir: app_storage_dir.into(),
            custom_driver_so: custom_driver_so.into(),
            expected_driver_name: "turnip".to_owned(),
        }
    }

    pub fn custom_turnip(
        native_library_dir: impl Into<String>,
        app_storage_dir: impl Into<String>,
        custom_driver_so: impl Into<String>,
        expected_driver_name: impl Into<String>,
    ) -> Self {
        Self {
            mode: DriverMode::Custom,
            use_legacy_packaging: true,
            native_library_dir: native_library_dir.into(),
            app_storage_dir: app_storage_dir.into(),
            custom_driver_so: custom_driver_so.into(),
            expected_driver_name: expected_driver_name.into(),
        }
    }

    pub fn system_adreno() -> Self {
        Self {
            mode: DriverMode::System,
            use_legacy_packaging: true,
            native_library_dir: String::new(),
            app_storage_dir: String::new(),
            custom_driver_so: String::new(),
            expected_driver_name: String::new(),
        }
    }

    pub fn system_mali() -> Self {
        Self {
            mode: DriverMode::System,
            use_legacy_packaging: true,
            native_library_dir: String::new(),
            app_storage_dir: String::new(),
            custom_driver_so: String::new(),
            expected_driver_name: "mali".to_owned(),
        }
    }

    /// Validates packaging and path preconditions before any driver handle is opened.
    pub fn validate_preconditions(&self) -> Result<Option<DriverPathParts>, DriverValidationError> {
        if !self.mode.is_custom_or_bundled() {
            return Ok(None);
        }
        if !self.use_legacy_packaging {
            return Err(DriverValidationError::LegacyPackagingRequired);
        }
        if self.native_library_dir.is_empty() {
            return Err(DriverValidationError::MissingNativeLibraryDir);
        }
        if self.expected_driver_name.is_empty() {
            return Err(DriverValidationError::MissingExpectedDriverName);
        }
        let (driver_dir, driver_name) =
            split_driver_path(&self.app_storage_dir, &self.custom_driver_so)
                .ok_or(DriverValidationError::InvalidCustomDriverPath)?;
        let storage_trimmed = self.app_storage_dir.trim_end_matches('/');
        let tmp_dir = format!("{storage_trimmed}/adrenotools_tmp");
        Ok(Some(DriverPathParts {
            driver_dir,
            driver_name,
            tmp_dir,
        }))
    }

    /// Verifies the reported Vulkan driver identity, refusing empty names or stock fallback.
    pub fn verify_reported_driver(&self, reported_name: &str) -> Result<(), DriverValidationError> {
        let reported = reported_name.trim();
        if reported.is_empty() {
            return Err(DriverValidationError::EmptyReportedDriver);
        }
        if self.mode.is_custom_or_bundled() {
            let expected = self.expected_driver_name.as_str();
            if expected.is_empty() || !contains_ascii_case_insensitive(reported, expected) {
                return Err(DriverValidationError::DriverIdentityMismatch {
                    expected: expected.to_owned(),
                    reported: reported.to_owned(),
                });
            }
        }
        Ok(())
    }
}

/// Errors returned by [`TurnipDriverConfig`] and [`open_validated_driver`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DriverValidationError {
    UnknownMode(u32),
    LegacyPackagingRequired,
    MissingNativeLibraryDir,
    MissingExpectedDriverName,
    InvalidCustomDriverPath,
    OpenFailed(String),
    EmptyReportedDriver,
    DriverIdentityMismatch { expected: String, reported: String },
    TurnipRequiredForAdreno,
    UnsupportedGpuVendor { vendor_id: u32, name: String },
    MaliUnsupportedCapability(String),
}

impl fmt::Display for DriverValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMode(mode) => write!(f, "unknown Android Vulkan driver mode: {mode}"),
            Self::LegacyPackagingRequired => {
                write!(f, "custom/bundled driver requires useLegacyPackaging=true")
            }
            Self::MissingNativeLibraryDir => {
                write!(f, "nativeLibraryDir is required for custom/bundled driver")
            }
            Self::MissingExpectedDriverName => {
                write!(
                    f,
                    "expected_driver_name is required for custom/bundled driver"
                )
            }
            Self::InvalidCustomDriverPath => {
                write!(f, "custom driver path must reside inside app_storage_dir")
            }
            Self::OpenFailed(reason) => write!(f, "failed to open Vulkan driver: {reason}"),
            Self::EmptyReportedDriver => {
                write!(f, "failed to validate Vulkan driver identity before startup")
            }
            Self::DriverIdentityMismatch { expected, reported } => write!(
                f,
                "selected custom driver identity mismatch (expected '{expected}', got '{reported}'; refusing silent stock fallback)"
            ),
            Self::TurnipRequiredForAdreno => write!(
                f,
                "Snapdragon/Adreno requires bundled or custom Turnip (refusing stock fallback)"
            ),
            Self::UnsupportedGpuVendor { vendor_id, name } => write!(
                f,
                "unsupported Android GPU vendor 0x{vendor_id:04x} ('{name}'); expected Snapdragon/Adreno or Tensor/Mali"
            ),
            Self::MaliUnsupportedCapability(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for DriverValidationError {}

/// Selects the explicit driver configuration for the detected Android GPU (`#536`):
/// - Snapdragon / Adreno (`0x5143`): requires a bundled/custom Turnip config (never falls back).
/// - Google Tensor / Mali (`0x13B5`): selects `system_mali()` (`libvulkan.so`) for cap validation.
/// - Unknown GPU: fails closed.
pub fn select_driver_for_gpu(
    vendor_id: u32,
    device_or_driver_name: &str,
    turnip_cfg: Option<&TurnipDriverConfig>,
) -> Result<TurnipDriverConfig, DriverValidationError> {
    match classify_android_gpu(vendor_id, device_or_driver_name) {
        AndroidGpuFamily::Adreno => match turnip_cfg {
            Some(cfg) if cfg.mode.is_custom_or_bundled() => Ok(cfg.clone()),
            _ => Err(DriverValidationError::TurnipRequiredForAdreno),
        },
        AndroidGpuFamily::Mali => Ok(TurnipDriverConfig::system_mali()),
        AndroidGpuFamily::Unknown => Err(DriverValidationError::UnsupportedGpuVendor {
            vendor_id,
            name: device_or_driver_name.to_owned(),
        }),
    }
}

/// Injectable probe mirroring `BbAndroidDriverProbe` for deterministic host tests.
pub trait DriverLoaderProbe {
    type Handle;

    fn open_system(&mut self, so_name: &str) -> Result<Self::Handle, String>;
    fn open_custom(
        &mut self,
        native_library_dir: &str,
        parts: &DriverPathParts,
    ) -> Result<Self::Handle, String>;
    fn query_driver_name(&mut self, handle: &Self::Handle) -> Result<String, String>;
    fn close_handle(&mut self, handle: Self::Handle);

    /// Optional Vulkan 1.3 capability probe for system Tensor/Mali drivers (`#536`).
    fn query_mali_caps(
        &mut self,
        _handle: &Self::Handle,
    ) -> Option<Result<MaliVulkanCaps, String>> {
        None
    }
}

/// Validated open driver handle returned by [`open_validated_driver`].
#[derive(Debug, PartialEq, Eq)]
pub struct ValidatedDriver<H> {
    pub mode: DriverMode,
    pub reported_name: String,
    pub handle: H,
}

/// Executes the strict fail-closed Android driver selection flow:
/// 1. Validates mode, packaging, and path boundaries before touching `dlopen`/`libadrenotools`.
/// 2. Opens only the explicitly requested driver (system vs custom/bundled).
/// 3. Queries the loaded driver name and closes + fails if empty or mismatched.
pub fn open_validated_driver<P: DriverLoaderProbe>(
    cfg: &TurnipDriverConfig,
    probe: &mut P,
) -> Result<ValidatedDriver<P::Handle>, DriverValidationError> {
    let custom_parts = cfg.validate_preconditions()?;
    let handle = match custom_parts {
        None => probe
            .open_system("libvulkan.so")
            .map_err(DriverValidationError::OpenFailed)?,
        Some(ref parts) => probe
            .open_custom(&cfg.native_library_dir, parts)
            .map_err(DriverValidationError::OpenFailed)?,
    };

    let reported = match probe.query_driver_name(&handle) {
        Ok(name) => name,
        Err(_) => {
            probe.close_handle(handle);
            return Err(DriverValidationError::EmptyReportedDriver);
        }
    };

    if let Err(err) = cfg.verify_reported_driver(&reported) {
        probe.close_handle(handle);
        return Err(err);
    }

    if cfg.mode == DriverMode::System {
        if let Some(caps_res) = probe.query_mali_caps(&handle) {
            let caps = match caps_res {
                Ok(caps) => caps,
                Err(reason) => {
                    probe.close_handle(handle);
                    return Err(DriverValidationError::MaliUnsupportedCapability(reason));
                }
            };
            let caps_name = if caps.driver_name.is_empty() {
                reported.as_str()
            } else {
                caps.driver_name.as_str()
            };
            if classify_android_gpu(caps.vendor_id, caps_name) == AndroidGpuFamily::Mali {
                if let Err(err) = caps.validate() {
                    probe.close_handle(handle);
                    return Err(err);
                }
            }
        }
    }

    Ok(ValidatedDriver {
        mode: cfg.mode,
        reported_name: reported.trim().to_owned(),
        handle,
    })
}

/// Helper linking [`CDriverConfig`] layout check in tests and future FFI wiring.
#[allow(dead_code)]
fn _assert_c_driver_config_non_null_mode(cfg: &CDriverConfig) -> Result<DriverMode, u32> {
    DriverMode::try_from(cfg.mode).map_err(|_| cfg.mode)
}

/// Placeholder for future `dlopen`/`libadrenotools` pointer bridging.
#[allow(dead_code)]
pub type OpaqueDriverHandle = *mut c_void;

fn split_driver_path(app_storage_dir: &str, custom_driver_so: &str) -> Option<(String, String)> {
    if app_storage_dir.is_empty() || custom_driver_so.is_empty() {
        return None;
    }
    if custom_driver_so.contains("..") {
        return None;
    }
    let storage_prefix = app_storage_dir.trim_end_matches('/');
    if storage_prefix.is_empty() {
        return None;
    }
    let rest = custom_driver_so.strip_prefix(storage_prefix)?;
    if !rest.starts_with('/') {
        return None;
    }
    let slash = custom_driver_so.rfind('/')?;
    if slash + 1 >= custom_driver_so.len() {
        return None;
    }
    let dir = custom_driver_so[..=slash].to_owned();
    let name = custom_driver_so[slash + 1..].to_owned();
    Some((dir, name))
}

fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    if haystack.is_empty() || needle.is_empty() {
        return false;
    }
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    h.windows(n.len())
        .any(|window| window.eq_ignore_ascii_case(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockProbe {
        allow_system: bool,
        allow_custom: bool,
        reported_driver: Option<String>,
        mali_caps: Option<Result<MaliVulkanCaps, String>>,
        open_system_calls: usize,
        open_custom_calls: usize,
        close_calls: usize,
        last_parts: Option<DriverPathParts>,
    }

    impl MockProbe {
        fn turnip_ok() -> Self {
            Self {
                allow_system: true,
                allow_custom: true,
                reported_driver: Some("turnip Mesa 26.2.5".to_owned()),
                ..Self::default()
            }
        }
    }

    impl DriverLoaderProbe for MockProbe {
        type Handle = u64;

        fn open_system(&mut self, _so_name: &str) -> Result<Self::Handle, String> {
            self.open_system_calls += 1;
            if self.allow_system {
                Ok(1)
            } else {
                Err("libvulkan.so failed".to_owned())
            }
        }

        fn open_custom(
            &mut self,
            _native_library_dir: &str,
            parts: &DriverPathParts,
        ) -> Result<Self::Handle, String> {
            self.open_custom_calls += 1;
            self.last_parts = Some(parts.clone());
            if self.allow_custom {
                Ok(2)
            } else {
                Err("libadrenotools failed".to_owned())
            }
        }

        fn query_driver_name(&mut self, _handle: &Self::Handle) -> Result<String, String> {
            self.reported_driver
                .clone()
                .ok_or_else(|| "query failed".to_owned())
        }

        fn close_handle(&mut self, _handle: Self::Handle) {
            self.close_calls += 1;
        }

        fn query_mali_caps(
            &mut self,
            _handle: &Self::Handle,
        ) -> Option<Result<MaliVulkanCaps, String>> {
            self.mali_caps.clone()
        }
    }

    #[test]
    fn driver_mode_discriminants_match_c_header() {
        assert_eq!(DriverMode::System.as_u32(), 0);
        assert_eq!(DriverMode::Custom.as_u32(), 1);
        assert_eq!(DriverMode::Bundled.as_u32(), 2);
        assert_eq!(DriverMode::try_from(0), Ok(DriverMode::System));
        assert_eq!(DriverMode::try_from(1), Ok(DriverMode::Custom));
        assert_eq!(DriverMode::try_from(2), Ok(DriverMode::Bundled));
        assert_eq!(
            DriverMode::try_from(99),
            Err(DriverValidationError::UnknownMode(99))
        );
    }

    #[test]
    fn bundled_and_custom_turnip_validate_paths_and_succeed() {
        let cfg = TurnipDriverConfig::bundled_turnip(
            "/data/app/lib/arm64",
            "/data/user/0/org.uncharted.universal/files",
            "/data/user/0/org.uncharted.universal/files/drivers/libvulkan_freedreno.so",
        );
        let mut probe = MockProbe::turnip_ok();
        let opened = open_validated_driver(&cfg, &mut probe).expect("bundled turnip should pass");
        assert_eq!(opened.mode, DriverMode::Bundled);
        assert_eq!(opened.handle, 2);
        assert_eq!(opened.reported_name, "turnip Mesa 26.2.5");
        assert_eq!(probe.open_custom_calls, 1);
        assert_eq!(probe.open_system_calls, 0);
        assert_eq!(
            probe.last_parts,
            Some(DriverPathParts {
                driver_dir: "/data/user/0/org.uncharted.universal/files/drivers/".to_owned(),
                driver_name: "libvulkan_freedreno.so".to_owned(),
                tmp_dir: "/data/user/0/org.uncharted.universal/files/adrenotools_tmp".to_owned(),
            })
        );

        let custom = TurnipDriverConfig::custom_turnip(
            "/data/app/lib/arm64",
            "/data/user/0/org.uncharted.universal/files/",
            "/data/user/0/org.uncharted.universal/files/drivers/libvulkan_freedreno.so",
            "TURNIP",
        );
        let mut probe2 = MockProbe::turnip_ok();
        assert!(open_validated_driver(&custom, &mut probe2).is_ok());
    }

    #[test]
    fn custom_turnip_fails_closed_on_stock_adreno_fallback() {
        let cfg = TurnipDriverConfig::custom_turnip(
            "/data/app/lib/arm64",
            "/data/user/0/org.uncharted.universal/files",
            "/data/user/0/org.uncharted.universal/files/drivers/libvulkan_freedreno.so",
            "turnip",
        );
        let mut probe = MockProbe {
            allow_system: true,
            allow_custom: true,
            reported_driver: Some("Qualcomm Adreno Vulkan".to_owned()),
            ..MockProbe::default()
        };
        let err = open_validated_driver(&cfg, &mut probe).unwrap_err();
        assert_eq!(
            err,
            DriverValidationError::DriverIdentityMismatch {
                expected: "turnip".to_owned(),
                reported: "Qualcomm Adreno Vulkan".to_owned(),
            }
        );
        assert_eq!(probe.close_calls, 1);
        assert_eq!(probe.open_system_calls, 0);
    }

    #[test]
    fn custom_mode_rejects_bad_packaging_and_escaped_paths_before_opening() {
        let mut no_legacy = TurnipDriverConfig::bundled_turnip(
            "/data/app/lib/arm64",
            "/data/user/0/org.uncharted.universal/files",
            "/data/user/0/org.uncharted.universal/files/drivers/libvulkan_freedreno.so",
        );
        no_legacy.use_legacy_packaging = false;
        let mut probe = MockProbe::turnip_ok();
        assert_eq!(
            open_validated_driver(&no_legacy, &mut probe).unwrap_err(),
            DriverValidationError::LegacyPackagingRequired
        );
        assert_eq!(probe.open_custom_calls, 0);

        for bad_so in [
            "/data/user/0/org.uncharted.universal/files_evil/libvulkan_freedreno.so",
            "/data/user/0/org.uncharted.universal/files/../libvulkan_freedreno.so",
            "/data/local/tmp/libvulkan_freedreno.so",
            "",
        ] {
            let cfg = TurnipDriverConfig::bundled_turnip(
                "/data/app/lib/arm64",
                "/data/user/0/org.uncharted.universal/files",
                bad_so,
            );
            assert_eq!(
                open_validated_driver(&cfg, &mut probe).unwrap_err(),
                DriverValidationError::InvalidCustomDriverPath
            );
        }
        assert_eq!(probe.open_custom_calls, 0);
    }

    #[test]
    fn explicit_system_mode_still_requires_non_empty_driver_identity() {
        let sys = TurnipDriverConfig::system_adreno();
        let mut ok_probe = MockProbe {
            allow_system: true,
            reported_driver: Some("Qualcomm Adreno Vulkan".to_owned()),
            ..MockProbe::default()
        };
        let opened = open_validated_driver(&sys, &mut ok_probe).expect("system adreno ok");
        assert_eq!(opened.mode, DriverMode::System);
        assert_eq!(opened.handle, 1);

        let mut empty_probe = MockProbe {
            allow_system: true,
            reported_driver: Some("   ".to_owned()),
            ..MockProbe::default()
        };
        assert_eq!(
            open_validated_driver(&sys, &mut empty_probe).unwrap_err(),
            DriverValidationError::EmptyReportedDriver
        );
        assert_eq!(empty_probe.close_calls, 1);
    }

    fn valid_mali_g710_caps() -> MaliVulkanCaps {
        MaliVulkanCaps {
            vendor_id: VENDOR_ID_ARM,
            api_version: VK_API_VERSION_1_3,
            dynamic_rendering: true,
            synchronization2: true,
            timeline_semaphore: true,
            buffer_device_address: true,
            swapchain_extension: true,
            driver_name: "Mali-G710".to_owned(),
        }
    }

    #[test]
    fn gpu_classification_and_driver_selection_cover_adreno_and_tensor_mali() {
        assert_eq!(
            classify_android_gpu(VENDOR_ID_QUALCOMM, "Adreno 750"),
            AndroidGpuFamily::Adreno
        );
        assert_eq!(
            classify_android_gpu(VENDOR_ID_ARM, "Mali-G710"),
            AndroidGpuFamily::Mali
        );
        assert_eq!(
            classify_android_gpu(0, "Mali-G710 r0p0"),
            AndroidGpuFamily::Mali
        );
        assert_eq!(
            classify_android_gpu(0x10de, "NVIDIA"),
            AndroidGpuFamily::Unknown
        );

        let turnip = TurnipDriverConfig::bundled_turnip(
            "/data/app/lib/arm64",
            "/data/user/0/org.uncharted.universal/files",
            "/data/user/0/org.uncharted.universal/files/drivers/libvulkan_freedreno.so",
        );
        assert_eq!(
            select_driver_for_gpu(VENDOR_ID_QUALCOMM, "Adreno 750", Some(&turnip)),
            Ok(turnip.clone())
        );
        assert_eq!(
            select_driver_for_gpu(
                VENDOR_ID_QUALCOMM,
                "Adreno 750",
                Some(&TurnipDriverConfig::system_adreno())
            ),
            Err(DriverValidationError::TurnipRequiredForAdreno)
        );
        assert_eq!(
            select_driver_for_gpu(VENDOR_ID_QUALCOMM, "Adreno 750", None),
            Err(DriverValidationError::TurnipRequiredForAdreno)
        );
        assert_eq!(
            select_driver_for_gpu(VENDOR_ID_ARM, "Mali-G710", None),
            Ok(TurnipDriverConfig::system_mali())
        );
        assert_eq!(
            select_driver_for_gpu(0x9999, "UnknownGPU", Some(&turnip)),
            Err(DriverValidationError::UnsupportedGpuVendor {
                vendor_id: 0x9999,
                name: "UnknownGPU".to_owned(),
            })
        );
    }

    #[test]
    fn tensor_mali_validates_vulkan_13_caps_and_fails_closed_when_missing() {
        let mali_cfg = TurnipDriverConfig::system_mali();
        let mut ok_probe = MockProbe {
            allow_system: true,
            reported_driver: Some("Mali-G710".to_owned()),
            mali_caps: Some(Ok(valid_mali_g710_caps())),
            ..MockProbe::default()
        };
        let opened =
            open_validated_driver(&mali_cfg, &mut ok_probe).expect("Tensor/Mali should pass");
        assert_eq!(opened.mode, DriverMode::System);
        assert_eq!(opened.reported_name, "Mali-G710");
        assert_eq!(ok_probe.open_system_calls, 1);
        assert_eq!(ok_probe.open_custom_calls, 0);

        // Vulkan 1.2 fails closed and closes handle.
        let mut vk12_caps = valid_mali_g710_caps();
        vk12_caps.api_version = (1 << 22) | (2 << 12);
        let mut vk12_probe = MockProbe {
            allow_system: true,
            reported_driver: Some("Mali-G710".to_owned()),
            mali_caps: Some(Ok(vk12_caps)),
            ..MockProbe::default()
        };
        assert!(matches!(
            open_validated_driver(&mali_cfg, &mut vk12_probe),
            Err(DriverValidationError::MaliUnsupportedCapability(_))
        ));
        assert_eq!(vk12_probe.close_calls, 1);

        // Missing dynamicRendering fails closed and closes handle.
        let mut no_dyn_caps = valid_mali_g710_caps();
        no_dyn_caps.dynamic_rendering = false;
        let mut no_dyn_probe = MockProbe {
            allow_system: true,
            reported_driver: Some("Mali-G710".to_owned()),
            mali_caps: Some(Ok(no_dyn_caps)),
            ..MockProbe::default()
        };
        assert_eq!(
            open_validated_driver(&mali_cfg, &mut no_dyn_probe),
            Err(DriverValidationError::MaliUnsupportedCapability(
                "Tensor/Mali driver missing required dynamicRendering".to_owned()
            ))
        );
        assert_eq!(no_dyn_probe.close_calls, 1);
    }
}
