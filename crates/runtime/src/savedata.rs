//! libSceSaveData and libSceSaveDataDialog on host directories (piece 3.06; the deleted
//! `src/runtime_savedata.c` and the save dialog of `src/runtime_services.c`), and the local
//! save-location metadata `where.json` (#168, SPEC.md M9).
//!
//! - `store`: the save store, plain Rust over decoded values: mounts `/savedata0` ..
//!   `/savedata15`, `param.bin` and `icon0.png`, delete, the directory search, SaveDataMemory,
//!   the save dialog's choice, `wait_idle` and the report. Its module doc has the layout, the
//!   `BB_SAVEDATA_DIRS` mapping and how it lines up with the save-relay manifest (1.17).
//! - `guest` and `dialog` (x86-64 hosts): the guest's `extern "sysv64"` functions, which read
//!   its structures and call the store; `runtime_resolve` reaches them through
//!   [`ffi::runtime_savedata_resolve`].
//! - [`commit`]: `where.json`. Only the three player-visible save strings cross this boundary;
//!   no guest buffers, icons, paths or binary metadata do. The store calls it when a directory
//!   mounted for writing is unmounted (a save committed).
//!
//! The file layer (`runtime_file.c`) is still C until piece 3.09: in the loader build
//! (`cfg(uncharted_loader)`) the store mounts through `runtime_file_mount` and reads
//! `runtime_file_user_dir`; elsewhere (tests, mobile builds) the user directory is `user` and a
//! mount always succeeds.

/// cbindgen:ignore
#[cfg(target_arch = "x86_64")]
mod dialog;
/// cbindgen:ignore
#[cfg(target_arch = "x86_64")]
mod guest;
/// cbindgen:ignore
mod store;

/// The runtime's side of [`store::Host`].
struct HostSide;

/// The C file layer the store mounts through (loader build only; declared in `src/runtime.h`).
/// cbindgen:ignore
#[cfg(uncharted_loader)]
mod file_layer {
    use std::ffi::{c_char, c_int};

    extern "C" {
        pub fn runtime_file_user_dir() -> *const c_char;
        pub fn runtime_file_mount(guest: *const c_char, host: *const c_char) -> c_int;
        pub fn runtime_file_unmount(guest: *const c_char);
    }
}

/// Without the loader: the user directory `runtime_file.c` starts with (tests set their own).
#[cfg(not(uncharted_loader))]
static USER_DIR: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

#[cfg(all(test, not(uncharted_loader)))]
fn set_user_dir(dir: &str) {
    *USER_DIR.lock().unwrap_or_else(|p| p.into_inner()) = dir.to_owned();
}

#[cfg(uncharted_loader)]
fn c_string(text: &str) -> std::ffi::CString {
    std::ffi::CString::new(text.replace('\0', "")).unwrap_or_default()
}

impl store::Host for HostSide {
    #[cfg(uncharted_loader)]
    fn user_dir(&self) -> String {
        // SAFETY: the file layer's user directory, a NUL-terminated string it keeps.
        let dir = unsafe { file_layer::runtime_file_user_dir() };
        if dir.is_null() {
            return "user".to_owned();
        }
        // SAFETY: as above.
        unsafe { std::ffi::CStr::from_ptr(dir) }
            .to_string_lossy()
            .into_owned()
    }

    #[cfg(not(uncharted_loader))]
    fn user_dir(&self) -> String {
        let dir = USER_DIR.lock().unwrap_or_else(|p| p.into_inner());
        if dir.is_empty() {
            "user".to_owned()
        } else {
            dir.clone()
        }
    }

    #[cfg(uncharted_loader)]
    fn mount(&self, guest: &str, host: &str) -> bool {
        let (guest, host) = (c_string(guest), c_string(host));
        // SAFETY: two NUL-terminated strings; the file layer copies them.
        unsafe { file_layer::runtime_file_mount(guest.as_ptr(), host.as_ptr()) == 0 }
    }

    #[cfg(not(uncharted_loader))]
    fn mount(&self, guest: &str, host: &str) -> bool {
        let _ = (guest, host);
        true
    }

    #[cfg(uncharted_loader)]
    fn unmount(&self, guest: &str) {
        let guest = c_string(guest);
        // SAFETY: a NUL-terminated string.
        unsafe { file_layer::runtime_file_unmount(guest.as_ptr()) }
    }

    #[cfg(not(uncharted_loader))]
    fn unmount(&self, guest: &str) {
        let _ = guest;
    }

    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn say(&self, line: &str) {
        // The loader's stdout is C stdio: the lines stay in order with the C modules' lines.
        #[cfg(target_arch = "x86_64")]
        crate::abi::host::out_line(line);
        #[cfg(not(target_arch = "x86_64"))]
        println!("{line}");
    }
}

/// The process's save store.
static STORE: store::Store<HostSide> = store::Store::new(HostSide);

/// The exported C functions of the save store, declared in `include/uncharted_runtime.h`.
pub mod ffi {
    use super::STORE;
    use std::ffi::{c_char, c_int, c_uint, CStr};

    /// The title the saves belong to (`INSTALL_DIR_SAVEDATA` or `TITLE_ID` of param.sfo; at
    /// most 15 characters; null or empty keeps `UNKNOWN`), then the Bloodborne sound flag of
    /// its saves.
    ///
    /// # Safety
    /// `title` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_savedata_configure(title: *const c_char) {
        let title = if title.is_null() {
            &[][..]
        } else {
            // SAFETY: a NUL-terminated string (caller).
            unsafe { CStr::from_ptr(title) }.to_bytes()
        };
        STORE.configure(title);
    }

    /// The libSceSaveData or libSceSaveDataDialog function for a scoped NID, or 0 (always 0
    /// off x86-64).
    ///
    /// # Safety
    /// `name` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_savedata_resolve(name: *const c_char) -> usize {
        if name.is_null() {
            return 0;
        }
        // SAFETY: a NUL-terminated string (caller).
        let name = unsafe { CStr::from_ptr(name) };
        super::resolve(name).unwrap_or(0)
    }

    /// `Runtime: save data mounts=<n>, memory writes=<n>` on stdout.
    #[no_mangle]
    pub extern "C" fn runtime_savedata_report() {
        STORE.report();
    }

    /// Waits up to `timeout_ms` for a save in progress (a directory mounted for writing):
    /// 0 none, 1 it finished, -1 timed out.
    #[no_mangle]
    pub extern "C" fn runtime_savedata_wait_idle(timeout_ms: c_uint) -> c_int {
        STORE.wait_idle(timeout_ms)
    }
}

/// The guest function for scoped NID `name` (x86-64 hosts only).
pub(crate) fn resolve(name: &std::ffi::CStr) -> Option<usize> {
    #[cfg(target_arch = "x86_64")]
    {
        guest::resolve(name)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = name;
        None
    }
}

/// `runtime_savedata_report` for `runtime_report` without the loader's chain.
pub(crate) fn report() {
    STORE.report();
}

#[cfg(test)]
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Tests that use the process's store hold this lock.
#[cfg(test)]
fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

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
