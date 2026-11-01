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

/// The built-in profiles, in the order of the ids above, with the values the stage-5 device
/// runs measure.
pub fn builtin() -> &'static [DeviceProfile] {
    not_built!("profile::builtin")
}

/// The built-in profile with this id.
pub fn by_id(id: &str) -> Option<&'static DeviceProfile> {
    not_built!("profile::by_id")
}

/// Pick a profile for this device from its OS and CPU model name (`/proc/cpuinfo` "model name",
/// or the OS's own): a Z1 Extreme is the AMD handheld, an AI Max+ 395 the Strix Halo tablet,
/// any other Linux or Windows PC the desktop; Mac, iOS and Android by OS.
pub fn detect(os: Os, cpu_model: &str) -> &'static DeviceProfile {
    not_built!("profile::detect")
}
