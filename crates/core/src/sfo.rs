//! Read-only parser for `sce_sys/param.sfo`.

use crate::error::{Error, Result};
use std::collections::BTreeMap;

/// `TITLE_ID` ("CUSA02320").
pub const TITLE_ID: &str = "TITLE_ID";
/// `APP_VER` ("01.02"): the build that runs; what patches and the revision gate key on.
pub const APP_VER: &str = "APP_VER";
/// `VERSION` ("01.00"): the content base version, used only when `APP_VER` is missing.
pub const VERSION: &str = "VERSION";
/// `TITLE`: the name shown on the console.
pub const TITLE: &str = "TITLE";

#[derive(Debug, Clone, PartialEq, Eq)]
/// One value of a `param.sfo`: an int for format 0x404, else text (trailing NULs stripped).
pub enum SfoValue {
    Int(u32),
    Text(String),
}

impl SfoValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(t),
            Self::Int(_) => None,
        }
    }
    pub fn as_int(&self) -> Option<u32> {
        match self {
            Self::Int(n) => Some(*n),
            Self::Text(_) => None,
        }
    }
}

/// Parse a `param.sfo`. A bad signature is `Error::BadDump("bad SFO signature")`; any offset,
/// count or length that reaches past the end of `data` is `Error::BadDump("param.sfo is cut short")`.
/// Every access is bounds-checked.
pub fn parse(data: &[u8]) -> Result<BTreeMap<String, SfoValue>> {
    if data.get(..4) != Some(b"\0PSF") {
        return Err(Error::BadDump("bad SFO signature".into()));
    }
    fn u32le(data: &[u8], at: usize) -> Option<usize> {
        Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?) as usize)
    }
    let key_base = u32le(data, 8).ok_or_else(|| Error::BadDump("param.sfo is cut short".into()))?;
    let value_base =
        u32le(data, 12).ok_or_else(|| Error::BadDump("param.sfo is cut short".into()))?;
    let count = u32le(data, 16).ok_or_else(|| Error::BadDump("param.sfo is cut short".into()))?;
    let table_end = 20usize
        .checked_add(
            count
                .checked_mul(16)
                .ok_or_else(|| Error::BadDump("param.sfo is cut short".into()))?,
        )
        .ok_or_else(|| Error::BadDump("param.sfo is cut short".into()))?;
    if table_end > data.len() {
        return Err(Error::BadDump("param.sfo is cut short".into()));
    }
    let mut values = BTreeMap::new();
    for i in 0..count {
        let entry = 20 + i * 16;
        let key_offset = u16::from_le_bytes(data[entry..entry + 2].try_into().unwrap()) as usize;
        let format = u16::from_le_bytes(data[entry + 2..entry + 4].try_into().unwrap());
        let len = u32le(data, entry + 4)
            .ok_or_else(|| Error::BadDump("param.sfo is cut short".into()))?;
        let max_len = u32le(data, entry + 8)
            .ok_or_else(|| Error::BadDump("param.sfo is cut short".into()))?;
        let value_offset = u32le(data, entry + 12)
            .ok_or_else(|| Error::BadDump("param.sfo is cut short".into()))?;
        let key_at = key_base
            .checked_add(key_offset)
            .ok_or_else(|| Error::BadDump("param.sfo is cut short".into()))?;
        let key_tail = data
            .get(key_at..)
            .ok_or_else(|| Error::BadDump("param.sfo is cut short".into()))?;
        let key_len = key_tail
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| Error::BadDump("param.sfo is cut short".into()))?;
        let key = String::from_utf8_lossy(&key_tail[..key_len]).into_owned();
        let val_at = value_base
            .checked_add(value_offset)
            .ok_or_else(|| Error::BadDump("param.sfo is cut short".into()))?;
        let val = data
            .get(
                val_at
                    ..val_at
                        .checked_add(len)
                        .ok_or_else(|| Error::BadDump("param.sfo is cut short".into()))?,
            )
            .ok_or_else(|| Error::BadDump("param.sfo is cut short".into()))?;
        if format == 0x0404 && len >= 4 {
            values.insert(
                key,
                SfoValue::Int(u32::from_le_bytes(val[..4].try_into().unwrap())),
            );
        } else {
            let end = val.iter().rposition(|b| *b != 0).map_or(0, |n| n + 1);
            values.insert(
                key,
                SfoValue::Text(String::from_utf8_lossy(&val[..end]).into_owned()),
            );
        }
        let _ = max_len;
    }
    Ok(values)
}

/// The text value of `key`, if it is there and is text.
pub fn text<'a>(values: &'a BTreeMap<String, SfoValue>, key: &str) -> Option<&'a str> {
    values.get(key).and_then(SfoValue::as_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(key_base: u32, value_base: u32, count: u32) -> Vec<u8> {
        let mut d = b"\0PSF\x01\x01\0\0".to_vec();
        for n in [key_base, value_base, count] {
            d.extend(n.to_le_bytes());
        }
        d
    }

    #[test]
    fn bad_or_short_input_is_an_error_never_a_panic() {
        assert_eq!(
            parse(&[]).unwrap_err(),
            Error::BadDump("bad SFO signature".into())
        );
        let cut = Error::BadDump("param.sfo is cut short".into());
        for n in 4..20 {
            assert_eq!(parse(&header(0, 0, 0)[..n]).unwrap_err(), cut);
        }
        // entry count too big for the buffer, or big enough to overflow count * 16
        for count in [1, 0x0fff_ffff, u32::MAX] {
            assert_eq!(parse(&header(20, 20, count)).unwrap_err(), cut);
        }
        // key table and value table past the end
        let mut d = header(0xffff_ffff, 0xffff_ffff, 1);
        d.extend([
            0xff, 0xff, 4, 4, 4, 0, 0, 0, 4, 0, 0, 0, 0xff, 0xff, 0xff, 0xff,
        ]);
        assert_eq!(parse(&d).unwrap_err(), cut);
        // value runs past the end
        let mut d = header(36, 40, 1);
        d.extend([
            0, 0, 4, 2, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0,
        ]);
        d.extend(b"A\0");
        assert_eq!(parse(&d).unwrap_err(), cut);
    }
}
