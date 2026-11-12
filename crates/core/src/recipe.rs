//! What each target needs and which build hub prepares it (ADR 0004): `Target` (linux, windows,
//! mac, ios, android; the same enum as `platform::Os`), `Hub` (strix-halo, desktop, mac), the
//! hub that serves each target, and `recipe_for`: the prepare steps for a target, in order, and
//! whether its install carries a recompiled library (ADR 0006). New module, no Python
//! counterpart: the step order is the preparation in `run.py` (`prepare`, `link_modules`,
//! `render_resolution`, `content_profile.py`, `trophies.py`, `patch_files_for`).
//!
//! # Per-chapter fixes (#178, piece 1.27)
//! Some fixes cost frame rate everywhere but help only in a few chapters. Each
//! [`ChapterFix`] carries the chapter names, as the save shows them, where it is on (or off);
//! [`fixes_at`] reads the current chapter from the runtime's `where.json` (#168) and says
//! which fixes are on and which are off there, and [`chapter_env`] turns that into `BB_*`
//! values on top of `run_env::fix_defaults`. Pure functions: the runtime reading `where.json`
//! on each save and the live toggles (`BB_TOGGLE_FILE`, roadmap 3.06) are not here.

use crate::error::{Error, Result};
use crate::games::Game;
use crate::platform::Os;
use serde_json::Value;
use std::collections::BTreeMap;

/// A target is a device family: the `Os` the install runs on.
pub type Target = Os;

/// A build hub: a machine that holds the dump and prepares installs (SPEC.md M11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hub {
    /// The Strix Halo tablet: prepares for linux and android.
    StrixHalo,
    /// The RTX 3090 Ti desktop: prepares for windows.
    Desktop,
    /// The Mac: prepares for mac and ios.
    Mac,
}

impl Hub {
    /// Every hub, in the order above.
    pub const ALL: [Hub; 3] = [Hub::StrixHalo, Hub::Desktop, Hub::Mac];

    /// "strix-halo", "desktop", "mac".
    pub fn key(self) -> &'static str {
        match self {
            Hub::StrixHalo => "strix-halo",
            Hub::Desktop => "desktop",
            Hub::Mac => "mac",
        }
    }

    /// The inverse of [`Hub::key`]; any other key is `Error::Hub("no hub called <key>")`.
    pub fn from_key(key: &str) -> Result<Hub> {
        Hub::ALL
            .into_iter()
            .find(|h| h.key() == key)
            .ok_or_else(|| Error::Hub(format!("no hub called {key}")))
    }
}

/// The hub that prepares `target` (ADR 0004).
pub fn hub_for(target: Target) -> Hub {
    match target {
        Os::Linux | Os::Android => Hub::StrixHalo,
        Os::Windows => Hub::Desktop,
        Os::Mac | Os::Ios => Hub::Mac,
    }
}

/// The targets `hub` prepares for. For every target t, `serves(hub_for(t))` holds t.
pub fn serves(hub: Hub) -> &'static [Target] {
    match hub {
        Hub::StrixHalo => &[Os::Linux, Os::Android],
        Hub::Desktop => &[Os::Windows],
        Hub::Mac => &[Os::Mac, Os::Ios],
    }
}

/// The prepare steps, as the first-run page (2.05) names them.
pub mod step {
    /// The dump's folder layout and decrypted executables (`dump::check`).
    pub const CHECK_DUMP: &str = "check dump";
    /// The executable revision the port supports (`games::check_revision`).
    pub const CHECK_REVISION: &str = "check revision";
    /// The executable's memory image (`prepare`, `boot.bin`).
    pub const BOOT_IMAGE: &str = "boot image";
    /// The module import tables of the boot image (`link::link_modules`).
    pub const LINK_MODULES: &str = "link modules";
    /// The libc import tables of the boot image (`link::link_libc`).
    pub const LINK_LIBC: &str = "link libc";
    /// The size the game draws at (`render_size`).
    pub const RENDER_SIZE: &str = "render size";
    /// The third-party patches for the executable (`patches`).
    pub const PATCHES: &str = "patches";
    /// The executable's x86-64 code recompiled into a native ARM64 library (ADR 0006).
    pub const RECOMPILE: &str = "recompile";
    /// The content profile record (`content_profile`).
    pub const CONTENT_PROFILE: &str = "content profile";
    /// The game's trophy set for the runtime (`trophies`).
    pub const TROPHIES: &str = "trophies";
    /// The sealed bundle other devices pull from the hub (`bundle`, SPEC.md M12).
    pub const BUNDLE: &str = "bundle";
    /// The iPhone/iPad app, built on the Mac with Xcode signing (ADR 0006, SPEC.md M15).
    pub const BUILD_IOS_APP: &str = "build ios app";
    /// The iPhone/iPad app installed over a cable (ADR 0006, SPEC.md M15).
    pub const INSTALL_BY_CABLE: &str = "install by cable";
}

/// What preparing for one target takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    pub target: Target,
    pub hub: Hub,
    /// Steps in order, as the first-run page names them ([`step`]).
    pub steps: Vec<&'static str>,
    /// True when the install carries a recompiled library instead of the executable (ADR 0006).
    pub recompiled_library: bool,
}

/// The recipe for `target`, from this table (ADR 0004, 0005, 0006):
///
/// | target  | hub       | recompiled_library | steps |
/// |---------|-----------|--------------------|-------|
/// | Linux   | StrixHalo | false | the executable steps, then bundle |
/// | Windows | Desktop   | false | the executable steps, then bundle |
/// | Android | StrixHalo | false | the executable steps, then bundle (FEXCore runs the executable, ADR 0005) |
/// | Mac     | Mac       | true  | the recompiled steps, then bundle |
/// | Ios     | Mac       | true  | the recompiled steps, then build ios app, install by cable |
///
/// The executable steps are check dump, check revision, boot image, link modules, link libc,
/// render size, content profile, trophies, patches (the order of `run.py`). The recompiled
/// steps put render size and patches before recompile, since both change the executable's
/// bytes and the library is made from them: check dump, check revision, boot image, link
/// modules, link libc, render size, patches, recompile, content profile, trophies.
pub fn recipe_for(target: Target) -> Recipe {
    use step::*;
    const EXECUTABLE: &[&str] = &[
        CHECK_DUMP,
        CHECK_REVISION,
        BOOT_IMAGE,
        LINK_MODULES,
        LINK_LIBC,
        RENDER_SIZE,
        CONTENT_PROFILE,
        TROPHIES,
        PATCHES,
    ];
    const RECOMPILED: &[&str] = &[
        CHECK_DUMP,
        CHECK_REVISION,
        BOOT_IMAGE,
        LINK_MODULES,
        LINK_LIBC,
        RENDER_SIZE,
        PATCHES,
        RECOMPILE,
        CONTENT_PROFILE,
        TROPHIES,
    ];
    let (prepare, finish, recompiled_library): (&[&str], &[&str], bool) = match target {
        Os::Linux | Os::Windows | Os::Android => (EXECUTABLE, &[BUNDLE], false),
        Os::Mac => (RECOMPILED, &[BUNDLE], true),
        Os::Ios => (RECOMPILED, &[BUILD_IOS_APP, INSTALL_BY_CABLE], true),
    };
    Recipe {
        target,
        hub: hub_for(target),
        steps: prepare.iter().chain(finish).copied().collect(),
        recompiled_library,
    }
}

/// One fix whose on/off state follows the chapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChapterFix {
    /// The fix's key on the launcher's Fixes page (`FIXES` in `launcher/uncharted_launcher.py`).
    pub key: &'static str,
    pub game: Game,
    /// The variables that turn it on, and the ones that turn it off (the Fixes page's values).
    pub on: &'static [(&'static str, &'static str)],
    pub off: &'static [(&'static str, &'static str)],
    /// Chapter names as the save shows them; each matches as a case-insensitive part of the
    /// chapter (the same rule as `logs::match_known` uses for locations).
    pub chapters: &'static [&'static str],
    /// True: on in `chapters`, off elsewhere. False: off in `chapters`, on elsewhere (a fix
    /// that is needed in general but costs too much frame rate there).
    pub on_in_chapters: bool,
    /// The issue that measured it.
    pub issue: &'static str,
}

/// The fixes that follow the chapter. Seeded with Uncharted 2's brightness flicker fix
/// (on in Nepal, where it removes the darkening and black weapons, #192) and its glitch fixes
/// (aim focus and exact GPU results, which cost most of the frame rate in the Borneo jungle,
/// #157). The chapter names are a seed until a real U2 save confirms them (#178).
pub const CHAPTER_FIXES: &[ChapterFix] = &[
    ChapterFix {
        key: "fix_small_buffers",
        game: Game::U2,
        on: &[("BB_READBACK_SMALL_BUFFERS", "1")],
        off: &[("BB_READBACK_SMALL_BUFFERS", "0")],
        chapters: &["Nepal", "Urban Warfare", "Desperate Times"],
        on_in_chapters: true,
        issue: "#192",
    },
    ChapterFix {
        key: "fix_tiny_readback",
        game: Game::U2,
        on: &[("BB_TINY_READBACK", "1")],
        off: &[("BB_TINY_READBACK", "0")],
        chapters: &["Borneo", "Getting Warmer"],
        on_in_chapters: false,
        issue: "#157",
    },
    ChapterFix {
        key: "fix_u2_gpu_results",
        game: Game::U2,
        on: &[("BB_GPU_MERGE", "0")],
        off: &[("BB_GPU_MERGE", "1")],
        chapters: &["Borneo", "Getting Warmer"],
        on_in_chapters: false,
        issue: "#157",
    },
];

impl ChapterFix {
    /// Whether this fix is on in `chapter`.
    pub fn on_in(&self, chapter: &str) -> bool {
        let chapter = chapter.to_lowercase();
        let listed = self
            .chapters
            .iter()
            .any(|name| !name.is_empty() && chapter.contains(&name.to_lowercase()));
        listed == self.on_in_chapters
    }

    /// The variables this fix sets in that state.
    pub fn values(&self, on: bool) -> &'static [(&'static str, &'static str)] {
        if on {
            self.on
        } else {
            self.off
        }
    }
}

/// The chapter in a `where.json` (#168): the subtitle, else the title (as `logs::record`
/// takes it), trimmed. `None` when the file is damaged, belongs to another game or names no
/// place. The file is the runtime's own; its text is never echoed.
pub fn chapter_of(game: Game, where_json: &str) -> Option<String> {
    let place: Value = serde_json::from_str(where_json).ok()?;
    if place.get("game")?.as_str()? != game.key() {
        return None;
    }
    ["subtitle", "title"]
        .iter()
        .filter_map(|key| place.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .find(|text| !text.is_empty())
        .map(str::to_owned)
}

/// One fix and whether it is on at the current place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixState {
    pub fix: &'static ChapterFix,
    pub on: bool,
}

/// Which of `game`'s [`CHAPTER_FIXES`] are on and which are off at the place `where_json`
/// names, in list order. With no `where.json`, or one that names no chapter of `game`, the
/// list is empty: every fix keeps its `run_env::fix_defaults` value.
pub fn fixes_at(game: Game, where_json: Option<&str>) -> Vec<FixState> {
    fixes_in(CHAPTER_FIXES, game, where_json)
}

/// [`fixes_at`] over another list of fixes.
pub fn fixes_in(
    fixes: &'static [ChapterFix],
    game: Game,
    where_json: Option<&str>,
) -> Vec<FixState> {
    let Some(chapter) = where_json.and_then(|json| chapter_of(game, json)) else {
        return Vec::new();
    };
    fixes
        .iter()
        .filter(|fix| fix.game == game)
        .map(|fix| FixState {
            fix,
            on: fix.on_in(&chapter),
        })
        .collect()
}

/// The `BB_*` values the chapter sets for `game`, to lay over `run_env::fix_defaults`. A fix
/// any of whose variables is in `player_env` is left out: the player's own choice wins.
pub fn chapter_env(
    player_env: &BTreeMap<String, String>,
    game: Game,
    where_json: Option<&str>,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for state in fixes_at(game, where_json) {
        let fix = state.fix;
        let chosen = fix
            .on
            .iter()
            .chain(fix.off)
            .any(|(name, _)| player_env.contains_key(*name));
        if chosen {
            continue;
        }
        for (name, value) in fix.values(state.on) {
            env.insert((*name).to_owned(), (*value).to_owned());
        }
    }
    env
}
