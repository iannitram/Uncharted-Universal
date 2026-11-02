//! Local save-location metadata (#168, SPEC.md M9). Only the three player-visible save
//! strings cross this boundary; no guest buffers, icons, paths or binary metadata do.
//!
//! This starts roadmap 3.06 with the metadata commit. The guest savedata ABI, mounts and
//! file VFS are not ported yet. Their successful save commit must call [`commit`]; this
//! module is not wired into the legacy C runtime.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The only save parameters allowed in `where.json` (not the whole guest Param struct).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SaveParam<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub detail: &'a str,
}

/// Publish the location of a successfully committed save to `<saves>/where.json`.
/// `saves` is the configured per-game saves folder, not the dump or an individual slot.
/// `game` must be `u1`, `u2` or `u3` (the collection title ID cannot distinguish games).
///
/// Writes UTF-8 through a new temporary file in the same folder, closes it, then renames
/// it over the previous object. On failure the previous object remains and the error is
/// returned to the caller. This performs no network operations and logs no save text.
pub fn commit(saves: &Path, game: &str, param: &SaveParam<'_>) -> io::Result<()> {
    commit_at(saves, game, param, SystemTime::now())
}

fn commit_at(saves: &Path, game: &str, param: &SaveParam<'_>, at: SystemTime) -> io::Result<()> {
    if !matches!(game, "u1" | "u2" | "u3") {
        return Err(invalid("unknown save game"));
    }
    let json = serde_json::json!({
        "game": game,
        "title": param.title,
        "subtitle": param.subtitle,
        "detail": param.detail,
        "saved_at": utc_timestamp(at)?,
    });
    let bytes = serde_json::to_vec(&json).map_err(io::Error::other)?;
    fs::create_dir_all(saves)?;

    // create_new never follows or truncates a leftover temporary file. Unique names also
    // allow concurrent commits; readers always see one complete, committed object.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let (temporary, mut file) = loop {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = saves.join(format!(".where.json.{}.{id}.tmp", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => break (path, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let written = file.write_all(&bytes).and_then(|()| file.sync_all());
    drop(file); // Windows requires the temporary file to be closed before replacement.
    let result = written.and_then(|()| fs::rename(&temporary, saves.join("where.json")));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

// UTC to whole seconds, without a locale, process timezone or a C library dependency.
fn utc_timestamp(at: SystemTime) -> io::Result<String> {
    let seconds = at
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("save time precedes Unix epoch"))?
        .as_secs();
    if seconds >= 253_402_300_800 {
        return Err(invalid("save time exceeds year 9999"));
    }
    let mut days = seconds / 86_400;
    let mut year = 1970;
    let leap = |y| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    loop {
        let length = if leap(year) { 366 } else { 365 };
        if days < length {
            break;
        }
        days -= length;
        year += 1;
    }
    let months = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0;
    while days >= months[month] {
        days -= months[month];
        month += 1;
    }
    Ok(format!(
        "{year:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        month + 1,
        days + 1,
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("uu-savedata-{}-{name}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn savedata_commit_reads_back_only_the_five_allowed_fields_and_overwrites() {
        let dir = TempDir::new("commit");
        let saves = dir.0.join("fake saves 演習");
        for game in ["u1", "u2", "u3"] {
            let param = SaveParam {
                title: "Made-up \"chapter\"",
                subtitle: "雪 and a backslash \\",
                detail: "Invented detail\nnext line\t\u{0}",
            };
            let before = utc_timestamp(SystemTime::now()).unwrap();
            commit(&saves, game, &param).unwrap();
            let after = utc_timestamp(SystemTime::now()).unwrap();
            let bytes = fs::read(saves.join("where.json")).unwrap();
            assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]));
            let got: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let saved_at = got["saved_at"].as_str().unwrap();
            assert!(saved_at >= before.as_str() && saved_at <= after.as_str());
            assert_eq!(
                got,
                serde_json::json!({
                    "game": game, "title": param.title, "subtitle": param.subtitle,
                    "detail": param.detail, "saved_at": saved_at,
                })
            );
            let entries: Vec<_> = fs::read_dir(&saves)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert_eq!(entries, [std::ffi::OsString::from("where.json")]);
        }
        let at = UNIX_EPOCH + Duration::from_secs(951_827_696);
        commit_at(&saves, "u2", &SaveParam::default(), at).unwrap();
        let got: serde_json::Value =
            serde_json::from_slice(&fs::read(saves.join("where.json")).unwrap()).unwrap();
        assert_eq!(
            got,
            serde_json::json!({
                "game": "u2", "title": "", "subtitle": "", "detail": "",
                "saved_at": "2000-02-29T12:34:56Z",
            })
        );
    }

    #[test]
    fn savedata_failed_commit_keeps_the_previous_location() {
        let dir = TempDir::new("failure");
        let param = SaveParam::default();
        commit(&dir.0, "u1", &param).unwrap();
        let path = dir.0.join("where.json");
        let previous = fs::read(&path).unwrap();
        assert_eq!(
            commit(&dir.0, "bad game", &param).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(fs::read(&path).unwrap(), previous);

        // Force the final rename to fail, even when tests run with elevated permissions.
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        fs::write(path.join("keep"), "made-up existing data").unwrap();
        assert!(commit(&dir.0, "u1", &param).is_err());
        assert_eq!(
            fs::read_to_string(path.join("keep")).unwrap(),
            "made-up existing data"
        );
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn savedata_utc_handles_leap_years_and_date_boundaries() {
        for (seconds, want) in [
            (0, "1970-01-01T00:00:00Z"),
            (86_399, "1970-01-01T23:59:59Z"),
            (86_400, "1970-01-02T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (4_107_542_400, "2100-03-01T00:00:00Z"),
            (253_402_300_799, "9999-12-31T23:59:59Z"),
        ] {
            assert_eq!(
                utc_timestamp(UNIX_EPOCH + Duration::from_secs(seconds)).unwrap(),
                want
            );
        }
        assert!(utc_timestamp(UNIX_EPOCH - Duration::from_secs(1)).is_err());
        assert!(utc_timestamp(UNIX_EPOCH + Duration::from_secs(253_402_300_800)).is_err());
    }
}
