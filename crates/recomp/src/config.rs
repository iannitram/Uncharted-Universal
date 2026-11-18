//! Guest address ranges are configuration, never constants in the shared layer (gate 2).
//! Text format, one `key = value` per line, `#` comments; values decimal or `0x` hex.

use std::fmt;

/// Where a title's guest code and the fixed mapping live. No default: each title supplies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuestConfig {
    pub code_base: u64,
    pub code_len: u64,
    pub mapping_base: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    Missing(&'static str),
    BadLine(usize),
    BadValue(usize),
    Unknown(String),
    Empty,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(k) => write!(f, "config: missing `{k}`"),
            Self::BadLine(n) => write!(f, "config: line {n} is not `key = value`"),
            Self::BadValue(n) => write!(f, "config: line {n} has a bad number"),
            Self::Unknown(k) => write!(f, "config: unknown key `{k}`"),
            Self::Empty => write!(f, "config: code_len must not be zero"),
        }
    }
}

fn number(s: &str) -> Option<u64> {
    match s.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

impl GuestConfig {
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let (mut base, mut len, mut map) = (None, None, None);
        for (i, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (k, v) = line.split_once('=').ok_or(ConfigError::BadLine(i + 1))?;
            let v = number(v.trim()).ok_or(ConfigError::BadValue(i + 1))?;
            match k.trim() {
                "code_base" => base = Some(v),
                "code_len" => len = Some(v),
                "mapping_base" => map = Some(v),
                other => return Err(ConfigError::Unknown(other.to_string())),
            }
        }
        let cfg = Self {
            code_base: base.ok_or(ConfigError::Missing("code_base"))?,
            code_len: len.ok_or(ConfigError::Missing("code_len"))?,
            mapping_base: map.ok_or(ConfigError::Missing("mapping_base"))?,
        };
        if cfg.code_len == 0 {
            return Err(ConfigError::Empty);
        }
        Ok(cfg)
    }

    /// True when `addr` is inside the code range.
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.code_base && addr - self.code_base < self.code_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_decimal() {
        let c = GuestConfig::parse("# t\ncode_base = 0x1000\ncode_len = 64\nmapping_base=0x9000\n");
        assert_eq!(
            c,
            Ok(GuestConfig {
                code_base: 0x1000,
                code_len: 64,
                mapping_base: 0x9000
            })
        );
        let c = c.unwrap_or(GuestConfig {
            code_base: 0,
            code_len: 1,
            mapping_base: 0,
        });
        assert!(c.contains(0x1000) && c.contains(0x103f) && !c.contains(0x1040));
    }

    #[test]
    fn refuses_bad_input() {
        assert_eq!(
            GuestConfig::parse("code_base = 1\ncode_len = 1"),
            Err(ConfigError::Missing("mapping_base"))
        );
        assert_eq!(
            GuestConfig::parse("x = 1"),
            Err(ConfigError::Unknown("x".into()))
        );
        assert_eq!(
            GuestConfig::parse("code_base"),
            Err(ConfigError::BadLine(1))
        );
        assert_eq!(
            GuestConfig::parse("code_base = zz"),
            Err(ConfigError::BadValue(1))
        );
        assert_eq!(
            GuestConfig::parse("code_base=1\ncode_len=0\nmapping_base=2"),
            Err(ConfigError::Empty)
        );
    }
}
