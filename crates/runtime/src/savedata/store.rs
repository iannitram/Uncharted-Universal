//! The save store behind the guest's libSceSaveData calls (the logic of the deleted
//! `src/runtime_savedata.c`), on host directories:
//!
//! ```text
//! <user>/savedata/<user id>/<title id>/<dir name>/        files the game writes
//! <user>/savedata/<user id>/<title id>/<dir name>.sce_sys/ param.bin, icon0.png
//! <user>/savedata/<user id>/<title id>.memory/memory.dat  SaveDataMemory
//! ```
//!
//! A mounted directory appears to the guest as `/savedata0` .. `/savedata15`. Metadata lives
//! beside the directory so the guest's own files are untouched. Nothing here touches guest
//! pointers: [`super::guest`] reads them and calls these methods.
//!
//! Decisions (roadmap 3.06; the review issues #36, #48-#53 and #57 stay after 1.0):
//! - the mount limit is the console's 16 slots; a 17th mount is `MOUNT_FULL`;
//! - read-only is bookkeeping: a `RDONLY` mount is not a save in progress and writes no
//!   `where.json`, but the host directory is not made read-only (as in the C module).

use super::SaveParam;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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
pub(crate) const SLOTS: usize = 16;

/// `OrbisSaveDataParam`: title[128], subtitle[128], detail[1024], user_param u32, pad,
/// mtime i64, reserved[32].
pub(crate) const PARAM_SIZE: usize = 1328;
const TITLE: (usize, usize) = (0, 128);
const SUBTITLE: (usize, usize) = (128, 128);
const DETAIL: (usize, usize) = (256, 1024);
const USER_PARAM: usize = 1280;
const MTIME: usize = 1288;

/// The guest's `OrbisSaveDataParam`, byte for byte (it is what `param.bin` holds).
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Param(pub [u8; PARAM_SIZE]);

impl Param {
    pub(crate) fn zero() -> Self {
        Self([0; PARAM_SIZE])
    }

    fn text(&self, (at, len): (usize, usize)) -> String {
        let field = &self.0[at..at + len];
        let end = field.iter().position(|&b| b == 0).unwrap_or(len);
        String::from_utf8_lossy(&field[..end]).into_owned()
    }

    /// `snprintf(field, len, "%.*s", size, data)`: at most `size` bytes up to a NUL, cut to
    /// `len - 1`; bytes after the new terminator keep their old value, as in C.
    fn set_text(&mut self, (at, len): (usize, usize), data: &[u8]) {
        let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
        let n = end.min(len - 1);
        self.0[at..at + n].copy_from_slice(&data[..n]);
        self.0[at + n] = 0;
    }

    pub(crate) fn title(&self) -> String {
        self.text(TITLE)
    }
    pub(crate) fn subtitle(&self) -> String {
        self.text(SUBTITLE)
    }
    pub(crate) fn detail(&self) -> String {
        self.text(DETAIL)
    }
    pub(crate) fn user_param(&self) -> u32 {
        let mut word = [0; 4];
        word.copy_from_slice(&self.0[USER_PARAM..USER_PARAM + 4]);
        u32::from_le_bytes(word)
    }
    pub(crate) fn mtime(&self) -> i64 {
        let mut word = [0; 8];
        word.copy_from_slice(&self.0[MTIME..MTIME + 8]);
        i64::from_le_bytes(word)
    }
    fn set_mtime(&mut self, seconds: i64) {
        self.0[MTIME..MTIME + 8].copy_from_slice(&seconds.to_le_bytes());
    }
}

/// A file's modification time in whole seconds (`st_mtime`).
fn file_seconds(meta: &fs::Metadata) -> Option<i64> {
    let at = meta.modified().ok()?;
    Some(match at.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).ok()?,
        Err(before) => -i64::try_from(before.duration().as_secs()).ok()?,
    })
}

/// Writes `param.bin` in `meta` with the modification time now.
fn write_param(meta: &str, param: &Param) -> std::io::Result<()> {
    let mut copy = param.clone();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    copy.set_mtime(now);
    fs::write(Path::new(meta).join("param.bin"), copy.0)
}

/// `param.bin` of `meta`: zeros for a missing file, a short file fills what it has, and the
/// time is the file's own. Callers use whatever there is, as the C reader's did.
pub(crate) fn read_param(meta: &str) -> Param {
    let path = Path::new(meta).join("param.bin");
    let mut param = Param::zero();
    let Ok(mut file) = fs::File::open(&path) else {
        return param;
    };
    let mut filled = 0;
    while filled < PARAM_SIZE {
        match file.read(&mut param.0[filled..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => filled += n,
        }
    }
    if let Some(seconds) = fs::metadata(&path).ok().as_ref().and_then(file_seconds) {
        param.set_mtime(seconds);
    }
    param
}

/// A directory name as the guest gives it (`char[32]`): the text up to its NUL. `None` when it
/// is empty, has no NUL within `max` bytes, or could leave the save folder (`/`, `\`, a leading
/// dot or `..`).
pub(crate) fn valid_name(name: &[u8], max: usize) -> Option<String> {
    let name = &name[..name.len().min(max)];
    let n = name.iter().position(|&b| b == 0)?;
    let name = &name[..n];
    if name.is_empty() {
        return None;
    }
    for (i, &c) in name.iter().enumerate() {
        if c == b'/' || c == b'\\' || (c == b'.' && (i == 0 || name[i - 1] == b'.')) {
            return None;
        }
    }
    Some(String::from_utf8_lossy(name).into_owned())
}

/// SQL `LIKE`: `%` any run, `_` one character (iterative, bounded O(|s| * |p|)).
pub(crate) fn like(s: &[u8], p: &[u8]) -> bool {
    let (mut si, mut pi) = (0usize, 0usize);
    let (mut star_pi, mut star_si) = (None, 0usize);
    while si < s.len() {
        if pi < p.len() && p[pi] == b'%' {
            star_pi = Some(pi);
            star_si = si;
            pi += 1;
        } else if pi < p.len() && (p[pi] == b'_' || p[pi] == s[si]) {
            si += 1;
            pi += 1;
        } else if let Some(sp) = star_pi {
            star_si += 1;
            si = star_si;
            pi = sp + 1;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'%' {
        pi += 1;
    }
    pi == p.len()
}

/// The game a save directory belongs to, by its `SAVEDATA0N` prefix (`u1`, `u2`, `u3`).
fn game_of(name: &str) -> Option<&'static str> {
    match name.as_bytes().get(..10)? {
        b"SAVEDATA01" => Some("u1"),
        b"SAVEDATA02" => Some("u2"),
        b"SAVEDATA03" => Some("u3"),
        _ => None,
    }
}

/// One mounted directory.
#[derive(Clone, Debug)]
struct Slot {
    host: String,
    meta: String,
    base: String,
    name: String,
    writable: bool,
    /// The game set the parameters or the icon, or created the directory: a save.
    dirty: bool,
}

/// A save that was written and is now closed: the `where.json` to publish.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Commit {
    pub(crate) saves: String,
    pub(crate) game: &'static str,
    pub(crate) meta: String,
}

impl Commit {
    /// Publishes the location through [`super::commit`]; the error is the caller's to show.
    pub(crate) fn publish(&self) -> std::io::Result<()> {
        let param = read_param(&self.meta);
        let (title, subtitle, detail) = (param.title(), param.subtitle(), param.detail());
        super::commit(
            Path::new(&self.saves),
            self.game,
            &SaveParam {
                title: &title,
                subtitle: &subtitle,
                detail: &detail,
            },
        )
    }
}

/// The outcome of an unmount.
pub(crate) struct Unmounted {
    pub(crate) slot: Option<usize>,
    pub(crate) code: i32,
    pub(crate) commit: Option<Commit>,
}

/// An entry of a directory-name search.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub(crate) name: [u8; 32],
    pub(crate) param: Param,
}

/// What a search was asked for.
pub(crate) struct Search<'a> {
    pub(crate) user: i32,
    pub(crate) title: Option<&'a str>,
    pub(crate) pattern: &'a [u8],
    pub(crate) key: u32,
    pub(crate) order: u32,
}

/// The mounted slots, the SaveDataMemory and the counters. `user` and `spec`
/// (`BB_SAVEDATA_DIRS`) are set by the owner before each call.
pub(crate) struct Store {
    pub(crate) initialized: bool,
    pub(crate) title_id: String,
    pub(crate) user: String,
    pub(crate) spec: Option<String>,
    /// Mount points in the guest file layer ([`crate::file`]); the tests leave it alone.
    pub(crate) vfs: bool,
    slots: [Option<Slot>; SLOTS],
    memory_path: Option<String>,
    memory_size: u64,
    pub(crate) mounts_done: usize,
    pub(crate) memory_writes: usize,
    pub(crate) where_failures: usize,
}

impl Store {
    pub(crate) const fn new() -> Self {
        Self {
            initialized: false,
            title_id: String::new(),
            user: String::new(),
            spec: None,
            vfs: true,
            slots: [const { None }; SLOTS],
            memory_path: None,
            memory_size: 0,
            mounts_done: 0,
            memory_writes: 0,
            where_failures: 0,
        }
    }

    pub(crate) fn title_id(&self) -> &str {
        if self.title_id.is_empty() {
            "UNKNOWN"
        } else {
            &self.title_id
        }
    }

    /// `<user>/savedata/<user id>/<title id>`: the title is the game's, else the configured one.
    pub(crate) fn root(&self, user: i32, title: Option<&str>) -> String {
        let title = title.filter(|t| !t.is_empty()).unwrap_or(self.title_id());
        format!("{}/savedata/{user}/{title}", self.user)
    }

    /// `BB_SAVEDATA_DIRS="SAVEDATA01=Uncharted 1;SAVEDATA02=Uncharted 2;..."` keeps the save
    /// directories whose names start with a prefix in their own folder of the user directory
    /// (`<user>/Uncharted 1/SAVEDATA01P`), whichever executable mounts them. By `name`, or
    /// (name `None`) the folder of entry `index`. Others stay in [`Self::root`].
    pub(crate) fn mapped_folder(&self, name: Option<&str>, index: usize) -> Option<String> {
        let spec = self.spec.as_deref()?;
        for (i, entry) in spec.split(';').enumerate() {
            let Some((prefix, folder)) = entry.split_once('=') else {
                continue;
            };
            let hit = match name {
                Some(name) => name.as_bytes().starts_with(prefix.as_bytes()),
                None => i == index,
            };
            if hit {
                return Some(format!("{}/{folder}", self.user));
            }
        }
        None
    }

    fn dir_root(&self, user: i32, title: Option<&str>, name: &str) -> String {
        self.mapped_folder(Some(name), 0)
            .unwrap_or_else(|| self.root(user, title))
    }

    /// `(host, meta)` of directory `name`.
    fn paths(&self, user: i32, title: Option<&str>, name: &str) -> (String, String, String) {
        let base = self.dir_root(user, title, name);
        (
            format!("{base}/{name}"),
            format!("{base}/{name}.sce_sys"),
            base,
        )
    }

    pub(crate) fn initialize(&mut self) -> i32 {
        self.initialized = true;
        0
    }

    pub(crate) fn terminate(&mut self) -> i32 {
        if !self.initialized {
            return ERR_NOT_INITIALIZED;
        }
        self.initialized = false;
        0
    }

    /// `sceSaveDataMount`: the mount point and whether the directory was created.
    pub(crate) fn mount(
        &mut self,
        user: i32,
        title: Option<&str>,
        dir: Option<&[u8; 32]>,
        mode: u32,
        has_result: bool,
    ) -> Result<(String, bool), i32> {
        if !self.initialized {
            return Err(ERR_NOT_INITIALIZED);
        }
        if user < 0 {
            return Err(ERR_INVALID_USER);
        }
        let name = dir.and_then(|d| valid_name(d, 32));
        let (true, Some(name)) = (has_result, name) else {
            return Err(ERR_PARAMETER);
        };
        let (host, meta, base) = self.paths(user, title, &name);
        let exists = fs::metadata(&host).is_ok_and(|m| m.is_dir());
        if mode & MODE_CREATE != 0 && exists {
            return Err(ERR_EXISTS);
        }
        if mode & (MODE_CREATE | MODE_CREATE2) == 0 && !exists {
            return Err(ERR_NOT_FOUND);
        }
        let mut free = None;
        for (i, slot) in self.slots.iter().enumerate() {
            match slot {
                Some(s) if s.host == host => return Err(ERR_BAD_MOUNTED),
                None if free.is_none() => free = Some(i),
                _ => {}
            }
        }
        let slot = free.ok_or(ERR_MOUNT_FULL)?;
        let point = format!("/savedata{}", slot & 15);
        // The mount point must exist in the file layer before the game is told it is mounted.
        if self.vfs
            && !crate::file::mount_with_mode(
                point.as_bytes(),
                host.as_bytes(),
                mode & MODE_RDONLY != 0,
            )
        {
            return Err(ERR_MOUNT_FULL);
        }
        if !exists && (fs::create_dir_all(&host).is_err() || fs::create_dir_all(&meta).is_err()) {
            if self.vfs {
                crate::file::unmount(point.as_bytes());
            }
            return Err(ERR_INTERNAL);
        }
        if !exists && write_param(&meta, &Param::zero()).is_err() {
            if self.vfs {
                crate::file::unmount(point.as_bytes());
            }
            return Err(ERR_INTERNAL);
        }
        self.slots[slot] = Some(Slot {
            host,
            meta,
            base,
            name,
            writable: mode & MODE_RDONLY == 0,
            dirty: !exists,
        });
        self.mounts_done += 1;
        Ok((point, !exists))
    }

    fn slot_of(&self, point: &[u8]) -> Option<usize> {
        let point = &point[..point.iter().position(|&b| b == 0).unwrap_or(point.len())];
        let rest = point.strip_prefix(b"/savedata")?;
        if rest.is_empty() || !rest.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let slot = std::str::from_utf8(rest).ok()?.parse::<usize>().ok()?;
        (slot < SLOTS && self.slots[slot].is_some()).then_some(slot)
    }

    pub(crate) fn mounted(&self, point: &[u8]) -> bool {
        self.slot_of(point).is_some()
    }

    /// `sceSaveDataUmount`: the slot, and the `where.json` to publish when a save was written.
    pub(crate) fn umount(&mut self, point: Option<&[u8]>) -> Unmounted {
        let failed = |code| Unmounted {
            slot: None,
            code,
            commit: None,
        };
        if !self.initialized {
            return failed(ERR_NOT_INITIALIZED);
        }
        let Some(slot) = point.and_then(|p| self.slot_of(p)) else {
            return failed(ERR_NOT_FOUND);
        };
        let Some(old) = self.slots[slot].take() else {
            return failed(ERR_NOT_FOUND);
        };
        if self.vfs {
            crate::file::unmount(format!("/savedata{slot}").as_bytes());
        }
        let commit = (old.writable && old.dirty)
            .then(|| {
                game_of(&old.name).map(|game| Commit {
                    saves: old.base.clone(),
                    game,
                    meta: old.meta.clone(),
                })
            })
            .flatten();
        Unmounted {
            slot: Some(slot),
            code: 0,
            commit,
        }
    }

    /// `sceSaveDataSetParam`: `kind` 0 all (`data` is the whole struct), 1 title, 2 subtitle,
    /// 3 detail, 4 user param (`data` holds at least 4 bytes).
    pub(crate) fn set_param(&mut self, point: &[u8], kind: u32, data: &[u8]) -> i32 {
        if !self.initialized {
            return ERR_NOT_INITIALIZED;
        }
        let Some(slot) = self.slot_of(point) else {
            return ERR_NOT_MOUNTED;
        };
        let Some(entry) = self.slots[slot].as_mut() else {
            return ERR_NOT_MOUNTED;
        };
        let mut param = read_param(&entry.meta);
        match kind {
            0 if data.len() >= PARAM_SIZE => param.0.copy_from_slice(&data[..PARAM_SIZE]),
            1 => param.set_text(TITLE, data),
            2 => param.set_text(SUBTITLE, data),
            3 => param.set_text(DETAIL, data),
            4 if data.len() >= 4 => param.0[USER_PARAM..USER_PARAM + 4].copy_from_slice(&data[..4]),
            _ => return ERR_PARAMETER,
        }
        if write_param(&entry.meta, &param).is_err() {
            return ERR_INTERNAL;
        }
        entry.dirty = true;
        0
    }

    pub(crate) fn save_icon(&mut self, point: &[u8], icon: &[u8]) -> i32 {
        if !self.initialized {
            return ERR_NOT_INITIALIZED;
        }
        let Some(entry) = self
            .slot_of(point)
            .and_then(|slot| self.slots[slot].as_mut())
        else {
            return ERR_NOT_MOUNTED;
        };
        entry.dirty = true;
        match fs::write(Path::new(&entry.meta).join("icon0.png"), icon) {
            Ok(()) => 0,
            Err(_) => ERR_INTERNAL,
        }
    }

    pub(crate) fn delete(&self, user: i32, title: Option<&str>, dir: Option<&[u8; 32]>) -> i32 {
        if !self.initialized {
            return ERR_NOT_INITIALIZED;
        }
        let Some(name) = dir.and_then(|d| valid_name(d, 32)) else {
            return ERR_PARAMETER;
        };
        let (host, meta, _) = self.paths(user, title, &name);
        if fs::metadata(&host).is_err() {
            return ERR_NOT_FOUND;
        }
        let remove_if_present = |p: &str| match fs::remove_dir_all(p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        };
        if remove_if_present(&host).is_err() || remove_if_present(&meta).is_err() {
            return ERR_INTERNAL;
        }
        0
    }

    /// An existing slot of a save list (slots that do not exist are not listed, as on the
    /// console): `which` 0 first, 1 last, 2 latest written, 3 oldest, 4 the one named `focus`.
    pub(crate) fn pick(
        &self,
        user: i32,
        title: Option<&str>,
        names: &[[u8; 32]],
        which: i32,
        focus: Option<&[u8]>,
    ) -> Option<([u8; 32], Param)> {
        let mut best: Option<(usize, Param)> = None;
        for (i, raw) in names.iter().enumerate() {
            let mut padded = [0u8; 33];
            padded[..32].copy_from_slice(raw);
            let Some(name) = valid_name(&padded, 33) else {
                continue;
            };
            let (host, meta, _) = self.paths(user, title, &name);
            if !fs::metadata(&host).is_ok_and(|m| m.is_dir()) {
                continue;
            }
            let param = read_param(&meta);
            let better = match which {
                0 => best.is_none(),
                1 => true,
                2 => best.as_ref().is_none_or(|(_, b)| param.mtime() > b.mtime()),
                3 => best.as_ref().is_none_or(|(_, b)| param.mtime() < b.mtime()),
                _ => focus.is_some_and(|f| {
                    let f = &f[..f.len().min(32)];
                    let f = &f[..f.iter().position(|&b| b == 0).unwrap_or(f.len())];
                    f == name.as_bytes()
                }),
            };
            if better {
                best = Some((i, param));
            }
        }
        best.map(|(i, param)| (names[i], param))
    }

    /// `sceSaveDataDirNameSearch`: the directories of the root folder and of each mapped
    /// folder that match, each listed only where it belongs, sorted as asked.
    pub(crate) fn search(&self, cond: &Search<'_>) -> Vec<Entry> {
        let mut entries: Vec<Entry> = Vec::new();
        // The root, then each BB_SAVEDATA_DIRS folder (up to the first entry without a folder).
        let mut bases = vec![self.root(cond.user, cond.title)];
        bases.extend((0..).map_while(|i| self.mapped_folder(None, i)));
        for base in bases {
            if let Ok(read) = fs::read_dir(&base) {
                for item in read.flatten() {
                    let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                        continue;
                    };
                    let n = name.len();
                    // Only directories are slots: `where.json` lives beside them (#168).
                    if name.starts_with('.')
                        || n >= 32
                        || (n > 8 && name.ends_with(".sce_sys"))
                        || !item.path().is_dir()
                    {
                        continue;
                    }
                    if !cond.pattern.is_empty() && !like(name.as_bytes(), cond.pattern) {
                        continue;
                    }
                    // Listed only where it belongs (no duplicates).
                    if self.dir_root(cond.user, cond.title, &name) != base {
                        continue;
                    }
                    let mut raw = [0u8; 32];
                    raw[..n].copy_from_slice(name.as_bytes());
                    let param = read_param(&format!("{base}/{name}.sce_sys"));
                    entries.push(Entry { name: raw, param });
                }
            }
        }
        let key = |e: &Entry| -> (i64, Vec<u8>) {
            match cond.key {
                1 => (i64::from(e.param.user_param()), Vec::new()),
                3 => (e.param.mtime(), Vec::new()),
                _ => (0, e.name.iter().copied().take_while(|&b| b != 0).collect()),
            }
        };
        entries.sort_by(|a, b| {
            let order = key(a).cmp(&key(b));
            if cond.order != 0 {
                order.reverse()
            } else {
                order
            }
        });
        entries
    }

    /// `sceSaveDataSetupSaveDataMemory`: one fixed-size blob per user and title.
    pub(crate) fn memory_setup(&mut self, user: i32, size: u64, param: Option<&Param>) -> i32 {
        if !self.initialized {
            return ERR_NOT_INITIALIZED;
        }
        if size == 0 {
            return ERR_PARAMETER;
        }
        let dir = format!("{}.memory", self.root(user, None));
        if fs::create_dir_all(&dir).is_err() {
            return ERR_INTERNAL;
        }
        let path = format!("{dir}/memory.dat");
        self.memory_path = Some(path.clone());
        let opened = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .or_else(|_| {
                fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&path)
            });
        let Ok(file) = opened else {
            return ERR_INTERNAL;
        };
        if file.metadata().map_or(true, |m| m.len() < size) && file.set_len(size).is_err() {
            return ERR_INTERNAL;
        }
        self.memory_size = size;
        if let Some(param) = param {
            if write_param(&dir, param).is_err() {
                return ERR_INTERNAL;
            }
        }
        0
    }

    /// The checks of a SaveDataMemory read or write of `size` bytes at `offset`.
    pub(crate) fn memory_check(&self, offset: i64, size: u64) -> i32 {
        if !self.initialized {
            return ERR_NOT_INITIALIZED;
        }
        let Ok(offset) = u64::try_from(offset) else {
            return ERR_PARAMETER;
        };
        if self.memory_size == 0 {
            return ERR_MEMORY_NOT_READY;
        }
        if offset
            .checked_add(size)
            .is_none_or(|end| end > self.memory_size)
        {
            return ERR_PARAMETER;
        }
        0
    }

    /// A read (`out`) or a write (`data`) once [`Self::memory_check`] passed.
    pub(crate) fn memory_io(&mut self, offset: i64, out: Option<&mut [u8]>, data: &[u8]) -> i32 {
        let Some(path) = self.memory_path.as_deref() else {
            return ERR_MEMORY_NOT_READY;
        };
        let Ok(offset) = u64::try_from(offset) else {
            return ERR_PARAMETER;
        };
        let Ok(mut file) = fs::OpenOptions::new().read(true).write(true).open(path) else {
            return ERR_INTERNAL;
        };
        let writing = out.is_none();
        let done = file.seek(SeekFrom::Start(offset)).is_ok()
            && match out {
                Some(out) => file.read_exact(out).is_ok(),
                None => file.write_all(data).is_ok(),
            };
        if !done {
            return ERR_INTERNAL;
        }
        if writing {
            self.memory_writes += 1;
        }
        0
    }

    /// Slots mounted for writing: a save is in progress while the game writes its files
    /// between mount and unmount.
    pub(crate) fn writing(&self) -> usize {
        self.slots.iter().flatten().filter(|s| s.writable).count()
    }
}

/// The Bloodborne "sound hack" (rainvmaker, from the Diegolix29 shadPS4 fork): the game data
/// save carries a flag at 0x204E of `userdata0010`; with it clear, parts of the game's audio
/// (e.g. the player's weapon sounds) never play. It is set before the game reads the save.
/// `BB_SOUND_HACK=0` leaves saves untouched. Returns the lines to print.
pub(crate) fn bloodborne_sound_hack(store: &Store, disabled: bool) -> Vec<String> {
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
    let mut lines = Vec::new();
    if disabled || !IDS.contains(&store.title_id()) {
        return lines;
    }
    let users = format!("{}/savedata", store.user);
    let Ok(read) = fs::read_dir(&users) else {
        return lines;
    };
    for item in read.flatten() {
        let name = item.file_name();
        let Some(name) = name.to_str().filter(|n| !n.starts_with('.')) else {
            continue;
        };
        let path = format!("{users}/{name}/{}/SPRJ0005/userdata0010", store.title_id());
        let Ok(mut file) = fs::OpenOptions::new().read(true).write(true).open(&path) else {
            continue;
        };
        let mut old = [0u8; 1];
        let read_ok =
            file.seek(SeekFrom::Start(0x204E)).is_ok() && file.read_exact(&mut old).is_ok();
        if read_ok
            && old[0] != 1
            && file.seek(SeekFrom::Start(0x204E)).is_ok()
            && file.write_all(&[1]).is_ok()
        {
            lines.push(format!(
                "Runtime: Bloodborne sound flag set in {path} (was {})",
                old[0]
            ));
        }
    }
    lines
}
