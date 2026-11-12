//! `libSceRtc`, calendar time (piece 3.02; the deleted `src/runtime_rtc.c`). A tick is one
//! microsecond since 0001-01-01 00:00 (proleptic Gregorian), the PS4 convention; the host clock
//! supplies "now" and the host time zone ([`crate::time::utc_offset`]) supplies local time.
//!
//! The calendar arithmetic is plain Rust ([`DateTime`], [`day_of_week`], [`rfc2822`]); the
//! guest's `sceRtc*` functions (x86-64 only) wrap it with the SDK's error codes and are found by
//! symbol name through the NID table, as `RUNTIME_LOOKUP` found them.

/// `SCE_RTC_ERROR_INVALID_POINTER` (only the guest functions take pointers).
#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
const ERR_INVALID_POINTER: i32 = 0x80B5_0002_u32 as i32;
/// `SCE_RTC_ERROR_INVALID_VALUE`.
const ERR_INVALID_VALUE: i32 = 0x80B5_0003_u32 as i32;
const ERR_INVALID_YEAR: i32 = 0x80B5_0008_u32 as i32;
const ERR_INVALID_MONTH: i32 = 0x80B5_0009_u32 as i32;
const ERR_INVALID_DAY: i32 = 0x80B5_000A_u32 as i32;
const ERR_INVALID_HOUR: i32 = 0x80B5_000B_u32 as i32;
const ERR_INVALID_MINUTE: i32 = 0x80B5_000C_u32 as i32;
const ERR_INVALID_SECOND: i32 = 0x80B5_000D_u32 as i32;
const ERR_INVALID_MICROSECOND: i32 = 0x80B5_000E_u32 as i32;
/// The tick of 1970-01-01 00:00 UTC.
const UNIX_EPOCH_TICKS: u64 = 0xdc_bffe_ff2b_c000;
const TICKS_PER_SECOND: u64 = 1_000_000;
const TICKS_PER_DAY: u64 = 86_400 * TICKS_PER_SECOND;

/// `OrbisRtcDateTime`: a broken-down calendar time, 16 bytes in the guest's layout.
/// cbindgen:ignore
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub microsecond: u32,
}

const _: () = assert!(std::mem::size_of::<DateTime>() == 16);

fn leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days in `month` (1..=12) of `year`.
fn days_in_month(year: i32, month: i32) -> i32 {
    const DAYS: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if month == 2 && leap(year) {
        29
    } else {
        DAYS[(month - 1) as usize]
    }
}

/// Days since 0001-01-01 (H. Hinnant's `days_from_civil`, epoch shifted so 0001-01-01 is 0).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 306
}

/// The (year, month, day) of day `days` since 0001-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 306;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// The day of the week (0 = Sunday) of day `days` since 0001-01-01, a Monday.
fn weekday(days: i64) -> usize {
    (days + 1).rem_euclid(7) as usize
}

/// `(year, month, day)` checked as the SDK checks it: the error code of the first field out of
/// range.
fn check_date(year: i32, month: i32, day: i32) -> Result<(), i32> {
    if !(1..=9999).contains(&year) {
        return Err(ERR_INVALID_YEAR);
    }
    if !(1..=12).contains(&month) {
        return Err(ERR_INVALID_MONTH);
    }
    if day < 1 || day > days_in_month(year, month) {
        return Err(ERR_INVALID_DAY);
    }
    Ok(())
}

impl DateTime {
    /// The calendar time of `tick`. Every tick has one; a tick past 9999-12-31 wraps the year
    /// to 16 bits, as the C did.
    pub fn from_tick(tick: u64) -> Self {
        let (year, month, day) = civil_from_days((tick / TICKS_PER_DAY) as i64);
        let us = tick % TICKS_PER_DAY;
        DateTime {
            year: year as u16,
            month: month as u16,
            day: day as u16,
            hour: (us / 3_600_000_000) as u16,
            minute: (us / 60_000_000 % 60) as u16,
            second: (us / TICKS_PER_SECOND % 60) as u16,
            microsecond: (us % TICKS_PER_SECOND) as u32,
        }
    }

    /// The SDK's validity check: `Err` holds the code of the first field out of range.
    pub fn validate(&self) -> Result<(), i32> {
        check_date(
            i32::from(self.year),
            i32::from(self.month),
            i32::from(self.day),
        )?;
        if self.hour > 23 {
            return Err(ERR_INVALID_HOUR);
        }
        if self.minute > 59 {
            return Err(ERR_INVALID_MINUTE);
        }
        if self.second > 59 {
            return Err(ERR_INVALID_SECOND);
        }
        if self.microsecond > 999_999 {
            return Err(ERR_INVALID_MICROSECOND);
        }
        Ok(())
    }

    /// The tick of this calendar time, or the code of its first invalid field.
    pub fn tick(&self) -> Result<u64, i32> {
        self.validate()?;
        let days = days_from_civil(
            i64::from(self.year),
            i64::from(self.month),
            i64::from(self.day),
        ) as u64;
        let seconds = u64::from(self.hour) * 3600 + u64::from(self.minute) * 60;
        Ok(days * TICKS_PER_DAY
            + (seconds + u64::from(self.second)) * TICKS_PER_SECOND
            + u64::from(self.microsecond))
    }

    /// Seconds since the Unix epoch (`sceRtcGetTime_t`): 0 before 1970, or the code of the
    /// first invalid field.
    pub fn time_t(&self) -> Result<i64, i32> {
        let tick = self.tick()?;
        Ok(tick
            .checked_sub(UNIX_EPOCH_TICKS)
            .map_or(0, |since| (since / TICKS_PER_SECOND) as i64))
    }

    /// The calendar time `seconds` after the Unix epoch (`sceRtcSetTime_t`); a negative time is
    /// `SCE_RTC_ERROR_INVALID_VALUE`.
    pub fn from_time_t(seconds: i64) -> Result<Self, i32> {
        if seconds < 0 {
            return Err(ERR_INVALID_VALUE);
        }
        Ok(Self::from_tick(UNIX_EPOCH_TICKS.wrapping_add(
            (seconds as u64).wrapping_mul(TICKS_PER_SECOND),
        )))
    }
}

/// `sceRtcGetDayOfWeek`: 0 (Sunday) to 6, or the negative code of the first invalid field.
pub fn day_of_week(year: i32, month: i32, day: i32) -> i32 {
    match check_date(year, month, day) {
        Ok(()) => weekday(days_from_civil(year.into(), month.into(), day.into())) as i32,
        Err(code) => code,
    }
}

/// The tick of the host clock now (UTC).
pub fn now_utc() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => UNIX_EPOCH_TICKS.wrapping_add(since.as_micros() as u64),
        Err(before) => UNIX_EPOCH_TICKS.wrapping_sub(before.duration().as_micros() as u64),
    }
}

/// The offset of the host's local time from UTC at the UTC tick `utc`, in microseconds.
pub fn local_offset(utc: u64) -> i64 {
    let seconds = utc.wrapping_sub(UNIX_EPOCH_TICKS) as i64 / TICKS_PER_SECOND as i64;
    crate::time::utc_offset(seconds) * TICKS_PER_SECOND as i64
}

/// `tick` moved by `offset` microseconds: local time from UTC with the zone's offset, or back.
fn shift(tick: u64, offset: i64) -> u64 {
    (tick as i64).wrapping_add(offset) as u64
}

/// RFC 2822 text of the UTC tick `utc` in a zone `offset` microseconds east of UTC:
/// `"Fri, 25 Sep 2026 12:00:00 +0300"` (31 characters for years 1000 to 9999).
pub fn rfc2822(utc: u64, offset: i64) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let local = shift(utc, offset);
    let t = DateTime::from_tick(local);
    let minutes = offset / 60_000_000;
    let sign = if minutes < 0 { '-' } else { '+' };
    let (hours, minutes) = (minutes.abs() / 60, minutes.abs() % 60);
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} {sign}{hours:02}{minutes:02}",
        DAYS[weekday((local / TICKS_PER_DAY) as i64)],
        t.day,
        MONTHS[usize::from(t.month - 1)],
        t.year,
        t.hour,
        t.minute,
        t.second,
    )
}

#[cfg(target_arch = "x86_64")]
mod guest {
    //! The guest's `sceRtc*` functions: null pointers and invalid fields return the SDK's codes.

    use super::*;
    use std::ffi::{c_char, CStr};

    /// `value` stored in `out`: 0, or the code of what went wrong.
    fn store<T>(out: &mut T, value: Result<T, i32>) -> i32 {
        match value {
            Ok(value) => {
                *out = value;
                0
            }
            Err(code) => code,
        }
    }

    pub(super) extern "sysv64" fn current_clock_local_time(t: *mut DateTime) -> i32 {
        // SAFETY: the guest passes null or its `OrbisRtcDateTime`.
        let Some(t) = (unsafe { t.as_mut() }) else {
            return ERR_INVALID_POINTER;
        };
        let utc = now_utc();
        *t = DateTime::from_tick(shift(utc, local_offset(utc)));
        0
    }

    pub(super) extern "sysv64" fn current_clock(t: *mut DateTime, minutes: i32) -> i32 {
        // SAFETY: the guest passes null or its `OrbisRtcDateTime`.
        let Some(t) = (unsafe { t.as_mut() }) else {
            return ERR_INVALID_POINTER;
        };
        *t = DateTime::from_tick(shift(now_utc(), i64::from(minutes) * 60_000_000));
        0
    }

    pub(super) extern "sysv64" fn current_tick(tick: *mut u64) -> i32 {
        // SAFETY: the guest passes null or its tick.
        let Some(tick) = (unsafe { tick.as_mut() }) else {
            return ERR_INVALID_POINTER;
        };
        *tick = now_utc();
        0
    }

    pub(super) extern "sysv64" fn utc_to_local(utc: *const u64, local: *mut u64) -> i32 {
        // SAFETY: the guest passes null or its ticks.
        let (Some(&utc), Some(local)) = (unsafe { utc.as_ref() }, unsafe { local.as_mut() }) else {
            return ERR_INVALID_POINTER;
        };
        *local = shift(utc, local_offset(utc));
        0
    }

    pub(super) extern "sysv64" fn local_to_utc(local: *const u64, utc: *mut u64) -> i32 {
        // SAFETY: the guest passes null or its ticks.
        let (Some(&local), Some(utc)) = (unsafe { local.as_ref() }, unsafe { utc.as_mut() }) else {
            return ERR_INVALID_POINTER;
        };
        *utc = shift(local, -local_offset(local));
        0
    }

    pub(super) extern "sysv64" fn get_day_of_week(year: i32, month: i32, day: i32) -> i32 {
        day_of_week(year, month, day)
    }

    pub(super) extern "sysv64" fn get_tick(t: *const DateTime, tick: *mut u64) -> i32 {
        // SAFETY: the guest passes null or its date and tick.
        let (Some(t), Some(tick)) = (unsafe { t.as_ref() }, unsafe { tick.as_mut() }) else {
            return ERR_INVALID_POINTER;
        };
        store(tick, t.tick())
    }

    pub(super) extern "sysv64" fn set_tick(t: *mut DateTime, tick: *const u64) -> i32 {
        // SAFETY: the guest passes null or its date and tick.
        let (Some(t), Some(&tick)) = (unsafe { t.as_mut() }, unsafe { tick.as_ref() }) else {
            return ERR_INVALID_POINTER;
        };
        *t = DateTime::from_tick(tick);
        0
    }

    pub(super) extern "sysv64" fn tick_resolution() -> u32 {
        TICKS_PER_SECOND as u32
    }

    pub(super) extern "sysv64" fn set_time_t(t: *mut DateTime, seconds: i64) -> i32 {
        // SAFETY: the guest passes null or its `OrbisRtcDateTime`.
        let Some(t) = (unsafe { t.as_mut() }) else {
            return ERR_INVALID_POINTER;
        };
        store(t, DateTime::from_time_t(seconds))
    }

    pub(super) extern "sysv64" fn get_time_t(t: *const DateTime, seconds: *mut i64) -> i32 {
        // SAFETY: the guest passes null or its date and time.
        let (Some(t), Some(seconds)) = (unsafe { t.as_ref() }, unsafe { seconds.as_mut() }) else {
            return ERR_INVALID_POINTER;
        };
        store(seconds, t.time_t())
    }

    /// `sceRtcFormatRFC2822LocalTime(out, utc)`: the UTC tick `utc` (null: now) in local time.
    /// `out` is the SDK's 32-byte buffer; the text is 31 characters and a NUL.
    pub(super) extern "sysv64" fn format_rfc2822_local_time(
        text: *mut c_char,
        utc: *const u64,
    ) -> i32 {
        if text.is_null() {
            return ERR_INVALID_POINTER;
        }
        // SAFETY: the guest passes null or its tick.
        let utc = unsafe { utc.as_ref() }.copied().unwrap_or_else(now_utc);
        let formatted = rfc2822(utc, local_offset(utc));
        // SAFETY: the guest's buffer holds the text and its NUL (the SDK's size).
        unsafe {
            std::ptr::copy_nonoverlapping(formatted.as_ptr(), text.cast(), formatted.len());
            *text.add(formatted.len()) = 0;
        }
        0
    }

    /// The function behind the scoped import `name`, found by its symbol in the NID table.
    pub(super) fn resolve(name: &CStr) -> Option<usize> {
        let symbol = crate::abi::nid_name(name.to_str().ok()?)?;
        let address = match symbol {
            "sceRtcGetCurrentClockLocalTime" => current_clock_local_time as *const () as usize,
            "sceRtcGetCurrentClock" => current_clock as *const () as usize,
            "sceRtcGetCurrentTick" | "sceRtcGetCurrentNetworkTick" => {
                current_tick as *const () as usize
            }
            "sceRtcConvertUtcToLocalTime" => utc_to_local as *const () as usize,
            "sceRtcConvertLocalTimeToUtc" => local_to_utc as *const () as usize,
            "sceRtcGetDayOfWeek" => get_day_of_week as *const () as usize,
            "sceRtcGetTick" => get_tick as *const () as usize,
            "sceRtcSetTick" => set_tick as *const () as usize,
            "sceRtcGetTickResolution" => tick_resolution as *const () as usize,
            "sceRtcSetTime_t" => set_time_t as *const () as usize,
            "sceRtcGetTime_t" => get_time_t as *const () as usize,
            "sceRtcFormatRFC2822LocalTime" => format_rfc2822_local_time as *const () as usize,
            _ => return None,
        };
        Some(address)
    }
}

/// The guest `sceRtc*` function behind the scoped import `name`, or `None`. Only x86-64 hosts
/// run guest code.
#[cfg(target_arch = "x86_64")]
pub fn resolve(name: &std::ffi::CStr) -> Option<usize> {
    guest::resolve(name)
}

/// The exported C function of this module, declared in `include/uncharted_runtime.h`.
#[cfg(target_arch = "x86_64")]
pub mod ffi {
    use std::ffi::{c_char, CStr};

    /// The address of the guest `sceRtc*` function behind the scoped import `name`, or 0 (the C
    /// `runtime_rtc_resolve`, now in `crates/runtime/src/rtc.rs`).
    ///
    /// # Safety
    /// `name` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_rtc_resolve(name: *const c_char) -> usize {
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

    fn date(year: u16, month: u16, day: u16, hour: u16, minute: u16, second: u16) -> DateTime {
        DateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
            microsecond: 0,
        }
    }

    #[test]
    fn epoch_ticks_are_the_unix_epoch() {
        assert_eq!(date(1970, 1, 1, 0, 0, 0).tick(), Ok(UNIX_EPOCH_TICKS));
        assert_eq!(UNIX_EPOCH_TICKS, 719_162 * TICKS_PER_DAY);
        assert_eq!(date(1, 1, 1, 0, 0, 0).tick(), Ok(0));
        assert_eq!(DateTime::from_tick(0), date(1, 1, 1, 0, 0, 0));
    }

    #[test]
    fn ticks_convert_both_ways() {
        let t = DateTime {
            microsecond: 123_456,
            ..date(2026, 9, 25, 12, 34, 56)
        };
        let tick = t.tick().unwrap();
        // 2026-09-25 12:34:56.123456 UTC is 1_790_339_696 s after the Unix epoch.
        assert_eq!(tick, UNIX_EPOCH_TICKS + 1_790_339_696_123_456);
        assert_eq!(DateTime::from_tick(tick), t);
        let last = DateTime {
            microsecond: 999_999,
            ..date(9999, 12, 31, 23, 59, 59)
        };
        assert_eq!(DateTime::from_tick(last.tick().unwrap()), last);
        // Leap days across the century rules.
        for (year, leap_day) in [(2000, true), (1900, false), (2024, true), (2100, false)] {
            let feb29 = date(year, 2, 29, 0, 0, 0);
            if leap_day {
                assert_eq!(DateTime::from_tick(feb29.tick().unwrap()), feb29);
            } else {
                assert_eq!(feb29.tick(), Err(ERR_INVALID_DAY));
            }
        }
        // Every day boundary of a 400-year cycle round-trips.
        let start = date(1601, 1, 1, 0, 0, 0).tick().unwrap() / TICKS_PER_DAY;
        for day in start..start + 146_097 {
            let t = DateTime::from_tick(day * TICKS_PER_DAY);
            assert_eq!(t.tick(), Ok(day * TICKS_PER_DAY), "{t:?}");
        }
    }

    #[test]
    fn validation_reports_the_first_bad_field() {
        let good = date(2026, 1, 31, 23, 59, 59);
        assert_eq!(good.validate(), Ok(()));
        let cases = [
            (DateTime { year: 0, ..good }, ERR_INVALID_YEAR),
            (
                DateTime {
                    year: 10000,
                    ..good
                },
                ERR_INVALID_YEAR,
            ),
            (DateTime { month: 0, ..good }, ERR_INVALID_MONTH),
            (DateTime { month: 13, ..good }, ERR_INVALID_MONTH),
            (DateTime { day: 0, ..good }, ERR_INVALID_DAY),
            (
                DateTime {
                    month: 4,
                    day: 31,
                    ..good
                },
                ERR_INVALID_DAY,
            ),
            (DateTime { hour: 24, ..good }, ERR_INVALID_HOUR),
            (DateTime { minute: 60, ..good }, ERR_INVALID_MINUTE),
            (DateTime { second: 60, ..good }, ERR_INVALID_SECOND),
            (
                DateTime {
                    microsecond: 1_000_000,
                    ..good
                },
                ERR_INVALID_MICROSECOND,
            ),
            (
                DateTime {
                    year: 0,
                    month: 0,
                    ..good
                },
                ERR_INVALID_YEAR,
            ),
        ];
        for (t, code) in cases {
            assert_eq!(t.validate(), Err(code), "{t:?}");
            assert_eq!(t.tick(), Err(code));
            assert_eq!(t.time_t(), Err(code));
        }
        assert_eq!(ERR_INVALID_POINTER as u32, 0x80B5_0002);
        assert_eq!(ERR_INVALID_MICROSECOND as u32, 0x80B5_000E);
    }

    #[test]
    fn days_of_week() {
        assert_eq!(day_of_week(1, 1, 1), 1); // a Monday
        assert_eq!(day_of_week(1970, 1, 1), 4); // a Thursday
        assert_eq!(day_of_week(2000, 2, 29), 2); // a Tuesday
        assert_eq!(day_of_week(2026, 9, 25), 5); // a Friday
        assert_eq!(day_of_week(2026, 11, 12), 4); // a Thursday
        assert_eq!(day_of_week(9999, 12, 31), 5); // a Friday
        assert_eq!(day_of_week(0, 1, 1), ERR_INVALID_YEAR);
        assert_eq!(day_of_week(10000, 1, 1), ERR_INVALID_YEAR);
        assert_eq!(day_of_week(2026, 13, 1), ERR_INVALID_MONTH);
        assert_eq!(day_of_week(2026, 2, 29), ERR_INVALID_DAY);
        assert_eq!(day_of_week(2024, 2, 29), 4);
    }

    #[test]
    fn time_t_round_trip() {
        let t = DateTime::from_time_t(1_790_339_696).unwrap();
        assert_eq!(t, date(2026, 9, 25, 12, 34, 56));
        assert_eq!(t.time_t(), Ok(1_790_339_696));
        assert_eq!(DateTime::from_time_t(0), Ok(date(1970, 1, 1, 0, 0, 0)));
        assert_eq!(DateTime::from_time_t(-1), Err(ERR_INVALID_VALUE));
        // Before 1970 reads as 0.
        assert_eq!(date(1969, 12, 31, 23, 59, 59).time_t(), Ok(0));
    }

    #[test]
    fn rfc2822_text() {
        let utc = date(2026, 9, 25, 9, 0, 0).tick().unwrap();
        let hour = 3_600_000_000_i64;
        assert_eq!(rfc2822(utc, 0), "Fri, 25 Sep 2026 09:00:00 +0000");
        assert_eq!(rfc2822(utc, 3 * hour), "Fri, 25 Sep 2026 12:00:00 +0300");
        assert_eq!(rfc2822(utc, -10 * hour), "Thu, 24 Sep 2026 23:00:00 -1000");
        assert_eq!(
            rfc2822(utc, -(3 * hour + hour / 2)),
            "Fri, 25 Sep 2026 05:30:00 -0330"
        );
        assert_eq!(
            rfc2822(utc, 5 * hour + 45 * 60_000_000),
            "Fri, 25 Sep 2026 14:45:00 +0545"
        );
        assert_eq!(rfc2822(utc, 3 * hour).len(), 31);
        assert_eq!(
            rfc2822(UNIX_EPOCH_TICKS, 0),
            "Thu, 01 Jan 1970 00:00:00 +0000"
        );
        assert_eq!(rfc2822(0, 0), "Mon, 01 Jan 0001 00:00:00 +0000");
    }

    #[test]
    fn utc_and_local_ticks_use_the_host_offset() {
        let utc = date(2026, 6, 1, 12, 0, 0).tick().unwrap();
        let offset = local_offset(utc);
        let seconds = (utc - UNIX_EPOCH_TICKS) as i64 / 1_000_000;
        assert_eq!(offset, crate::time::utc_offset(seconds) * 1_000_000);
        assert_eq!(shift(shift(utc, offset), -offset), utc);
        assert_eq!(shift(utc, 3_600_000_000), utc + 3_600_000_000);
        assert_eq!(shift(utc, -3_600_000_000), utc - 3_600_000_000);
        let now = now_utc();
        let system = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;
        assert!(now.abs_diff(UNIX_EPOCH_TICKS + system) < 2_000_000);
    }

    #[cfg(target_arch = "x86_64")]
    mod guest_abi {
        use super::super::*;
        use std::ffi::{c_char, CStr};

        fn get(name: &CStr) -> usize {
            resolve(name).unwrap_or_else(|| panic!("{name:?} does not resolve"))
        }

        /// The guest function behind `name` as the function-pointer type `F`.
        ///
        /// # Safety
        /// `F` is the `extern "sysv64"` type of that function.
        unsafe fn function<F: Copy>(name: &CStr) -> F {
            let address = get(name);
            assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<usize>());
            // SAFETY: an address of a function of type `F` (caller), the size of a pointer.
            unsafe { std::mem::transmute_copy(&address) }
        }

        #[test]
        fn exports_resolve_by_symbol_name() {
            for nid in [
                c"zO9UL3qIINQ#H#I",
                c"bDEVVP4bTjQ#H#I",
                c"8w-H19ip48I#H#I",
                c"jMNwqYr4R-k#H#I",
                c"ZPD1YOKI+Kw#H#I",
                c"M1TvFst-jrM#H#I",
                c"8Yr143yEnRo#H#I",
                c"AxHBk3eat04#H#I",
                c"ueega6v3GUw#H#I",
                c"CyIK-i4XdgQ#H#I",
                c"BtqmpTRXHgk#H#I",
            ] {
                // SAFETY: a NUL-terminated literal; the C wrapper and the Rust function agree.
                assert_eq!(
                    unsafe { ffi::runtime_rtc_resolve(nid.as_ptr()) },
                    get(nid),
                    "{nid:?}"
                );
            }
            assert_eq!(resolve(c"n88vx3C5nW8#I#J"), None);
            assert_eq!(resolve(c"unknown"), None);
            // SAFETY: a null name is accepted and resolves to nothing.
            assert_eq!(unsafe { ffi::runtime_rtc_resolve(std::ptr::null()) }, 0);
        }

        #[test]
        fn guest_ticks_and_dates() {
            type GetTick = extern "sysv64" fn(*const DateTime, *mut u64) -> i32;
            type SetTick = extern "sysv64" fn(*mut DateTime, *const u64) -> i32;
            // SAFETY: the addresses of `sceRtcGetTick` and `sceRtcSetTick`.
            let (get_tick, set_tick): (GetTick, SetTick) =
                unsafe { (function(c"8w-H19ip48I#H#I"), function(c"ueega6v3GUw#H#I")) };
            let t = DateTime {
                year: 2026,
                month: 9,
                day: 25,
                hour: 12,
                minute: 34,
                second: 56,
                microsecond: 7,
            };
            let mut tick = 0u64;
            assert_eq!(get_tick(&t, &mut tick), 0);
            assert_eq!(tick, t.tick().unwrap());
            let mut back = DateTime::default();
            assert_eq!(set_tick(&mut back, &tick), 0);
            assert_eq!(back, t);
            assert_eq!(get_tick(std::ptr::null(), &mut tick), ERR_INVALID_POINTER);
            assert_eq!(get_tick(&t, std::ptr::null_mut()), ERR_INVALID_POINTER);
            assert_eq!(set_tick(&mut back, std::ptr::null()), ERR_INVALID_POINTER);
            let bad = DateTime { month: 13, ..t };
            assert_eq!(get_tick(&bad, &mut tick), ERR_INVALID_MONTH);

            type DayOfWeek = extern "sysv64" fn(i32, i32, i32) -> i32;
            type Resolution = extern "sysv64" fn() -> u32;
            // SAFETY: the addresses of `sceRtcGetDayOfWeek` and `sceRtcGetTickResolution`.
            let (day_of_week, resolution): (DayOfWeek, Resolution) =
                unsafe { (function(c"CyIK-i4XdgQ#H#I"), function(c"jMNwqYr4R-k#H#I")) };
            assert_eq!(day_of_week(2026, 9, 25), 5);
            assert_eq!(day_of_week(2026, 2, 30), ERR_INVALID_DAY);
            assert_eq!(resolution(), 1_000_000);

            type SetTimeT = extern "sysv64" fn(*mut DateTime, i64) -> i32;
            type GetTimeT = extern "sysv64" fn(*const DateTime, *mut i64) -> i32;
            // SAFETY: the addresses of `sceRtcSetTime_t` and `sceRtcGetTime_t`.
            let (set_time_t, get_time_t): (SetTimeT, GetTimeT) =
                unsafe { (function(c"bDEVVP4bTjQ#H#I"), function(c"BtqmpTRXHgk#H#I")) };
            let mut t = DateTime::default();
            assert_eq!(set_time_t(&mut t, 1_790_339_696), 0);
            let mut seconds = 0i64;
            assert_eq!(get_time_t(&t, &mut seconds), 0);
            assert_eq!(seconds, 1_790_339_696);
            assert_eq!(set_time_t(&mut t, -1), ERR_INVALID_VALUE);
            assert_eq!(set_time_t(std::ptr::null_mut(), 0), ERR_INVALID_POINTER);
            assert_eq!(get_time_t(&t, std::ptr::null_mut()), ERR_INVALID_POINTER);
        }

        #[test]
        fn guest_clock_and_local_time() {
            type Tick = extern "sysv64" fn(*mut u64) -> i32;
            type Convert = extern "sysv64" fn(*const u64, *mut u64) -> i32;
            type Local = extern "sysv64" fn(*mut DateTime) -> i32;
            // SAFETY: the addresses of the `sceRtc*` functions named below.
            let (network_tick, to_local, to_utc, clock_local): (Tick, Convert, Convert, Local) = unsafe {
                (
                    function(c"zO9UL3qIINQ#H#I"), // sceRtcGetCurrentNetworkTick
                    function(c"M1TvFst-jrM#H#I"), // sceRtcConvertUtcToLocalTime
                    function(c"8Yr143yEnRo#H#I"), // sceRtcConvertLocalTimeToUtc
                    function(c"ZPD1YOKI+Kw#H#I"), // sceRtcGetCurrentClockLocalTime
                )
            };
            let mut now = 0u64;
            assert_eq!(network_tick(&mut now), 0);
            assert!(now.abs_diff(now_utc()) < 2_000_000);
            assert_eq!(network_tick(std::ptr::null_mut()), ERR_INVALID_POINTER);

            let utc = DateTime {
                year: 2026,
                month: 6,
                day: 1,
                hour: 12,
                ..DateTime::default()
            }
            .tick()
            .unwrap();
            let mut local = 0u64;
            assert_eq!(to_local(&utc, &mut local), 0);
            assert_eq!(local, shift(utc, local_offset(utc)));
            let mut back = 0u64;
            assert_eq!(to_utc(&local, &mut back), 0);
            assert_eq!(back, shift(local, -local_offset(local)));
            assert_eq!(to_local(std::ptr::null(), &mut local), ERR_INVALID_POINTER);
            assert_eq!(to_utc(&local, std::ptr::null_mut()), ERR_INVALID_POINTER);

            let mut t = DateTime::default();
            assert_eq!(clock_local(&mut t), 0);
            assert_eq!(t.validate(), Ok(()));
            assert_eq!(clock_local(std::ptr::null_mut()), ERR_INVALID_POINTER);
        }

        #[test]
        fn guest_rfc2822_is_local_time() {
            type Format = extern "sysv64" fn(*mut c_char, *const u64) -> i32;
            // SAFETY: the address of `sceRtcFormatRFC2822LocalTime`.
            let format: Format = unsafe { function(c"AxHBk3eat04#H#I") };
            let utc = DateTime {
                year: 2026,
                month: 9,
                day: 25,
                hour: 9,
                ..DateTime::default()
            }
            .tick()
            .unwrap();
            let mut buffer = [0x55 as c_char; 32];
            assert_eq!(format(buffer.as_mut_ptr(), &utc), 0);
            // SAFETY: the function wrote a NUL-terminated string into `buffer`.
            let text = unsafe { CStr::from_ptr(buffer.as_ptr()) };
            assert_eq!(text.to_str().unwrap(), rfc2822(utc, local_offset(utc)));
            assert_eq!(text.to_bytes().len(), 31);
            // Null tick: now.
            assert_eq!(format(buffer.as_mut_ptr(), std::ptr::null()), 0);
            // SAFETY: as above.
            assert_eq!(
                unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_bytes().len(),
                31
            );
            assert_eq!(format(std::ptr::null_mut(), &utc), ERR_INVALID_POINTER);
        }
    }
}
