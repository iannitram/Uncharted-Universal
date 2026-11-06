//! The offline AppContent metadata the game asks for through `sceAppContentAppParamGetInt`:
//! the four user-defined parameters, the SKU, no add-ons. Replaces `scripts/content_profile.py`
//! (the SKU is a probe setting, not a licence check).

use crate::error::{Error, Result};
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
    let title_id = match values.get("TITLE_ID") {
        None => None,
        Some(SfoValue::Text(value)) => Some(value.clone()),
        Some(SfoValue::Int(_)) => {
            return Err(Error::Prepare("invalid title id".into()));
        }
    };

    let mut user_params = [0; 4];
    for (index, value) in user_params.iter_mut().enumerate() {
        let key = format!("USER_DEFINED_PARAM_{}", index + 1);
        if let Some(parameter) = values.get(&key) {
            *value = parameter
                .as_int()
                .ok_or_else(|| Error::Prepare("invalid user-defined parameter".into()))?;
        }
    }

    // Collection titles omit parameter 1. The console supplies the numeric title ID in that
    // case; an explicitly present zero (or any other value) takes precedence.
    if !values.contains_key("USER_DEFINED_PARAM_1") {
        let digits: String = title_id
            .as_deref()
            .unwrap_or_default()
            .chars()
            .filter(char::is_ascii_digit)
            .collect();
        user_params[0] = if digits.is_empty() {
            0
        } else {
            digits
                .parse::<u32>()
                .map_err(|_| Error::Prepare("invalid user-defined parameter".into()))?
        };
    }

    Ok(ContentProfile {
        title_id,
        sku,
        user_params,
    })
}

impl ContentProfile {
    /// The 28 bytes of `content.bin`.
    pub fn to_bin(&self) -> [u8; 28] {
        let mut bytes = [0; 28];
        bytes[..8].copy_from_slice(&MAGIC);
        bytes[8..12].copy_from_slice(&self.sku.code().to_le_bytes());
        for (index, parameter) in self.user_params.iter().enumerate() {
            let start = 12 + index * 4;
            bytes[start..start + 4].copy_from_slice(&parameter.to_le_bytes());
        }
        bytes
    }

    /// `content-profile.json` as the runtime reads it: `title_id`, `sku` ("full"/"trial"),
    /// `sku_source` ("explicit probe setting"), `user_params`, `mounted_addons` ([]),
    /// `boot_attr` (0), indented two spaces, ending in a newline.
    pub fn to_json(&self) -> String {
        // Serialize the optional title independently so serde_json applies its full string
        // escaping while keeping the field order identical to the legacy contract.
        let title_id =
            serde_json::to_string(&self.title_id).expect("serializing a string is infallible");
        let sku = match self.sku {
            Sku::Full => "full",
            Sku::Trial => "trial",
        };
        format!(
            "{{\n  \"title_id\": {title_id},\n  \"sku\": \"{sku}\",\n  \"sku_source\": \"explicit probe setting\",\n  \"user_params\": [\n    {},\n    {},\n    {},\n    {}\n  ],\n  \"mounted_addons\": [],\n  \"boot_attr\": 0\n}}\n",
            self.user_params[0],
            self.user_params[1],
            self.user_params[2],
            self.user_params[3],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(entries: &[(&str, SfoValue)]) -> BTreeMap<String, SfoValue> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn full_and_trial_profiles_have_exact_binary_layout() {
        let input = values(&[
            ("TITLE_ID", SfoValue::Text("CUSA02320".into())),
            ("USER_DEFINED_PARAM_1", SfoValue::Int(1)),
            ("USER_DEFINED_PARAM_2", SfoValue::Int(0x1234_5678)),
            ("USER_DEFINED_PARAM_3", SfoValue::Int(u32::MAX)),
            ("USER_DEFINED_PARAM_4", SfoValue::Int(4)),
        ]);
        for (sku, code) in [(Sku::Full, 3), (Sku::Trial, 1)] {
            let profile = from_sfo(&input, sku).unwrap();
            assert_eq!(profile.user_params, [1, 0x1234_5678, u32::MAX, 4]);
            let mut expected = Vec::from(*b"BBCONT01");
            for value in [code, 1, 0x1234_5678, u32::MAX, 4] {
                expected.extend(value.to_le_bytes());
            }
            assert_eq!(profile.to_bin().as_slice(), expected);
            assert_eq!(profile.to_bin().len(), 28);
        }
    }

    #[test]
    fn absent_parameters_default_to_zero_and_title_number_fallback_is_only_for_absent_one() {
        let input = values(&[("TITLE_ID", SfoValue::Text("CUSA02320".into()))]);
        assert_eq!(
            from_sfo(&input, Sku::Full).unwrap().user_params,
            [2320, 0, 0, 0]
        );

        let explicit_zero = values(&[
            ("TITLE_ID", SfoValue::Text("CUSA02320".into())),
            ("USER_DEFINED_PARAM_1", SfoValue::Int(0)),
        ]);
        assert_eq!(
            from_sfo(&explicit_zero, Sku::Full).unwrap().user_params,
            [0, 0, 0, 0]
        );

        let no_digits = values(&[("TITLE_ID", SfoValue::Text("COLLECTION".into()))]);
        assert_eq!(from_sfo(&no_digits, Sku::Full).unwrap().user_params[0], 0);
    }

    #[test]
    fn invalid_parameters_title_id_and_fallback_overflow_are_rejected() {
        let invalid_parameter = values(&[("USER_DEFINED_PARAM_2", SfoValue::Text("1".into()))]);
        assert_eq!(
            from_sfo(&invalid_parameter, Sku::Full).unwrap_err(),
            Error::Prepare("invalid user-defined parameter".into())
        );
        let bad_title = values(&[("TITLE_ID", SfoValue::Int(2320))]);
        assert_eq!(
            from_sfo(&bad_title, Sku::Full).unwrap_err(),
            Error::Prepare("invalid title id".into())
        );
        let overflow = values(&[("TITLE_ID", SfoValue::Text("CUSA4294967296".into()))]);
        assert_eq!(
            from_sfo(&overflow, Sku::Full).unwrap_err(),
            Error::Prepare("invalid user-defined parameter".into())
        );
    }

    #[test]
    fn json_is_pretty_escaped_deterministic_and_round_trips() {
        let input = values(&[
            ("TITLE_ID", SfoValue::Text("CUSA\"\n1".into())),
            ("USER_DEFINED_PARAM_1", SfoValue::Int(9)),
        ]);
        let profile = from_sfo(&input, Sku::Trial).unwrap();
        let json = profile.to_json();
        assert!(json.ends_with('\n'));
        assert!(json.contains("CUSA\\\"\\n1"));
        assert_eq!(profile.to_json(), json);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["title_id"], "CUSA\"\n1");
        assert_eq!(parsed["sku"], "trial");
        assert_eq!(parsed["sku_source"], "explicit probe setting");
        assert_eq!(parsed["user_params"], serde_json::json!([9, 0, 0, 0]));
        assert_eq!(parsed["mounted_addons"], serde_json::json!([]));
        assert_eq!(parsed["boot_attr"], 0);
    }
}
