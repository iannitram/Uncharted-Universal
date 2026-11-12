//! What each target needs and which build hub prepares it (ADR 0004): `Target` (linux, windows,
//! mac, ios, android; the same enum as `platform::Os`), `Hub` (strix-halo, desktop, mac), the
//! hub that serves each target, and `recipe_for`: the prepare steps for a target, in order, and
//! whether its install carries a recompiled library (ADR 0006). New module, no Python
//! counterpart: the step order is the preparation in `run.py` (`prepare`, `link_modules`,
//! `render_resolution`, `content_profile.py`, `trophies.py`, `patch_files_for`). The chapter
//! list of #178 is piece 1.27 (`tune`), not here.

use crate::error::{Error, Result};
use crate::platform::Os;

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
