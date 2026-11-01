//! The fleet view (SPEC.md M12): what each paired device last reported, and the warning before
//! play when this device's build differs from the one that made the latest save. New module;
//! the shape is copied from iannitram/totk-native's `fleet.rs` (MIT). Records are plain data and
//! live in core (no network), unlike `hub`.

use crate::error::{not_built, Result};
use crate::platform::Os;

/// What one launcher reports to its hubs after each sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRecord {
    /// The device's name as the player set it ("AMD handheld").
    pub device: String,
    pub os: Os,
    /// `bundle::Bundle::build_id` of the installed build.
    pub build_id: String,
    pub patch_level: u32,
    /// Seconds since the Unix epoch of the last save sync; 0 for never.
    pub last_sync_unix: u64,
    pub free_bytes: u64,
}

/// What `drift` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    /// Same build.
    None,
    /// A different build with a different number of our patches.
    Patches { theirs: u32, ours: u32 },
    /// A different build at the same patch level (a different port revision).
    Build { theirs: String, ours: String },
}

/// Compares this device with the one that made the latest save. Same `build_id`: `None`.
/// Otherwise a different `patch_level` is `Patches`, else `Build`.
pub fn drift(here: &DeviceRecord, saved_by: &DeviceRecord) -> Drift {
    not_built!("fleet::drift")
}

/// The warning the Continue screen shows, or `None` for `Drift::None`. It names
/// `saved_by.device`, says what differs in plain words, and offers "update this device first, or
/// play on and keep both saves".
pub fn warning(drift: &Drift, saved_by: &DeviceRecord) -> Option<String> {
    not_built!("fleet::warning")
}

/// A record as one tab-separated line, no trailing newline: device, os key, build id, patch
/// level, last sync, free bytes. A tab or newline in `device` is
/// `Error::Hub("a device name cannot hold a tab or a new line")`.
pub fn to_line(r: &DeviceRecord) -> Result<String> {
    not_built("fleet::to_line")
}

/// Reads `to_line`'s format back; anything else is `Error::Hub("bad device record: <line>")`.
pub fn from_line(line: &str) -> Result<DeviceRecord> {
    not_built("fleet::from_line")
}
