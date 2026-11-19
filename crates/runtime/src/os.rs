//! Host OS shims (roadmap 3.15; replaces `src/compat_win.c` piece by piece, #39).
//!
//! [`Description`] is one open file description: a host file plus the position that
//! sequential reads and writes share. Every host call is positioned, so the host's own file
//! pointer never carries state; positioned calls leave the position alone, as POSIX
//! `pread` and `pwrite` do, without the save-and-restore that raced in `compat_win.c`.
//! Duplicated descriptors share one `Description` (an `Arc`), and so share its position.
//!
//! The guest file layer ([`crate::file`], roadmap 3.09) keeps one per open guest file.

use std::ffi::{c_char, c_int, c_void, CStr};
use std::fs::{self, File};
use std::io::{self, SeekFrom};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

/// An open file description with its own sequential position.
///
/// Open the file without `append`: the description places every write itself.
#[derive(Debug)]
pub struct Description {
    file: File,
    position: Mutex<u64>,
}

impl Description {
    /// Wrap `file`, starting at its current host position.
    pub fn new(file: File) -> io::Result<Self> {
        use std::io::Seek;
        let position = (&file).stream_position()?;
        Ok(Self {
            file,
            position: Mutex::new(position),
        })
    }

    /// Read at the shared position and advance it by the bytes read.
    ///
    /// The position is held for the whole host call, so concurrent sequential calls on one
    /// description never hand out the same bytes twice or step over any.
    pub fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut position = self.lock();
        let n = read_at(&self.file, buffer, *position)?;
        *position += n as u64;
        Ok(n)
    }

    /// Write at the shared position and advance it by the bytes written.
    pub fn write(&self, buffer: &[u8]) -> io::Result<usize> {
        let mut position = self.lock();
        let n = write_at(&self.file, buffer, *position)?;
        *position += n as u64;
        Ok(n)
    }

    /// Write at the end of the file and leave the position after the bytes written
    /// (`O_APPEND`). The position is held for the whole host call, so appends on one
    /// description never land on each other.
    pub fn append(&self, buffer: &[u8]) -> io::Result<usize> {
        let mut position = self.lock();
        let end = self.file.metadata()?.len();
        let n = write_at(&self.file, buffer, end)?;
        *position = end + n as u64;
        Ok(n)
    }

    /// The host file, for the calls that never move the position (`fstat`, `ftruncate`,
    /// `fsync`).
    pub fn file(&self) -> &File {
        &self.file
    }

    /// Read at `offset`; the shared position is unchanged (`pread`). An offset past
    /// `i64::MAX` is refused with `InvalidInput` (`EINVAL`) before any host call.
    pub fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        read_at(&self.file, buffer, host_offset(offset)?)
    }

    /// Write at `offset`; the shared position is unchanged (`pwrite`). An offset past
    /// `i64::MAX` is refused with `InvalidInput` (`EINVAL`) before any host call.
    pub fn write_at(&self, buffer: &[u8], offset: u64) -> io::Result<usize> {
        write_at(&self.file, buffer, host_offset(offset)?)
    }

    /// Move the shared position (`lseek`) and return it. A target before the start of the
    /// file, or past `i64::MAX` (an `off_t`), is refused with `InvalidInput` (`EINVAL`) and
    /// the position stays where it was.
    pub fn seek(&self, to: SeekFrom) -> io::Result<u64> {
        let mut position = self.lock();
        let target = match to {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::Current(delta) => position.checked_add_signed(delta),
            SeekFrom::End(delta) => self.file.metadata()?.len().checked_add_signed(delta),
        };
        let target = target
            .filter(|&t| i64::try_from(t).is_ok())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "seek target before the start of the file or past i64::MAX",
                )
            })?;
        *position = target;
        Ok(target)
    }

    /// The shared position.
    pub fn position(&self) -> u64 {
        *self.lock()
    }

    // A poisoned lock still holds a valid position: a panic never leaves it half-updated.
    fn lock(&self) -> std::sync::MutexGuard<'_, u64> {
        self.position.lock().unwrap_or_else(|e| e.into_inner())
    }
}

// A caller's offset as a host offset: it must fit an `off_t`, as `pread` and `pwrite` (and
// `compat_win.c`) require. Windows takes the bits as a signed offset, so one past `i64::MAX`
// would arrive negative, where -1 writes at the end of the file and -2 reads or writes at the
// handle's own file pointer. The shared position needs no check: `seek` keeps it within
// `i64::MAX`, and no file holds a byte past it.
fn host_offset(offset: u64) -> io::Result<u64> {
    if i64::try_from(offset).is_ok() {
        Ok(offset)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file offset past i64::MAX",
        ))
    }
}

// The host calls. Both are positioned; the Windows one moves the handle's own file pointer as
// a side effect, which nothing here reads.
#[cfg(unix)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buffer, offset)
}

#[cfg(unix)]
fn write_at(file: &File, buffer: &[u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::write_at(file, buffer, offset)
}

#[cfg(windows)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    // ReadFile with an OVERLAPPED offset; end of file reads as 0 bytes, as pread does.
    std::os::windows::fs::FileExt::seek_read(file, buffer, offset)
}

#[cfg(windows)]
fn write_at(file: &File, buffer: &[u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_write(file, buffer, offset)
}

#[cfg(not(any(unix, windows)))]
fn read_at(_: &File, _: &mut [u8], _: u64) -> io::Result<usize> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(not(any(unix, windows)))]
fn write_at(_: &File, _: &[u8], _: u64) -> io::Result<usize> {
    Err(io::ErrorKind::Unsupported.into())
}

// errno values of the Windows C runtime (UCRT, which MinGW-w64 uses). These are the numbers the
// guest sees on Windows; the POSIX values differ for some of them.
const EIO: i32 = 5;
const EBADF: i32 = 9;
const ENOMEM: i32 = 12;
const EACCES: i32 = 13;
const EFAULT: i32 = 14;
const EEXIST: i32 = 17;
const ENOTDIR: i32 = 20;
const EINVAL: i32 = 22;
const EMFILE: i32 = 24;
const ENOSPC: i32 = 28;
const ENOENT: i32 = 2;
const ENAMETOOLONG: i32 = 38;
const ENOTEMPTY: i32 = 41;

/// errno for a Win32 error code, as `compat_errno_from_win32` mapped it. `ERROR_SUCCESS` and
/// `ERROR_HANDLE_EOF` (38) are 0; a code without a mapping is `EIO`.
pub fn errno_from_win32(error: u32) -> i32 {
    match error {
        0 | 38 => 0,
        // FILE_NOT_FOUND, PATH_NOT_FOUND, INVALID_DRIVE, BAD_NETPATH, BAD_NET_NAME, INVALID_NAME
        2 | 3 | 15 | 53 | 67 | 123 => ENOENT,
        // ACCESS_DENIED, WRITE_PROTECT, SHARING_VIOLATION, LOCK_VIOLATION
        5 | 19 | 32 | 33 => EACCES,
        // FILE_EXISTS, ALREADY_EXISTS
        80 | 183 => EEXIST,
        6 => EBADF,
        // NOT_ENOUGH_MEMORY, OUTOFMEMORY, COMMITMENT_LIMIT
        8 | 14 | 1455 => ENOMEM,
        // DISK_FULL, HANDLE_DISK_FULL
        112 | 39 => ENOSPC,
        145 => ENOTEMPTY,
        267 => ENOTDIR,
        // FILENAME_EXCED_RANGE, BUFFER_OVERFLOW
        206 | 111 => ENAMETOOLONG,
        4 => EMFILE,
        998 => EFAULT,
        // INVALID_PARAMETER, NEGATIVE_SEEK
        87 | 131 => EINVAL,
        _ => EIO,
    }
}

/// Sleeps for `ns` nanoseconds; 0 gives up the rest of the thread's time slice. Rust's standard
/// library sleeps on a high-resolution waitable timer on Windows, so the wait is not rounded to
/// the 15.6 ms scheduler tick that `nanosleep` in winpthreads rounds to.
pub fn sleep_ns(ns: u64) {
    if ns == 0 {
        std::thread::yield_now();
    } else {
        std::thread::sleep(Duration::from_nanos(ns));
    }
}

/// Seconds east of UTC at `seconds` since the Unix epoch, daylight saving included. The Windows
/// `struct tm` has no `tm_gmtoff`, so this is `time::utc_offset`, the one computation for both.
pub fn utc_offset(seconds: i64) -> i64 {
    crate::time::utc_offset(seconds)
}

/// Fills `buffer` with bytes from the operating system's cryptographic random source.
pub fn random_bytes(buffer: &mut [u8]) -> io::Result<()> {
    getrandom::fill(buffer).map_err(|_| io::Error::other("the OS random source failed"))
}

/// Removes `path` and everything under it, as the C shim's `compat_remove_tree` did. A path that
/// does not exist is success. Each entry is attempted even after a failure, so as much as
/// possible is removed; the first error is returned. Symbolic links and Windows reparse points
/// are removed themselves, never followed.
pub fn remove_tree(path: &Path) -> io::Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    clear_readonly(path, &meta);
    let mut first_error: Option<io::Error> = None;
    if meta.is_dir() {
        match fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries {
                    let result = entry.and_then(|entry| remove_tree(&entry.path()));
                    if let Err(error) = result {
                        first_error.get_or_insert(error);
                    }
                }
            }
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    let removed = if meta.is_dir() {
        fs::remove_dir(path)
    } else {
        // A directory symbolic link on Windows is removed with remove_dir, not remove_file.
        fs::remove_file(path).or_else(|error| fs::remove_dir(path).map_err(|_| error))
    };
    if let Err(error) = removed {
        first_error.get_or_insert(error);
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Windows refuses to delete read-only files and directories; the C shim cleared the attribute
/// first, and so does this. POSIX deletion depends on the directory, not the file's mode.
#[cfg(windows)]
// The lint warns that `set_readonly(false)` makes files world-writable on Unix; this is Windows only.
#[allow(clippy::permissions_set_readonly_false)]
fn clear_readonly(path: &Path, meta: &fs::Metadata) {
    if meta.permissions().readonly() {
        let mut permissions = meta.permissions();
        permissions.set_readonly(false);
        let _ = fs::set_permissions(path, permissions);
    }
}

#[cfg(not(windows))]
fn clear_readonly(_: &Path, _: &fs::Metadata) {}

/// User and kernel CPU time in microseconds of the calling thread (`thread`) or the process.
/// Only Windows reads these (GetThreadTimes and GetProcessTimes); other hosts get `Unsupported`.
pub fn cpu_times(thread: bool) -> io::Result<(i64, i64)> {
    let (user, kernel) = cpu_times_100ns(thread)?;
    // 100 ns units to microseconds.
    Ok(((user / 10) as i64, (kernel / 10) as i64))
}

#[cfg(windows)]
fn cpu_times_100ns(thread: bool) -> io::Result<(u64, u64)> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentThread, GetProcessTimes, GetThreadTimes,
    };
    let blank = || FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (blank(), blank(), blank(), blank());
    // SAFETY: each pseudo-handle names the calling process or thread; the four out-pointers are
    // live, writable FILETIME locals.
    let ok = unsafe {
        if thread {
            GetThreadTimes(
                GetCurrentThread(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        } else {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        }
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Ok((ticks(user), ticks(kernel)))
}

#[cfg(not(windows))]
fn cpu_times_100ns(_: bool) -> io::Result<(u64, u64)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "CPU times are read on Windows only",
    ))
}

// The C symbols of compat_win.h that this module replaces. They are declared for C in
// include/uncharted_runtime.h (written by cbindgen, checked by tests/header.rs).

/// errno for a Win32 error code (see [`errno_from_win32`]).
#[no_mangle]
pub extern "C" fn compat_errno_from_win32(error: u32) -> c_int {
    errno_from_win32(error)
}

/// Seconds east of UTC at `seconds` (see [`utc_offset`]).
#[no_mangle]
pub extern "C" fn compat_utc_offset(seconds: i64) -> i64 {
    utc_offset(seconds)
}

/// Cryptographic random bytes: 0 on success, -1 on failure.
///
/// # Safety
/// `buffer` must be valid for writes of `size` bytes, or `size` must be 0.
#[no_mangle]
pub unsafe extern "C" fn compat_random(buffer: *mut c_void, size: usize) -> c_int {
    if size == 0 {
        return 0;
    }
    if buffer.is_null() {
        return -1;
    }
    // SAFETY: the caller's contract gives `size` writable bytes at `buffer`.
    let bytes = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<u8>(), size) };
    match random_bytes(bytes) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Sleeps for `ns` nanoseconds (see [`sleep_ns`]).
#[no_mangle]
pub extern "C" fn compat_sleep_ns(ns: u64) {
    sleep_ns(ns);
}

/// Removes the tree at a NUL-terminated UTF-8 `path`: 0 when it is gone or never existed, -1
/// otherwise (see [`remove_tree`]).
///
/// # Safety
/// `path` must be null or point to a NUL-terminated string that stays valid during the call.
#[no_mangle]
pub unsafe extern "C" fn compat_remove_tree(path: *const c_char) -> c_int {
    if path.is_null() {
        return -1;
    }
    // SAFETY: the caller's contract gives a NUL-terminated string at `path`.
    let Ok(text) = unsafe { CStr::from_ptr(path) }.to_str() else {
        return -1;
    };
    match remove_tree(Path::new(text)) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// User (`who` == 0: process; otherwise the calling thread) and kernel CPU time in microseconds,
/// written to `user_us` and `kernel_us`. 0 on success, -1 on failure.
///
/// # Safety
/// `user_us` and `kernel_us` must each be null or valid for one `int64_t` write.
#[no_mangle]
pub unsafe extern "C" fn compat_cpu_times(
    who: c_int,
    user_us: *mut i64,
    kernel_us: *mut i64,
) -> c_int {
    if user_us.is_null() || kernel_us.is_null() {
        return -1;
    }
    match cpu_times(who != 0) {
        Ok((user, kernel)) => {
            // SAFETY: both pointers are non-null and valid for one i64 write, by the contract.
            unsafe {
                user_us.write(user);
                kernel_us.write(kernel);
            }
            0
        }
        Err(_) => -1,
    }
}

#[cfg(test)]
mod shim_tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn errno_map_matches_the_compat_table() {
        assert_eq!(errno_from_win32(0), 0);
        assert_eq!(errno_from_win32(38), 0);
        for code in [2, 3, 15, 53, 67, 123] {
            assert_eq!(errno_from_win32(code), ENOENT, "win32 {code}");
        }
        for code in [5, 19, 32, 33] {
            assert_eq!(errno_from_win32(code), EACCES, "win32 {code}");
        }
        assert_eq!(errno_from_win32(80), EEXIST);
        assert_eq!(errno_from_win32(183), EEXIST);
        assert_eq!(errno_from_win32(6), EBADF);
        for code in [8, 14, 1455] {
            assert_eq!(errno_from_win32(code), ENOMEM, "win32 {code}");
        }
        assert_eq!(errno_from_win32(112), ENOSPC);
        assert_eq!(errno_from_win32(39), ENOSPC);
        assert_eq!(errno_from_win32(145), ENOTEMPTY);
        assert_eq!(errno_from_win32(267), ENOTDIR);
        assert_eq!(errno_from_win32(206), ENAMETOOLONG);
        assert_eq!(errno_from_win32(111), ENAMETOOLONG);
        assert_eq!(errno_from_win32(4), EMFILE);
        assert_eq!(errno_from_win32(998), EFAULT);
        assert_eq!(errno_from_win32(87), EINVAL);
        assert_eq!(errno_from_win32(131), EINVAL);
        assert_eq!(errno_from_win32(9999), EIO);
        assert_eq!(compat_errno_from_win32(5), EACCES);
    }

    #[test]
    fn sleep_waits_at_least_the_requested_time() {
        let start = Instant::now();
        sleep_ns(2_000_000);
        assert!(start.elapsed() >= Duration::from_millis(2));
        sleep_ns(0);
    }

    #[test]
    fn utc_offset_is_a_real_zone_offset() {
        for seconds in [0, 1_700_000_000, 1_720_000_000] {
            let offset = utc_offset(seconds);
            assert!(
                (-14 * 3600..=14 * 3600).contains(&offset),
                "offset {offset}"
            );
            assert_eq!(compat_utc_offset(seconds), offset);
        }
    }

    #[test]
    fn random_bytes_are_filled_and_differ() {
        let mut first = [0u8; 32];
        let mut second = [0u8; 32];
        random_bytes(&mut first).unwrap();
        random_bytes(&mut second).unwrap();
        assert!(first.iter().any(|&byte| byte != 0));
        assert_ne!(first, second);
        let mut empty: [u8; 0] = [];
        assert_eq!(unsafe { compat_random(empty.as_mut_ptr().cast(), 0) }, 0);
        assert_eq!(unsafe { compat_random(std::ptr::null_mut(), 4) }, -1);
        let mut third = [0u8; 16];
        assert_eq!(
            unsafe { compat_random(third.as_mut_ptr().cast(), third.len()) },
            0
        );
        assert!(third.iter().any(|&byte| byte != 0));
    }

    #[test]
    fn remove_tree_removes_nested_entries_and_tolerates_absence() {
        let root = std::env::temp_dir().join(format!(
            "uncharted-os-tree-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/b/file.bin"), b"data").unwrap();
        fs::write(root.join("top.txt"), b"x").unwrap();
        remove_tree(&root).unwrap();
        assert!(!root.exists());
        remove_tree(&root).unwrap();

        let file = root.with_extension("single");
        fs::write(&file, b"y").unwrap();
        remove_tree(&file).unwrap();
        assert!(!file.exists());

        assert_eq!(unsafe { compat_remove_tree(std::ptr::null()) }, -1);
        let missing = std::ffi::CString::new(root.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(unsafe { compat_remove_tree(missing.as_ptr()) }, 0);
    }

    #[test]
    fn cpu_times_are_read_on_windows_only() {
        if cfg!(windows) {
            let (user, kernel) = cpu_times(false).unwrap();
            assert!(user >= 0 && kernel >= 0);
            let (thread_user, thread_kernel) = cpu_times(true).unwrap();
            assert!(thread_user >= 0 && thread_kernel >= 0);
            let (mut user_us, mut kernel_us) = (-1i64, -1i64);
            assert_eq!(
                unsafe { compat_cpu_times(1, &mut user_us, &mut kernel_us) },
                0
            );
            assert!(user_us >= 0 && kernel_us >= 0);
        } else {
            let error = cpu_times(false).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::Unsupported);
            let (mut user_us, mut kernel_us) = (0i64, 0i64);
            assert_eq!(
                unsafe { compat_cpu_times(0, &mut user_us, &mut kernel_us) },
                -1
            );
        }
        assert_eq!(
            unsafe { compat_cpu_times(0, std::ptr::null_mut(), std::ptr::null_mut()) },
            -1
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    const WORDS: u32 = 1 << 15; // 128 KiB of made-up data
    const BYTES: u64 = WORDS as u64 * 4;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("uu-os-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Word `i` of the file is `i` (little-endian), so every 4-byte read names its offset.
    fn counters() -> Vec<u8> {
        (0..WORDS).flat_map(u32::to_le_bytes).collect()
    }

    fn word(bytes: &[u8]) -> u32 {
        u32::from_le_bytes(bytes.try_into().unwrap())
    }

    fn open(path: &std::path::Path) -> Description {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        Description::new(file).unwrap()
    }

    /// A small deterministic generator, so every run tries the same interleaving seeds.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
    }

    /// The #39 check: two threads share one descriptor (as after `dup`), and each mixes
    /// sequential reads with positioned reads. The sequential reads must hand out every
    /// word exactly once, in order per thread; each positioned read must see its own offset.
    #[test]
    fn os_sequential_and_positioned_reads_from_two_threads_neither_skip_nor_repeat() {
        let dir = TempDir::new("mixed-reads");
        let path = dir.0.join("made-up.bin");
        fs::write(&path, counters()).unwrap();
        let shared = Arc::new(open(&path));
        let start = Arc::new(Barrier::new(2));

        let workers: Vec<_> = (0..2u64)
            .map(|seed| {
                let shared = Arc::clone(&shared);
                let start = Arc::clone(&start);
                thread::spawn(move || {
                    let mut random = Lcg(seed + 1);
                    let mut seen = Vec::new();
                    let mut chunk = [0u8; 64];
                    start.wait();
                    loop {
                        let words = 1 + random.next() as usize % 16;
                        let n = shared.read(&mut chunk[..words * 4]).unwrap();
                        if n == 0 {
                            break;
                        }
                        assert_eq!(n % 4, 0, "a regular file read stops only at the end");
                        seen.extend(chunk[..n].chunks(4).map(word));

                        let at = random.next() % WORDS as u64;
                        let mut probe = [0u8; 4];
                        assert_eq!(shared.read_at(&mut probe, at * 4).unwrap(), 4);
                        assert_eq!(word(&probe) as u64, at, "positioned read at word {at}");
                    }
                    seen
                })
            })
            .collect();

        let mut all = Vec::new();
        for worker in workers {
            let seen = worker.join().unwrap();
            assert!(seen.windows(2).all(|w| w[0] < w[1]), "out of order");
            all.extend(seen);
        }
        all.sort_unstable();
        assert_eq!(all.len(), WORDS as usize, "a word was skipped or repeated");
        assert!(all.iter().enumerate().all(|(i, &w)| w == i as u32));
        assert_eq!(shared.position(), BYTES);
    }

    /// Sequential writes from one thread and positioned writes from another, on one
    /// descriptor: the sequential stream lands contiguous, with nothing lost or overlapped.
    #[test]
    fn os_sequential_and_positioned_writes_from_two_threads_land_where_asked() {
        let dir = TempDir::new("mixed-writes");
        let path = dir.0.join("made-up.bin");
        fs::write(&path, b"").unwrap();
        let shared = Arc::new(open(&path));
        let start = Arc::new(Barrier::new(2));
        let done = Arc::new(AtomicBool::new(false));

        let positioned = {
            let (shared, start, done) =
                (Arc::clone(&shared), Arc::clone(&start), Arc::clone(&done));
            thread::spawn(move || {
                let mut random = Lcg(7);
                start.wait();
                // Rewrite words of a second region, after the sequential one, until it ends.
                while !done.load(Ordering::Relaxed) {
                    let at = WORDS as u64 + random.next() % WORDS as u64;
                    let bytes = (at as u32).to_le_bytes();
                    assert_eq!(shared.write_at(&bytes, at * 4).unwrap(), 4);
                }
            })
        };

        start.wait();
        for i in 0..WORDS {
            assert_eq!(shared.write(&i.to_le_bytes()).unwrap(), 4);
        }
        done.store(true, Ordering::Relaxed);
        positioned.join().unwrap();

        assert_eq!(shared.position(), BYTES);
        let written = fs::read(&path).unwrap();
        assert_eq!(&written[..BYTES as usize], &counters()[..]);
        for (i, w) in written[BYTES as usize..].chunks(4).enumerate() {
            let w = word(w);
            assert!(
                w == 0 || w == WORDS + i as u32,
                "word {i} of the second region is {w}"
            );
        }
    }

    #[test]
    fn os_positioned_calls_leave_the_shared_position_alone() {
        let dir = TempDir::new("position");
        let path = dir.0.join("made-up.bin");
        fs::write(&path, counters()).unwrap();
        let file = open(&path);

        let mut buffer = [0u8; 8];
        assert_eq!(file.read(&mut buffer).unwrap(), 8);
        assert_eq!((word(&buffer[..4]), word(&buffer[4..])), (0, 1));
        assert_eq!(file.position(), 8);

        assert_eq!(file.read_at(&mut buffer[..4], 400).unwrap(), 4);
        assert_eq!(word(&buffer[..4]), 100);
        assert_eq!(file.write_at(&9_999u32.to_le_bytes(), 800).unwrap(), 4);
        assert_eq!(file.position(), 8);

        assert_eq!(file.read(&mut buffer[..4]).unwrap(), 4);
        assert_eq!(
            word(&buffer[..4]),
            2,
            "the next sequential read follows the last one"
        );
        assert_eq!(file.write(&7_777u32.to_le_bytes()).unwrap(), 4);
        assert_eq!(file.position(), 16);

        assert_eq!(file.read_at(&mut buffer[..4], 12).unwrap(), 4);
        assert_eq!(word(&buffer[..4]), 7_777);
        assert_eq!(file.read_at(&mut buffer[..4], 800).unwrap(), 4);
        assert_eq!(word(&buffer[..4]), 9_999);
    }

    #[test]
    fn os_a_second_open_has_its_own_position() {
        let dir = TempDir::new("independent");
        let path = dir.0.join("made-up.bin");
        fs::write(&path, counters()).unwrap();
        let (first, second) = (open(&path), open(&path));

        let mut buffer = [0u8; 4];
        first.seek(SeekFrom::Start(40)).unwrap();
        assert_eq!(second.read(&mut buffer).unwrap(), 4);
        assert_eq!(word(&buffer), 0);
        assert_eq!(first.read(&mut buffer).unwrap(), 4);
        assert_eq!(word(&buffer), 10);
        assert_eq!((first.position(), second.position()), (44, 4));
    }

    #[test]
    fn os_new_starts_at_the_host_position() {
        use std::io::Seek;
        let dir = TempDir::new("start");
        let path = dir.0.join("made-up.bin");
        fs::write(&path, counters()).unwrap();
        let mut file = File::open(&path).unwrap();
        file.seek(SeekFrom::Start(20)).unwrap();
        let description = Description::new(file).unwrap();
        assert_eq!(description.position(), 20);
        let mut buffer = [0u8; 4];
        assert_eq!(description.read(&mut buffer).unwrap(), 4);
        assert_eq!(word(&buffer), 5);
    }

    #[test]
    fn os_seek_follows_lseek_and_refuses_a_negative_position() {
        let dir = TempDir::new("seek");
        let path = dir.0.join("made-up.bin");
        fs::write(&path, counters()).unwrap();
        let file = open(&path);

        assert_eq!(file.seek(SeekFrom::Start(100)).unwrap(), 100);
        assert_eq!(file.seek(SeekFrom::Current(-60)).unwrap(), 40);
        assert_eq!(file.seek(SeekFrom::Current(8)).unwrap(), 48);
        assert_eq!(file.seek(SeekFrom::End(-4)).unwrap(), BYTES - 4);
        let mut buffer = [0u8; 8];
        assert_eq!(
            file.read(&mut buffer).unwrap(),
            4,
            "a read stops at the end"
        );
        assert_eq!(word(&buffer[..4]), WORDS - 1);
        assert_eq!(file.read(&mut buffer).unwrap(), 0);

        for bad in [
            SeekFrom::Current(-(BYTES as i64) - 1),
            SeekFrom::End(-(BYTES as i64) - 1),
        ] {
            let error = file.seek(bad).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert_eq!(file.position(), BYTES, "a refused seek keeps the position");
        }

        // Past the end: reads find nothing, and a write there extends the file.
        assert_eq!(file.seek(SeekFrom::End(8)).unwrap(), BYTES + 8);
        assert_eq!(file.read(&mut buffer).unwrap(), 0);
        assert_eq!(file.read_at(&mut buffer, BYTES + 100).unwrap(), 0);
        assert_eq!(file.write(&1u32.to_le_bytes()).unwrap(), 4);
        assert_eq!(fs::metadata(&path).unwrap().len(), BYTES + 12);
    }

    /// An offset past `i64::MAX` is no `off_t`: `read_at` and `write_at` refuse it with
    /// `InvalidInput` (`EINVAL`) before any host call. Windows would take its bits as a
    /// negative offset, where -1 appends and -2 uses the handle's own file pointer.
    #[test]
    fn os_positioned_calls_refuse_an_offset_past_i64_max_before_the_host() {
        let dir = TempDir::new("offset-range");
        let path = dir.0.join("made-up.bin");
        fs::write(&path, counters()).unwrap();
        let file = open(&path);
        assert_eq!(file.seek(SeekFrom::Start(8)).unwrap(), 8);

        let mut buffer = [0u8; 4];
        let mut reached = Vec::new();
        for offset in [1 << 63, u64::MAX - 1, u64::MAX] {
            let read = file.read_at(&mut buffer, offset);
            let write = file.write_at(b"abcd", offset);
            for (call, result) in [("read_at", read), ("write_at", write)] {
                match result {
                    Err(e)
                        if e.kind() == io::ErrorKind::InvalidInput
                            && e.raw_os_error().is_none() => {}
                    other => reached.push(format!("{call} at {offset:#x}: {other:?}")),
                }
            }
        }
        let after = fs::read(&path).unwrap();
        if after != counters() {
            let first = after.iter().zip(counters()).position(|(a, b)| *a != b);
            reached.push(format!(
                "file: {} bytes, first change at {first:?}",
                after.len()
            ));
        }
        assert!(reached.is_empty(), "reached the host: {reached:#?}");
        assert_eq!(file.position(), 8);
    }
}
