//! What each target needs and which build hub prepares it (ADR 0004): `Target` (linux, windows,
//! mac, ios, android; the same enum as `platform::Os`), `Hub` (strix-halo, desktop, mac), the
//! hub that serves each target. New module, no Python counterpart. The hub table is the one in
//! ADR 0004 and filled in; the per-target steps are the recipe issue's (piece 1.24).

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
    /// "strix-halo", "desktop", "mac".
    pub fn key(self) -> &'static str {
        match self {
            Hub::StrixHalo => "strix-halo",
            Hub::Desktop => "desktop",
            Hub::Mac => "mac",
        }
    }

    /// The inverse of [`Hub::key`].
    pub fn from_key(key: &str) -> Result<Hub> {
        [Hub::StrixHalo, Hub::Desktop, Hub::Mac]
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

/// The targets `hub` prepares for.
pub fn serves(hub: Hub) -> &'static [Target] {
    match hub {
        Hub::StrixHalo => &[Os::Linux, Os::Android],
        Hub::Desktop => &[Os::Windows],
        Hub::Mac => &[Os::Mac, Os::Ios],
    }
}

/// What preparing for one target takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    pub target: Target,
    pub hub: Hub,
    /// Steps in order, as the first-run page names them ("check dump", "boot image", ...).
    pub steps: Vec<&'static str>,
    /// True when the install carries a recompiled library instead of the executable (ADR 0006).
    pub recompiled_library: bool,
}

/// The recipe for `target`.
pub fn recipe_for(target: Target) -> Recipe {
    not_built!("recipe::recipe_for")
}
