//! Clocks of `compat_win.c`: the zone offset (`compat_utc_offset`), a sleep finer than the
//! scheduler tick (`compat_sleep_ns`) and the process's and thread's CPU times
//! (`compat_cpu_times`). Each has a Unix form too, so the Rust modules that call it later
//! (3.02 `time`, 3.11 `kernel`) need no host branches of their own.

use std::ffi::c_long;
use std::io;

/// Seconds east of UTC at time `t` (seconds since 1970), daylight saving included; 0 when the
/// host cannot convert `t`.
///
/// Windows' `struct tm` has no `tm_gmtoff`, so the offset is `t` less the UTC fields of `t`
/// read back as local time (`mktime`), with the local daylight-saving flag, as
/// `compat_utc_offset` did.
#[cfg(windows)]
pub fn utc_offset(t: i64) -> c_long {
    extern "C" {
        fn _mktime64(fields: *mut libc::tm) -> i64;
    }
    // SAFETY: `struct tm` is nine `int`s; all zero is a valid value.
    let (mut local, mut utc): (libc::tm, libc::tm) = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are to live locals; the CRT writes only the `tm` it is given.
    let converted = unsafe { libc::localtime_s(&mut local, &t) == 0 && libc::gmtime_s(&mut utc, &t) == 0 };
    if !converted {
        return 0;
    }
    utc.tm_isdst = local.tm_isdst;
    // SAFETY: `utc` is a filled-in `tm` the CRT may normalize in place.
    let as_local = unsafe { _mktime64(&mut utc) };
    if as_local == -1 {
        return 0;
    }
    // An offset is under a day, so it fits a 32-bit `long`.
    (t - as_local) as c_long
}

/// Seconds east of UTC at time `t` (seconds since 1970), daylight saving included; 0 when the
/// host cannot convert `t`. Unix hosts keep it in `tm_gmtoff`.
#[cfg(unix)]
pub fn utc_offset(t: i64) -> c_long {
    // SAFETY: `struct tm` is integers and a nullable `tm_zone` pointer; all zero is valid.
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are to live locals; `localtime_r` is the reentrant form.
    if unsafe { libc::localtime_r(&t, &mut local) }.is_null() {
        return 0;
    }
    local.tm_gmtoff
}

#[cfg(not(any(unix, windows)))]
pub fn utc_offset(_: i64) -> c_long {
    0
}

/// Sleeps `ns` nanoseconds; 0 gives up the rest of the time slice.
///
/// Windows: one high-resolution waitable timer per thread (Windows 10 1803 and later; a plain
/// waitable timer before), since `Sleep` and winpthreads' `nanosleep` round up to the 15.6 ms
/// scheduler tick. Without a timer it falls back to `Sleep`, rounded up to whole milliseconds.
#[cfg(windows)]
pub fn sleep_ns(ns: u64) {
    use windows_sys::Win32::System::Threading::{Sleep, SwitchToThread};
    if ns == 0 {
        // SAFETY: no arguments; it only yields.
        unsafe { SwitchToThread() };
        return;
    }
    if TIMER.try_with(|timer| timer.wait(ns)).unwrap_or(false) {
        return;
    }
    // INFINITE is u32::MAX: a longer sleep is cut just short of it.
    let ms = u32::try_from(ns.div_ceil(1_000_000)).map_or(u32::MAX - 1, |ms| ms.min(u32::MAX - 1));
    // SAFETY: no pointers.
    unsafe { Sleep(ms) };
}

/// Sleeps `ns` nanoseconds; 0 gives up the rest of the time slice.
#[cfg(not(windows))]
pub fn sleep_ns(ns: u64) {
    if ns == 0 {
        std::thread::yield_now();
    } else {
        std::thread::sleep(std::time::Duration::from_nanos(ns));
    }
}

#[cfg(windows)]
thread_local! {
    static TIMER: Timer = Timer::new();
}

/// This thread's waitable timer (null when the host would not make one), closed when the
/// thread ends (`compat_win.c` kept its handle until the process ended).
#[cfg(windows)]
struct Timer(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Timer {
    fn new() -> Self {
        use windows_sys::Win32::System::Threading::{
            CreateWaitableTimerExW, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS,
        };
        let create = |flags| {
            // SAFETY: no attributes and no name: an unnamed timer with default security.
            unsafe { CreateWaitableTimerExW(std::ptr::null(), std::ptr::null(), flags, TIMER_ALL_ACCESS) }
        };
        let mut handle = create(CREATE_WAITABLE_TIMER_HIGH_RESOLUTION);
        if handle.is_null() {
            handle = create(0);
        }
        Self(handle)
    }

    /// Waits `ns` (not 0) on the timer; false when there is no timer or it would not start.
    fn wait(&self, ns: u64) -> bool {
        use windows_sys::Win32::System::Threading::{
            SetWaitableTimer, WaitForSingleObject, INFINITE,
        };
        if self.0.is_null() {
            return false;
        }
        // Negative: relative to now, in 100 ns units.
        let due = -i64::try_from(ns.div_ceil(100)).unwrap_or(i64::MAX);
        // SAFETY: `self.0` is this thread's live timer; `due` outlives the call; no completion
        // routine.
        if unsafe { SetWaitableTimer(self.0, &due, 0, None, std::ptr::null(), 0) } == 0 {
            return false;
        }
        // SAFETY: as above; the timer is signalled once `due` has passed.
        unsafe { WaitForSingleObject(self.0, INFINITE) };
        true
    }
}

#[cfg(windows)]
impl Drop for Timer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle is this timer's own and is closed once.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
        }
    }
}

/// Whose CPU time [`cpu_times`] reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuClock {
    /// Every thread of the process, ended ones included (`who` 0 in C).
    Process,
    /// The calling thread (`who` 1 in C).
    Thread,
}

/// User and kernel CPU time, in microseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub user_us: i64,
    pub kernel_us: i64,
}

/// The CPU time `of` the process or the calling thread has used so far.
#[cfg(windows)]
pub fn cpu_times(of: CpuClock) -> io::Result<CpuTimes> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentThread, GetProcessTimes, GetThreadTimes,
    };
    let mut times = [FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    }; 4];
    let [created, exited, kernel, user] = &mut times;
    // SAFETY: the pseudo-handles of this process and thread; four live FILETIMEs.
    let ok = unsafe {
        match of {
            CpuClock::Process => GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user),
            CpuClock::Thread => GetThreadTimes(GetCurrentThread(), created, exited, kernel, user),
        }
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // FILETIME counts 100 ns.
    let micros = |t: &FILETIME| ((u64::from(t.dwHighDateTime) << 32 | u64::from(t.dwLowDateTime)) / 10) as i64;
    Ok(CpuTimes {
        user_us: micros(user),
        kernel_us: micros(kernel),
    })
}

/// The CPU time `of` the process or the calling thread has used so far (`getrusage`). Only
/// Linux reports one thread's time; elsewhere [`CpuClock::Thread`] is `Unsupported`.
#[cfg(unix)]
pub fn cpu_times(of: CpuClock) -> io::Result<CpuTimes> {
    let who = match of {
        CpuClock::Process => libc::RUSAGE_SELF,
        #[cfg(target_os = "linux")]
        CpuClock::Thread => libc::RUSAGE_THREAD,
        #[cfg(not(target_os = "linux"))]
        CpuClock::Thread => return Err(io::ErrorKind::Unsupported.into()),
    };
    // SAFETY: `struct rusage` is integers and `timeval`s; all zero is valid.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: `usage` is a live local the call fills in.
    if unsafe { libc::getrusage(who, &mut usage) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // `time_t` and `suseconds_t` are not the same type on every Unix.
    #[allow(clippy::unnecessary_cast)]
    let micros = |t: libc::timeval| t.tv_sec as i64 * 1_000_000 + t.tv_usec as i64;
    Ok(CpuTimes {
        user_us: micros(usage.ru_utime),
        kernel_us: micros(usage.ru_stime),
    })
}

#[cfg(not(any(unix, windows)))]
pub fn cpu_times(_: CpuClock) -> io::Result<CpuTimes> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Days from 1970-01-01 to the proleptic Gregorian date (H. Hinnant's `days_from_civil`).
    fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
        let year = if month <= 2 { year - 1 } else { year };
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    /// The host's local broken-down time of `t`, as seconds since 1970 if it were UTC.
    fn local_fields_as_seconds(t: i64) -> i64 {
        // SAFETY: as in `utc_offset`.
        let mut local: libc::tm = unsafe { std::mem::zeroed() };
        #[cfg(windows)]
        // SAFETY: live locals.
        assert_eq!(unsafe { libc::localtime_s(&mut local, &t) }, 0);
        #[cfg(unix)]
        // SAFETY: live locals.
        assert!(!unsafe { libc::localtime_r(&t, &mut local) }.is_null());
        let days = days_from_civil(
            i64::from(local.tm_year) + 1900,
            i64::from(local.tm_mon) + 1,
            i64::from(local.tm_mday),
        );
        days * 86_400
            + i64::from(local.tm_hour) * 3_600
            + i64::from(local.tm_min) * 60
            + i64::from(local.tm_sec)
    }

    #[test]
    fn os_days_from_civil_matches_known_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2024, 1, 15) * 86_400, 1_705_276_800);
    }

    /// Local time is UTC plus the offset: in winter and in summer (whichever has daylight
    /// saving in the host's zone), and at a few other instants away from a clock change.
    #[test]
    fn os_utc_offset_is_local_time_less_utc() {
        let instants = [
            0,
            1_000_000_000,
            1_705_320_000, // 2024-01-15 12:00 UTC
            1_721_044_800, // 2024-07-15 12:00 UTC
            1_767_268_800, // 2026-01-01 12:00 UTC
        ];
        for t in instants {
            let offset = i64::from(utc_offset(t));
            assert!(offset.abs() <= 14 * 3_600, "{offset} s at {t}");
            assert_eq!(offset % 60, 0, "zones are whole minutes: {offset} s at {t}");
            assert_eq!(local_fields_as_seconds(t), t + offset, "at {t}");
        }
    }

    #[test]
    fn os_sleep_ns_sleeps_at_least_as_long_as_asked() {
        sleep_ns(0);
        for ns in [1, 1_000, 3_000_000] {
            let start = Instant::now();
            sleep_ns(ns);
            assert!(start.elapsed() >= Duration::from_nanos(ns), "{ns} ns");
        }
    }

    /// The point of the Windows timer: a 1 ms sleep does not round up to the 15.6 ms tick. The
    /// median of nine keeps one late wake on a busy runner from failing the test.
    #[test]
    fn os_sleep_ns_is_finer_than_the_scheduler_tick() {
        let mut slept: Vec<Duration> = (0..9)
            .map(|_| {
                let start = Instant::now();
                sleep_ns(1_000_000);
                start.elapsed()
            })
            .collect();
        slept.sort();
        assert!(slept[4] < Duration::from_millis(12), "median {:?}", slept[4]);
    }

    /// Each thread makes its own timer and closes it when it ends.
    #[test]
    fn os_sleep_ns_works_on_many_threads_at_once() {
        let threads: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    let start = Instant::now();
                    for _ in 0..5 {
                        sleep_ns(500_000);
                    }
                    start.elapsed()
                })
            })
            .collect();
        for thread in threads {
            assert!(thread.join().unwrap() >= Duration::from_micros(2_500));
        }
    }

    fn spin(for_how_long: Duration) -> u64 {
        let start = Instant::now();
        let mut x = 0u64;
        while start.elapsed() < for_how_long {
            x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(7));
        }
        x
    }

    #[test]
    fn os_process_cpu_time_grows_with_work() {
        let before = cpu_times(CpuClock::Process).unwrap();
        spin(Duration::from_millis(100));
        let after = cpu_times(CpuClock::Process).unwrap();
        assert!(before.user_us >= 0 && before.kernel_us >= 0);
        assert!(after.user_us >= before.user_us && after.kernel_us >= before.kernel_us);
        let used = (after.user_us + after.kernel_us) - (before.user_us + before.kernel_us);
        assert!(used > 0, "100 ms of work used no CPU time");
    }

    /// A thread's time is its own: a new thread starts near zero, grows with its work, and
    /// never passes the process's total, read after it.
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn os_thread_cpu_time_is_the_calling_threads() {
        let (start, end) = std::thread::spawn(|| {
            let start = cpu_times(CpuClock::Thread).unwrap();
            spin(Duration::from_millis(100));
            (start, cpu_times(CpuClock::Thread).unwrap())
        })
        .join()
        .unwrap();
        let process = cpu_times(CpuClock::Process).unwrap();
        let total = |t: CpuTimes| t.user_us + t.kernel_us;
        assert!(total(start) < 50_000, "a new thread has used {start:?}");
        assert!(total(end) > total(start), "100 ms of work used no CPU time");
        assert!(total(end) <= total(process), "thread {end:?}, process {process:?}");
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    #[test]
    fn os_thread_cpu_time_is_unsupported_without_rusage_thread() {
        assert_eq!(
            cpu_times(CpuClock::Thread).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }
}
