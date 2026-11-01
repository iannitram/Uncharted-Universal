//! Steam's binary KeyValues, the format of `userdata/<id>/config/shortcuts.vdf` (SPEC.md M13).
//! Replaces the `parse` / `dump` half of `launcher/bbport_steam.py`. Copied from
//! iannitram/totk-native's `vdf.rs` (MIT), whose stubs this mirrors.
//!
//! A document is a list of items followed by the byte 0x08. An item is a type byte, a
//! NUL-terminated UTF-8 key, then its value:
//!
//! ```text
//! 0x00 key\0 <items> 0x08     a map (nested list of items, closed by 0x08)
//! 0x01 key\0 text\0           a string
//! 0x02 key\0 u32 LE           a 32-bit number
//! ```
//!
//! shortcuts.vdf is one map item "shortcuts" whose items are "0", "1", ... (one map per
//! shortcut), so the file ends 0x08 0x08. (The Python also knows a type 7, uint64; Steam does
//! not write it in shortcuts.vdf, so a file holding one is refused like any unknown type.)

use crate::error::{not_built, Result};

/// A value; maps keep their items in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Map(Vec<(String, Value)>),
    Str(String),
    Int(u32),
}

/// Encode a document: each item as above, then 0x08.
pub fn write(doc: &[(String, Value)]) -> Vec<u8> {
    not_built!("vdf::write")
}

/// Decode a document written as above. Errors, all `Error::Steam`:
/// - the bytes end before the document's closing 0x08, inside a key, a string or a number:
///   "shortcuts.vdf is cut short";
/// - a type byte other than 0x00, 0x01, 0x02 where an item starts:
///   "shortcuts.vdf has an unknown item type 0x<hex, two digits>";
/// - a key or string that is not UTF-8: "shortcuts.vdf has text that is not UTF-8";
/// - bytes after the closing 0x08: "shortcuts.vdf has bytes after its end".
pub fn read(bytes: &[u8]) -> Result<Vec<(String, Value)>> {
    not_built("vdf::read")
}
