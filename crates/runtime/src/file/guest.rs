//! The guest's file calls, `src/runtime_file.c`'s export table: each `sceKernel*` function
//! returns an error as `0x8002xxxx` (an `int32` widened to 64 bits), its POSIX twin -1 with the
//! guest's errno set. The same symbol from libkernel or libScePosix is the same contract, so
//! [`lookup`] goes by symbol name. Guest code calls these with the System V ABI.

use super::{Errno, GuestStat, Result};
use std::ffi::{c_char, c_int, c_void, CStr};

/// `sceKernel*` convention.
fn sce(result: Result<i64>) -> i64 {
    match result {
        Ok(value) => value,
        Err(Errno(e)) => i64::from((0x8002_0000u32 | e as u32) as i32),
    }
}

/// POSIX convention.
fn posix(result: Result<i64>) -> i64 {
    match result {
        Ok(value) => value,
        Err(Errno(e)) => {
            set_errno(e);
            -1
        }
    }
}

/// The guest thread's errno: `runtime_errno` (`runtime_thread.c`) in the loader.
#[cfg(uncharted_loader)]
fn set_errno(value: i32) {
    extern "C" {
        fn runtime_errno() -> *mut i32;
    }
    // SAFETY: the calling guest thread's errno slot, valid for the thread's life.
    unsafe { *runtime_errno() = value };
}

#[cfg(not(uncharted_loader))]
thread_local! {
    static ERRNO: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

/// Without the loader (the tests) errno is a thread-local of this module.
#[cfg(not(uncharted_loader))]
fn set_errno(value: i32) {
    ERRNO.with(|errno| errno.set(value));
}

#[cfg(all(test, not(uncharted_loader)))]
pub(super) fn errno() -> i32 {
    ERRNO.with(std::cell::Cell::get)
}

/// A guest path argument; null is `EFAULT` (the C file read through it).
///
/// # Safety
/// `path` is null or a NUL-terminated string that outlives the call.
unsafe fn path<'a>(path: *const c_char) -> Result<&'a [u8]> {
    if path.is_null() {
        return Err(Errno::EFAULT);
    }
    // SAFETY: a NUL-terminated string (caller).
    Ok(unsafe { CStr::from_ptr(path) }.to_bytes())
}

fn ok(result: Result<()>) -> Result<i64> {
    result.map(|()| 0)
}

// Each body is the operation once; the macro makes the `sce_` and `posix_` entry points.
macro_rules! pair {
    ($sce:ident, $posix:ident, ($($arg:ident: $ty:ty),*) $body:block) => {
        /// # Safety
        /// Called by guest code with the arguments of the C function.
        unsafe fn $posix($($arg: $ty),*) -> Result<i64> $body
        unsafe extern "sysv64" fn $sce($($arg: $ty),*) -> i64 {
            // SAFETY: forwarded from the guest.
            sce(unsafe { $posix($($arg),*) })
        }
    };
}

pair!(sce_open, open, (p: *const c_char, flags: c_int, mode: c_int) {
    // SAFETY: forwarded from the guest.
    super::open(unsafe { path(p) }?, flags, mode).map(i64::from)
});
pair!(sce_close, close, (fd: c_int) { ok(super::close(fd)) });
pair!(sce_read, read, (fd: c_int, buffer: *mut c_void, size: u64) {
    // SAFETY: forwarded from the guest.
    unsafe { super::read_into(fd, buffer.cast(), size, -1) }.map(|n| n as i64)
});
pair!(sce_pread, pread, (fd: c_int, buffer: *mut c_void, size: u64, offset: i64) {
    // SAFETY: forwarded from the guest.
    unsafe { super::read_into(fd, buffer.cast(), size, offset) }.map(|n| n as i64)
});
pair!(sce_write, write, (fd: c_int, buffer: *const c_void, size: u64) {
    // SAFETY: forwarded from the guest.
    unsafe { super::write_from(fd, buffer.cast(), size, None) }.map(|n| n as i64)
});
pair!(sce_pwrite, pwrite, (fd: c_int, buffer: *const c_void, size: u64, offset: i64) {
    // SAFETY: forwarded from the guest.
    unsafe { super::write_from(fd, buffer.cast(), size, Some(offset)) }
        .map(|n| n as i64)
});
pair!(sce_lseek, lseek, (fd: c_int, offset: i64, whence: c_int) {
    super::lseek(fd, offset, whence)
});
pair!(sce_fstat, fstat, (fd: c_int, out: *mut GuestStat) {
    super::target(fd)?;
    // SAFETY: forwarded from the guest.
    unsafe { super::ffi::put_stat(out.cast(), || super::fstat(fd)) }
});
pair!(sce_stat, stat, (p: *const c_char, out: *mut GuestStat) {
    // SAFETY: forwarded from the guest.
    let guest = unsafe { path(p) }?;
    super::translate(guest)?;
    // SAFETY: forwarded from the guest.
    unsafe { super::ffi::put_stat(out.cast(), || super::stat(guest)) }
});
pair!(sce_getdents, getdents, (fd: c_int, buffer: *mut c_char, size: u64, base: *mut i64) {
    // SAFETY: forwarded from the guest.
    unsafe { super::getdents_raw(fd, buffer.cast(), size, base) }
});
pair!(sce_rename, rename, (from: *const c_char, to: *const c_char) {
    // SAFETY: forwarded from the guest.
    let (from, to) = unsafe { (path(from)?, path(to)?) };
    ok(super::rename(from, to))
});
pair!(sce_ftruncate, ftruncate, (fd: c_int, size: i64) {
    ok(super::ftruncate(fd, size))
});
pair!(sce_truncate, truncate, (p: *const c_char, size: i64) {
    // SAFETY: forwarded from the guest.
    ok(super::truncate(unsafe { path(p) }?, size))
});
pair!(sce_fsync, fsync, (fd: c_int) { ok(super::fsync(fd)) });
pair!(sce_mkdir, mkdir, (p: *const c_char, mode: c_int) {
    // SAFETY: forwarded from the guest.
    ok(super::path_op(unsafe { path(p) }?, super::PathOp::Mkdir(mode)))
});
pair!(sce_rmdir, rmdir, (p: *const c_char) {
    // SAFETY: forwarded from the guest.
    ok(super::path_op(unsafe { path(p) }?, super::PathOp::Rmdir))
});
pair!(sce_unlink, unlink, (p: *const c_char) {
    // SAFETY: forwarded from the guest.
    ok(super::path_op(unsafe { path(p) }?, super::PathOp::Unlink))
});

/// # Safety
/// As the C function: `p` is a guest path.
unsafe fn access(p: *const c_char, mode: c_int) -> Result<i64> {
    // SAFETY: forwarded from the guest.
    ok(super::access(unsafe { path(p) }?, mode))
}

macro_rules! posix_entry {
    ($name:ident => $operation:ident ($($arg:ident: $ty:ty),*)) => {
        unsafe extern "sysv64" fn $name($($arg: $ty),*) -> i64 {
            // SAFETY: forwarded from the guest.
            posix(unsafe { $operation($($arg),*) })
        }
    };
}

posix_entry!(posix_open => open(p: *const c_char, flags: c_int, mode: c_int));
posix_entry!(posix_close => close(fd: c_int));
posix_entry!(posix_read => read(fd: c_int, buffer: *mut c_void, size: u64));
posix_entry!(posix_pread => pread(fd: c_int, buffer: *mut c_void, size: u64, offset: i64));
posix_entry!(posix_write => write(fd: c_int, buffer: *const c_void, size: u64));
posix_entry!(posix_pwrite => pwrite(fd: c_int, buffer: *const c_void, size: u64, offset: i64));
posix_entry!(posix_lseek => lseek(fd: c_int, offset: i64, whence: c_int));
posix_entry!(posix_fstat => fstat(fd: c_int, out: *mut GuestStat));
posix_entry!(posix_stat => stat(p: *const c_char, out: *mut GuestStat));
posix_entry!(posix_getdents => getdents(fd: c_int, buffer: *mut c_char, size: u64, base: *mut i64));
posix_entry!(posix_rename => rename(from: *const c_char, to: *const c_char));
posix_entry!(posix_ftruncate => ftruncate(fd: c_int, size: i64));
posix_entry!(posix_truncate => truncate(p: *const c_char, size: i64));
posix_entry!(posix_fsync => fsync(fd: c_int));
posix_entry!(posix_access => access(p: *const c_char, mode: c_int));
posix_entry!(posix_mkdir => mkdir(p: *const c_char, mode: c_int));
posix_entry!(posix_rmdir => rmdir(p: *const c_char));
posix_entry!(posix_unlink => unlink(p: *const c_char));

/// `sceKernelCheckReachability`: whether the path exists, as an `int32` result.
unsafe extern "sysv64" fn sce_check_reachability(p: *const c_char) -> i32 {
    let mut out = GuestStat::default();
    // SAFETY: forwarded from the guest; `out` is a local.
    sce(unsafe { stat(p, &mut out) }) as i32
}

/// `sceKernelUtimes`: the times are left to the host file system; the path must exist.
unsafe extern "sysv64" fn sce_utimes(p: *const c_char, times: *const c_void) -> i32 {
    let _ = times;
    // SAFETY: forwarded from the guest.
    unsafe { sce_check_reachability(p) }
}

/// The function behind exported `symbol`, `src/runtime_file.c`'s table.
pub(super) fn lookup(symbol: &[u8]) -> Option<usize> {
    Some(match symbol {
        b"sceKernelOpen" => sce_open as *const () as usize,
        b"open" | b"_open" => posix_open as *const () as usize,
        b"sceKernelClose" => sce_close as *const () as usize,
        b"close" | b"_close" => posix_close as *const () as usize,
        b"sceKernelRead" => sce_read as *const () as usize,
        b"read" | b"_read" => posix_read as *const () as usize,
        b"sceKernelPread" => sce_pread as *const () as usize,
        b"pread" => posix_pread as *const () as usize,
        b"sceKernelWrite" => sce_write as *const () as usize,
        b"write" | b"_write" => posix_write as *const () as usize,
        b"sceKernelPwrite" => sce_pwrite as *const () as usize,
        b"pwrite" => posix_pwrite as *const () as usize,
        b"sceKernelLseek" => sce_lseek as *const () as usize,
        b"lseek" => posix_lseek as *const () as usize,
        b"sceKernelFstat" => sce_fstat as *const () as usize,
        b"fstat" => posix_fstat as *const () as usize,
        b"sceKernelStat" => sce_stat as *const () as usize,
        b"stat" => posix_stat as *const () as usize,
        b"sceKernelGetdirentries" => sce_getdents as *const () as usize,
        b"getdirentries" => posix_getdents as *const () as usize,
        b"sceKernelRename" => sce_rename as *const () as usize,
        b"rename" => posix_rename as *const () as usize,
        b"sceKernelFtruncate" => sce_ftruncate as *const () as usize,
        b"ftruncate" => posix_ftruncate as *const () as usize,
        b"sceKernelTruncate" => sce_truncate as *const () as usize,
        b"truncate" => posix_truncate as *const () as usize,
        b"sceKernelFsync" => sce_fsync as *const () as usize,
        b"fsync" => posix_fsync as *const () as usize,
        b"access" => posix_access as *const () as usize,
        b"sceKernelCheckReachability" => sce_check_reachability as *const () as usize,
        b"sceKernelMkdir" => sce_mkdir as *const () as usize,
        b"mkdir" => posix_mkdir as *const () as usize,
        b"sceKernelRmdir" => sce_rmdir as *const () as usize,
        b"rmdir" => posix_rmdir as *const () as usize,
        b"sceKernelUnlink" => sce_unlink as *const () as usize,
        b"unlink" => posix_unlink as *const () as usize,
        b"sceKernelUtimes" => sce_utimes as *const () as usize,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conventions() {
        assert_eq!(sce(Ok(7)), 7);
        assert_eq!(sce(Err(Errno::ENOENT)), 0xffff_ffff_8002_0002u64 as i64);
        assert_eq!(sce(Err(Errno::EROFS)) as i32 as u32, 0x8002_001e);
        assert_eq!(posix(Ok(3)), 3);
        assert_eq!(posix(Err(Errno::EACCES)), -1);
        #[cfg(not(uncharted_loader))]
        assert_eq!(errno(), 13);
    }

    #[test]
    fn every_name_of_the_c_table_resolves() {
        for name in [
            "sceKernelOpen",
            "open",
            "_open",
            "sceKernelClose",
            "close",
            "_close",
            "sceKernelRead",
            "read",
            "_read",
            "sceKernelPread",
            "pread",
            "sceKernelWrite",
            "write",
            "_write",
            "sceKernelPwrite",
            "pwrite",
            "sceKernelLseek",
            "lseek",
            "sceKernelFstat",
            "fstat",
            "sceKernelStat",
            "stat",
            "sceKernelGetdirentries",
            "getdirentries",
            "sceKernelRename",
            "rename",
            "sceKernelFtruncate",
            "ftruncate",
            "sceKernelTruncate",
            "truncate",
            "sceKernelFsync",
            "fsync",
            "access",
            "sceKernelCheckReachability",
            "sceKernelMkdir",
            "mkdir",
            "sceKernelRmdir",
            "rmdir",
            "sceKernelUnlink",
            "unlink",
            "sceKernelUtimes",
        ] {
            assert!(lookup(name.as_bytes()).is_some(), "{name}");
        }
        assert_eq!(lookup(b"sceKernelOpen"), lookup(b"sceKernelOpen"));
        assert_ne!(lookup(b"sceKernelOpen"), lookup(b"open"));
        assert_eq!(lookup(b"open"), lookup(b"_open"));
        assert_eq!(lookup(b"sceKernelMmap"), None);
        assert_eq!(lookup(b""), None);
    }
}
