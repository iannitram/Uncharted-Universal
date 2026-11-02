//! Read-only parser for `sce_sys/param.sfo`.

use crate::error::{Error, Result};
use std::collections::BTreeMap;

pub const TITLE_ID: &str = "TITLE_ID";
pub const APP_VER: &str = "APP_VER";
pub const VERSION: &str = "VERSION";
pub const TITLE: &str = "TITLE";

#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Parse a bounded PSF header and its key/value entries.
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

pub fn text<'a>(values: &'a BTreeMap<String, SfoValue>, key: &str) -> Option<&'a str> {
    values.get(key).and_then(SfoValue::as_text)
}
