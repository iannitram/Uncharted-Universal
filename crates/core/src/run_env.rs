//! The environment the runtime starts with: per-game defaults for the `BB_*` switches
//! (`fix_defaults`), and the `BB_*` variables built from the player's settings and the device
//! profile. The player's own variables always win. Replaces `fix_defaults` and the environment
//! assembly in `main` of `run.py`.

use crate::error::{not_built, Result};
use crate::games::Game;
use crate::profile::DeviceProfile;
use std::collections::BTreeMap;

/// The player's settings, as the settings page stores them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Settings {
    /// `BB_RESOLUTION[_U1..]`: "WxH".
    pub resolution: Option<String>,
    /// `BB_UPSCALE_QUALITY[_U1..]`: quality, balanced, performance or ultra.
    pub upscale_quality: Option<String>,
    /// Anything else the player set, by variable name.
    pub extra: BTreeMap<String, String>,
}

/// Defaults for `game` that the player has not set. `player_env` holds the variables the player
/// already set; a name in it is never returned. U2: `BB_GPU_MERGE=0`, `BB_TINY_READBACK=1`,
/// `BB_SCALAR_COPY64_SYNC=2` with `_DEFER=1`, `BB_READBACK_SMALL_BUFFERS=1`. U1: `BB_TINY_READBACK=0`,
/// the same copy64 pair, `BB_TOGGLE2=12`. U3: `BB_GPU_MERGE=0`, `BB_TINY_READBACK=0`, copy64
/// `0`/`0`, `BB_TOGGLE2=12`. Setting `BB_SCALAR_COPY64_SYNC` drops both copy64 defaults.
pub fn fix_defaults(player_env: &BTreeMap<String, String>, game: Game) -> BTreeMap<String, String> {
    not_built!("run_env::fix_defaults")
}

/// The full environment to start `game` with, in name order: `fix_defaults`, then the device
/// profile's values, then `settings`, then the player's own variables.
pub fn build_env(
    game: Game,
    settings: &Settings,
    profile: &DeviceProfile,
    player_env: &BTreeMap<String, String>,
) -> Result<Vec<(String, String)>> {
    not_built("run_env::build_env")
}
