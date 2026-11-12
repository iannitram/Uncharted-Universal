//! The host clock for the guest (piece 3.02; the deleted `src/runtime_time.c`): the guest's
//! `gettimeofday` (`n88vx3C5nW8#I#J`) and the host time zone that [`crate::rtc`] uses for
//! local time.
//!
//! The guest's `timeval` and `timezone` have an explicit layout (two `int64_t`, two `int32_t`);
//! host structures never reach the guest. On Windows the zone is the C runtime's offset of
//! local time from UTC (as `compat_utc_offset` in `src/compat_win.c` computes it) and `dsttime`
//! is 0; elsewhere it is what the host `gettimeofday` reports.

use std::ffi::c_int;
use std::io;

/// The host's time of day: what `gettimeofday` fills in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeOfDay {
    /// Seconds since 1970-01-01 00:00 UTC.
    pub seconds: i64,
    /// Microseconds within the second, 0..=999999.
    pub microseconds: i64,
    /// Minutes west of Greenwich.
    pub minutes_west: i32,
    /// The host's daylight-saving correction type (0 on Windows).
    pub dst_time: i32,
}

/// The time of day and zone from the host clock.
#[cfg(unix)]
pub fn time_of_day() -> io::Result<TimeOfDay> {
    // `suseconds_t` is 32-bit on Apple hosts and 64-bit elsewhere; `time_t` is 64-bit on every
    // host this crate builds for (all LP64).
    #[cfg(target_vendor = "apple")]
    type Microseconds = i32;
    #[cfg(not(target_vendor = "apple"))]
    type Microseconds = i64;
    #[repr(C)]
    struct Timeval {
        seconds: i64,
        microseconds: Microseconds,
    }
    #[repr(C)]
    struct Timezone {
        minutes_west: c_int,
        dst_time: c_int,
    }
    extern "C" {
        fn gettimeofday(value: *mut Timeval, zone: *mut Timezone) -> c_int;
    }
    let mut value = Timeval {
        seconds: 0,
        microseconds: 0,
    };
    let mut zone = Timezone {
        minutes_west: 0,
        dst_time: 0,
    };
    // SAFETY: both pointers are to live structures of the host's layout.
    if unsafe { gettimeofday(&mut value, &mut zone) } != 0 {
        return Err(io::Error::last_os_error());
    }
    #[allow(clippy::useless_conversion)] // `i32` on Apple hosts
    let microseconds = i64::from(value.microseconds);
    Ok(TimeOfDay {
        seconds: value.seconds,
        microseconds,
        minutes_west: zone.minutes_west,
        dst_time: zone.dst_time,
    })
}

/// The time of day from the system clock and the zone from the C runtime's local time.
#[cfg(windows)]
pub fn time_of_day() -> io::Result<TimeOfDay> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    let seconds = i64::try_from(since.as_secs()).map_err(io::Error::other)?;
    Ok(TimeOfDay {
        seconds,
        microseconds: i64::from(since.subsec_micros()),
        minutes_west: (-utc_offset(seconds) / 60) as i32,
        dst_time: 0,
    })
}

/// The offset of local time from UTC at `seconds` since the Unix epoch, in seconds east of
/// UTC (daylight saving included): `localtime_r`'s `tm_gmtoff`, 0 when the host cannot convert.
#[cfg(unix)]
pub fn utc_offset(seconds: i64) -> i64 {
    use std::ffi::c_char;
    // `time_t` and `long` are 64-bit on every host this crate builds for (all LP64).
    #[repr(C)]
    struct Tm {
        _fields: [c_int; 9],
        gmtoff: i64,
        _zone: *const c_char,
    }
    extern "C" {
        fn localtime_r(time: *const i64, out: *mut Tm) -> *mut Tm;
    }
    let mut local = Tm {
        _fields: [0; 9],
        gmtoff: 0,
        _zone: std::ptr::null(),
    };
    // SAFETY: `seconds` and `local` are live; `localtime_r` writes only `local`.
    if unsafe { localtime_r(&seconds, &mut local) }.is_null() {
        return 0;
    }
    local.gmtoff
}

/// The offset of local time from UTC at `seconds` since the Unix epoch, in seconds east of
/// UTC (daylight saving included), 0 when the C runtime cannot convert. Windows' `struct tm`
/// has no `tm_gmtoff`: UTC's broken-down time read back as local time (`mktime`) is off from
/// `seconds` by the zone's offset, as `compat_utc_offset` computes it.
#[cfg(windows)]
pub fn utc_offset(seconds: i64) -> i64 {
    #[repr(C)]
    #[derive(Default)]
    struct Tm {
        fields: [c_int; 9],
    }
    const ISDST: usize = 8;
    extern "C" {
        fn _localtime64_s(out: *mut Tm, time: *const i64) -> c_int;
        fn _gmtime64_s(out: *mut Tm, time: *const i64) -> c_int;
        fn _mktime64(time: *mut Tm) -> i64;
    }
    let (mut local, mut utc) = (Tm::default(), Tm::default());
    // SAFETY: each call reads `seconds` and writes one live `Tm`.
    if unsafe { _localtime64_s(&mut local, &seconds) } != 0
        || unsafe { _gmtime64_s(&mut utc, &seconds) } != 0
    {
        return 0;
    }
    utc.fields[ISDST] = local.fields[ISDST];
    // SAFETY: `utc` is a valid broken-down time; `_mktime64` normalizes it in place.
    let as_local = unsafe { _mktime64(&mut utc) };
    if as_local == -1 {
        0
    } else {
        seconds - as_local
    }
}

#[cfg(target_arch = "x86_64")]
mod guest {
    use std::ffi::{c_int, CStr};

    #[repr(C)]
    pub(super) struct GuestTimeval {
        pub(super) seconds: i64,
        pub(super) microseconds: i64,
    }

    #[repr(C)]
    pub(super) struct GuestTimezone {
        pub(super) minutes_west: i32,
        pub(super) dst_time: i32,
    }

    /// `gettimeofday(value, zone)`: either pointer may be null. A host failure is a `STOP`
    /// (exit code 21): the guest errno path is not implemented yet.
    pub(super) extern "sysv64" fn gettimeofday(
        value: *mut GuestTimeval,
        zone: *mut GuestTimezone,
    ) -> c_int {
        let now = match super::time_of_day() {
            Ok(now) => now,
            Err(error) => {
                crate::abi::libc::stop(&format!("STOP: host gettimeofday failed: {error}\n"))
            }
        };
        // SAFETY: the guest passes null or a pointer to its `timezone`.
        if let Some(zone) = unsafe { zone.as_mut() } {
            zone.minutes_west = now.minutes_west;
            zone.dst_time = now.dst_time;
        }
        // SAFETY: the guest passes null or a pointer to its `timeval`.
        if let Some(value) = unsafe { value.as_mut() } {
            value.seconds = now.seconds;
            value.microseconds = now.microseconds;
        }
        0
    }

    /// The function behind the scoped import `name`, or `None`.
    pub(super) fn resolve(name: &CStr) -> Option<usize> {
        (name.to_bytes() == b"n88vx3C5nW8#I#J").then_some(gettimeofday as *const () as usize)
    }
}

/// The function behind the scoped import `name` (the guest's `gettimeofday`), or `None`. Only
/// x86-64 hosts run guest code.
#[cfg(target_arch = "x86_64")]
pub fn resolve(name: &std::ffi::CStr) -> Option<usize> {
    guest::resolve(name)
}

/// The exported C function of this module, declared in `include/uncharted_runtime.h`.
#[cfg(target_arch = "x86_64")]
pub mod ffi {
    use std::ffi::{c_char, CStr};

    /// The address of the guest `gettimeofday` when `name` is its scoped import, or 0 (the C
    /// `runtime_time_resolve`, now in `crates/runtime/src/time.rs`).
    ///
    /// # Safety
    /// `name` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_time_resolve(name: *const c_char) -> usize {
        if name.is_null() {
            return 0;
        }
        // SAFETY: the caller passes a NUL-terminated string.
        super::resolve(unsafe { CStr::from_ptr(name) }).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn time_of_day_is_now() {
        let now = time_of_day().unwrap();
        let system = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!((now.seconds - system).abs() <= 2, "{now:?} vs {system}");
        assert!((0..1_000_000).contains(&now.microseconds));
        assert!((-24 * 60..=24 * 60).contains(&now.minutes_west));
    }

    #[test]
    fn utc_offset_is_a_whole_zone_offset() {
        // Every zone in use is a multiple of 15 minutes, within a day of UTC.
        for seconds in [0, 1_000_000_000, 1_789_000_000, 4_102_444_800] {
            let offset = utc_offset(seconds);
            assert!(offset.abs() < 24 * 3600, "{offset}");
            assert_eq!(offset % 900, 0, "{offset}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_zone_is_the_c_runtime_offset() {
        let now = time_of_day().unwrap();
        assert_eq!(now.dst_time, 0);
        assert_eq!(i64::from(now.minutes_west), -utc_offset(now.seconds) / 60);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn guest_gettimeofday_fills_the_guest_layout() {
        use guest::{GuestTimeval, GuestTimezone};
        type Gettimeofday = extern "sysv64" fn(*mut GuestTimeval, *mut GuestTimezone) -> c_int;
        assert_eq!(std::mem::size_of::<GuestTimeval>(), 16);
        assert_eq!(std::mem::size_of::<GuestTimezone>(), 8);
        let address = resolve(c"n88vx3C5nW8#I#J").unwrap();
        // SAFETY: a NUL-terminated literal; the C wrapper and the Rust function agree.
        assert_eq!(
            unsafe { ffi::runtime_time_resolve(c"n88vx3C5nW8#I#J".as_ptr()) },
            address
        );
        // SAFETY: the address of `guest::gettimeofday`.
        let call: Gettimeofday = unsafe { std::mem::transmute(address) };
        let mut value = GuestTimeval {
            seconds: -1,
            microseconds: -1,
        };
        let mut zone = GuestTimezone {
            minutes_west: i32::MIN,
            dst_time: i32::MIN,
        };
        assert_eq!(call(&mut value, &mut zone), 0);
        let host = time_of_day().unwrap();
        assert!((value.seconds - host.seconds).abs() <= 2);
        assert!((0..1_000_000).contains(&value.microseconds));
        assert_eq!(
            (zone.minutes_west, zone.dst_time),
            (host.minutes_west, host.dst_time)
        );
        // Either pointer may be null.
        assert_eq!(call(std::ptr::null_mut(), &mut zone), 0);
        assert_eq!(call(&mut value, std::ptr::null_mut()), 0);
        assert_eq!(call(std::ptr::null_mut(), std::ptr::null_mut()), 0);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn only_the_posix_import_resolves() {
        // Only the libScePosix import is this module's, as in C.
        assert_eq!(resolve(c"n88vx3C5nW8#p#J"), None);
        assert_eq!(resolve(c"unknown"), None);
        // SAFETY: a null name is accepted and resolves to nothing.
        assert_eq!(unsafe { ffi::runtime_time_resolve(std::ptr::null()) }, 0);
    }
}
