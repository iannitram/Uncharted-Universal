//! The environment the runtime starts with: per-game defaults for the `BB_*` switches
//! (`fix_defaults`), and the `BB_*` variables built from the player's settings and the device
//! profile. The player's own variables always win. Replaces `fix_defaults` and the environment
//! assembly in `main` of `run.py`.

use crate::error::Result;
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
/// the same copy64 pair, `BB_READBACK_SMALL_BUFFERS=0`, `BB_TOGGLE2=12`. U3: `BB_GPU_MERGE=0`,
/// `BB_TINY_READBACK=0`, copy64 `0`/`0`, `BB_READBACK_SMALL_BUFFERS=0`, `BB_TOGGLE2=12`. Setting
/// `BB_SCALAR_COPY64_SYNC` drops both copy64 defaults.
pub fn fix_defaults(player_env: &BTreeMap<String, String>, game: Game) -> BTreeMap<String, String> {
    let mut defaults = BTreeMap::new();

    let (gpu_merge, tiny_readback, copy64_sync, copy64_defer) = match game {
        Game::U1 => ("0", "0", "2", "1"),
        Game::U2 => ("0", "1", "2", "1"),
        Game::U3 => ("0", "0", "0", "0"),
    };

    for (name, value) in [
        ("BB_GPU_MERGE", gpu_merge),
        ("BB_TINY_READBACK", tiny_readback),
    ] {
        if !player_env.contains_key(name) {
            defaults.insert(name.to_owned(), value.to_owned());
        }
    }

    if !player_env.contains_key("BB_SCALAR_COPY64_SYNC") {
        defaults.insert("BB_SCALAR_COPY64_SYNC".to_owned(), copy64_sync.to_owned());
        defaults.insert("BB_SCALAR_COPY64_DEFER".to_owned(), copy64_defer.to_owned());
    }

    // Brightness flicker fix: on for U2 only; U1 and U3 are explicitly off (issue #192).
    let small_buffers = if game == Game::U2 { "1" } else { "0" };
    if !player_env.contains_key("BB_READBACK_SMALL_BUFFERS") {
        defaults.insert(
            "BB_READBACK_SMALL_BUFFERS".to_owned(),
            small_buffers.to_owned(),
        );
    }

    if game != Game::U2 && !player_env.contains_key("BB_TOGGLE2") {
        defaults.insert("BB_TOGGLE2".to_owned(), "12".to_owned());
    }

    // An explicit scalar-copy mode disables both defaults as a pair.
    if player_env.contains_key("BB_SCALAR_COPY64_SYNC") {
        defaults.remove("BB_SCALAR_COPY64_SYNC");
        defaults.remove("BB_SCALAR_COPY64_DEFER");
    }

    defaults
}

/// The full environment to start `game` with, in name order: `fix_defaults`, then the device
/// profile's values, then `settings`, then the player's own variables.
pub fn build_env(
    game: Game,
    settings: &Settings,
    profile: &DeviceProfile,
    player_env: &BTreeMap<String, String>,
) -> Result<Vec<(String, String)>> {
    let mut env = fix_defaults(player_env, game);

    // Device defaults, launcher settings, and explicit player variables override in that order.
    env.extend(
        profile
            .env
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
    );
    if let Some(resolution) = &settings.resolution {
        env.insert("BB_RESOLUTION".to_owned(), resolution.clone());
    }
    if let Some(quality) = &settings.upscale_quality {
        env.insert("BB_UPSCALE_QUALITY".to_owned(), quality.clone());
    }
    env.extend(settings.extra.clone());
    env.extend(player_env.clone());

    // Values `run.py` derives again on every start, so a stale one never survives.
    for name in [
        "BB_SCENE_SIZE",
        "BB_MOVIE_SIZE",
        "BB_RENDER_RES",
        "BB_OUTPUT_RES",
        "BB_UI_COMPOSITE_PS",
        "BB_UI_TONEMAP_PS",
    ] {
        env.remove(name);
    }

    // Preserve the legacy `run.py::game_res` behavior: canonicalize a valid explicit game size,
    // derive its heap sizes, and discard an invalid/default 1080p value. Existing numeric heap
    // overrides are passed through by `game_res_env`.
    let game_res = crate::render_size::game_res_env(&env);
    if env.contains_key("BB_GAME_RES") && !game_res.contains_key("BB_GAME_RES") {
        env.remove("BB_GAME_RES");
    }
    env.extend(game_res);

    // A direct memory size the player set stays theirs (`run.py` copies it after `game_res`).
    if let Some(dmem) = env.get("BB_DMEM_MB").cloned() {
        if !dmem.is_empty() {
            env.entry("BB_DMEM_MB_USER".to_owned()).or_insert(dmem);
        }
    }
    // An explicit game size is also the scene and movie size (`render_resolution`).
    if let Some(size) = env.get("BB_GAME_RES").filter(|v| !v.is_empty()).cloned() {
        env.insert("BB_SCENE_SIZE".to_owned(), size.clone());
        env.insert("BB_MOVIE_SIZE".to_owned(), size);
    }

    // The renderer's shader hashes (the same for all three games) and the display rate.
    for (name, value) in [
        ("BB_VELOCITY_PS", "f4b18264"),
        ("BB_UPSCALE_BEFORE_CS", "aec539f0"),
        ("BB_HUDLESS_PS", "484cdbf5"),
        ("BB_JITTER_PS", "f614041e,b2a42acf,1a08df2d"),
        ("BB_VBLANK_HZ", "60"),
    ] {
        env.entry(name.to_owned())
            .or_insert_with(|| value.to_owned());
    }

    // U3's raycaster copy readback is on unless the player chose; no other game gets it.
    if !player_env.contains_key("BB_COPY64_EOP_READBACK") {
        if game == Game::U3 {
            env.insert("BB_COPY64_EOP_READBACK".to_owned(), "1".to_owned());
        } else {
            env.remove("BB_COPY64_EOP_READBACK");
        }
    }
    if !player_env.contains_key("BB_READBACK_SMALL_GATE") {
        env.remove("BB_READBACK_SMALL_GATE");
    }

    // U1 opens its own menu, not the collection's, unless the player (or the direct start of
    // `BB_U1_DIRECT=1`) says otherwise.
    if game == Game::U1
        && !env.contains_key("BB_SKIP_COLLECTION_MENU")
        && env.get("BB_U1_DIRECT").map(String::as_str) != Some("1")
    {
        env.insert("BB_SKIP_COLLECTION_MENU".to_owned(), "1".to_owned());
    }

    Ok(env.into_iter().collect())
}
