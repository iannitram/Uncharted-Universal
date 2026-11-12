//! The C library's `errno` for a Win32 error code (`compat_errno_from_win32` of
//! `compat_win.c`). A pure table: it runs on every host, with that host's `errno` numbers, and
//! the Windows build hands the CRT's numbers to C.

/// Win32 error codes the table names (`winerror.h`). Plain numbers, so the table is the same
/// on every host; the Windows tests check them against `windows-sys`. Not in the C header:
/// `<windows.h>` has them.
/// cbindgen:ignore
pub(crate) mod win32 {
    pub const ERROR_SUCCESS: u32 = 0;
    pub const ERROR_FILE_NOT_FOUND: u32 = 2;
    pub const ERROR_PATH_NOT_FOUND: u32 = 3;
    pub const ERROR_TOO_MANY_OPEN_FILES: u32 = 4;
    pub const ERROR_ACCESS_DENIED: u32 = 5;
    pub const ERROR_INVALID_HANDLE: u32 = 6;
    pub const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
    pub const ERROR_OUTOFMEMORY: u32 = 14;
    pub const ERROR_INVALID_DRIVE: u32 = 15;
    pub const ERROR_WRITE_PROTECT: u32 = 19;
    pub const ERROR_SHARING_VIOLATION: u32 = 32;
    pub const ERROR_LOCK_VIOLATION: u32 = 33;
    pub const ERROR_HANDLE_EOF: u32 = 38;
    pub const ERROR_HANDLE_DISK_FULL: u32 = 39;
    pub const ERROR_BAD_NETPATH: u32 = 53;
    pub const ERROR_BAD_NET_NAME: u32 = 67;
    pub const ERROR_FILE_EXISTS: u32 = 80;
    pub const ERROR_INVALID_PARAMETER: u32 = 87;
    pub const ERROR_BUFFER_OVERFLOW: u32 = 111;
    pub const ERROR_DISK_FULL: u32 = 112;
    pub const ERROR_INVALID_NAME: u32 = 123;
    pub const ERROR_NEGATIVE_SEEK: u32 = 131;
    pub const ERROR_DIR_NOT_EMPTY: u32 = 145;
    pub const ERROR_ALREADY_EXISTS: u32 = 183;
    pub const ERROR_FILENAME_EXCED_RANGE: u32 = 206;
    pub const ERROR_DIRECTORY: u32 = 267;
    pub const ERROR_NOACCESS: u32 = 998;
    pub const ERROR_COMMITMENT_LIMIT: u32 = 1455;
}

/// The `errno` for Win32 error `error`: 0 for success and for end of file, `EIO` for any code
/// the table does not name.
pub fn errno_from_win32(error: u32) -> i32 {
    use win32::*;
    match error {
        ERROR_SUCCESS | ERROR_HANDLE_EOF => 0,
        ERROR_FILE_NOT_FOUND
        | ERROR_PATH_NOT_FOUND
        | ERROR_INVALID_DRIVE
        | ERROR_BAD_NETPATH
        | ERROR_BAD_NET_NAME
        | ERROR_INVALID_NAME => libc::ENOENT,
        ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION
        | ERROR_WRITE_PROTECT => libc::EACCES,
        ERROR_FILE_EXISTS | ERROR_ALREADY_EXISTS => libc::EEXIST,
        ERROR_INVALID_HANDLE => libc::EBADF,
        ERROR_NOT_ENOUGH_MEMORY | ERROR_OUTOFMEMORY | ERROR_COMMITMENT_LIMIT => libc::ENOMEM,
        ERROR_DISK_FULL | ERROR_HANDLE_DISK_FULL => libc::ENOSPC,
        ERROR_DIR_NOT_EMPTY => libc::ENOTEMPTY,
        ERROR_DIRECTORY => libc::ENOTDIR,
        ERROR_FILENAME_EXCED_RANGE | ERROR_BUFFER_OVERFLOW => libc::ENAMETOOLONG,
        ERROR_TOO_MANY_OPEN_FILES => libc::EMFILE,
        ERROR_NOACCESS => libc::EFAULT,
        ERROR_NEGATIVE_SEEK | ERROR_INVALID_PARAMETER => libc::EINVAL,
        _ => libc::EIO,
    }
}

#[cfg(test)]
mod tests {
    use super::win32::*;
    use super::*;

    /// Every row of `compat_errno_from_win32`'s switch, and two codes it does not name.
    #[test]
    fn os_errno_from_win32_follows_the_compat_win_table() {
        let table = [
            (ERROR_SUCCESS, 0),
            (ERROR_FILE_NOT_FOUND, libc::ENOENT),
            (ERROR_PATH_NOT_FOUND, libc::ENOENT),
            (ERROR_INVALID_DRIVE, libc::ENOENT),
            (ERROR_BAD_NETPATH, libc::ENOENT),
            (ERROR_BAD_NET_NAME, libc::ENOENT),
            (ERROR_INVALID_NAME, libc::ENOENT),
            (ERROR_ACCESS_DENIED, libc::EACCES),
            (ERROR_SHARING_VIOLATION, libc::EACCES),
            (ERROR_LOCK_VIOLATION, libc::EACCES),
            (ERROR_WRITE_PROTECT, libc::EACCES),
            (ERROR_FILE_EXISTS, libc::EEXIST),
            (ERROR_ALREADY_EXISTS, libc::EEXIST),
            (ERROR_INVALID_HANDLE, libc::EBADF),
            (ERROR_NOT_ENOUGH_MEMORY, libc::ENOMEM),
            (ERROR_OUTOFMEMORY, libc::ENOMEM),
            (ERROR_COMMITMENT_LIMIT, libc::ENOMEM),
            (ERROR_DISK_FULL, libc::ENOSPC),
            (ERROR_HANDLE_DISK_FULL, libc::ENOSPC),
            (ERROR_DIR_NOT_EMPTY, libc::ENOTEMPTY),
            (ERROR_DIRECTORY, libc::ENOTDIR),
            (ERROR_FILENAME_EXCED_RANGE, libc::ENAMETOOLONG),
            (ERROR_BUFFER_OVERFLOW, libc::ENAMETOOLONG),
            (ERROR_TOO_MANY_OPEN_FILES, libc::EMFILE),
            (ERROR_NOACCESS, libc::EFAULT),
            (ERROR_HANDLE_EOF, 0),
            (ERROR_NEGATIVE_SEEK, libc::EINVAL),
            (ERROR_INVALID_PARAMETER, libc::EINVAL),
            // Not in the table: ERROR_NOT_SUPPORTED, ERROR_SEM_TIMEOUT.
            (50, libc::EIO),
            (121, libc::EIO),
            (u32::MAX, libc::EIO),
        ];
        for (error, errno) in table {
            assert_eq!(errno_from_win32(error), errno, "Win32 error {error}");
        }
    }

    /// The numbers above are the SDK's.
    #[cfg(windows)]
    #[test]
    fn os_win32_error_numbers_are_the_sdk_ones() {
        use windows_sys::Win32::Foundation as sdk;
        let pairs = [
            (ERROR_SUCCESS, sdk::ERROR_SUCCESS),
            (ERROR_FILE_NOT_FOUND, sdk::ERROR_FILE_NOT_FOUND),
            (ERROR_PATH_NOT_FOUND, sdk::ERROR_PATH_NOT_FOUND),
            (ERROR_TOO_MANY_OPEN_FILES, sdk::ERROR_TOO_MANY_OPEN_FILES),
            (ERROR_ACCESS_DENIED, sdk::ERROR_ACCESS_DENIED),
            (ERROR_INVALID_HANDLE, sdk::ERROR_INVALID_HANDLE),
            (ERROR_NOT_ENOUGH_MEMORY, sdk::ERROR_NOT_ENOUGH_MEMORY),
            (ERROR_OUTOFMEMORY, sdk::ERROR_OUTOFMEMORY),
            (ERROR_INVALID_DRIVE, sdk::ERROR_INVALID_DRIVE),
            (ERROR_WRITE_PROTECT, sdk::ERROR_WRITE_PROTECT),
            (ERROR_SHARING_VIOLATION, sdk::ERROR_SHARING_VIOLATION),
            (ERROR_LOCK_VIOLATION, sdk::ERROR_LOCK_VIOLATION),
            (ERROR_HANDLE_EOF, sdk::ERROR_HANDLE_EOF),
            (ERROR_HANDLE_DISK_FULL, sdk::ERROR_HANDLE_DISK_FULL),
            (ERROR_BAD_NETPATH, sdk::ERROR_BAD_NETPATH),
            (ERROR_BAD_NET_NAME, sdk::ERROR_BAD_NET_NAME),
            (ERROR_FILE_EXISTS, sdk::ERROR_FILE_EXISTS),
            (ERROR_INVALID_PARAMETER, sdk::ERROR_INVALID_PARAMETER),
            (ERROR_BUFFER_OVERFLOW, sdk::ERROR_BUFFER_OVERFLOW),
            (ERROR_DISK_FULL, sdk::ERROR_DISK_FULL),
            (ERROR_INVALID_NAME, sdk::ERROR_INVALID_NAME),
            (ERROR_NEGATIVE_SEEK, sdk::ERROR_NEGATIVE_SEEK),
            (ERROR_DIR_NOT_EMPTY, sdk::ERROR_DIR_NOT_EMPTY),
            (ERROR_ALREADY_EXISTS, sdk::ERROR_ALREADY_EXISTS),
            (ERROR_FILENAME_EXCED_RANGE, sdk::ERROR_FILENAME_EXCED_RANGE),
            (ERROR_DIRECTORY, sdk::ERROR_DIRECTORY),
            (ERROR_NOACCESS, sdk::ERROR_NOACCESS),
            (ERROR_COMMITMENT_LIMIT, sdk::ERROR_COMMITMENT_LIMIT),
        ];
        for (ours, sdk) in pairs {
            assert_eq!(ours, sdk);
        }
    }

    /// On Windows the numbers are the CRT's (`errno.h` of the UCRT), which C reads.
    #[cfg(windows)]
    #[test]
    fn os_errno_numbers_are_the_crt_ones_on_windows() {
        assert_eq!(
            [
                libc::ENOENT,
                libc::EIO,
                libc::EBADF,
                libc::ENOMEM,
                libc::EACCES,
                libc::EFAULT,
                libc::EEXIST,
                libc::ENOTDIR,
                libc::EINVAL,
                libc::EMFILE,
                libc::ENOSPC,
                libc::ENAMETOOLONG,
                libc::ENOTEMPTY,
            ],
            [2, 5, 9, 12, 13, 14, 17, 20, 22, 24, 28, 38, 41]
        );
    }
}
