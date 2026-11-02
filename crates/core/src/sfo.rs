//! The `param.sfo` reader (`sce_sys/param.sfo` of a dump or a save). Reads only, never writes.
//! Replaces `sfo()` in `scripts/prepare.py` (and the `game_title_id` / `game_app_version`
//! helpers of `scripts/patches.py`, which become callers of [`parse`]).

use crate::error::{not_built, Result};
use std::collections::BTreeMap;

/// `TITLE_ID` ("CUSA02320").
pub const TITLE_ID: &str = "TITLE_ID";
/// `APP_VER` ("01.02"): the build that runs; what patches and the revision gate key on.
pub const APP_VER: &str = "APP_VER";
/// `VERSION` ("01.00"): the content base version; used only when `APP_VER` is missing.
pub const VERSION: &str = "VERSION";
/// `TITLE`: the name shown on the console.
pub const TITLE: &str = "TITLE";

/// One value of a `param.sfo`. The Python reader returns an `int` for format 0x404 and
/// a string (trailing NULs stripped, invalid UTF-8 replaced) for every other format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SfoValue {
    Int(u32),
    Text(String),
}

impl SfoValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            SfoValue::Text(t) => Some(t),
            SfoValue::Int(_) => None,
        }
    }

    pub fn as_int(&self) -> Option<u32> {
        match self {
            SfoValue::Int(n) => Some(*n),
            SfoValue::Text(_) => None,
        }
    }
}

/// Parse a `param.sfo` (magic `\0PSF`, header `<4sIIII` of magic, version, key table offset,
/// value table offset, entry count; 16-byte entries from offset 20). A bad signature is
/// `Error::BadDump("bad SFO signature")`; an offset past the end of `data` is
/// `Error::BadDump("param.sfo is cut short")`. Every access is bounds-checked.
pub fn parse(data: &[u8]) -> Result<BTreeMap<String, SfoValue>> {
    not_built("sfo::parse")
}

/// The text value of `key`, if it is there and is text.
pub fn text<'a>(values: &'a BTreeMap<String, SfoValue>, key: &str) -> Option<&'a str> {
    values.get(key).and_then(SfoValue::as_text)
}
