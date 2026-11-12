//! Device profiles (SPEC.md M5, M13): the settings a device starts the game with (resolution,
//! frame cap, power advice). Values are starting points, replaced by measured ones in the device
//! runs (stage 5). Power is advice shown on the Settings page; setting a power limit is the
//! player's choice in Steam's performance menu, never ours. Replaces Deck mode in
//! `launcher/uncharted_linux.py`.
//!
//! The shape follows iannitram/totk-native's `profile.rs` (MIT).

use crate::platform::Os;

/// One device's profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceProfile {
    /// "amd-handheld", "strix-halo-tablet", "desktop", "mac", "ios", "android".
    pub id: &'static str,
    pub name: &'static str,
    /// The hardware it is for, in words.
    pub hardware: &'static str,
    pub os: Os,
    /// The `BB_*` values this device starts with.
    pub env: &'static [(&'static str, &'static str)],
    /// Frame rates the device offers, in Hz (the AMD handheld: 30 and 60).
    pub frame_rates: &'static [u32],
    /// Power advice for the Settings page; empty when there is none (the AMD handheld: 20 W).
    pub power: &'static str,
}

/// What Steam Deck mode in `launcher/uncharted_linux.py` (`deck_environment`) sets: the 7-inch
/// screen at 1280x800, a 720p render, FIFO presentation (gamescope paces it best), fullscreen,
/// frame stats for tuning, Xbox button pictures, and no upscaler or frame generation the RDNA 2
/// GPU cannot run. 30 Hz by default (`DECK_SETTINGS`); 60 is the other choice (`DECK_FRAME_RATES`).
const AMD_HANDHELD_ENV: &[(&str, &str)] = &[
    ("BB_VBLANK_HZ", "30"),
    ("BB_WINDOW_SIZE", "1280x800"),
    ("BB_RENDER_720P", "1"),
    ("BB_PRESENT_MODE", "Fifo"),
    ("BB_FULLSCREEN", "1"),
    ("BB_FRAME_STATS", "1"),
    ("BB_DECK_PROMPTS", "1"),
    ("BB_UPSCALER", "off"),
    ("BB_FRAME_GEN", "off"),
];

const STRIX_HALO_TABLET_ENV: &[(&str, &str)] = &[
    ("BB_VBLANK_HZ", "60"),
    ("BB_WINDOW_SIZE", "2560x1600"),
    ("BB_PRESENT_MODE", "Fifo"),
    ("BB_FULLSCREEN", "1"),
];

const DESKTOP_ENV: &[(&str, &str)] = &[("BB_VBLANK_HZ", "60")];

const MAC_ENV: &[(&str, &str)] = &[("BB_VBLANK_HZ", "60"), ("BB_PRESENT_MODE", "Fifo")];

const MOBILE_ENV: &[(&str, &str)] = &[
    ("BB_VBLANK_HZ", "30"),
    ("BB_PRESENT_MODE", "Fifo"),
    ("BB_FULLSCREEN", "1"),
];

static BUILTIN: [DeviceProfile; 6] = [
    DeviceProfile {
        id: "amd-handheld",
        name: "AMD handheld",
        hardware: "SteamOS handhelds: Steam Deck (Van Gogh APU), ROG Ally Z1 Extreme and the like",
        os: Os::Linux,
        env: AMD_HANDHELD_ENV,
        frame_rates: &[30, 60],
        power: "20 W",
    },
    DeviceProfile {
        id: "strix-halo-tablet",
        name: "Strix Halo tablet",
        hardware: "AMD Ryzen AI Max+ 395 tablet",
        os: Os::Linux,
        env: STRIX_HALO_TABLET_ENV,
        frame_rates: &[30, 60, 120],
        power: "",
    },
    DeviceProfile {
        id: "desktop",
        name: "Desktop",
        hardware: "Linux or Windows PC with a discrete GPU",
        os: Os::Windows,
        env: DESKTOP_ENV,
        frame_rates: &[30, 60, 120],
        power: "",
    },
    DeviceProfile {
        id: "mac",
        name: "Mac",
        hardware: "Apple silicon Mac",
        os: Os::Mac,
        env: MAC_ENV,
        frame_rates: &[30, 60],
        power: "",
    },
    DeviceProfile {
        id: "ios",
        name: "iPhone and iPad",
        hardware: "Apple A- and M-series iPhone and iPad",
        os: Os::Ios,
        env: MOBILE_ENV,
        frame_rates: &[30, 60],
        power: "",
    },
    DeviceProfile {
        id: "android",
        name: "Android",
        hardware: "Android arm64 with an Adreno GPU",
        os: Os::Android,
        env: MOBILE_ENV,
        frame_rates: &[30, 60],
        power: "",
    },
];

/// The built-in profiles, in the order of the ids above, with the values the stage-5 device
/// runs measure.
pub fn builtin() -> &'static [DeviceProfile] {
    &BUILTIN
}

/// The built-in profile with this id.
pub fn by_id(id: &str) -> Option<&'static DeviceProfile> {
    BUILTIN.iter().find(|profile| profile.id == id)
}

fn builtin_profile(id: &str) -> &'static DeviceProfile {
    by_id(id).expect("built-in profile id")
}

/// Pick a profile for this device from its OS and CPU model name (`/proc/cpuinfo` "model name",
/// or the OS's own): a Z1 Extreme is the AMD handheld, an AI Max+ 395 the Strix Halo tablet,
/// any other Linux or Windows PC the desktop; Mac, iOS and Android by OS.
pub fn detect(os: Os, cpu_model: &str) -> &'static DeviceProfile {
    detect_with_os_release(os, cpu_model, "")
}

/// `detect`, with the contents of `/etc/os-release` (read by the caller; empty when there is
/// none). `ID=steamos` marks a SteamOS device, as `steam_deck()` does in the launcher; on Linux
/// that is the AMD handheld unless the CPU is a Strix Halo. The Steam Deck's own APU reports as
/// "AMD Custom APU 0405" (LCD) or "0932" (OLED).
pub fn detect_with_os_release(os: Os, cpu_model: &str, os_release: &str) -> &'static DeviceProfile {
    match os {
        Os::Mac => builtin_profile("mac"),
        Os::Ios => builtin_profile("ios"),
        Os::Android => builtin_profile("android"),
        Os::Linux | Os::Windows => {
            let cpu = cpu_model.to_ascii_lowercase();
            if cpu.contains("ai max") && cpu.contains("395") {
                builtin_profile("strix-halo-tablet")
            } else if is_handheld_cpu(&cpu) || (os == Os::Linux && is_steamos(os_release)) {
                builtin_profile("amd-handheld")
            } else {
                builtin_profile("desktop")
            }
        }
    }
}

fn is_handheld_cpu(cpu: &str) -> bool {
    cpu.contains("z1 extreme") || cpu.contains("custom apu 0405") || cpu.contains("custom apu 0932")
}

fn is_steamos(os_release: &str) -> bool {
    os_release.lines().any(|line| {
        line.trim()
            .strip_prefix("ID=")
            .is_some_and(|id| id.trim_matches(['"', '\'']) == "steamos")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn ids_are_unique_and_found() {
        let ids: Vec<_> = builtin().iter().map(|p| p.id).collect();
        assert_eq!(
            ids,
            [
                "amd-handheld",
                "strix-halo-tablet",
                "desktop",
                "mac",
                "ios",
                "android"
            ]
        );
        for profile in builtin() {
            assert_eq!(by_id(profile.id), Some(profile));
        }
        assert_eq!(by_id("nope"), None);
    }

    #[test]
    fn handheld_matches_deck_mode() {
        let p = by_id("amd-handheld").unwrap();
        assert_eq!(p.frame_rates, &[30, 60]);
        assert_eq!(p.power, "20 W");
        let env: BTreeMap<_, _> = p.env.iter().copied().collect();
        assert_eq!(env["BB_VBLANK_HZ"], "30");
        assert_eq!(env["BB_WINDOW_SIZE"], "1280x800");
        assert_eq!(env["BB_RENDER_720P"], "1");
        assert_eq!(env["BB_PRESENT_MODE"], "Fifo");
        assert_eq!(env["BB_FULLSCREEN"], "1");
        assert_eq!(env["BB_DECK_PROMPTS"], "1");
        assert_eq!(env["BB_UPSCALER"], "off");
    }

    #[test]
    fn detects_by_os_and_cpu() {
        let id = |os, cpu| detect(os, cpu).id;
        assert_eq!(id(Os::Linux, "AMD Ryzen Z1 Extreme"), "amd-handheld");
        assert_eq!(id(Os::Windows, "AMD Ryzen Z1 Extreme"), "amd-handheld");
        assert_eq!(id(Os::Linux, "AMD Custom APU 0405"), "amd-handheld");
        assert_eq!(
            id(Os::Linux, "AMD RYZEN AI MAX+ 395 w/ Radeon 8060S"),
            "strix-halo-tablet"
        );
        assert_eq!(id(Os::Linux, "AMD Ryzen AI Max 390"), "desktop");
        assert_eq!(
            id(Os::Windows, "13th Gen Intel(R) Core(TM) i9-13900K"),
            "desktop"
        );
        assert_eq!(id(Os::Linux, ""), "desktop");
        assert_eq!(id(Os::Mac, "Apple M3"), "mac");
        assert_eq!(id(Os::Ios, "Apple A17"), "ios");
        assert_eq!(id(Os::Android, "Snapdragon"), "android");
    }

    #[test]
    fn steamos_marks_a_handheld() {
        let steamos = "NAME=\"SteamOS\"\nID=steamos\nID_LIKE=arch\n";
        let arch = "NAME=\"Arch Linux\"\nID=arch\n";
        let det = |os, cpu, rel| detect_with_os_release(os, cpu, rel).id;
        assert_eq!(det(Os::Linux, "Some APU", steamos), "amd-handheld");
        assert_eq!(
            det(Os::Linux, "Some APU", "ID=\"steamos\"\n"),
            "amd-handheld"
        );
        assert_eq!(det(Os::Linux, "Some APU", arch), "desktop");
        assert_eq!(
            det(Os::Linux, "AMD Ryzen AI Max+ 395", steamos),
            "strix-halo-tablet"
        );
        // The launcher only looks at os-release on Linux.
        assert_eq!(det(Os::Windows, "Some CPU", steamos), "desktop");
        // ID_LIKE=steamos is not ID=steamos.
        assert_eq!(det(Os::Linux, "x", "ID_LIKE=steamos\n"), "desktop");
    }
}
