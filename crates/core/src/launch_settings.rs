//! Shared display policy and fresh persisted settings for launching.
use crate::profile::{self, DeviceProfile};
use crate::run_env;
use serde_json::Value;
use std::path::Path;

/// Display fields shared by the app and launchers; UI-only settings remain in the app.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Display {
    pub device_profile: String,
    pub frame_rate: String,
    pub render_resolution: String,
    pub anti_aliasing: String,
    pub upscale_preset: String,
    pub frame_generation: String,
    pub stereo_mode: String,
}

impl Display {
    pub fn from_json(value: &Value) -> Self {
        let get = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        Self {
            device_profile: get("device_profile"),
            frame_rate: get("frame_rate"),
            render_resolution: get("render_resolution"),
            anti_aliasing: get("anti_aliasing"),
            upscale_preset: get("upscale_preset"),
            frame_generation: get("frame_generation"),
            stereo_mode: get("stereo_mode"),
        }
    }

    fn has_display_values(&self) -> bool {
        [
            &self.frame_rate,
            &self.render_resolution,
            &self.anti_aliasing,
            &self.upscale_preset,
            &self.frame_generation,
        ]
        .iter()
        .any(|s| !s.is_empty())
    }
}

pub fn profile_rate(profile: &DeviceProfile) -> &'static str {
    profile
        .env
        .iter()
        .find(|(name, _)| *name == "BB_VBLANK_HZ")
        .map_or("60", |(_, value)| value)
}

/// Whether `profile` allows `value` for `setting` (a settings key). The frame rate must be one
/// the profile offers; the AMD handheld keeps Steam Deck mode's rules (`DECK_UNAVAILABLE` in
/// `launcher/uncharted_linux.py`): no FSR 4, FSR 4.1.1 or DLSS, and no DLSS Frame Generation.
pub fn profile_allows(profile: &DeviceProfile, setting: &str, value: &str) -> bool {
    match (profile.id, setting) {
        (_, "frame_rate") => profile
            .frame_rates
            .iter()
            .any(|rate| rate.to_string() == value),
        ("amd-handheld", "anti_aliasing") => !matches!(value, "fsr4" | "fsr411" | "dlss"),
        ("amd-handheld", "frame_generation") => value != "dlss",
        _ => true,
    }
}

pub fn run_settings(stored: &Display, profile: &DeviceProfile) -> run_env::Settings {
    let or_off = |setting: &str, value: &str| {
        if value.is_empty() || !profile_allows(profile, setting, value) {
            "off".to_owned()
        } else {
            value.to_owned()
        }
    };
    let upscaler = or_off("anti_aliasing", &stored.anti_aliasing);
    let rate = if profile_allows(profile, "frame_rate", &stored.frame_rate) {
        stored.frame_rate.clone()
    } else {
        profile_rate(profile).to_owned()
    };
    let mut settings = run_env::Settings {
        resolution: (!stored.render_resolution.is_empty())
            .then(|| stored.render_resolution.clone()),
        upscale_quality: Some(if upscaler == "off" || stored.upscale_preset.is_empty() {
            "off".to_owned()
        } else {
            stored.upscale_preset.clone()
        }),
        ..run_env::Settings::default()
    };
    let extra = &mut settings.extra;
    extra.insert("BB_VBLANK_HZ".into(), rate);
    if upscaler == "fsr1" {
        extra.insert("BB_FSR1".into(), "1".into());
    }
    extra.insert(
        "BB_UPSCALER".into(),
        if upscaler == "fsr1" {
            "off".into()
        } else {
            upscaler
        },
    );
    extra.insert(
        "BB_FRAME_GEN".into(),
        or_off("frame_generation", &stored.frame_generation),
    );
    if !stored.stereo_mode.is_empty() && stored.stereo_mode != "off" {
        extra.insert("BB_STEREO_MODE".into(), stored.stereo_mode.clone());
    }
    settings
}

/// Profile and settings resolved from the current file, before process overrides.
#[derive(Debug)]
pub struct Resolved {
    pub profile: &'static DeviceProfile,
    pub settings: run_env::Settings,
}

/// Read the file afresh; invalid/missing data retains detected profile and launch defaults.
pub fn load_for_launch(settings_file: &Path, detected: &'static DeviceProfile) -> Resolved {
    let json = std::fs::read_to_string(settings_file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or(Value::Null);
    let display = Display::from_json(&json);
    let profile = profile::by_id(&display.device_profile).unwrap_or(detected);
    let mut settings = if display.has_display_values() {
        run_settings(&display, profile)
    } else {
        run_env::Settings::default()
    };
    if let Some(extra) = json.get("extra").and_then(Value::as_object) {
        settings.extra.extend(extra.iter().filter_map(|(k, v)| {
            if k.starts_with("BB_") {
                v.as_str().map(|v| (k.clone(), v.to_owned()))
            } else {
                None
            }
        }));
    }
    Resolved { profile, settings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::Game;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct File(std::path::PathBuf);
    impl File {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            Self(std::env::temp_dir().join(format!(
                "uu-launch-settings-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )))
        }
        fn write(&self, value: &str) {
            std::fs::write(&self.0, value).unwrap();
        }
    }
    impl Drop for File {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn absent_invalid_and_non_display_data_preserve_launch_defaults() {
        let file = File::new();
        let detected = profile::by_id("amd-handheld").unwrap();
        for value in [
            None,
            Some("null"),
            Some("[]"),
            Some("{broken"),
            Some("{}"),
            Some(r#"{"device_profile":"unknown","frame_rate":3,"button_prompts":"xbox"}"#),
        ] {
            if let Some(value) = value {
                file.write(value);
            }
            let got = load_for_launch(&file.0, detected);
            assert_eq!(got.profile.id, detected.id);
            assert_eq!(got.settings, run_env::Settings::default());
        }
    }

    #[test]
    fn persisted_display_extra_and_process_precedence() {
        let file = File::new();
        file.write(r#"{"device_profile":"desktop","frame_rate":"30","render_resolution":"1280x720","anti_aliasing":"fsr1","upscale_preset":"quality","frame_generation":"off","extra":{"BB_VBLANK_HZ":"60","BB_RESOLUTION_U2":"960x540","BB_IGNORED":3,"OTHER":"x"}}"#);
        let got = load_for_launch(&file.0, profile::by_id("amd-handheld").unwrap());
        assert_eq!(got.profile.id, "desktop");
        assert_eq!(got.settings.resolution.as_deref(), Some("1280x720"));
        assert_eq!(got.settings.upscale_quality.as_deref(), Some("quality"));
        assert_eq!(got.settings.extra["BB_FSR1"], "1");
        assert_eq!(got.settings.extra["BB_UPSCALER"], "off");
        assert_eq!(got.settings.extra["BB_FRAME_GEN"], "off");
        assert_eq!(got.settings.extra["BB_VBLANK_HZ"], "60");
        assert_eq!(got.settings.extra["BB_RESOLUTION_U2"], "960x540");
        assert!(!got.settings.extra.contains_key("BB_IGNORED"));
        assert!(!got.settings.extra.contains_key("OTHER"));
        let process = BTreeMap::from([("BB_VBLANK_HZ".into(), "120".into())]);
        let env: BTreeMap<_, _> =
            run_env::build_env(Game::U2, &got.settings, got.profile, &process)
                .unwrap()
                .into_iter()
                .collect();
        assert_eq!(env["BB_VBLANK_HZ"], "120");
        assert_eq!(env["BB_RESOLUTION_U2"], "960x540");
        file.write(r#"{"frame_rate":"30"}"#);
        let reread = load_for_launch(&file.0, got.profile);
        assert_eq!(reread.settings.extra["BB_VBLANK_HZ"], "30");
        assert_eq!(got.settings.extra["BB_VBLANK_HZ"], "60");
    }

    #[test]
    fn profile_restrictions_and_invalid_profile_follow_existing_policy() {
        let detected = profile::by_id("amd-handheld").unwrap();
        let file = File::new();
        file.write(r#"{"device_profile":"invalid","frame_rate":"120","anti_aliasing":"dlss","upscale_preset":"quality","frame_generation":"dlss"}"#);
        let got = load_for_launch(&file.0, detected);
        assert_eq!(got.profile.id, detected.id);
        assert_eq!(got.settings.extra["BB_VBLANK_HZ"], profile_rate(detected));
        assert_eq!(got.settings.extra["BB_UPSCALER"], "off");
        assert_eq!(got.settings.extra["BB_FRAME_GEN"], "off");
        assert_eq!(got.settings.upscale_quality.as_deref(), Some("off"));
    }
}
