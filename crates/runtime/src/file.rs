//! The guest file system (piece 3.09; order 9 in docs/code-map.md): PS4 mount points on host
//! directories, the guest's descriptor table, and the `sceKernel*` and POSIX file calls of the
//! deleted `src/runtime_file.c`, exported with the same names and ABI. `tests/test_file_mods.c`
//! is now the C side's link check of this module; its cases are the Rust tests below.
//!
//! - `/app0` and `/hostapp` are the game's root and read-only. A mod overlay is that tree with
//!   links to the mod files in it (`scripts/mods.py`, `crates/core::mods`), so the guest reads a
//!   mod file in place of the dump's, and can never write through one. `/temp0`, `/download0`,
//!   `/data` and the mounts SaveData adds are under the user directory.
//! - A path without a leading `/` is under `/app0`. A `..` component (with either separator) is
//!   refused with `EACCES`, never normalized. The longest mount that is a whole-component prefix
//!   of the path wins.
//! - PS4 paths ignore case and Linux's do not: the game asks for
//!   `cubemap/train-wreck-a/Train_Wreck_a_07.dds`, the dump has `train_wreck_a_07.dds`. Only when
//!   the exact path is missing, each missing component below the mount is matched without (ASCII)
//!   case against its directory, keeping its length. Not on Windows, whose file systems ignore
//!   case already.
//! - Descriptors 3..1023 are this module's table; 0-2 pass through to the host's stdio. A file
//!   is an [`os::Description`](crate::os::Description): sequential calls share its position,
//!   positioned ones never touch it. A directory is a listing taken when it is opened, read by
//!   `getdirentries` with entry indices as offsets.
//! - Errors are FreeBSD errno values ([`Errno`]). The `sceKernel*` calls return them as
//!   `0x8002xxxx`, the POSIX ones -1 with the guest's errno set. The host helpers the C modules
//!   still call (`runtime_file_open` and the rest) return `-errno` and `runtime_file_translate` an
//!   errno, FreeBSD numbers too: the C file returned the host's, and every C caller only checks
//!   for failure.
//!
//! Traces: `BB_MOD_TRACE=1` prints the first 32 opens that land in `BB_MODS_DIR`;
//! `BB_AUDIO_TRACE=1` sound file opens and failed reads, `=2` also every sound bank read.

use crate::os::Description;
use std::ffi::{c_char, c_int, CStr, CString};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read as _, SeekFrom, Write as _};
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

/// Guest descriptors are below this; 0-2 are the host's stdio.
const MAX_FILES: usize = 1024;
/// 5 fixed mounts (`/app0`, `/hostapp`, `/temp0`, `/download0`, `/data`) and one per save-data
/// slot (16), with room to spare.
const MAX_MOUNTS: usize = 32;
/// A mount name is shorter than this, its host root shorter than [`HOST_ROOT_MAX`].
const GUEST_MOUNT_MAX: usize = 64;
const HOST_ROOT_MAX: usize = 512;
/// A guest or host path is shorter than this (the C buffers, with their NUL).
const PATH_MAX: usize = 1024;
/// The guest path kept with a descriptor, for the traces.
const KEPT_PATH_MAX: usize = 511;

// Guest `open` flags (FreeBSD).
const O_ACCMODE: c_int = 0x3;
const O_APPEND: c_int = 0x8;
const O_SYNC: c_int = 0x80;
const O_CREAT: c_int = 0x200;
const O_TRUNC: c_int = 0x400;
const O_EXCL: c_int = 0x800;
const O_DIRECTORY: c_int = 0x20000;

// Guest `d_type` values.
const DT_UNKNOWN: u8 = 0;
const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;
const DT_LNK: u8 = 10;

/// A guest (FreeBSD) errno value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Errno(pub i32);

// Rust-only values, not part of the C header.
/// cbindgen:ignore
impl Errno {
    pub const EPERM: Self = Self(1);
    pub const ENOENT: Self = Self(2);
    pub const EINTR: Self = Self(4);
    pub const EIO: Self = Self(5);
    pub const E2BIG: Self = Self(7);
    pub const EBADF: Self = Self(9);
    pub const EDEADLK: Self = Self(11);
    pub const ENOMEM: Self = Self(12);
    pub const EACCES: Self = Self(13);
    pub const EFAULT: Self = Self(14);
    pub const EBUSY: Self = Self(16);
    pub const EEXIST: Self = Self(17);
    pub const EXDEV: Self = Self(18);
    pub const ENOTDIR: Self = Self(20);
    pub const EISDIR: Self = Self(21);
    pub const EINVAL: Self = Self(22);
    pub const EMFILE: Self = Self(24);
    pub const EFBIG: Self = Self(27);
    pub const ENOSPC: Self = Self(28);
    pub const ESPIPE: Self = Self(29);
    pub const EROFS: Self = Self(30);
    pub const EMLINK: Self = Self(31);
    pub const EPIPE: Self = Self(32);
    pub const EAGAIN: Self = Self(35);
    pub const ETIMEDOUT: Self = Self(60);
    pub const ENAMETOOLONG: Self = Self(63);
    pub const ENOTEMPTY: Self = Self(66);
    pub const ENOSYS: Self = Self(78);

    /// The guest's value for a host error: `runtime_guest_errno` (`runtime_kernel.c`) for a
    /// host errno, the error's kind otherwise. An error with no guest value is `EIO`.
    pub fn from_io(error: &io::Error) -> Self {
        if let Some(code) = error.raw_os_error() {
            if let Some(errno) = from_host_code(code) {
                return errno;
            }
        }
        Self::from_kind(error.kind())
    }

    fn from_kind(kind: io::ErrorKind) -> Self {
        use io::ErrorKind as K;
        match kind {
            K::NotFound => Self::ENOENT,
            K::PermissionDenied => Self::EACCES,
            K::AlreadyExists => Self::EEXIST,
            K::Interrupted => Self::EINTR,
            K::InvalidInput => Self::EINVAL,
            K::WouldBlock => Self::EAGAIN,
            K::NotADirectory => Self::ENOTDIR,
            K::IsADirectory => Self::EISDIR,
            K::DirectoryNotEmpty => Self::ENOTEMPTY,
            K::ReadOnlyFilesystem => Self::EROFS,
            K::StorageFull => Self::ENOSPC,
            K::NotSeekable => Self::ESPIPE,
            K::FileTooLarge => Self::EFBIG,
            K::ResourceBusy => Self::EBUSY,
            K::CrossesDevices => Self::EXDEV,
            K::TooManyLinks => Self::EMLINK,
            K::InvalidFilename => Self::ENAMETOOLONG,
            K::BrokenPipe => Self::EPIPE,
            K::TimedOut => Self::ETIMEDOUT,
            K::Unsupported => Self::ENOSYS,
            K::OutOfMemory => Self::ENOMEM,
            K::Deadlock => Self::EDEADLK,
            K::ArgumentListTooLong => Self::E2BIG,
            _ => Self::EIO,
        }
    }
}

/// `runtime_guest_errno`'s table for Linux (and Android) errno values.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn from_host_code(code: i32) -> Option<Errno> {
    Some(Errno(match code {
        // EPERM .. ENOEXEC, EBADF, ECHILD, ENOMEM .. EFAULT, EBUSY .. EMLINK except ENOTBLK
        // (15) and ETXTBSY (26), EPIPE and ERANGE have FreeBSD's numbers.
        1..=10 | 12..=14 | 16..=25 | 27..=32 | 34 => code,
        11 => 35,  // EAGAIN
        35 => 11,  // EDEADLK
        36 => 63,  // ENAMETOOLONG
        38 => 78,  // ENOSYS
        39 => 66,  // ENOTEMPTY
        40 => 62,  // ELOOP
        75 => 84,  // EOVERFLOW
        110 => 60, // ETIMEDOUT
        125 => 85, // ECANCELED
        _ => 5,    // EIO: unmapped host error
    }))
}

/// Win32 errors as the C runtime's `errno` saw them; the rest go by their kind.
#[cfg(windows)]
fn from_host_code(code: i32) -> Option<Errno> {
    match code {
        2 | 3 | 123 | 161 | 206 => Some(Errno::ENOENT), // file, path, name, bad path, too long
        5 | 32 | 33 => Some(Errno::EACCES),             // denied, sharing, lock violation
        80 | 183 => Some(Errno::EEXIST),
        145 => Some(Errno::ENOTEMPTY),
        112 => Some(Errno::ENOSPC),
        6 => Some(Errno::EBADF),
        _ => None,
    }
}

/// Other hosts number errno as FreeBSD does for the values the guest sees.
#[cfg(not(any(target_os = "linux", target_os = "android", windows)))]
fn from_host_code(_code: i32) -> Option<Errno> {
    None
}

impl From<io::Error> for Errno {
    fn from(error: io::Error) -> Self {
        Self::from_io(&error)
    }
}

type Result<T> = std::result::Result<T, Errno>;

// ---- host paths and messages ----

#[cfg(unix)]
fn host_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(bytes).into()
}

#[cfg(not(unix))]
fn host_path(bytes: &[u8]) -> PathBuf {
    String::from_utf8_lossy(bytes).into_owned().into()
}

#[cfg(unix)]
fn name_bytes(name: std::ffi::OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStringExt;
    name.into_vec()
}

#[cfg(not(unix))]
fn name_bytes(name: std::ffi::OsString) -> Vec<u8> {
    name.to_string_lossy().into_owned().into_bytes()
}

fn shown(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}

/// A line on the loader's stdout (C stdio, in order with the C modules' lines).
fn out_line(text: &str) {
    #[cfg(target_arch = "x86_64")]
    crate::abi::host::out_line(text);
    #[cfg(not(target_arch = "x86_64"))]
    println!("{text}");
}

/// A line on stderr.
fn err_line(text: &str) {
    #[cfg(target_arch = "x86_64")]
    crate::abi::host::err(&format!("{text}\n"));
    #[cfg(not(target_arch = "x86_64"))]
    eprintln!("{text}");
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every update below is a single assignment, so a poisoned lock still holds a whole table.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---- the mount table ----

struct Mount {
    guest: Vec<u8>,
    host: Vec<u8>,
}

static MOUNTS: Mutex<Vec<Mount>> = Mutex::new(Vec::new());
/// `runtime_file_user_dir`: "user" until [`configure`] sets it.
static USER_ROOT: AtomicPtr<c_char> = AtomicPtr::new(c"user".as_ptr().cast_mut());

/// Maps guest mount point `guest` (`/app0`) on host directory `host`, replacing an earlier
/// mount of the same name. `false` when the table is full (32) or a name is too long (63 bytes
/// for `guest`, 511 for `host`).
pub fn mount(guest: &[u8], host: &[u8]) -> bool {
    let mut mounts = lock(&MOUNTS);
    if let Some(existing) = mounts.iter_mut().find(|m| m.guest == guest) {
        existing.host = host[..host.len().min(HOST_ROOT_MAX - 1)].to_vec();
        return true;
    }
    if mounts.len() == MAX_MOUNTS || guest.len() >= GUEST_MOUNT_MAX || host.len() >= HOST_ROOT_MAX {
        return false;
    }
    mounts.push(Mount {
        guest: guest.to_vec(),
        host: host.to_vec(),
    });
    true
}

/// Removes the mount `guest`, if there is one.
pub fn unmount(guest: &[u8]) {
    let mut mounts = lock(&MOUNTS);
    if let Some(i) = mounts.iter().position(|m| m.guest == guest) {
        mounts.swap_remove(i);
    }
}

/// The user directory [`configure`] was given (saves, trophies), "user" before.
pub fn user_dir() -> &'static CStr {
    // SAFETY: USER_ROOT is a static literal or a leaked CString, never freed.
    unsafe { CStr::from_ptr(USER_ROOT.load(Ordering::Acquire)) }
}

/// Mounts the game at `app0` (`/app0`, `/hostapp`) and creates and mounts `user`'s `temp0`,
/// `download0` and `data`. Creating a directory that exists is not an error.
pub fn configure(app0: &[u8], user: &[u8]) {
    let kept = &user[..user.len().min(HOST_ROOT_MAX - 1)];
    if let Ok(root) = CString::new(kept) {
        // One small leak per call: the loader configures once, and C keeps the pointer.
        USER_ROOT.store(root.into_raw(), Ordering::Release);
    }
    mount(b"/app0", app0);
    mount(b"/hostapp", app0);
    make_dir(&host_path(user));
    for name in ["temp0", "download0", "data"] {
        let mut path = user.to_vec();
        path.push(b'/');
        path.extend_from_slice(name.as_bytes());
        make_dir(&host_path(&path));
        mount(format!("/{name}").as_bytes(), &path);
    }
}

fn make_dir(path: &Path) {
    let _ = dir_builder(0o755).create(path);
}

fn dir_builder(mode: u32) -> fs::DirBuilder {
    #[cfg(unix)]
    {
        let mut builder = fs::DirBuilder::new();
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, mode);
        builder
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        fs::DirBuilder::new()
    }
}

/// The host path for guest path `guest`, at most `size - 1` bytes (the C caller's buffer).
fn translate_within(guest: &[u8], size: usize) -> Result<Vec<u8>> {
    if guest.is_empty() {
        return Err(Errno::ENOENT);
    }
    let mut path = Vec::with_capacity(guest.len() + 6);
    if guest[0] != b'/' {
        path.extend_from_slice(b"/app0/");
    }
    path.extend_from_slice(guest);
    path.truncate(PATH_MAX - 1);
    // ".." as a whole component; Windows also takes '\' as a separator, so "/data/..\..\x"
    // must not slip past on '/' alone.
    let separator = |c: u8| c == b'/' || c == b'\\';
    let escapes = path.windows(2).enumerate().any(|(i, pair)| {
        pair == b".."
            && (i == 0 || separator(path[i - 1]))
            && path.get(i + 2).is_none_or(|&c| separator(c))
    });
    if escapes {
        return Err(Errno::EACCES);
    }
    let (out, root) = {
        let mounts = lock(&MOUNTS);
        let found = mounts
            .iter()
            .filter(|m| {
                path.starts_with(&m.guest) && path.get(m.guest.len()).is_none_or(|&c| c == b'/')
            })
            .max_by_key(|m| m.guest.len());
        let Some(found) = found else {
            drop(mounts);
            err_line(&format!(
                "Runtime: no mount for guest path {}",
                shown(guest)
            ));
            return Err(Errno::ENOENT);
        };
        let mut out = found.host.clone();
        out.extend_from_slice(&path[found.guest.len()..]);
        (out, found.host.len())
    };
    if out.len() >= size {
        return Err(Errno::ENAMETOOLONG);
    }
    #[cfg(not(windows))]
    let out = {
        let mut out = out;
        fix_case(&mut out, root);
        out
    };
    #[cfg(windows)]
    let _ = root;
    Ok(out)
}

/// The host path for guest path `guest`.
pub fn translate(guest: &[u8]) -> Result<Vec<u8>> {
    translate_within(guest, PATH_MAX)
}

/// Where the exact `path` is missing, each missing component after `root` bytes (the mount's
/// host root) takes the spelling of the one entry of its directory that matches it without
/// ASCII case and has its length. Stops at the first component nothing matches.
#[cfg(not(windows))]
fn fix_case(path: &mut [u8], root: usize) {
    let exists = |bytes: &[u8]| fs::metadata(host_path(bytes)).is_ok();
    if exists(path) {
        return;
    }
    let mut start = root;
    while path.get(start) == Some(&b'/') {
        start += 1;
    }
    while start < path.len() && start > 0 {
        let end = path[start..]
            .iter()
            .position(|&c| c == b'/')
            .map_or(path.len(), |i| start + i);
        if !exists(&path[..end]) {
            let parent = if start == 1 {
                PathBuf::from("/")
            } else {
                host_path(&path[..start - 1])
            };
            let Ok(entries) = fs::read_dir(parent) else {
                return;
            };
            let component = &path[start..end];
            let found = entries
                .filter_map(|entry| entry.ok())
                .map(|entry| name_bytes(entry.file_name()))
                .find(|name| name.len() == component.len() && name.eq_ignore_ascii_case(component));
            let Some(name) = found else {
                return;
            };
            path[start..end].copy_from_slice(&name);
        }
        start = end + 1;
    }
}

/// Game paths are read-only (`/app0`, `/hostapp`, and a relative path, which is under
/// `/app0`); saves use the other mounts.
fn game_path(guest: &[u8]) -> bool {
    let under =
        |root: &[u8]| guest.starts_with(root) && guest.get(root.len()).is_none_or(|&c| c == b'/');
    !guest.is_empty() && (guest[0] != b'/' || under(b"/app0") || under(b"/hostapp"))
}

// ---- the descriptor table ----

struct OpenFile {
    description: Description,
    append: bool,
    sync: bool,
}

struct Directory {
    entries: Vec<(Vec<u8>, u8)>,
    /// The next entry `getdirentries` returns.
    position: usize,
    host: PathBuf,
}

enum Kind {
    File(Arc<OpenFile>),
    Directory(Directory),
}

struct Slot {
    kind: Kind,
    guest: Vec<u8>,
}

/// Index = descriptor; 0-2 are never used.
static FILES: Mutex<Vec<Option<Slot>>> = Mutex::new(Vec::new());

static OPENS: AtomicUsize = AtomicUsize::new(0);
static READS: AtomicUsize = AtomicUsize::new(0);
static WRITES: AtomicUsize = AtomicUsize::new(0);
static MISSING: AtomicUsize = AtomicUsize::new(0);
static BYTES_READ: AtomicU64 = AtomicU64::new(0);

/// What a descriptor reaches.
enum Target {
    Stdio(ManuallyDrop<File>),
    File(Arc<OpenFile>),
    Directory(PathBuf),
}

fn target(fd: c_int) -> Result<Target> {
    if (0..3).contains(&fd) {
        return stdio(fd).map(Target::Stdio);
    }
    let files = lock(&FILES);
    let slot = usize::try_from(fd)
        .ok()
        .and_then(|i| files.get(i))
        .and_then(Option::as_ref)
        .ok_or(Errno::EBADF)?;
    Ok(match &slot.kind {
        Kind::File(file) => Target::File(Arc::clone(file)),
        Kind::Directory(directory) => Target::Directory(directory.host.clone()),
    })
}

/// The host's stdin, stdout or stderr (`fd` 0-2), never closed.
#[cfg(unix)]
fn stdio(fd: c_int) -> Result<ManuallyDrop<File>> {
    use std::os::fd::FromRawFd;
    // SAFETY: 0-2 are the process's standard descriptors; ManuallyDrop never closes them.
    Ok(ManuallyDrop::new(unsafe { File::from_raw_fd(fd) }))
}

#[cfg(windows)]
fn stdio(fd: c_int) -> Result<ManuallyDrop<File>> {
    use std::os::windows::io::{FromRawHandle, RawHandle};
    extern "C" {
        fn _get_osfhandle(fd: c_int) -> isize;
    }
    // SAFETY: the C runtime's handle for one of its own descriptors, or -1/-2 for none.
    let handle = unsafe { _get_osfhandle(fd) };
    if handle == -1 || handle == -2 {
        return Err(Errno::EBADF);
    }
    // SAFETY: the C runtime owns the handle; ManuallyDrop never closes it.
    Ok(ManuallyDrop::new(unsafe {
        File::from_raw_handle(handle as RawHandle)
    }))
}

#[cfg(not(any(unix, windows)))]
fn stdio(_fd: c_int) -> Result<ManuallyDrop<File>> {
    Err(Errno::EBADF)
}

/// `BB_AUDIO_TRACE`: 0, 1 (sound file opens and failed reads) or 2 (also every bank read).
fn audio_trace() -> u8 {
    static LEVEL: OnceLock<u8> = OnceLock::new();
    *LEVEL.get_or_init(|| match std::env::var_os("BB_AUDIO_TRACE") {
        Some(value) if value.as_encoded_bytes().first() == Some(&b'1') => 1,
        Some(value) if value.as_encoded_bytes().first() == Some(&b'2') => 2,
        _ => 0,
    })
}

/// Opens `path` as the guest asked: access mode, create (with `mode`, 0644 when 0), exclusive,
/// truncate. The host's read-only create or truncate (which std refuses) happens with a
/// write-only open first.
fn open_host(path: &Path, flags: c_int, mode: c_int) -> io::Result<File> {
    let (read, write) = match flags & O_ACCMODE {
        0 => (true, false),
        1 => (false, true),
        _ => (true, true),
    };
    let create = flags & O_CREAT != 0;
    let exclusive = create && flags & O_EXCL != 0;
    let truncate = flags & O_TRUNC != 0;
    let options = |options: &mut OpenOptions| {
        if exclusive {
            options.create_new(true);
        } else if create {
            options.create(true);
        }
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(
            options,
            if mode != 0 { mode as u32 } else { 0o644 },
        );
        #[cfg(not(unix))]
        let _ = mode;
    };
    if !write && (create || truncate) {
        let exists = fs::metadata(path).is_ok();
        if truncate || exclusive || !exists {
            let mut first = OpenOptions::new();
            first.write(true).truncate(truncate);
            options(&mut first);
            drop(first.open(path)?);
        }
        return OpenOptions::new().read(true).open(path);
    }
    let mut host = OpenOptions::new();
    host.read(read).write(write).truncate(truncate);
    options(&mut host);
    host.open(path)
}

fn list_directory(path: &Path) -> io::Result<Vec<(Vec<u8>, u8)>> {
    // readdir's own two entries, which std leaves out.
    let mut entries = vec![(b".".to_vec(), DT_DIR), (b"..".to_vec(), DT_DIR)];
    for entry in fs::read_dir(path)? {
        let Ok(entry) = entry else { break };
        let kind = match entry.file_type() {
            // A link (a mod file in the overlay) is listed as what it points to.
            Ok(kind) if kind.is_symlink() => match fs::metadata(entry.path()) {
                Ok(target) if target.is_dir() => DT_DIR,
                Ok(target) if target.is_file() => DT_REG,
                _ => DT_LNK,
            },
            Ok(kind) if kind.is_dir() => DT_DIR,
            Ok(kind) if kind.is_file() => DT_REG,
            _ => DT_UNKNOWN,
        };
        entries.push((name_bytes(entry.file_name()), kind));
    }
    Ok(entries)
}

fn not_found(guest: &[u8], error: Errno) -> Errno {
    if error == Errno::ENOENT {
        MISSING.fetch_add(1, Ordering::Relaxed);
        out_line(&format!("Runtime: open({}) -> not found", shown(guest)));
    }
    error
}

/// Opens guest path `guest` with guest `flags` (and `mode` when it creates a file); the new
/// descriptor (3..1023).
pub fn open(guest: &[u8], flags: c_int, mode: c_int) -> Result<c_int> {
    let game = game_path(guest);
    if game && flags & (O_ACCMODE | O_APPEND | O_TRUNC | O_EXCL) != 0 {
        return Err(Errno::EROFS);
    }
    let host = translate(guest)?;
    let path = host_path(&host);
    let mut flags = flags;
    // O_CREAT on a read-only mount opens an existing file for reading, as FreeBSD does (the
    // games read their sound configuration this way); only creating a new file fails.
    if game && flags & O_CREAT != 0 {
        if fs::metadata(&path).is_err() {
            return Err(Errno::EROFS);
        }
        flags &= !O_CREAT;
    }
    let (kind, size) = match fs::metadata(&path) {
        Ok(found) if found.is_dir() => {
            if flags & O_CREAT != 0 && flags & O_EXCL != 0 {
                return Err(Errno::EEXIST);
            }
            if flags & (O_ACCMODE | O_CREAT | O_TRUNC) != 0 {
                return Err(Errno::EISDIR);
            }
            let entries = list_directory(&path).map_err(|e| not_found(guest, e.into()))?;
            let directory = Directory {
                entries,
                position: 0,
                host: path.clone(),
            };
            (Kind::Directory(directory), found.len())
        }
        Ok(_) if flags & O_DIRECTORY != 0 => return Err(Errno::ENOTDIR),
        Err(e) if flags & O_DIRECTORY != 0 => return Err(not_found(guest, e.into())),
        _ => {
            let file = open_host(&path, flags, mode).map_err(|e| not_found(guest, e.into()))?;
            let size = file.metadata().map_or(0, |m| m.len());
            let file = OpenFile {
                description: Description::new(file)?,
                append: flags & O_APPEND != 0,
                sync: flags & O_SYNC != 0,
            };
            (Kind::File(Arc::new(file)), size)
        }
    };
    let fd = {
        let mut files = lock(&FILES);
        if files.len() < 3 {
            files.resize_with(3, || None);
        }
        let free = files
            .iter()
            .skip(3)
            .position(Option::is_none)
            .map(|i| i + 3);
        let fd = match free {
            Some(fd) => fd,
            None if files.len() < MAX_FILES => {
                files.push(None);
                files.len() - 1
            }
            None => return Err(Errno::EMFILE),
        };
        files[fd] = Some(Slot {
            kind,
            guest: guest[..guest.len().min(KEPT_PATH_MAX)].to_vec(),
        });
        OPENS.fetch_add(1, Ordering::Relaxed);
        fd as c_int
    };
    if audio_trace() > 0 && guest.windows(5).any(|w| w == b"sound") {
        out_line(&format!(
            "Audio trace: open({}) -> fd {fd}, {size} bytes",
            shown(guest)
        ));
    }
    trace_mod_open(guest, &path);
    Ok(fd)
}

/// `BB_MOD_TRACE=1`: the first 32 opens whose file is in `BB_MODS_DIR` (a mod's file linked
/// into the overlay).
fn trace_mod_open(guest: &[u8], path: &Path) {
    static TRACED: AtomicU32 = AtomicU32::new(0);
    let on = std::env::var_os("BB_MOD_TRACE")
        .is_some_and(|value| value.as_encoded_bytes().first() == Some(&b'1'));
    let Some(root) = std::env::var_os("BB_MODS_DIR").filter(|_| on) else {
        return;
    };
    let (Ok(actual), Ok(root)) = (fs::canonicalize(path), fs::canonicalize(root)) else {
        return;
    };
    if actual != root && actual.starts_with(&root) && TRACED.fetch_add(1, Ordering::Relaxed) < 32 {
        out_line(&format!(
            "Mods: open {} -> {}",
            shown(guest),
            actual.display()
        ));
    }
}

/// Closes `fd`; closing 0-2 does nothing.
pub fn close(fd: c_int) -> Result<()> {
    if (0..3).contains(&fd) {
        return Ok(());
    }
    let slot = {
        let mut files = lock(&FILES);
        usize::try_from(fd)
            .ok()
            .and_then(|i| files.get_mut(i))
            .and_then(Option::take)
            .ok_or(Errno::EBADF)?
    };
    // A read still running on another thread keeps the file open until it returns.
    drop(slot);
    Ok(())
}

/// Reads up to `size` bytes into guest memory at `dst`, calling `read(buffer, done)` until the
/// request is met or the file ends (one call only when `once`: stdin, a pipe). Pages of the
/// destination may be write-protected for GPU tracking, and the host's copy into such a page
/// fails or stops short instead of faulting to the handler (a short read taken as complete
/// lost data, #101). On Linux the file is read into a per-thread host buffer and copied in user
/// space, where a protected page faults like any guest write and the handler unprotects it.
///
/// # Safety
/// `dst` is null only with `size` 0, and is otherwise writable for `size` bytes.
#[cfg(not(windows))]
unsafe fn read_full(
    dst: *mut u8,
    size: u64,
    once: bool,
    mut read: impl FnMut(&mut [u8], u64) -> io::Result<usize>,
) -> io::Result<u64> {
    use std::cell::RefCell;
    const BOUNCE: usize = 4 << 20;
    thread_local! {
        static BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }
    BUFFER.with(|buffer| {
        let mut owned = Vec::new();
        let mut borrowed = buffer.try_borrow_mut();
        let bounce = match borrowed.as_deref_mut() {
            Ok(bounce) => bounce,
            Err(_) => &mut owned,
        };
        if bounce.len() < BOUNCE {
            bounce
                .try_reserve_exact(BOUNCE - bounce.len())
                .map_err(|_| io::Error::from(io::ErrorKind::OutOfMemory))?;
            bounce.resize(BOUNCE, 0);
        }
        let mut done = 0u64;
        while done < size {
            let want = (size - done).min(BOUNCE as u64) as usize;
            let n = match read(&mut bounce[..want], done) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) if done > 0 => break,
                Err(e) => return Err(e),
            };
            // SAFETY: `n <= want` bytes of the bounce buffer, into the caller's `size` bytes
            // at `dst`, from `done` on (`done + n <= size`).
            unsafe { std::ptr::copy_nonoverlapping(bounce.as_ptr(), dst.add(done as usize), n) };
            done += n as u64;
            if once {
                break;
            }
        }
        Ok(done)
    })
}

/// Windows: each page of the rest of the destination is written once from user mode first
/// (the handler unprotects it), the host reads straight into it, and a failed read is tried
/// again, up to 8 times, as the GPU side can protect a page again in between.
///
/// # Safety
/// As the other `read_full`.
#[cfg(windows)]
unsafe fn read_full(
    dst: *mut u8,
    size: u64,
    once: bool,
    mut read: impl FnMut(&mut [u8], u64) -> io::Result<usize>,
) -> io::Result<u64> {
    // The C runtime read at most this much per call.
    const CHUNK: u64 = 0x7fff_f000;
    let mut done = 0u64;
    let mut retries = 0;
    while done < size {
        // SAFETY: `done < size`, inside the caller's buffer.
        let at = unsafe { dst.add(done as usize) };
        // SAFETY: the rest of the caller's buffer.
        unsafe { touch_for_write(at, size - done) };
        let len = (size - done).min(CHUNK) as usize;
        // SAFETY: `len` bytes of the caller's buffer from `at`; only the host read writes them.
        let buffer = unsafe { std::slice::from_raw_parts_mut(at, len) };
        match read(buffer, done) {
            Ok(0) => break,
            Ok(n) => {
                done += n as u64;
                retries = 0;
                if once {
                    break;
                }
            }
            Err(e) => {
                retries += 1;
                if retries > 8 {
                    if done > 0 {
                        break;
                    }
                    return Err(e);
                }
            }
        }
    }
    Ok(done)
}

/// # Safety
/// `size` bytes at `at` are writable.
#[cfg(windows)]
unsafe fn touch_for_write(at: *mut u8, size: u64) {
    const PAGE: usize = 4096;
    let start = at as usize;
    let end = start + size as usize;
    let mut page = start & !(PAGE - 1);
    while page < end {
        let byte = page.max(start) as *mut u8;
        // SAFETY: a byte of the caller's buffer, rewritten with its own value.
        unsafe { byte.write_volatile(byte.read_volatile()) };
        page += PAGE;
    }
}

/// Reads up to `size` bytes from `fd` into `dst`: at the file position when `offset` is
/// negative (and moving it), else at `offset` (`pread`, the position unchanged).
///
/// # Safety
/// `dst` is writable for `size` bytes (or null, which fails with `EFAULT` unless `size` is 0).
pub unsafe fn read_into(fd: c_int, dst: *mut u8, size: u64, offset: i64) -> Result<u64> {
    let target = target(fd)?;
    if size == 0 {
        return Ok(0);
    }
    if dst.is_null() {
        return Err(Errno::EFAULT);
    }
    let result = match &target {
        Target::Directory(_) => Err(io::ErrorKind::IsADirectory.into()),
        Target::Stdio(file) => {
            let mut file: &File = file;
            // SAFETY: forwarded from the caller.
            unsafe {
                read_full(dst, size, true, |buffer, done| {
                    if offset < 0 {
                        file.read(buffer)
                    } else {
                        positioned_read(file, buffer, offset as u64 + done)
                    }
                })
            }
        }
        Target::File(file) => {
            let description = &file.description;
            // SAFETY: forwarded from the caller.
            unsafe {
                read_full(dst, size, false, |buffer, done| {
                    if offset < 0 {
                        description.read(buffer)
                    } else {
                        description.read_at(buffer, offset as u64 + done)
                    }
                })
            }
        }
    };
    match result {
        Ok(n) => {
            READS.fetch_add(1, Ordering::Relaxed);
            BYTES_READ.fetch_add(n, Ordering::Relaxed);
            trace_bank_read(fd, size, offset, n);
            Ok(n)
        }
        Err(e) => {
            let errno = Errno::from_io(&e);
            if audio_trace() > 0 {
                if offset < 0 {
                    out_line(&format!(
                        "Audio trace: read(fd {fd}, {size}) failed, errno {}",
                        errno.0
                    ));
                } else {
                    out_line(&format!(
                        "Audio trace: pread(fd {fd}, {size} @{offset}) failed, errno {}",
                        errno.0
                    ));
                }
            }
            Err(errno)
        }
    }
}

#[cfg(unix)]
fn positioned_read(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buffer, offset)
}

#[cfg(windows)]
fn positioned_read(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buffer, offset)
}

#[cfg(not(any(unix, windows)))]
fn positioned_read(_: &File, _: &mut [u8], _: u64) -> io::Result<usize> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(unix)]
fn positioned_write(file: &File, buffer: &[u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::write_at(file, buffer, offset)
}

#[cfg(windows)]
fn positioned_write(file: &File, buffer: &[u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_write(file, buffer, offset)
}

#[cfg(not(any(unix, windows)))]
fn positioned_write(_: &File, _: &[u8], _: u64) -> io::Result<usize> {
    Err(io::ErrorKind::Unsupported.into())
}

/// `BB_AUDIO_TRACE=2`: every read of a sound bank.
fn trace_bank_read(fd: c_int, size: u64, offset: i64, n: u64) {
    if audio_trace() < 2 {
        return;
    }
    let guest = {
        let files = lock(&FILES);
        match usize::try_from(fd).ok().and_then(|i| files.get(i)) {
            Some(Some(slot)) => slot.guest.clone(),
            _ => return,
        }
    };
    let bank = |part: &[u8]| guest.windows(part.len()).any(|w| w == part);
    if bank(b".bnk") || bank(b"config/ps4") {
        out_line(&format!(
            "Audio trace: read {} fd {fd}: {size} bytes @{offset} -> {n}",
            shown(&guest)
        ));
    }
}

/// Writes up to `size` bytes from `src` to `fd`: at the file position (or the end, for a file
/// opened with `O_APPEND`) when `offset` is negative, else at `offset` (the position
/// unchanged). One host call; it may write fewer bytes.
///
/// # Safety
/// `src` is readable for `size` bytes (or null, which fails with `EFAULT` unless `size` is 0).
pub unsafe fn write_from(fd: c_int, src: *const u8, size: u64, offset: Option<i64>) -> Result<u64> {
    let target = target(fd)?;
    if offset.is_some_and(|offset| offset < 0) {
        return Err(Errno::EINVAL);
    }
    if size == 0 {
        return Ok(0);
    }
    if src.is_null() {
        return Err(Errno::EFAULT);
    }
    // The C runtime wrote at most this much per call; Linux does the same.
    let len = size.min(0x7fff_f000) as usize;
    // SAFETY: `len <= size` readable bytes at `src` (caller).
    let buffer = unsafe { std::slice::from_raw_parts(src, len) };
    let written = match (&target, offset) {
        // A directory's host descriptor was read-only (Linux) or none (Windows).
        (Target::Directory(_), _) => return Err(Errno::EBADF),
        (Target::Stdio(file), None) => {
            let mut file: &File = file;
            file.write(buffer)
        }
        (Target::Stdio(file), Some(at)) => positioned_write(file, buffer, at as u64),
        (Target::File(file), None) => {
            let n = if file.append {
                file.description.append(buffer)
            } else {
                file.description.write(buffer)
            }?;
            if file.sync {
                file.description.file().sync_data()?;
            }
            Ok(n)
        }
        (Target::File(file), Some(at)) => {
            // Linux and FreeBSD both write an `O_APPEND` file's positioned writes at the end.
            let at = if file.append {
                file.description.file().metadata()?.len()
            } else {
                at as u64
            };
            let n = file.description.write_at(buffer, at)?;
            if file.sync {
                file.description.file().sync_data()?;
            }
            Ok(n)
        }
    }?;
    if offset.is_none() {
        WRITES.fetch_add(1, Ordering::Relaxed);
    }
    Ok(written as u64)
}

/// Moves `fd`'s position (`whence` 0 set, 1 current, 2 end) and returns it. On a directory the
/// position is the next entry's index, and only an absolute one can be set.
pub fn lseek(fd: c_int, offset: i64, whence: c_int) -> Result<i64> {
    if (0..3).contains(&fd) {
        return Err(Errno::EBADF);
    }
    if !(0..=2).contains(&whence) {
        // The descriptor is checked first, as the C file did.
        target(fd)?;
        return Err(Errno::EINVAL);
    }
    let file = {
        let mut files = lock(&FILES);
        let slot = usize::try_from(fd)
            .ok()
            .and_then(|i| files.get_mut(i))
            .and_then(Option::as_mut)
            .ok_or(Errno::EBADF)?;
        match &mut slot.kind {
            Kind::Directory(directory) => {
                return if whence == 0 && offset >= 0 {
                    directory.position = offset as usize;
                    Ok(offset)
                } else {
                    Err(Errno::EINVAL)
                };
            }
            Kind::File(file) => Arc::clone(file),
        }
    };
    let to = match whence {
        0 => SeekFrom::Start(u64::try_from(offset).map_err(|_| Errno::EINVAL)?),
        1 => SeekFrom::Current(offset),
        _ => SeekFrom::End(offset),
    };
    Ok(file.description.seek(to)? as i64)
}

/// The FreeBSD `struct timespec`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GuestTimespec {
    pub sec: i64,
    pub nsec: i64,
}

/// The FreeBSD `struct stat` the guest reads (120 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GuestStat {
    pub dev: u32,
    pub ino: u32,
    pub mode: u16,
    pub nlink: u16,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u32,
    pub atime: GuestTimespec,
    pub mtime: GuestTimespec,
    pub ctime: GuestTimespec,
    pub size: i64,
    pub blocks: i64,
    pub blksize: u32,
    pub flags: u32,
    pub gen: u32,
    pub lspare: i32,
    pub birthtime: GuestTimespec,
}

const _: () = assert!(std::mem::size_of::<GuestStat>() == 120);

impl GuestStat {
    #[cfg(unix)]
    fn from_metadata(metadata: &Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        let time = |sec: i64, nsec: i64| GuestTimespec { sec, nsec };
        let ctime = time(metadata.ctime(), metadata.ctime_nsec());
        Self {
            dev: metadata.dev() as u32,
            ino: metadata.ino() as u32,
            mode: metadata.mode() as u16,
            nlink: metadata.nlink() as u16,
            size: metadata.size() as i64,
            blocks: metadata.blocks() as i64,
            blksize: metadata.blksize() as u32,
            atime: time(metadata.atime(), metadata.atime_nsec()),
            mtime: time(metadata.mtime(), metadata.mtime_nsec()),
            ctime,
            birthtime: ctime,
            ..Self::default()
        }
    }

    /// Windows: FreeBSD mode bits (directories rwxr-xr-x, files rw-r--r--), whole seconds,
    /// and the creation time as `ctime`, as the C runtime's `_stat64` gave them.
    #[cfg(not(unix))]
    fn from_metadata(metadata: &Metadata) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let seconds = |time: io::Result<SystemTime>| {
            let sec = match time {
                Ok(time) => match time.duration_since(UNIX_EPOCH) {
                    Ok(after) => after.as_secs() as i64,
                    Err(before) => -(before.duration().as_secs() as i64),
                },
                Err(_) => 0,
            };
            GuestTimespec { sec, nsec: 0 }
        };
        let size = metadata.len() as i64;
        let ctime = seconds(metadata.created());
        Self {
            mode: if metadata.is_dir() {
                0o040755
            } else {
                0o100644
            },
            nlink: 1,
            size,
            blocks: (size + 511) / 512,
            blksize: 4096,
            atime: seconds(metadata.accessed()),
            mtime: seconds(metadata.modified()),
            ctime,
            birthtime: ctime,
            ..Self::default()
        }
    }
}

/// The guest `stat` of `fd`.
pub fn fstat(fd: c_int) -> Result<GuestStat> {
    let metadata = match target(fd)? {
        Target::Stdio(file) => file.metadata()?,
        Target::File(file) => file.description.file().metadata()?,
        // A directory is a listing: stat the path it was opened with.
        Target::Directory(host) => fs::metadata(host).map_err(|_| Errno::EBADF)?,
    };
    Ok(GuestStat::from_metadata(&metadata))
}

/// The guest `stat` of guest path `guest`.
pub fn stat(guest: &[u8]) -> Result<GuestStat> {
    let host = translate(guest)?;
    let metadata = fs::metadata(host_path(&host))?;
    Ok(GuestStat::from_metadata(&metadata))
}

/// FreeBSD `getdirentries`: the entries from the directory's position into `buffer`, each
/// `{u32 fileno; u16 reclen; u8 type; u8 namlen; name; NUL}` padded to 8 bytes, as many as fit
/// whole. Returns the bytes written (0 at the end) and the position before (`basep`). The
/// buffer must hold at least 512 bytes.
pub fn getdents(fd: c_int, buffer: &mut [u8]) -> Result<(usize, i64)> {
    let mut files = lock(&FILES);
    let slot = usize::try_from(fd)
        .ok()
        .and_then(|i| files.get_mut(i))
        .and_then(Option::as_mut)
        .ok_or(Errno::EBADF)?;
    let Kind::Directory(directory) = &mut slot.kind else {
        return Err(Errno::EINVAL);
    };
    if buffer.len() < 512 {
        return Err(Errno::EINVAL);
    }
    let base = directory.position as i64;
    let mut written = 0;
    while let Some((name, kind)) = directory.entries.get(directory.position) {
        let n = name.len().min(255);
        let reclen = (8 + n + 1 + 7) & !7;
        if written + reclen > buffer.len() {
            break;
        }
        let record = &mut buffer[written..written + reclen];
        record.fill(0);
        let fileno = directory.position as u32 + 1;
        record[..4].copy_from_slice(&fileno.to_ne_bytes());
        record[4..6].copy_from_slice(&(reclen as u16).to_ne_bytes());
        record[6] = *kind;
        record[7] = n as u8;
        record[8..8 + n].copy_from_slice(&name[..n]);
        written += reclen;
        directory.position += 1;
    }
    Ok((written, base))
}

/// `mkdir` (with `mode`, 0755 when 0), `rmdir` or `unlink` of a guest path off the game mounts.
pub enum PathOp {
    Mkdir(c_int),
    Rmdir,
    Unlink,
}

pub fn path_op(guest: &[u8], op: PathOp) -> Result<()> {
    if game_path(guest) {
        return Err(Errno::EROFS);
    }
    let path = host_path(&translate(guest)?);
    match op {
        PathOp::Mkdir(mode) => {
            dir_builder(if mode != 0 { mode as u32 } else { 0o755 }).create(path)
        }
        PathOp::Rmdir => fs::remove_dir(path),
        PathOp::Unlink => fs::remove_file(path),
    }?;
    Ok(())
}

/// Renames `from` to `to`, replacing an existing `to` (POSIX `rename`, on Windows too).
pub fn rename(from: &[u8], to: &[u8]) -> Result<()> {
    if game_path(from) || game_path(to) {
        return Err(Errno::EROFS);
    }
    let from = translate(from)?;
    let to = translate(to)?;
    fs::rename(host_path(&from), host_path(&to))?;
    Ok(())
}

fn length(length: i64) -> Result<u64> {
    u64::try_from(length).map_err(|_| Errno::EINVAL)
}

pub fn ftruncate(fd: c_int, size: i64) -> Result<()> {
    let target = target(fd)?;
    let size = length(size)?;
    match target {
        Target::Stdio(file) => file.set_len(size)?,
        Target::File(file) => file.description.file().set_len(size)?,
        Target::Directory(_) => return Err(Errno::EINVAL),
    }
    Ok(())
}

pub fn truncate(guest: &[u8], size: i64) -> Result<()> {
    if game_path(guest) {
        return Err(Errno::EROFS);
    }
    let path = host_path(&translate(guest)?);
    let size = length(size)?;
    OpenOptions::new().write(true).open(path)?.set_len(size)?;
    Ok(())
}

pub fn fsync(fd: c_int) -> Result<()> {
    match target(fd)? {
        Target::Stdio(file) => file.sync_all()?,
        Target::File(file) => file.description.file().sync_all()?,
        Target::Directory(_) => {}
    }
    Ok(())
}

/// POSIX `access`: `mode` is F_OK (0) or R_OK (4), W_OK (2), X_OK (1) together.
pub fn access(guest: &[u8], mode: c_int) -> Result<()> {
    let host = translate(guest)?;
    host_access(&host_path(&host), mode)
}

#[cfg(unix)]
fn host_access(path: &Path, mode: c_int) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    extern "C" {
        fn access(path: *const c_char, mode: c_int) -> c_int;
    }
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| Errno::ENOENT)?;
    // SAFETY: a NUL-terminated path.
    if unsafe { access(path.as_ptr(), mode & 7) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error().into())
    }
}

/// Windows: no execute bit (the C runtime refused X_OK); write needs a file that is not
/// read-only.
#[cfg(not(unix))]
fn host_access(path: &Path, mode: c_int) -> Result<()> {
    let metadata = fs::metadata(path)?;
    if mode & 2 != 0 && metadata.permissions().readonly() {
        return Err(Errno::EACCES);
    }
    Ok(())
}

/// The module's counters, one line on stdout.
pub fn report() {
    out_line(&format!(
        "Runtime: files opened={}, reads={} ({} bytes), writes={}, not found={}",
        OPENS.load(Ordering::Relaxed),
        READS.load(Ordering::Relaxed),
        BYTES_READ.load(Ordering::Relaxed),
        WRITES.load(Ordering::Relaxed),
        MISSING.load(Ordering::Relaxed)
    ));
}

/// The guest-facing calls: the `sceKernel*` and POSIX pairs, looked up by symbol name.
#[cfg(target_arch = "x86_64")]
mod guest;

/// The host function behind guest import `nid` (a scoped NID), or `None` when this module does
/// not export its symbol.
pub fn resolve(nid: &CStr) -> Option<usize> {
    let symbol = crate::abi::nid_entry(nid.to_bytes())?;
    #[cfg(target_arch = "x86_64")]
    {
        guest::lookup(symbol.to_bytes())
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = symbol;
        None
    }
}

/// The exported C functions of this module, declared in `include/uncharted_runtime.h`: those
/// of `src/runtime_file.c`, same names and ABI.
pub mod ffi {
    use super::{Errno, Result};
    use std::ffi::{c_char, c_int, c_void, CStr};

    /// # Safety
    /// `text` is null or a NUL-terminated string that outlives the returned slice.
    unsafe fn bytes<'a>(text: *const c_char) -> Option<&'a [u8]> {
        // SAFETY: the caller passes null or a valid NUL-terminated string.
        (!text.is_null()).then(|| unsafe { CStr::from_ptr(text) }.to_bytes())
    }

    /// A result as the host helpers return it: the value or `-errno`.
    fn value(result: Result<i64>) -> i64 {
        result.unwrap_or_else(|Errno(e)| -i64::from(e))
    }

    /// Maps guest mount point `guest` on host directory `host` (a mount of the same name is
    /// replaced). 0, or -1 when the table is full or a name is too long.
    ///
    /// # Safety
    /// Both are null or NUL-terminated strings.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_file_mount(
        guest: *const c_char,
        host: *const c_char,
    ) -> c_int {
        // SAFETY: forwarded from the caller.
        match unsafe { (bytes(guest), bytes(host)) } {
            (Some(guest), Some(host)) if super::mount(guest, host) => 0,
            _ => -1,
        }
    }

    /// Removes the mount `guest`, if there is one.
    ///
    /// # Safety
    /// `guest` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_file_unmount(guest: *const c_char) {
        // SAFETY: forwarded from the caller.
        if let Some(guest) = unsafe { bytes(guest) } {
            super::unmount(guest);
        }
    }

    /// The user directory (saves, trophies): `runtime_file_configure`'s `user`, "user" before.
    #[no_mangle]
    pub extern "C" fn runtime_file_user_dir() -> *const c_char {
        super::user_dir().as_ptr()
    }

    /// Mounts the game at `app0` and the writable mounts under `user`, creating them.
    ///
    /// # Safety
    /// Both are null or NUL-terminated strings; null leaves the call out.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_file_configure(app0: *const c_char, user: *const c_char) {
        // SAFETY: forwarded from the caller.
        if let (Some(app0), Some(user)) = unsafe { (bytes(app0), bytes(user)) } {
            super::configure(app0, user);
        }
    }

    /// The host path of guest path `guest` into `out` (`size` bytes with the NUL): 0, or an
    /// errno (`ENAMETOOLONG` when it does not fit).
    ///
    /// # Safety
    /// `guest` is null or a NUL-terminated string; `out` is writable for `size` bytes.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_file_translate(
        guest: *const c_char,
        out: *mut c_char,
        size: usize,
    ) -> c_int {
        // SAFETY: forwarded from the caller.
        let guest = unsafe { bytes(guest) }.unwrap_or_default();
        match super::translate_within(guest, size) {
            Ok(_) if out.is_null() => Errno::EFAULT.0,
            Ok(path) => {
                // SAFETY: `path.len() < size` bytes and the NUL fit the caller's buffer.
                unsafe {
                    std::ptr::copy_nonoverlapping(path.as_ptr(), out.cast(), path.len());
                    *out.add(path.len()) = 0;
                }
                0
            }
            Err(Errno(e)) => e,
        }
    }

    /// Opens a guest path: the descriptor, or `-errno`.
    ///
    /// # Safety
    /// `path` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_file_open(
        path: *const c_char,
        flags: c_int,
        mode: c_int,
    ) -> i64 {
        // SAFETY: forwarded from the caller.
        let path = unsafe { bytes(path) }.unwrap_or_default();
        value(super::open(path, flags, mode).map(i64::from))
    }

    /// 0, or `-errno`.
    #[no_mangle]
    pub extern "C" fn runtime_file_close(fd: c_int) -> i64 {
        value(super::close(fd).map(|()| 0))
    }

    /// Reads at the position: the bytes read, or `-errno`.
    ///
    /// # Safety
    /// `buffer` is writable for `size` bytes.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_file_read(fd: c_int, buffer: *mut c_void, size: u64) -> i64 {
        // SAFETY: forwarded from the caller.
        value(unsafe { super::read_into(fd, buffer.cast(), size, -1) }.map(|n| n as i64))
    }

    /// Reads at `offset` (at the position when negative): the bytes read, or `-errno`.
    ///
    /// # Safety
    /// `buffer` is writable for `size` bytes.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_file_pread(
        fd: c_int,
        buffer: *mut c_void,
        size: u64,
        offset: i64,
    ) -> i64 {
        // SAFETY: forwarded from the caller.
        value(unsafe { super::read_into(fd, buffer.cast(), size, offset) }.map(|n| n as i64))
    }

    /// Writes at the position: the bytes written, or `-errno`.
    ///
    /// # Safety
    /// `buffer` is readable for `size` bytes.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_file_write(
        fd: c_int,
        buffer: *const c_void,
        size: u64,
    ) -> i64 {
        // SAFETY: forwarded from the caller.
        value(unsafe { super::write_from(fd, buffer.cast(), size, None) }.map(|n| n as i64))
    }

    /// The new position, or `-errno`.
    #[no_mangle]
    pub extern "C" fn runtime_file_lseek(fd: c_int, offset: i64, whence: c_int) -> i64 {
        value(super::lseek(fd, offset, whence))
    }

    /// Fills the FreeBSD `struct stat` (120 bytes) at `guest_stat`: 0, or `-errno`.
    ///
    /// # Safety
    /// `path` is null or a NUL-terminated string; `guest_stat` is null or writable for 120
    /// bytes.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_file_stat(
        path: *const c_char,
        guest_stat: *mut c_void,
    ) -> i64 {
        // SAFETY: forwarded from the caller.
        let path = unsafe { bytes(path) }.unwrap_or_default();
        // SAFETY: forwarded from the caller.
        value(
            super::translate(path)
                .and_then(|_| unsafe { put_stat(guest_stat, || super::stat(path)) }),
        )
    }

    /// As `runtime_file_stat`, for a descriptor.
    ///
    /// # Safety
    /// `guest_stat` is null or writable for 120 bytes.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_file_fstat(fd: c_int, guest_stat: *mut c_void) -> i64 {
        // SAFETY: forwarded from the caller.
        value(super::target(fd).and_then(|_| unsafe { put_stat(guest_stat, || super::fstat(fd)) }))
    }

    /// `getdirentries`: the bytes written, or `-errno`; `*base` (when not null) gets the
    /// position before.
    ///
    /// # Safety
    /// `buffer` is null or writable for `size` bytes; `base` is null or writable.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_file_getdents(
        fd: c_int,
        buffer: *mut c_char,
        size: u64,
        base: *mut i64,
    ) -> i64 {
        // SAFETY: forwarded from the caller.
        value(unsafe { super::getdents_raw(fd, buffer.cast(), size, base) })
    }

    /// The module's export for scoped NID `name`, or 0.
    ///
    /// # Safety
    /// `name` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_file_resolve(name: *const c_char) -> usize {
        if name.is_null() {
            return 0;
        }
        // SAFETY: a NUL-terminated string (caller).
        super::resolve(unsafe { CStr::from_ptr(name) }).unwrap_or(0)
    }

    /// Files opened, reads (and bytes), writes and opens of missing files, on stdout.
    #[no_mangle]
    pub extern "C" fn runtime_file_report() {
        super::report();
    }

    /// Writes `stat()` to `out`; `EFAULT` for null, before `stat` runs (the C order: a bad
    /// path or descriptor, then the buffer, then the host's error).
    ///
    /// # Safety
    /// `out` is null or writable for 120 bytes.
    pub(super) unsafe fn put_stat(
        out: *mut c_void,
        stat: impl FnOnce() -> Result<super::GuestStat>,
    ) -> Result<i64> {
        if out.is_null() {
            return Err(Errno::EFAULT);
        }
        let stat = stat()?;
        // SAFETY: 120 writable bytes (caller); the guest's buffer need not be aligned.
        unsafe { out.cast::<super::GuestStat>().write_unaligned(stat) };
        Ok(0)
    }
}

/// [`getdents`] on a raw guest buffer.
///
/// # Safety
/// `buffer` is null or writable for `size` bytes; `base` is null or writable.
unsafe fn getdents_raw(fd: c_int, buffer: *mut u8, size: u64, base: *mut i64) -> Result<i64> {
    {
        let files = lock(&FILES);
        match usize::try_from(fd).ok().and_then(|i| files.get(i)) {
            Some(Some(Slot {
                kind: Kind::Directory(_),
                ..
            })) => {}
            Some(Some(_)) => return Err(Errno::EINVAL),
            _ => return Err(Errno::EBADF),
        }
    }
    if buffer.is_null() {
        return Err(Errno::EFAULT);
    }
    // Records never exceed 272 bytes, so a larger buffer is only filled further.
    let len = usize::try_from(size)
        .unwrap_or(usize::MAX)
        .min(isize::MAX as usize);
    // SAFETY: `len <= size` writable bytes at `buffer` (caller).
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer, len) };
    let (written, before) = getdents(fd, buffer)?;
    if !base.is_null() {
        // SAFETY: writable (caller); the guest's need not be aligned.
        unsafe { base.write_unaligned(before) };
    }
    Ok(written as i64)
}

#[cfg(test)]
mod tests;
