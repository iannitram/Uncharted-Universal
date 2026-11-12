//! Steam's binary KeyValues, the format of `userdata/<id>/config/shortcuts.vdf` (SPEC.md M13).
//! Replaces the `parse` / `dump` half of `launcher/bbport_steam.py`. Copied from
//! iannitram/totk-native's `vdf.rs` (MIT), its depth bound included.
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

use crate::error::{Error, Result};

/// Deepest map nesting `read` accepts. Steam's files nest about three deep; the bound
/// keeps untrusted input from overflowing the stack.
const MAX_DEPTH: usize = 64;

/// A value; maps keep their items in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Map(Vec<(String, Value)>),
    Str(String),
    Int(u32),
}

/// Encode a document: each item as above, then 0x08.
pub fn write(doc: &[(String, Value)]) -> Vec<u8> {
    let mut out = Vec::new();
    write_items(&mut out, doc);
    out
}

/// Append the items, then the closing 0x08 of their list.
fn write_items(out: &mut Vec<u8>, items: &[(String, Value)]) {
    for (key, value) in items {
        let type_byte = match value {
            Value::Map(_) => 0x00,
            Value::Str(_) => 0x01,
            Value::Int(_) => 0x02,
        };
        out.push(type_byte);
        out.extend_from_slice(key.as_bytes());
        out.push(0x00);
        match value {
            Value::Map(inner) => write_items(out, inner),
            Value::Str(text) => {
                out.extend_from_slice(text.as_bytes());
                out.push(0x00);
            }
            Value::Int(n) => out.extend_from_slice(&n.to_le_bytes()),
        }
    }
    out.push(0x08);
}

/// Decode a document written as above. Errors, all `Error::Steam`:
/// - the bytes end before the document's closing 0x08, inside a key, a string or a number:
///   "shortcuts.vdf is cut short";
/// - a type byte other than 0x00, 0x01, 0x02 where an item starts:
///   "shortcuts.vdf has an unknown item type 0x<hex, two digits>";
/// - a key or string that is not UTF-8: "shortcuts.vdf has text that is not UTF-8";
/// - bytes after the closing 0x08: "shortcuts.vdf has bytes after its end";
/// - maps nested more than 64 deep: "shortcuts.vdf is nested too deep".
pub fn read(bytes: &[u8]) -> Result<Vec<(String, Value)>> {
    fn cut_short() -> Error {
        Error::Steam("shortcuts.vdf is cut short".to_string())
    }

    fn text(bytes: &[u8]) -> Result<String> {
        String::from_utf8(bytes.to_vec())
            .map_err(|_| Error::Steam("shortcuts.vdf has text that is not UTF-8".to_string()))
    }

    fn read_items(bytes: &[u8], pos: &mut usize, depth: usize) -> Result<Vec<(String, Value)>> {
        if depth > MAX_DEPTH {
            return Err(Error::Steam("shortcuts.vdf is nested too deep".to_string()));
        }
        let mut items = Vec::new();
        loop {
            let t = *bytes.get(*pos).ok_or_else(cut_short)?;
            *pos += 1;
            if t == 0x08 {
                return Ok(items);
            }
            if !matches!(t, 0x00..=0x02) {
                return Err(Error::Steam(format!(
                    "shortcuts.vdf has an unknown item type 0x{t:02x}"
                )));
            }

            let key_start = *pos;
            let key_end = bytes[key_start..]
                .iter()
                .position(|&b| b == 0)
                .map(|n| key_start + n)
                .ok_or_else(cut_short)?;
            let key = text(&bytes[key_start..key_end])?;
            *pos = key_end + 1;

            let value = match t {
                0x00 => Value::Map(read_items(bytes, pos, depth + 1)?),
                0x01 => {
                    let start = *pos;
                    let end = bytes[start..]
                        .iter()
                        .position(|&b| b == 0)
                        .map(|n| start + n)
                        .ok_or_else(cut_short)?;
                    let value = text(&bytes[start..end])?;
                    *pos = end + 1;
                    Value::Str(value)
                }
                0x02 => {
                    let end = pos.checked_add(4).ok_or_else(cut_short)?;
                    let n = bytes.get(*pos..end).ok_or_else(cut_short)?;
                    *pos = end;
                    Value::Int(u32::from_le_bytes([n[0], n[1], n[2], n[3]]))
                }
                _ => unreachable!(),
            };
            items.push((key, value));
        }
    }

    let mut pos = 0;
    let doc = read_items(bytes, &mut pos, 0)?;
    if pos != bytes.len() {
        return Err(Error::Steam(
            "shortcuts.vdf has bytes after its end".to_string(),
        ));
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steam(message: &str) -> Error {
        Error::Steam(message.to_string())
    }

    fn sample() -> Vec<(String, Value)> {
        vec![(
            "shortcuts".to_string(),
            Value::Map(vec![(
                "0".to_string(),
                Value::Map(vec![
                    ("appid".to_string(), Value::Int(0xB460_2143)),
                    (
                        "AppName".to_string(),
                        Value::Str("Uncharted Universal".to_string()),
                    ),
                    ("tags".to_string(), Value::Map(vec![])),
                ]),
            )]),
        )]
    }

    #[test]
    fn writes_the_bytes_the_python_dump_writes() {
        // dump([(MAP, 'shortcuts', [(MAP, '0', [(INT32, 'appid', ...), (STRING, 'AppName', ..),
        // (MAP, 'tags', [])])])]) + bytes([END]) in launcher/bbport_steam.py.
        let mut want = b"\x00shortcuts\0\x000\0\x02appid\0".to_vec();
        want.extend(0xB460_2143u32.to_le_bytes());
        want.extend(b"\x01AppName\0Uncharted Universal\0\x00tags\0\x08\x08\x08\x08");
        assert_eq!(write(&sample()), want);
    }

    #[test]
    fn an_empty_document_is_one_byte() {
        assert_eq!(write(&[]), vec![0x08]);
        assert_eq!(read(&[0x08]).unwrap(), vec![]);
    }

    #[test]
    fn round_trips() {
        let doc = sample();
        assert_eq!(read(&write(&doc)).unwrap(), doc);
        // Text is UTF-8 both ways; numbers keep all 32 bits.
        let doc = vec![
            ("Ünïcode ✓".to_string(), Value::Str("日本語".to_string())),
            ("max".to_string(), Value::Int(u32::MAX)),
            ("empty".to_string(), Value::Str(String::new())),
        ];
        assert_eq!(read(&write(&doc)).unwrap(), doc);
    }

    #[test]
    fn reads_the_python_written_file() {
        // tests/data/shortcuts.vdf: two shortcuts written by bbport_steam.dump (one with a tag and
        // a play time, as Steam leaves them), regenerated only by hand.
        let bytes = include_bytes!("../tests/data/shortcuts.vdf");
        let doc = read(bytes).unwrap();
        assert_eq!(write(&doc), bytes.to_vec());
        let [(key, Value::Map(entries))] = doc.as_slice() else {
            panic!("one shortcuts map: {doc:?}");
        };
        assert_eq!(key, "shortcuts");
        let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["0", "1"]);
        let Value::Map(first) = &entries[0].1 else {
            panic!("entry 0 is a map");
        };
        let field = |name: &str| first.iter().find(|(k, _)| k == name).map(|(_, v)| v);
        assert_eq!(field("appid"), Some(&Value::Int(0xB460_2143)));
        assert_eq!(
            field("Exe"),
            Some(&Value::Str(
                "\"/home/deck/.local/bin/uncharted-universal\"".to_string()
            ))
        );
        assert_eq!(field("LastPlayTime"), Some(&Value::Int(1_731_400_000)));
        assert_eq!(
            field("tags"),
            Some(&Value::Map(vec![(
                "0".to_string(),
                Value::Str("favorite".to_string())
            )]))
        );
    }

    #[test]
    fn cut_short_anywhere_is_refused() {
        let bytes = write(&sample());
        for len in 0..bytes.len() {
            assert_eq!(
                read(&bytes[..len]),
                Err(steam("shortcuts.vdf is cut short")),
                "cut at {len}"
            );
        }
    }

    #[test]
    fn unknown_types_are_refused() {
        // Type 7 (uint64) is known to the Python but never written in shortcuts.vdf.
        assert_eq!(
            read(b"\x07big\0\0\0\0\0\0\0\0\0\x08"),
            Err(steam("shortcuts.vdf has an unknown item type 0x07"))
        );
        assert_eq!(
            read(b"\x00m\0\x1f"),
            Err(steam("shortcuts.vdf has an unknown item type 0x1f"))
        );
    }

    #[test]
    fn text_that_is_not_utf8_is_refused() {
        assert_eq!(
            read(b"\x01k\0\xff\xfe\0\x08"),
            Err(steam("shortcuts.vdf has text that is not UTF-8"))
        );
        assert_eq!(
            read(b"\x02\xc3\0\0\0\0\0\x08"),
            Err(steam("shortcuts.vdf has text that is not UTF-8"))
        );
    }

    #[test]
    fn bytes_after_the_end_are_refused() {
        let mut bytes = write(&sample());
        bytes.push(0x08);
        assert_eq!(
            read(&bytes),
            Err(steam("shortcuts.vdf has bytes after its end"))
        );
    }

    #[test]
    fn deep_nesting_is_refused() {
        let mut bytes = Vec::new();
        for _ in 0..=MAX_DEPTH {
            bytes.extend(b"\x00m\0");
        }
        bytes.extend(std::iter::repeat_n(0x08, MAX_DEPTH + 2));
        assert_eq!(read(&bytes), Err(steam("shortcuts.vdf is nested too deep")));

        let mut ok = Vec::new();
        for _ in 0..MAX_DEPTH {
            ok.extend(b"\x00m\0");
        }
        ok.extend(std::iter::repeat_n(0x08, MAX_DEPTH + 1));
        assert!(read(&ok).is_ok());
    }
}
