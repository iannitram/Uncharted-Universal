//! The C symbols of `src/compat_win.h` that `compat_win.c` defined, Windows only: the C
//! runtime's `pread`, `pwrite`, zone offset, random bytes, sleep, `rm -rf`, CPU times and
//! Win32 error map come from here, declared for C in `include/uncharted_runtime.h`.
//! `compat_win.h` keeps the macros and inline shims that point the POSIX names at them.

use std::ffi::{c_char, c_int, c_long, c_ulong, c_void, CStr};
use std::fs::File;
use std::io::{self, Seek, SeekFrom};
use std::mem::ManuallyDrop;
use std::os::windows::fs::FileExt;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::Path;

/// What this module calls in the CRT, not something it exports.
/// cbindgen:ignore
mod crt {
    extern "C" {
        /// The CRT's `errno` of the calling thread.
        pub fn _errno() -> *mut std::ffi::c_int;
    }
}

fn set_errno(value: c_int) {
    // SAFETY: the CRT's per-thread `errno` slot is always valid to write.
    unsafe { *crt::_errno() = value };
}

/// The `errno` for an I/O error from a Win32 call; `EIO` when it carries no Win32 code.
fn errno_of(error: &io::Error) -> c_int {
    error
        .raw_os_error()
        .map_or(libc::EIO, |code| super::errno_from_win32(code as u32))
}

/// The C library's `errno` for a Win32 error code.
#[no_mangle]
pub extern "C" fn compat_errno_from_win32(error: c_ulong) -> c_int {
    super::errno_from_win32(error)
}

/// The largest piece of one `ReadFile` or `WriteFile` (`compat_win.c`'s 1 GiB).
const PIECE: usize = 0x4000_0000;

/// `pread`/`pwrite` on a CRT descriptor: positioned I/O, then the handle's file pointer put
/// back where it was, as `compat_win.c` did. Another thread's `read` on the same descriptor
/// between the two can still see the moved pointer (#39); the guest file layer leaves this
/// path when it moves to [`super::Description`] (3.09). Returns the bytes moved, or -1 with
/// `errno` set. A failure after some bytes moved returns those bytes.
///
/// # Safety
/// `buffer` is `size` bytes the call may read (`pwrite`) or write (`pread`).
unsafe fn positioned(
    fd: c_int,
    size: usize,
    offset: i64,
    mut call: impl FnMut(&File, usize, usize, u64) -> io::Result<usize>,
) -> isize {
    // SAFETY: `_get_osfhandle` only looks the descriptor up.
    let handle = unsafe { libc::get_osfhandle(fd) };
    if handle == -1 {
        set_errno(libc::EBADF);
        return -1;
    }
    let Ok(offset) = u64::try_from(offset) else {
        set_errno(libc::EINVAL);
        return -1;
    };
    // SAFETY: the CRT owns the handle and keeps it open for the call; `ManuallyDrop` never
    // closes it.
    let owned_by_the_crt = ManuallyDrop::new(unsafe { File::from_raw_handle(handle as RawHandle) });
    let mut file: &File = &owned_by_the_crt;
    let saved = match file.stream_position() {
        Ok(saved) => saved,
        Err(error) => {
            set_errno(errno_of(&error));
            return -1;
        }
    };
    let mut done = 0;
    let mut failed = None;
    while done < size {
        let piece = (size - done).min(PIECE);
        match call(file, done, piece, offset + done as u64) {
            Ok(n) => {
                done += n;
                if n < piece {
                    break;
                }
            }
            Err(error) => {
                failed = Some(error);
                break;
            }
        }
    }
    let _ = file.seek(SeekFrom::Start(saved));
    match failed {
        Some(error) if done == 0 => {
            set_errno(errno_of(&error));
            -1
        }
        _ => done as isize,
    }
}

/// `pread` on a CRT descriptor: reads up to `size` bytes at `offset` into `buffer` and leaves
/// the descriptor's offset where it was. 0 at the end of the file; -1 with `errno` set.
///
/// # Safety
/// `buffer` is `size` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn compat_pread(
    fd: c_int,
    buffer: *mut c_void,
    size: usize,
    offset: i64,
) -> isize {
    let buffer = buffer.cast::<u8>();
    // SAFETY: forwarded; each piece lies inside the caller's `size` bytes at `buffer`.
    unsafe {
        positioned(fd, size, offset, |file, done, piece, at| {
            let piece = std::slice::from_raw_parts_mut(buffer.add(done), piece);
            // End of file reads as 0 bytes (ERROR_HANDLE_EOF).
            file.seek_read(piece, at)
        })
    }
}

/// `pwrite` on a CRT descriptor: writes `size` bytes of `buffer` at `offset` and leaves the
/// descriptor's offset where it was. -1 with `errno` set.
///
/// # Safety
/// `buffer` is `size` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn compat_pwrite(
    fd: c_int,
    buffer: *const c_void,
    size: usize,
    offset: i64,
) -> isize {
    let buffer = buffer.cast::<u8>();
    // SAFETY: forwarded; each piece lies inside the caller's `size` bytes at `buffer`.
    unsafe {
        positioned(fd, size, offset, |file, done, piece, at| {
            file.seek_write(std::slice::from_raw_parts(buffer.add(done), piece), at)
        })
    }
}

/// Seconds east of UTC at time `t` (Windows' `struct tm` has no `tm_gmtoff`).
#[no_mangle]
pub extern "C" fn compat_utc_offset(t: i64) -> c_long {
    super::utc_offset(t)
}

/// Fills `buffer` with `size` cryptographically secure random bytes; 0, or -1 on failure.
///
/// # Safety
/// `buffer` is `size` writable bytes (or `size` is 0).
#[no_mangle]
pub unsafe extern "C" fn compat_random(buffer: *mut c_void, size: usize) -> c_int {
    if size == 0 {
        return 0;
    }
    // SAFETY: the caller passes `size` writable bytes.
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<u8>(), size) };
    match super::random(buffer) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Sleeps `ns` nanoseconds with sub-millisecond precision (a high-resolution waitable timer;
/// winpthreads' `nanosleep` rounds up to the 15.6 ms scheduler tick).
#[no_mangle]
pub extern "C" fn compat_sleep_ns(ns: u64) {
    super::sleep_ns(ns);
}

/// `rm -rf` of a UTF-8 path: 0 when the tree is gone (or never existed), -1 when something
/// stayed (or the path is not UTF-8).
///
/// # Safety
/// `path` is null or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn compat_remove_tree(path: *const c_char) -> c_int {
    // `GetFileAttributesA(NULL)` failed, which `compat_win.c` took for "nothing there".
    if path.is_null() {
        return 0;
    }
    // SAFETY: the caller passes a NUL-terminated string.
    let Ok(path) = unsafe { CStr::from_ptr(path) }.to_str() else {
        return -1;
    };
    match super::remove_tree(Path::new(path)) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// User and kernel CPU time of the process (`who` 0) or the calling thread (any other
/// `who`), in microseconds; 0, or -1 when the host would not say.
///
/// # Safety
/// `user_us` and `kernel_us` are writable.
#[no_mangle]
pub unsafe extern "C" fn compat_cpu_times(
    who: c_int,
    user_us: *mut i64,
    kernel_us: *mut i64,
) -> c_int {
    let of = if who == 0 {
        super::CpuClock::Process
    } else {
        super::CpuClock::Thread
    };
    match super::cpu_times(of) {
        Ok(times) => {
            // SAFETY: the caller passes two writable `int64_t`s.
            unsafe {
                *user_us = times.user_us;
                *kernel_us = times.kernel_us;
            }
            0
        }
        Err(_) => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::IntoRawHandle;
    use std::path::PathBuf;

    fn errno() -> c_int {
        // SAFETY: the CRT's per-thread `errno` slot.
        unsafe { *crt::_errno() }
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("uu-os-ffi-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A CRT descriptor for a new made-up file, as the C runtime's `open` gives one.
    fn crt_descriptor(path: &Path, content: &[u8]) -> c_int {
        std::fs::write(path, content).unwrap();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        // SAFETY: the CRT takes the handle over; `_close` closes it.
        let fd = unsafe { libc::open_osfhandle(file.into_raw_handle() as isize, 0) };
        assert!(fd >= 0);
        fd
    }

    fn crt_position(fd: c_int) -> i64 {
        // SAFETY: a live descriptor.
        unsafe { libc::lseek64(fd, 0, libc::SEEK_CUR) }
    }

    /// `test_file_mods.c` reads mods through `pread`; this is the contract it relies on: the
    /// bytes at the offset, the descriptor's own offset untouched, 0 at the end, a short read
    /// across the end, and `EINVAL` for a negative offset.
    #[test]
    fn os_compat_pread_and_pwrite_leave_the_descriptor_offset_alone() {
        let dir = TempDir::new("pread");
        let fd = crt_descriptor(&dir.0.join("made-up.bin"), b"0123456789abcdef");
        let mut buffer = [0u8; 4];
        // SAFETY: `buffer` is 4 bytes; the strings are as long as their sizes.
        unsafe {
            assert_eq!(libc::lseek64(fd, 3, libc::SEEK_SET), 3);
            assert_eq!(compat_pread(fd, buffer.as_mut_ptr().cast(), 4, 10), 4);
            assert_eq!(&buffer, b"abcd");
            assert_eq!(crt_position(fd), 3);

            assert_eq!(compat_pwrite(fd, b"WXYZ".as_ptr().cast(), 4, 2), 4);
            assert_eq!(crt_position(fd), 3);
            assert_eq!(compat_pread(fd, buffer.as_mut_ptr().cast(), 4, 1), 4);
            assert_eq!(&buffer, b"1WXY");

            assert_eq!(compat_pread(fd, buffer.as_mut_ptr().cast(), 4, 14), 2);
            assert_eq!(&buffer[..2], b"ef");
            assert_eq!(compat_pread(fd, buffer.as_mut_ptr().cast(), 4, 16), 0);
            assert_eq!(compat_pread(fd, buffer.as_mut_ptr().cast(), 4, 1_000), 0);
            assert_eq!(compat_pread(fd, buffer.as_mut_ptr().cast(), 0, 0), 0);

            // Past the end: the file grows, zero-filled.
            assert_eq!(compat_pwrite(fd, b"!".as_ptr().cast(), 1, 20), 1);
            assert_eq!(crt_position(fd), 3);

            set_errno(0);
            assert_eq!(compat_pread(fd, buffer.as_mut_ptr().cast(), 4, -1), -1);
            assert_eq!(errno(), libc::EINVAL);
            set_errno(0);
            assert_eq!(compat_pwrite(fd, b"x".as_ptr().cast(), 1, -5), -1);
            assert_eq!(errno(), libc::EINVAL);

            assert_eq!(libc::close(fd), 0);
        }
        let content = std::fs::read(dir.0.join("made-up.bin")).unwrap();
        assert_eq!(content, b"01WXYZ6789abcdef\0\0\0\0!");
    }

    #[test]
    fn os_compat_pwrite_to_a_read_only_descriptor_sets_errno() {
        let dir = TempDir::new("read-only");
        let path = dir.0.join("made-up.bin");
        std::fs::write(&path, b"made up").unwrap();
        let handle = File::open(&path).unwrap().into_raw_handle();
        // SAFETY: the CRT takes the handle over.
        let fd = unsafe { libc::open_osfhandle(handle as isize, libc::O_RDONLY) };
        set_errno(0);
        // SAFETY: a live descriptor and a 1-byte string.
        unsafe {
            assert_eq!(compat_pwrite(fd, b"x".as_ptr().cast(), 1, 0), -1);
            assert_eq!(errno(), libc::EACCES);
            assert_eq!(libc::close(fd), 0);
        }
    }

    #[test]
    fn os_compat_exports_forward_to_the_rust_shims() {
        assert_eq!(compat_errno_from_win32(2), libc::ENOENT);
        assert_eq!(compat_errno_from_win32(38), 0);
        assert_eq!(compat_utc_offset(1_721_044_800), super::super::utc_offset(1_721_044_800));
        compat_sleep_ns(1_000);

        let mut bytes = [0u8; 32];
        // SAFETY: `bytes` is 32 writable bytes; a null buffer of size 0 is accepted.
        unsafe {
            assert_eq!(compat_random(bytes.as_mut_ptr().cast(), 32), 0);
            assert_eq!(compat_random(std::ptr::null_mut(), 0), 0);
        }
        assert_ne!(bytes, [0; 32]);

        let (mut user, mut kernel) = (-1i64, -1i64);
        // SAFETY: two live locals.
        unsafe {
            assert_eq!(compat_cpu_times(0, &mut user, &mut kernel), 0);
            assert!(user >= 0 && kernel >= 0);
            assert_eq!(compat_cpu_times(1, &mut user, &mut kernel), 0);
            assert!(user >= 0 && kernel >= 0);
        }

        let dir = TempDir::new("remove");
        let tree = dir.0.join("tree");
        std::fs::create_dir_all(tree.join("a/b")).unwrap();
        std::fs::write(tree.join("a/b/made-up.bin"), b"made up").unwrap();
        let c_path = std::ffi::CString::new(tree.to_str().unwrap()).unwrap();
        // SAFETY: NUL-terminated strings and null.
        unsafe {
            assert_eq!(compat_remove_tree(c_path.as_ptr()), 0);
            assert_eq!(compat_remove_tree(c_path.as_ptr()), 0, "already gone");
            assert_eq!(compat_remove_tree(std::ptr::null()), 0);
        }
        assert!(!tree.exists());
    }
}
