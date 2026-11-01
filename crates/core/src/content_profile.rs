//! The offline AppContent metadata the game asks for through `sceAppContentAppParamGetInt`:
//! the four user-defined parameters, the SKU, no add-ons. Replaces `scripts/content_profile.py`
//! (the SKU is a probe setting, not a licence check).

use crate::error::{not_built, Result};
use crate::sfo::SfoValue;
use std::collections::BTreeMap;

/// `content.bin` starts with this and is 28 bytes: the magic, then five little-endian u32
/// (the SKU code, then the four user parameters).
pub const MAGIC: [u8; 8] = *b"BBCONT01";

/// The SKU the probe reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sku {
    /// Code 3 in `content.bin`.
    Full,
    /// Code 1 in `content.bin`.
    Trial,
}

impl Sku {
    /// The code written to `content.bin`: Full 3, Trial 1.
    pub fn code(self) -> u32 {
        match self {
            Sku::Full => 3,
            Sku::Trial => 1,
        }
    }
}

/// The values for one title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentProfile {
    pub title_id: Option<String>,
    pub sku: Sku,
    /// `USER_DEFINED_PARAM_1` .. `_4`, 0 when absent.
    pub user_params: [u32; 4],
}

/// Build the profile from a parsed `param.sfo`. A dump without `USER_DEFINED_PARAM_1` (the
/// collection) answers parameter 1 with its title number, as the console does
/// (`CUSA02320` -> 2320; no digits -> 0). A parameter that is not a `u32` is
/// `Error::Prepare("invalid user-defined parameter")`.
pub fn from_sfo(values: &BTreeMap<String, SfoValue>, sku: Sku) -> Result<ContentProfile> {
    not_built("content_profile::from_sfo")
}

impl ContentProfile {
    /// The 28 bytes of `content.bin`.
    pub fn to_bin(&self) -> [u8; 28] {
        not_built!("content_profile::ContentProfile::to_bin")
    }

    /// `content-profile.json` as the runtime reads it: `title_id`, `sku` ("full"/"trial"),
    /// `sku_source` ("explicit probe setting"), `user_params`, `mounted_addons` ([]),
    /// `boot_attr` (0), indented two spaces, ending in a newline.
    pub fn to_json(&self) -> String {
        not_built!("content_profile::ContentProfile::to_json")
    }
}
