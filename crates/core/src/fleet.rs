//! The fleet view (SPEC.md M12): what each paired device last reported, and the warning before
//! play when this device's build differs from the one that made the latest save. New module;
//! the shape is copied from iannitram/totk-native's `fleet.rs` (MIT). Records are plain data and
//! live in core (no network), unlike `hub`.

use crate::error::{Error, Result};
use crate::platform::Os;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// What one launcher reports to its hubs after each sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRecord {
    /// The device's name as the player set it ("AMD handheld").
    pub device: String,
    pub os: Os,
    /// `bundle::Bundle::build_id` of the installed build.
    pub build_id: String,
    pub patch_level: u32,
    /// Seconds since the Unix epoch of the last save sync; 0 for never.
    pub last_sync_unix: u64,
    pub free_bytes: u64,
}

/// What `drift` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    /// Same build.
    None,
    /// A different build with a different number of our patches.
    Patches { theirs: u32, ours: u32 },
    /// A different build at the same patch level (a different port revision).
    Build { theirs: String, ours: String },
}

/// What every warning offers the player.
const CHOICE: &str = "Update this device first, or play on and keep both saves.";

/// Compares this device with the one that made the latest save. Same `build_id`: `None`.
/// Otherwise a different `patch_level` is `Patches`, else `Build`.
pub fn drift(here: &DeviceRecord, saved_by: &DeviceRecord) -> Drift {
    if here.build_id == saved_by.build_id {
        Drift::None
    } else if here.patch_level != saved_by.patch_level {
        Drift::Patches {
            theirs: saved_by.patch_level,
            ours: here.patch_level,
        }
    } else {
        Drift::Build {
            theirs: saved_by.build_id.clone(),
            ours: here.build_id.clone(),
        }
    }
}

/// The warning the Continue screen shows, or `None` for `Drift::None`. It names
/// `saved_by.device`, says what differs in plain words, and offers "update this device first, or
/// play on and keep both saves".
///
/// `Patches`: "<device> made this save with <theirs> of our fixes; this device has <ours>. ..."
/// `Build`: "<device> made this save with a different build of the game (<theirs>); this device
/// has <ours>. ..." Both end with the same choice.
pub fn warning(drift: &Drift, saved_by: &DeviceRecord) -> Option<String> {
    match drift {
        Drift::None => None,
        Drift::Patches { theirs, ours } => Some(format!(
            "{} made this save with {} of our fixes; this device has {}. {CHOICE}",
            saved_by.device, theirs, ours
        )),
        Drift::Build { theirs, ours } => Some(format!(
            "{} made this save with a different build of the game ({}); this device has {}. \
             {CHOICE}",
            saved_by.device, theirs, ours
        )),
    }
}

/// A record as one tab-separated line, no trailing newline: device, os key, build id, patch
/// level, last sync, free bytes. A tab or newline in `device` is
/// `Error::Hub("a device name cannot hold a tab or a new line")`.
pub fn to_line(r: &DeviceRecord) -> Result<String> {
    if r.device.contains(['\t', '\n']) {
        return Err(Error::Hub(
            "a device name cannot hold a tab or a new line".to_string(),
        ));
    }
    Ok(format!(
        "{}\t{}\t{}\t{}\t{}\t{}",
        r.device,
        r.os.key(),
        r.build_id,
        r.patch_level,
        r.last_sync_unix,
        r.free_bytes
    ))
}

/// Reads `to_line`'s format back; anything else is `Error::Hub("bad device record: <line>")`.
/// Numbers are plain decimal digits only (no sign, no spaces), so a line reads back to exactly
/// one record.
pub fn from_line(line: &str) -> Result<DeviceRecord> {
    let bad = || Error::Hub(format!("bad device record: {line}"));
    let fields: Vec<&str> = line.split('\t').collect();
    let [device, os, build_id, patch_level, last_sync, free] = fields[..] else {
        return Err(bad());
    };
    if device.contains('\n') || build_id.contains('\n') || build_id.is_empty() {
        return Err(bad());
    }
    let os = Os::from_key(os).ok_or_else(bad)?;
    Ok(DeviceRecord {
        device: device.to_string(),
        os,
        build_id: build_id.to_string(),
        patch_level: number(patch_level).ok_or_else(bad)?,
        last_sync_unix: number(last_sync).ok_or_else(bad)?,
        free_bytes: number(free).ok_or_else(bad)?,
    })
}

/// Reads all device records from `path`, ignoring blank lines. Returns an empty vector if `path`
/// does not exist yet.
pub fn read_records(path: &Path) -> Result<Vec<DeviceRecord>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(Error::from(err)),
    };
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(from_line)
        .collect()
}

/// Inserts or replaces `record` in `records` by `device` name, preserving existing order.
pub fn upsert(records: &mut Vec<DeviceRecord>, record: DeviceRecord) {
    if let Some(existing) = records.iter_mut().find(|r| r.device == record.device) {
        *existing = record;
    } else {
        records.push(record);
    }
}

/// Atomically writes `records` to `path` in `to_line` format, creating parent directories if
/// needed.
pub fn write_records(path: &Path, records: &[DeviceRecord]) -> Result<()> {
    let mut out = String::new();
    for record in records {
        out.push_str(&to_line(record)?);
        out.push('\n');
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("txt.tmp");
    fs::write(&tmp, out.as_bytes())?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Reads `path` (if present), upserts `record` by `device` name, atomically writes the result,
/// and returns the updated list.
pub fn upsert_file(path: &Path, record: DeviceRecord) -> Result<Vec<DeviceRecord>> {
    let _ = to_line(&record)?;
    let mut records = read_records(path)?;
    upsert(&mut records, record);
    write_records(path, &records)?;
    Ok(records)
}

/// A field of decimal digits; `None` for anything else, including a sign or an overflow.
fn number<T: std::str::FromStr>(field: &str) -> Option<T> {
    if field.is_empty() || !field.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    field.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(device: &str, build_id: &str, patch_level: u32) -> DeviceRecord {
        DeviceRecord {
            device: device.to_string(),
            os: Os::Linux,
            build_id: build_id.to_string(),
            patch_level,
            last_sync_unix: 1_762_905_600,
            free_bytes: 64_000_000_000,
        }
    }

    fn hub_message(err: Error) -> String {
        match err {
            Error::Hub(m) => m,
            other => panic!("expected a hub error, got {other:?}"),
        }
    }

    #[test]
    fn same_build_id_is_no_drift_whatever_else_differs() {
        let here = record("Desktop", "abc123", 4);
        let mut saved_by = record("AMD handheld", "abc123", 9);
        saved_by.os = Os::Windows;
        assert_eq!(drift(&here, &saved_by), Drift::None);
        assert_eq!(warning(&Drift::None, &saved_by), None);
    }

    #[test]
    fn different_patch_level_is_patches_with_theirs_from_the_saving_device() {
        let here = record("Desktop", "aaa", 3);
        let saved_by = record("AMD handheld", "bbb", 5);
        assert_eq!(
            drift(&here, &saved_by),
            Drift::Patches { theirs: 5, ours: 3 }
        );
    }

    #[test]
    fn same_patch_level_with_another_build_is_build() {
        let here = record("Desktop", "aaa", 5);
        let saved_by = record("AMD handheld", "bbb", 5);
        assert_eq!(
            drift(&here, &saved_by),
            Drift::Build {
                theirs: "bbb".to_string(),
                ours: "aaa".to_string(),
            }
        );
    }

    #[test]
    fn patches_warning_names_the_device_the_counts_and_the_choice() {
        let saved_by = record("AMD handheld", "bbb", 5);
        let text = warning(&Drift::Patches { theirs: 5, ours: 3 }, &saved_by).unwrap();
        assert_eq!(
            text,
            "AMD handheld made this save with 5 of our fixes; this device has 3. \
             Update this device first, or play on and keep both saves."
        );
    }

    #[test]
    fn build_warning_names_the_device_both_builds_and_the_choice() {
        let saved_by = record("Strix Halo tablet", "bbb", 5);
        let drift = Drift::Build {
            theirs: "bbb".to_string(),
            ours: "aaa".to_string(),
        };
        let text = warning(&drift, &saved_by).unwrap();
        assert_eq!(
            text,
            "Strix Halo tablet made this save with a different build of the game (bbb); \
             this device has aaa. Update this device first, or play on and keep both saves."
        );
    }

    #[test]
    fn line_has_six_tab_separated_fields_and_no_trailing_newline() {
        let r = DeviceRecord {
            device: "AMD handheld".to_string(),
            os: Os::Linux,
            build_id: "0f1e2d".to_string(),
            patch_level: 7,
            last_sync_unix: 0,
            free_bytes: 123,
        };
        assert_eq!(
            to_line(&r).unwrap(),
            "AMD handheld\tlinux\t0f1e2d\t7\t0\t123"
        );
    }

    #[test]
    fn every_os_round_trips_through_a_line() {
        for os in Os::ALL {
            let r = DeviceRecord {
                device: "My device, with spaces".to_string(),
                os,
                build_id: "a".repeat(64),
                patch_level: u32::MAX,
                last_sync_unix: u64::MAX,
                free_bytes: 0,
            };
            let line = to_line(&r).unwrap();
            assert!(!line.ends_with('\n'));
            assert_eq!(from_line(&line).unwrap(), r);
        }
    }

    #[test]
    fn a_tab_or_newline_in_the_device_name_is_refused() {
        for name in ["AMD\thandheld", "AMD\nhandheld", "handheld\n"] {
            let err = to_line(&record(name, "abc", 1)).unwrap_err();
            assert_eq!(err.code(), "hub");
            assert_eq!(
                hub_message(err),
                "a device name cannot hold a tab or a new line"
            );
        }
    }

    #[test]
    fn malformed_lines_are_bad_device_records() {
        let lines = [
            "",
            "AMD handheld",
            "AMD handheld\tlinux\tabc\t1\t2",
            "AMD handheld\tlinux\tabc\t1\t2\t3\t4",
            "AMD handheld\tbeos\tabc\t1\t2\t3",
            "AMD handheld\tLinux\tabc\t1\t2\t3",
            "AMD handheld\tlinux\t\t1\t2\t3",
            "AMD handheld\tlinux\tabc\tone\t2\t3",
            "AMD handheld\tlinux\tabc\t-1\t2\t3",
            "AMD handheld\tlinux\tabc\t+1\t2\t3",
            "AMD handheld\tlinux\tabc\t1\t\t3",
            "AMD handheld\tlinux\tabc\t1\t2\t 3",
            "AMD handheld\tlinux\tabc\t4294967296\t2\t3",
            "AMD handheld\tlinux\tabc\t1\t2\t18446744073709551616",
            "AMD handheld\tlinux\tabc\t1\t2\t3\n",
            "AMD\nhandheld\tlinux\tabc\t1\t2\t3",
        ];
        for line in lines {
            let err = from_line(line).unwrap_err();
            assert_eq!(err.code(), "hub", "{line:?}");
            assert_eq!(hub_message(err), format!("bad device record: {line}"));
        }
    }

    #[test]
    fn upsert_replaces_matching_device_and_appends_new_ones() {
        let mut records = vec![record("Desktop", "aaa", 1), record("Handheld", "bbb", 2)];
        upsert(&mut records, record("Desktop", "ccc", 3));
        upsert(&mut records, record("Tablet", "ddd", 4));
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].build_id, "ccc");
        assert_eq!(records[0].patch_level, 3);
        assert_eq!(records[1].device, "Handheld");
        assert_eq!(records[2].device, "Tablet");
    }

    #[test]
    fn upsert_file_creates_reads_and_updates_atomically() {
        let dir = std::env::temp_dir().join(format!("uncharted-fleet-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("fleet.txt");
        assert!(read_records(&path).unwrap().is_empty());

        let first = record("Desktop", "b1", 1);
        let updated = upsert_file(&path, first.clone()).unwrap();
        assert_eq!(updated, vec![first]);

        let second = record("Handheld", "b2", 2);
        let updated_desktop = record("Desktop", "b3", 3);
        upsert_file(&path, second.clone()).unwrap();
        let final_records = upsert_file(&path, updated_desktop.clone()).unwrap();
        assert_eq!(final_records, vec![updated_desktop, second]);
        assert_eq!(read_records(&path).unwrap(), final_records);

        let _ = fs::remove_dir_all(&dir);
    }
}
