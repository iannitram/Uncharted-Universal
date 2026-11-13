//! libSceSaveData on host directories, as the deleted `src/runtime_savedata.c` kept it:
//!
//! - `<user>/savedata/<user id>/<title id>/<dir name>/`: the files the game writes;
//! - `<user>/savedata/<user id>/<title id>/<dir name>.sce_sys/`: `param.bin`, `icon0.png`;
//! - `<user>/savedata/<user id>/<title id>.memory/memory.dat`: SaveDataMemory.
//!
//! A mounted directory appears to the guest as `/savedata0` .. `/savedata15`. Metadata lives
//! beside the directory so the guest's own files are untouched. `BB_SAVEDATA_DIRS`
//! (`SAVEDATA01=Uncharted 1;..`, written by `crates/core` `saves::mapping`) moves the directories
//! whose names start with a prefix into their own folder of the user directory, the folder of the
//! game's selected save slot, which is the `--saves-dir` save-relay syncs
//! (`saves::relay_saves_dir`, piece 1.17): the directory and its `.sce_sys` sibling both start
//! with the game's prefix, so the manifest's `SAVEDATA0N*/**` carries them, and the temporary
//! files written here end in `.tmp`, which it excludes.
//!
//! Everything here is plain Rust over decoded values; `guest.rs` reads and writes the guest's
//! structures. The file layer (`/savedataN` mounts, the user directory) is still C until piece
//! 3.09, reached through [`Host`].
//!
//! Decisions of piece 3.06 (the review issues stay after 1.0):
//! - 16 mount points, as in C; a seventeenth mount is `ERR_MOUNT_FULL` (#36 is that check).
//! - A read-only mount only marks the slot as not writing (for `wait_idle` and `where.json`);
//!   `SetParam` and `SaveIcon` still write through it (#48).
//! - The C parsing of mount points (`atoi`, #49), listing of non-directories (#50) and the
//!   result of a delete whose removal failed (#51) are kept and pinned by tests. Sort settings
//!   are per search (#52) and the lists grow without unchecked allocations (#53) by construction.
//!   The `LIKE` matcher is iterative with the same results (#57 asks for adversarial benchmarks).
//! - Metadata writes are atomic (temporary file, then rename) and a failure is
//!   `ERR_INTERNAL` with the previous `param.bin` kept (#38).

use super::{commit, SaveParam};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) const ERR_PARAMETER: i32 = 0x809F_0000_u32 as i32;
pub(crate) const ERR_NOT_INITIALIZED: i32 = 0x809F_0001_u32 as i32;
pub(crate) const ERR_NOT_MOUNTED: i32 = 0x809F_0004_u32 as i32;
pub(crate) const ERR_EXISTS: i32 = 0x809F_0007_u32 as i32;
pub(crate) const ERR_NOT_FOUND: i32 = 0x809F_0008_u32 as i32;
pub(crate) const ERR_INTERNAL: i32 = 0x809F_000B_u32 as i32;
pub(crate) const ERR_MOUNT_FULL: i32 = 0x809F_000C_u32 as i32;
pub(crate) const ERR_BAD_MOUNTED: i32 = 0x809F_000D_u32 as i32;
pub(crate) const ERR_INVALID_USER: i32 = 0x809F_0011_u32 as i32;
pub(crate) const ERR_MEMORY_NOT_READY: i32 = 0x809F_0012_u32 as i32;

pub(crate) const MODE_RDONLY: u32 = 1;
pub(crate) const MODE_CREATE: u32 = 4;
pub(crate) const MODE_CREATE2: u32 = 32;

/// Mount points `/savedata0` .. `/savedata15`.
pub(crate) const SLOTS: usize = 16;
/// `OrbisSaveDataDirName`: up to 31 characters and a NUL.
pub(crate) const DIR_NAME: usize = 32;
/// `OrbisSaveDataParam`.
pub(crate) const PARAM_SIZE: usize = 1328;

/// `OrbisSaveDataParam`, as the guest and `param.bin` hold it (native byte order).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Param {
    pub title: [u8; 128],
    pub subtitle: [u8; 128],
    pub detail: [u8; 1024],
    pub user_param: u32,
    pub pad: i32,
    pub mtime: i64,
    pub reserved: [u8; 32],
}
const _: () = assert!(std::mem::size_of::<Param>() == PARAM_SIZE);

impl Param {
    pub(crate) const ZERO: Param = Param {
        title: [0; 128],
        subtitle: [0; 128],
        detail: [0; 1024],
        user_param: 0,
        pad: 0,
        mtime: 0,
        reserved: [0; 32],
    };

    pub(crate) fn to_bytes(self) -> [u8; PARAM_SIZE] {
        let mut out = [0u8; PARAM_SIZE];
        out[..128].copy_from_slice(&self.title);
        out[128..256].copy_from_slice(&self.subtitle);
        out[256..1280].copy_from_slice(&self.detail);
        out[1280..1284].copy_from_slice(&self.user_param.to_ne_bytes());
        out[1284..1288].copy_from_slice(&self.pad.to_ne_bytes());
        out[1288..1296].copy_from_slice(&self.mtime.to_ne_bytes());
        out[1296..].copy_from_slice(&self.reserved);
        out
    }

    pub(crate) fn from_bytes(bytes: &[u8; PARAM_SIZE]) -> Param {
        let mut p = Param::ZERO;
        p.title.copy_from_slice(&bytes[..128]);
        p.subtitle.copy_from_slice(&bytes[128..256]);
        p.detail.copy_from_slice(&bytes[256..1280]);
        p.user_param = u32::from_ne_bytes(bytes[1280..1284].try_into().unwrap());
        p.pad = i32::from_ne_bytes(bytes[1284..1288].try_into().unwrap());
        p.mtime = i64::from_ne_bytes(bytes[1288..1296].try_into().unwrap());
        p.reserved.copy_from_slice(&bytes[1296..]);
        p
    }
}

/// `OrbisSaveDataMountResult`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct MountResult {
    pub point: [u8; 16],
    pub required_blocks: u64,
    pub unused: u32,
    pub status: u32,
    pub reserved: [u8; 28],
    pub pad: i32,
}
const _: () = assert!(std::mem::size_of::<MountResult>() == 64);

impl MountResult {
    pub(crate) const ZERO: MountResult = MountResult {
        point: [0; 16],
        required_blocks: 0,
        unused: 0,
        status: 0,
        reserved: [0; 28],
        pad: 0,
    };
}

/// One `SetParam` request: `type` 0 (all) to 4, already read from the guest's buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ParamValue {
    All(Box<Param>),
    /// Types 1 to 3 (title, subtitle, detail): the text up to its NUL or the given size.
    Text(u32, Vec<u8>),
    UserParam(u32),
    /// An unknown type, or a buffer smaller than the type needs.
    Bad,
}

/// One save directory a search found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub name: String,
    pub param: Param,
}

/// What the store needs from the rest of the runtime: the file layer (C until piece 3.09),
/// the environment and the log.
pub(crate) trait Host {
    /// `runtime_file_user_dir()`.
    fn user_dir(&self) -> String;
    /// `runtime_file_mount(guest, host) == 0`.
    fn mount(&self, guest: &str, host: &str) -> bool;
    /// `runtime_file_unmount(guest)`.
    fn unmount(&self, guest: &str);
    /// `getenv(name)`.
    fn var(&self, name: &str) -> Option<String>;
    /// One line on the loader's stdout.
    fn say(&self, line: &str);
}

#[derive(Clone)]
struct Slot {
    used: bool,
    writable: bool,
    host: String,
    meta: String,
    /// The folder holding the directory and whether `BB_SAVEDATA_DIRS` chose it.
    base: String,
    mapped: bool,
    name: String,
}

const FREE: Slot = Slot {
    used: false,
    writable: false,
    host: String::new(),
    meta: String::new(),
    base: String::new(),
    mapped: false,
    name: String::new(),
};

struct State {
    slots: [Slot; SLOTS],
    memory_path: String,
    memory_size: u64,
}

/// The save store of one process (`savedata::STORE` in the runtime; tests make their own).
pub(crate) struct Store<H> {
    pub(crate) host: H,
    initialized: AtomicBool,
    /// Empty until `configure`: then `UNKNOWN`, as in C.
    title_id: Mutex<String>,
    state: Mutex<State>,
    mounts_done: AtomicUsize,
    memory_writes: AtomicUsize,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Nothing here panics while holding a lock; a poisoned lock still holds whole state.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The bytes of a C string field: up to its first NUL, or all of it.
pub(crate) fn c_text(field: &[u8]) -> &[u8] {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    &field[..end]
}

/// `valid_name`: 1 to `max - 1` characters (a NUL within `max`), no `/` or `\`, no `.` at the
/// start or after another `.`.
pub(crate) fn valid_name(field: &[u8], max: usize) -> Option<String> {
    let field = &field[..field.len().min(max)];
    let name = c_text(field);
    if name.is_empty() || name.len() == max {
        return None;
    }
    let bad = name
        .iter()
        .enumerate()
        .any(|(i, &c)| c == b'/' || c == b'\\' || (c == b'.' && (i == 0 || name[i - 1] == b'.')));
    (!bad).then(|| String::from_utf8_lossy(name).into_owned())
}

/// SQL `LIKE` as the C matched it: `%` any run (also empty), `_` exactly one character, the rest
/// literally. Iterative (the C recursed).
pub(crate) fn like(text: &[u8], pattern: &[u8]) -> bool {
    let (mut t, mut p) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == b'%' {
            star = Some((p, t));
            p += 1;
        } else if p < pattern.len() && (pattern[p] == b'_' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if let Some((sp, st)) = star {
            p = sp + 1;
            t = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == b'%')
}

/// C `atoi`: blanks, a sign, then digits; anything else stops it (no digits is 0).
fn atoi(text: &[u8]) -> i64 {
    let mut i = 0;
    while i < text.len() && matches!(text[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let negative = i < text.len() && text[i] == b'-';
    if i < text.len() && matches!(text[i], b'+' | b'-') {
        i += 1;
    }
    let mut value: i64 = 0;
    while i < text.len() && text[i].is_ascii_digit() {
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(text[i] - b'0'));
        i += 1;
    }
    if negative {
        -value
    } else {
        value
    }
}

/// `where.json`'s game for a save directory: `SAVEDATA01*` is `u1` and so on (as
/// `crates/core` `saves::save_prefix`), with the slot-1 folder of that game.
fn game_of(name: &str) -> Option<(&'static str, &'static str)> {
    match name.get(..10)? {
        "SAVEDATA01" => Some(("u1", "Uncharted 1")),
        "SAVEDATA02" => Some(("u2", "Uncharted 2")),
        "SAVEDATA03" => Some(("u3", "Uncharted 3")),
        _ => None,
    }
}

fn unix_seconds(at: SystemTime) -> i64 {
    match at.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_secs() as i64,
        Err(before) => -(before.duration().as_secs() as i64),
    }
}

/// `make_dirs`: the folder and its parents.
fn make_dirs(path: &str) -> io::Result<()> {
    fs::create_dir_all(path)
}

fn is_dir(path: &str) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_dir())
}

/// `<meta>/param.bin`, with `mtime` the time of writing. A temporary file in the same folder
/// (named `*.tmp`, which save-relay never syncs) replaces it, so a failure keeps the old one.
fn write_param(meta: &str, param: &Param) -> io::Result<()> {
    let mut copy = *param;
    copy.mtime = unix_seconds(SystemTime::now());
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let (temporary, mut file) = loop {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = format!("{meta}/.param.bin.{}.{id}.tmp", std::process::id());
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => break (path, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let written = file
        .write_all(&copy.to_bytes())
        .and_then(|()| file.sync_all());
    drop(file); // Windows: closed before it replaces param.bin.
    let result = written.and_then(|()| fs::rename(&temporary, format!("{meta}/param.bin")));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// `<meta>/param.bin`, zeroed where it is missing or short, with `mtime` the file's own time.
fn read_param(meta: &str) -> Param {
    let path = format!("{meta}/param.bin");
    let Ok(mut file) = File::open(&path) else {
        return Param::ZERO;
    };
    let mut bytes = [0u8; PARAM_SIZE];
    let mut filled = 0;
    while filled < PARAM_SIZE {
        match file.read(&mut bytes[filled..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => filled += n,
        }
    }
    let mut param = Param::from_bytes(&bytes);
    if let Ok(modified) = fs::metadata(&path).and_then(|m| m.modified()) {
        param.mtime = unix_seconds(modified);
    }
    param
}

impl<H: Host> Store<H> {
    pub(crate) const fn new(host: H) -> Self {
        Store {
            host,
            initialized: AtomicBool::new(false),
            title_id: Mutex::new(String::new()),
            state: Mutex::new(State {
                slots: [FREE; SLOTS],
                memory_path: String::new(),
                memory_size: 0,
            }),
            mounts_done: AtomicUsize::new(0),
            memory_writes: AtomicUsize::new(0),
        }
    }

    fn title_id(&self) -> String {
        let title = locked(&self.title_id);
        if title.is_empty() {
            "UNKNOWN".to_owned()
        } else {
            title.clone()
        }
    }

    /// `BB_SAVE_LOG=1`: every mount, unmount and save dialog call is printed (diagnosis).
    pub(crate) fn save_log(&self) -> bool {
        self.host
            .var("BB_SAVE_LOG")
            .is_some_and(|v| v.starts_with('1'))
    }

    /// `runtime_savedata_configure`: the title the saves belong to (`INSTALL_DIR_SAVEDATA` or
    /// `TITLE_ID` of param.sfo, at most 15 characters), then the Bloodborne sound flag.
    pub(crate) fn configure(&self, title: &[u8]) {
        if !title.is_empty() {
            let title = &title[..title.len().min(15)];
            *locked(&self.title_id) = String::from_utf8_lossy(title).into_owned();
        }
        self.bloodborne_sound_hack();
    }

    /// Bloodborne "sound hack" (rainvmaker, from the Diegolix29 shadPS4 fork): the game data
    /// save carries a flag at 0x204E of `userdata0010`; with it clear, parts of the game's audio
    /// never play. It is set before the game reads the save. `BB_SOUND_HACK=0` leaves saves
    /// untouched. Only Bloodborne's title ids.
    fn bloodborne_sound_hack(&self) {
        const IDS: [&str; 8] = [
            "CUSA00207",
            "CUSA00208",
            "CUSA00299",
            "CUSA00900",
            "CUSA01363",
            "CUSA03014",
            "CUSA03023",
            "CUSA03173",
        ];
        if self
            .host
            .var("BB_SOUND_HACK")
            .is_some_and(|v| v.starts_with('0'))
        {
            return;
        }
        let title = self.title_id();
        if !IDS.contains(&title.as_str()) {
            return;
        }
        let users = format!("{}/savedata", self.host.user_dir());
        let Ok(entries) = fs::read_dir(&users) else {
            return;
        };
        for entry in entries.flatten() {
            let user = entry.file_name().to_string_lossy().into_owned();
            if user.starts_with('.') {
                continue;
            }
            let path = format!("{users}/{user}/{title}/SPRJ0005/userdata0010");
            let Ok(mut file) = OpenOptions::new().read(true).write(true).open(&path) else {
                continue;
            };
            let mut old = [0u8; 1];
            let read = file
                .seek(SeekFrom::Start(0x204E))
                .and_then(|_| file.read_exact(&mut old));
            if read.is_ok()
                && old[0] != 1
                && file.seek(SeekFrom::Start(0x204E)).is_ok()
                && file.write_all(&[1]).is_ok()
            {
                self.host.say(&format!(
                    "Runtime: Bloodborne sound flag set in {path} (was {})",
                    old[0]
                ));
            }
        }
    }

    /// `<user>/savedata/<user id>/<title>`, the configured title when `title` is empty.
    fn root(&self, user: i32, title: Option<&str>) -> String {
        let title = match title {
            Some(t) if !t.is_empty() => t.to_owned(),
            _ => self.title_id(),
        };
        format!("{}/savedata/{user}/{title}", self.host.user_dir())
    }

    /// `mapped_folder`: the folder `BB_SAVEDATA_DIRS` gives the directory `name` (by prefix), or
    /// with no name its entry number `index`; `<user>/<folder>`.
    fn mapped_folder(&self, name: Option<&str>, index: usize) -> Option<String> {
        let spec = self.host.var("BB_SAVEDATA_DIRS")?;
        for (i, entry) in spec.split(';').enumerate() {
            let Some((prefix, folder)) = entry.split_once('=') else {
                continue;
            };
            let hit = match name {
                Some(name) => name.as_bytes().starts_with(prefix.as_bytes()),
                None => i == index,
            };
            if hit {
                return Some(format!("{}/{folder}", self.host.user_dir()));
            }
        }
        None
    }

    /// The folder that holds directory `name`, and whether `BB_SAVEDATA_DIRS` chose it.
    fn dir_root(&self, user: i32, title: Option<&str>, name: &str) -> (String, bool) {
        match self.mapped_folder(Some(name), 0) {
            Some(folder) => (folder, true),
            None => (self.root(user, title), false),
        }
    }

    pub(crate) fn initialize(&self) -> i32 {
        self.initialized.store(true, Ordering::Release);
        0
    }

    pub(crate) fn terminate(&self) -> i32 {
        if !self.initialized.swap(false, Ordering::AcqRel) {
            return ERR_NOT_INITIALIZED;
        }
        0
    }

    pub(crate) fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    /// `sceSaveDataMount` / `Mount2`: `dir` is the guest's 32-byte name (None for a null
    /// pointer), `result` its result (None for null). With `BB_SAVE_LOG=1` every mount is printed
    /// with its arguments and result code.
    pub(crate) fn mount(
        &self,
        user: i32,
        title: Option<&str>,
        dir: Option<&[u8]>,
        mode: u32,
        result: Option<&mut MountResult>,
    ) -> i32 {
        let mut result = result;
        let r = self.mount_do(user, title, dir, mode, result.as_deref_mut());
        if self.save_log() {
            let name = dir.map_or_else(
                || "(null)".to_owned(),
                |d| String::from_utf8_lossy(c_text(&d[..d.len().min(DIR_NAME)])).into_owned(),
            );
            let point = match (&result, r) {
                (Some(result), 0) => String::from_utf8_lossy(c_text(&result.point)).into_owned(),
                _ => String::new(),
            };
            self.host.say(&format!(
                "Runtime: SaveLog mount dir '{name}' mode 0x{mode:x} user {user} -> 0x{:08x}{}{point}",
                r as u32,
                if r != 0 { " FAILED" } else { " point " },
            ));
        }
        r
    }

    fn mount_do(
        &self,
        user: i32,
        title: Option<&str>,
        dir: Option<&[u8]>,
        mode: u32,
        result: Option<&mut MountResult>,
    ) -> i32 {
        if !self.is_initialized() {
            return ERR_NOT_INITIALIZED;
        }
        if user < 0 {
            return ERR_INVALID_USER;
        }
        let (Some(dir), Some(result)) = (dir, result) else {
            return ERR_PARAMETER;
        };
        let Some(name) = valid_name(dir, DIR_NAME) else {
            return ERR_PARAMETER;
        };
        let (base, mapped) = self.dir_root(user, title, &name);
        let host = format!("{base}/{name}");
        let meta = format!("{base}/{name}.sce_sys");
        let exists = is_dir(&host);
        if mode & MODE_CREATE != 0 && exists {
            return ERR_EXISTS;
        }
        if mode & (MODE_CREATE | MODE_CREATE2) == 0 && !exists {
            return ERR_NOT_FOUND;
        }
        let mut state = locked(&self.state);
        if state.slots.iter().any(|s| s.used && s.host == host) {
            return ERR_BAD_MOUNTED;
        }
        let Some(slot) = state.slots.iter().position(|s| !s.used) else {
            return ERR_MOUNT_FULL;
        };
        let point = format!("/savedata{}", slot & 15);
        // The mount point must exist in the file layer before the game is told it is mounted.
        if !self.host.mount(&point, &host) {
            return ERR_MOUNT_FULL;
        }
        if !exists {
            let made = make_dirs(&host).and_then(|()| make_dirs(&meta));
            // A new directory starts with an empty param.bin; without it the mount fails (#38).
            if made
                .and_then(|()| write_param(&meta, &Param::ZERO))
                .is_err()
            {
                self.host.unmount(&point);
                let _ = fs::remove_dir(&meta);
                let _ = fs::remove_dir(&host);
                return ERR_INTERNAL;
            }
        }
        state.slots[slot] = Slot {
            used: true,
            writable: mode & MODE_RDONLY == 0,
            host,
            meta,
            base,
            mapped,
            name: name.clone(),
        };
        *result = MountResult::ZERO;
        result.point[..point.len()].copy_from_slice(point.as_bytes());
        result.status = u32::from(!exists); // CREATED
        self.mounts_done.fetch_add(1, Ordering::Relaxed);
        drop(state);
        self.host.say(&format!(
            "Runtime: save data '{name}' mounted at {point} ({}{})",
            if exists { "existing" } else { "created" },
            if mode & MODE_RDONLY != 0 {
                ", read-only"
            } else {
                ""
            },
        ));
        0
    }

    /// The slot of a mount point (`/savedata` and C `atoi` of the rest), if it is in use.
    fn slot_of(state: &State, point: Option<&[u8]>) -> Option<usize> {
        let rest = c_text(point?).strip_prefix(b"/savedata")?;
        let slot = usize::try_from(atoi(rest)).ok()?;
        (slot < SLOTS && state.slots[slot].used).then_some(slot)
    }

    /// `sceSaveDataUmount`: `point` is the guest's 16-byte mount point. Unmounting a directory
    /// that was mounted for writing commits the save: its title, subtitle and detail go to
    /// `where.json` (#168).
    pub(crate) fn umount(&self, point: Option<&[u8]>) -> i32 {
        if !self.is_initialized() {
            return ERR_NOT_INITIALIZED;
        }
        let mut state = locked(&self.state);
        let slot = Self::slot_of(&state, point);
        let committed = slot.map(|slot| {
            let text = c_text(point.unwrap_or_default());
            self.host.unmount(&String::from_utf8_lossy(text));
            std::mem::replace(&mut state.slots[slot], FREE)
        });
        drop(state);
        let r = if slot.is_some() { 0 } else { ERR_NOT_FOUND };
        if self.save_log() {
            let shown = point.map_or_else(
                || "(null)".to_owned(),
                |p| String::from_utf8_lossy(c_text(&p[..p.len().min(16)])).into_owned(),
            );
            self.host.say(&format!(
                "Runtime: SaveLog umount '{shown}' slot {} -> 0x{:08x}",
                slot.map_or(-1, |s| s as i64),
                r as u32
            ));
        }
        if let Some(saved) = committed.filter(|s| s.writable) {
            self.publish(&saved);
        }
        r
    }

    /// `where.json` for a committed save of Uncharted 1, 2 or 3: in the folder
    /// `BB_SAVEDATA_DIRS` gives the game (its selected slot), else `<user>/Uncharted N`. A
    /// failure is printed; the guest's save itself is already on disk.
    fn publish(&self, saved: &Slot) {
        let Some((game, folder)) = game_of(&saved.name) else {
            return;
        };
        let folder = if saved.mapped {
            saved.base.clone()
        } else {
            format!("{}/{folder}", self.host.user_dir())
        };
        let param = read_param(&saved.meta);
        let text = |field: &[u8]| String::from_utf8_lossy(c_text(field)).into_owned();
        let (title, subtitle, detail) = (
            text(&param.title),
            text(&param.subtitle),
            text(&param.detail),
        );
        let param = SaveParam {
            title: &title,
            subtitle: &subtitle,
            detail: &detail,
        };
        if let Err(e) = commit(Path::new(&folder), game, &param) {
            self.host
                .say(&format!("Runtime: where.json not written in {folder}: {e}"));
        }
    }

    /// `sceSaveDataSetParam`: `value` is None for a null buffer; it reads the guest's buffer
    /// and is called only for a mounted point, as the C read it.
    pub(crate) fn set_param(
        &self,
        point: Option<&[u8]>,
        value: Option<impl FnOnce() -> ParamValue>,
    ) -> i32 {
        if !self.is_initialized() {
            return ERR_NOT_INITIALIZED;
        }
        let Some(value) = value else {
            return ERR_PARAMETER;
        };
        let state = locked(&self.state);
        let Some(slot) = Self::slot_of(&state, point) else {
            return ERR_NOT_MOUNTED;
        };
        let meta = &state.slots[slot].meta;
        let mut p = read_param(meta);
        match value() {
            ParamValue::All(all) => p = *all,
            ParamValue::Text(kind, text) => {
                let field: &mut [u8] = match kind {
                    1 => &mut p.title,
                    2 => &mut p.subtitle,
                    3 => &mut p.detail,
                    _ => return ERR_PARAMETER,
                };
                // snprintf: the text up to the field's last byte and a NUL; the rest stays.
                let text = c_text(&text);
                let n = text.len().min(field.len() - 1);
                field[..n].copy_from_slice(&text[..n]);
                field[n] = 0;
            }
            ParamValue::UserParam(value) => p.user_param = value,
            ParamValue::Bad => return ERR_PARAMETER,
        }
        match write_param(meta, &p) {
            Ok(()) => 0,
            Err(_) => ERR_INTERNAL,
        }
    }

    /// `sceSaveDataSaveIcon`: `icon` is None for a null icon or buffer; it gives the guest's
    /// PNG bytes and is called only for a mounted point.
    pub(crate) fn save_icon<'a>(
        &self,
        point: Option<&[u8]>,
        icon: Option<impl FnOnce() -> &'a [u8]>,
    ) -> i32 {
        if !self.is_initialized() {
            return ERR_NOT_INITIALIZED;
        }
        let Some(icon) = icon else {
            return ERR_PARAMETER;
        };
        let state = locked(&self.state);
        let Some(slot) = Self::slot_of(&state, point) else {
            return ERR_NOT_MOUNTED;
        };
        match fs::write(format!("{}/icon0.png", state.slots[slot].meta), icon()) {
            Ok(()) => 0,
            Err(_) => ERR_INTERNAL,
        }
    }

    /// `sceSaveDataDelete`: the directory and its metadata. Removal errors are not reported
    /// (as in C; #51).
    pub(crate) fn delete(&self, user: i32, title: Option<&str>, dir: Option<&[u8]>) -> i32 {
        if !self.is_initialized() {
            return ERR_NOT_INITIALIZED;
        }
        let Some(name) = dir.and_then(|d| valid_name(d, DIR_NAME)) else {
            return ERR_PARAMETER;
        };
        let (base, _) = self.dir_root(user, title, &name);
        let host = format!("{base}/{name}");
        if fs::metadata(&host).is_err() {
            return ERR_NOT_FOUND;
        }
        let _ = fs::remove_dir_all(&host);
        let _ = fs::remove_dir_all(format!("{base}/{name}.sce_sys"));
        self.host
            .say(&format!("Runtime: save data '{name}' deleted"));
        0
    }

    /// `runtime_savedata_pick`: an existing directory of a save list (directories that do not
    /// exist are not listed, as on the console). `which`: 0 first, 1 last, 2 latest written,
    /// 3 oldest, 4 the one named `focus`. The index in `names` and its parameters, or None.
    pub(crate) fn pick(
        &self,
        user: i32,
        title: Option<&str>,
        names: &[[u8; DIR_NAME]],
        which: i32,
        focus: Option<&[u8]>,
    ) -> Option<(usize, Param)> {
        let mut best: Option<(usize, Param)> = None;
        for (i, raw) in names.iter().enumerate() {
            let Some(name) = valid_name(raw, DIR_NAME + 1) else {
                continue;
            };
            let (base, _) = self.dir_root(user, title, &name);
            if !is_dir(&format!("{base}/{name}")) {
                continue;
            }
            let p = read_param(&format!("{base}/{name}.sce_sys"));
            let better = match which {
                0 => best.is_none(),
                1 => true,
                2 => best.as_ref().is_none_or(|(_, b)| p.mtime > b.mtime),
                3 => best.as_ref().is_none_or(|(_, b)| p.mtime < b.mtime),
                _ => focus.is_some_and(|f| c_text(&f[..f.len().min(DIR_NAME)]) == c_text(&raw[..])),
            };
            if better {
                best = Some((i, p));
            }
        }
        best
    }

    /// `sceSaveDataDirNameSearch`: the directories in the title's folder and in each
    /// `BB_SAVEDATA_DIRS` folder (each listed only in the folder it belongs to) whose names
    /// match `pattern` (`LIKE`; None or empty matches all), sorted by `key` (1 user param,
    /// 3 time, else the name), descending when `order` is not 0.
    pub(crate) fn search(
        &self,
        user: i32,
        title: Option<&str>,
        pattern: Option<&[u8]>,
        key: u32,
        order: u32,
    ) -> Result<Vec<Entry>, i32> {
        if !self.is_initialized() {
            return Err(ERR_NOT_INITIALIZED);
        }
        let pattern = pattern.map(c_text).filter(|p| !p.is_empty());
        let mut entries = Vec::new();
        let folders = std::iter::once(self.root(user, title))
            .chain((0..).map_while(|i| self.mapped_folder(None, i)));
        for base in folders {
            let Ok(listing) = fs::read_dir(&base) else {
                continue;
            };
            let mut names: Vec<String> = listing
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort(); // readdir's order is the file system's; the sort below decides
            for name in names {
                let n = name.len();
                if name.starts_with('.') || n >= DIR_NAME || (n > 8 && name.ends_with(".sce_sys")) {
                    continue;
                }
                if pattern.is_some_and(|p| !like(name.as_bytes(), p)) {
                    continue;
                }
                if self.dir_root(user, title, &name).0 != base {
                    continue; // listed only where it belongs (no duplicates)
                }
                let param = read_param(&format!("{base}/{name}.sce_sys"));
                entries.push(Entry { name, param });
            }
        }
        entries.sort_by(|x, y| {
            let c = match key {
                1 => x.param.user_param.cmp(&y.param.user_param),
                3 => x.param.mtime.cmp(&y.param.mtime),
                _ => x.name.as_bytes().cmp(y.name.as_bytes()),
            };
            if order != 0 {
                c.reverse()
            } else {
                c
            }
        });
        Ok(entries)
    }

    /// `sceSaveDataSetupSaveDataMemory`: one fixed-size blob per user and title, at least `size`
    /// bytes; `param` (if any) is written beside it.
    pub(crate) fn memory_setup(&self, user: i32, size: u64, param: Option<&Param>) -> i32 {
        if !self.is_initialized() {
            return ERR_NOT_INITIALIZED;
        }
        if size == 0 {
            return ERR_PARAMETER;
        }
        let dir = format!("{}.memory", self.root(user, None));
        if make_dirs(&dir).is_err() {
            return ERR_INTERNAL;
        }
        let mut state = locked(&self.state);
        state.memory_path = format!("{dir}/memory.dat");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&state.memory_path)
            .or_else(|_| {
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&state.memory_path)
            });
        let ready = file.and_then(|file| {
            if file.metadata()?.len() < size {
                file.set_len(size)?;
            }
            Ok(())
        });
        if ready.is_err() {
            return ERR_INTERNAL;
        }
        state.memory_size = size;
        if let Some(param) = param {
            if write_param(&dir, param).is_err() {
                return ERR_INTERNAL;
            }
        }
        drop(state);
        self.host
            .say(&format!("Runtime: save data memory ready ({size} bytes)"));
        0
    }

    /// `sceSaveDataGetSaveDataMemory` / `Set..` (`write`): `size` bytes at `offset` of the
    /// blob; `io` reads or writes them on the open file (the guest's buffer is touched only
    /// there, after the checks). `present` is false for a null buffer.
    pub(crate) fn memory_io(
        &self,
        present: bool,
        size: u64,
        offset: i64,
        write: bool,
        io: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> i32 {
        if !self.is_initialized() {
            return ERR_NOT_INITIALIZED;
        }
        if !present || offset < 0 {
            return ERR_PARAMETER;
        }
        let state = locked(&self.state);
        if state.memory_size == 0 {
            return ERR_MEMORY_NOT_READY;
        }
        if (offset as u64)
            .checked_add(size)
            .is_none_or(|end| end > state.memory_size)
        {
            return ERR_PARAMETER;
        }
        let done = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&state.memory_path)
            .and_then(|mut file| {
                file.seek(SeekFrom::Start(offset as u64))?;
                io(&mut file)
            });
        if done.is_err() {
            return ERR_INTERNAL;
        }
        if write {
            self.memory_writes.fetch_add(1, Ordering::Relaxed);
        }
        0
    }

    /// `runtime_savedata_wait_idle`: a save is in progress while a directory is mounted for
    /// writing. Waits up to `timeout_ms` for none: 0 nothing was in progress, 1 it finished,
    /// -1 still in progress at the timeout.
    pub(crate) fn wait_idle(&self, timeout_ms: u32) -> i32 {
        let mut waited_ms = 0u32;
        loop {
            let writing = locked(&self.state)
                .slots
                .iter()
                .any(|s| s.used && s.writable);
            if !writing {
                return i32::from(waited_ms != 0);
            }
            if waited_ms >= timeout_ms {
                return -1;
            }
            std::thread::sleep(Duration::from_millis(20));
            waited_ms = waited_ms.saturating_add(20);
        }
    }

    /// `runtime_savedata_report`.
    pub(crate) fn report(&self) {
        self.host.say(&format!(
            "Runtime: save data mounts={}, memory writes={}",
            self.mounts_done.load(Ordering::Relaxed),
            self.memory_writes.load(Ordering::Relaxed)
        ));
    }
}

#[cfg(test)]
pub(crate) mod tests;
