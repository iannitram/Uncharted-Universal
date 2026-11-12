//! libkernel and libScePosix process services (piece 3.11; the deleted `src/runtime_kernel.c`):
//! the guest errno mapping, clocks, process time and the TSC, sleeping, the time of day and the
//! time zone, UTC and local time, page size and pid, signal bookkeeping, `getrusage`, a narrow
//! `sysctl` (random bytes, page size, CPU count), `pthread_once` and thread-specific data.
//!
//! Guest values use FreeBSD numbering; host errno values never reach the guest directly
//! ([`guest_errno`]). The host time zone is [`crate::time::utc_offset`] (piece 3.02 took the
//! libScePosix `gettimeofday` import; this module keeps the other time-of-day entry points that
//! were in `runtime_kernel.c`).
//!
//! The plain parts ([`guest_errno`], [`host_clock`], [`utc_to_local`], [`local_to_utc`]) build
//! for every target; the host calls and the guest functions (x86-64 only) sit behind them and are
//! found by symbol name through the NID table, as `RUNTIME_LOOKUP` found them. The exported C
//! functions (`runtime_kernel_resolve`, `runtime_guest_errno`, the process-time clock the GPU
//! library shares, `runtime_thread_keys_cleanup`) are in [`ffi`].

/// Host errno values (the C library's `errno.h` on this target).
#[cfg(any(target_os = "linux", target_os = "android"))]
#[allow(missing_docs)]
pub mod host_errno {
    pub const EPERM: i32 = 1;
    pub const ENOENT: i32 = 2;
    pub const ESRCH: i32 = 3;
    pub const EINTR: i32 = 4;
    pub const EIO: i32 = 5;
    pub const ENXIO: i32 = 6;
    pub const E2BIG: i32 = 7;
    pub const ENOEXEC: i32 = 8;
    pub const EBADF: i32 = 9;
    pub const ECHILD: i32 = 10;
    pub const EAGAIN: i32 = 11;
    pub const ENOMEM: i32 = 12;
    pub const EACCES: i32 = 13;
    pub const EFAULT: i32 = 14;
    pub const EBUSY: i32 = 16;
    pub const EEXIST: i32 = 17;
    pub const EXDEV: i32 = 18;
    pub const ENODEV: i32 = 19;
    pub const ENOTDIR: i32 = 20;
    pub const EISDIR: i32 = 21;
    pub const EINVAL: i32 = 22;
    pub const ENFILE: i32 = 23;
    pub const EMFILE: i32 = 24;
    pub const ENOTTY: i32 = 25;
    pub const EFBIG: i32 = 27;
    pub const ENOSPC: i32 = 28;
    pub const ESPIPE: i32 = 29;
    pub const EROFS: i32 = 30;
    pub const EMLINK: i32 = 31;
    pub const EPIPE: i32 = 32;
    pub const ERANGE: i32 = 34;
    pub const EDEADLK: i32 = 35;
    pub const ENAMETOOLONG: i32 = 36;
    pub const ENOSYS: i32 = 38;
    pub const ENOTEMPTY: i32 = 39;
    pub const ELOOP: i32 = 40;
    pub const EOVERFLOW: i32 = 75;
    pub const ETIMEDOUT: i32 = 110;
    pub const ECANCELED: i32 = 125;
}

/// Host errno values (the C library's `errno.h` on this target: the UCRT's, with its POSIX
/// supplement, which mingw-w64 shares).
#[cfg(windows)]
#[allow(missing_docs)]
pub mod host_errno {
    pub const EPERM: i32 = 1;
    pub const ENOENT: i32 = 2;
    pub const ESRCH: i32 = 3;
    pub const EINTR: i32 = 4;
    pub const EIO: i32 = 5;
    pub const ENXIO: i32 = 6;
    pub const E2BIG: i32 = 7;
    pub const ENOEXEC: i32 = 8;
    pub const EBADF: i32 = 9;
    pub const ECHILD: i32 = 10;
    pub const EAGAIN: i32 = 11;
    pub const ENOMEM: i32 = 12;
    pub const EACCES: i32 = 13;
    pub const EFAULT: i32 = 14;
    pub const EBUSY: i32 = 16;
    pub const EEXIST: i32 = 17;
    pub const EXDEV: i32 = 18;
    pub const ENODEV: i32 = 19;
    pub const ENOTDIR: i32 = 20;
    pub const EISDIR: i32 = 21;
    pub const EINVAL: i32 = 22;
    pub const ENFILE: i32 = 23;
    pub const EMFILE: i32 = 24;
    pub const ENOTTY: i32 = 25;
    pub const EFBIG: i32 = 27;
    pub const ENOSPC: i32 = 28;
    pub const ESPIPE: i32 = 29;
    pub const EROFS: i32 = 30;
    pub const EMLINK: i32 = 31;
    pub const EPIPE: i32 = 32;
    pub const ERANGE: i32 = 34;
    pub const EDEADLK: i32 = 36;
    pub const ENAMETOOLONG: i32 = 38;
    pub const ENOSYS: i32 = 40;
    pub const ENOTEMPTY: i32 = 41;
    pub const ECANCELED: i32 = 105;
    pub const ELOOP: i32 = 114;
    pub const EOVERFLOW: i32 = 132;
    pub const ETIMEDOUT: i32 = 138;
}

/// Host errno values (the BSD numbering of Apple's `errno.h`).
#[cfg(not(any(target_os = "linux", target_os = "android", windows)))]
#[allow(missing_docs)]
pub mod host_errno {
    pub const EPERM: i32 = 1;
    pub const ENOENT: i32 = 2;
    pub const ESRCH: i32 = 3;
    pub const EINTR: i32 = 4;
    pub const EIO: i32 = 5;
    pub const ENXIO: i32 = 6;
    pub const E2BIG: i32 = 7;
    pub const ENOEXEC: i32 = 8;
    pub const EBADF: i32 = 9;
    pub const ECHILD: i32 = 10;
    pub const EDEADLK: i32 = 11;
    pub const ENOMEM: i32 = 12;
    pub const EACCES: i32 = 13;
    pub const EFAULT: i32 = 14;
    pub const EBUSY: i32 = 16;
    pub const EEXIST: i32 = 17;
    pub const EXDEV: i32 = 18;
    pub const ENODEV: i32 = 19;
    pub const ENOTDIR: i32 = 20;
    pub const EISDIR: i32 = 21;
    pub const EINVAL: i32 = 22;
    pub const ENFILE: i32 = 23;
    pub const EMFILE: i32 = 24;
    pub const ENOTTY: i32 = 25;
    pub const EFBIG: i32 = 27;
    pub const ENOSPC: i32 = 28;
    pub const ESPIPE: i32 = 29;
    pub const EROFS: i32 = 30;
    pub const EMLINK: i32 = 31;
    pub const EPIPE: i32 = 32;
    pub const ERANGE: i32 = 34;
    pub const EAGAIN: i32 = 35;
    pub const ETIMEDOUT: i32 = 60;
    pub const ELOOP: i32 = 62;
    pub const ENAMETOOLONG: i32 = 63;
    pub const ENOTEMPTY: i32 = 66;
    pub const ENOSYS: i32 = 78;
    pub const EOVERFLOW: i32 = 84;
    pub const ECANCELED: i32 = 89;
}

/// The guest's (FreeBSD) errno for the host errno `host`; 0 stays 0 and an error the table does
/// not know becomes `EIO` (5).
pub fn guest_errno(host: i32) -> i32 {
    use host_errno::*;
    match host {
        0 => 0,
        EPERM => 1,
        ENOENT => 2,
        ESRCH => 3,
        EINTR => 4,
        EIO => 5,
        ENXIO => 6,
        E2BIG => 7,
        ENOEXEC => 8,
        EBADF => 9,
        ECHILD => 10,
        EDEADLK => 11,
        ENOMEM => 12,
        EACCES => 13,
        EFAULT => 14,
        EBUSY => 16,
        EEXIST => 17,
        EXDEV => 18,
        ENODEV => 19,
        ENOTDIR => 20,
        EISDIR => 21,
        EINVAL => 22,
        ENFILE => 23,
        EMFILE => 24,
        ENOTTY => 25,
        EFBIG => 27,
        ENOSPC => 28,
        ESPIPE => 29,
        EROFS => 30,
        EMLINK => 31,
        EPIPE => 32,
        ERANGE => 34,
        EAGAIN => 35,
        ENAMETOOLONG => 63,
        ENOTEMPTY => 66,
        ETIMEDOUT => 60,
        ELOOP => 62,
        ENOSYS => 78,
        EOVERFLOW => 84,
        ECANCELED => 85,
        _ => 5,
    }
}

/// A libkernel error code: `0x80020000` with the guest errno in the low bits.
pub const fn kernel_error(guest_errno: i32) -> i32 {
    (0x8002_0000_u32 | guest_errno as u32) as i32
}

/// The host clock behind a guest clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostClock {
    /// Wall-clock time (`CLOCK_REALTIME`).
    Realtime,
    /// Wall-clock time at the scheduler tick (`CLOCK_REALTIME_COARSE`; plain realtime where the
    /// host has no coarse clock).
    RealtimeCoarse,
    /// Time since an arbitrary start that never jumps (`CLOCK_MONOTONIC`).
    Monotonic,
    /// CPU time of the calling thread.
    ThreadCpu,
    /// CPU time of the process.
    ProcessCpu,
}

/// The host clock for the guest (FreeBSD) clock id, or `None` for an id the guest may not use.
/// 13 (`CLOCK_SECOND`) reads [`HostClock::RealtimeCoarse`] with the nanoseconds cleared.
pub fn host_clock(id: u32) -> Option<HostClock> {
    Some(match id {
        0 | 9 | 10 => HostClock::Realtime, // REALTIME(_PRECISE/_FAST)
        4 | 5 | 7 | 8 | 11 | 12 => HostClock::Monotonic, // MONOTONIC/UPTIME
        13 => HostClock::RealtimeCoarse,   // SECOND
        14 => HostClock::ThreadCpu,
        2 | 15 => HostClock::ProcessCpu, // PROF/PROCTIME
        16..=19 => HostClock::Monotonic, // PS4 network clocks
        _ => return None,
    })
}

/// UTC seconds `utc` as local time, with the offset east of UTC in seconds that `offset_at`
/// gives for it (the host zone: [`crate::time::utc_offset`]).
pub fn utc_to_local(utc: i64, offset_at: impl Fn(i64) -> i64) -> (i64, i64) {
    let offset = offset_at(utc);
    (utc.wrapping_add(offset), offset)
}

/// Local seconds `local` as UTC, with the offset east of UTC in seconds. As the C did, the
/// offset is the one `offset_at` gives for `local` read as a UTC time.
pub fn local_to_utc(local: i64, offset_at: impl Fn(i64) -> i64) -> (i64, i64) {
    let offset = offset_at(local);
    (local.wrapping_sub(offset), offset)
}

/// The guest page size (`getpagesize`, `hw.pagesize`): the PS4's 16 KiB.
pub const PAGE_SIZE: i32 = 16384;

#[cfg(target_arch = "x86_64")]
pub use host::{monotonic_ns, sleep_ns};

#[cfg(target_arch = "x86_64")]
mod host {
    //! The host's clocks, sleep, CPU times and random bytes.

    use super::HostClock;
    use std::sync::OnceLock;

    /// A host time: seconds and nanoseconds.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct Time {
        pub(super) seconds: i64,
        pub(super) nanoseconds: i64,
    }

    #[cfg(unix)]
    mod sys {
        use super::{HostClock, Time};
        use std::ffi::{c_int, c_void};

        #[repr(C)]
        struct Timespec {
            seconds: i64,
            nanoseconds: i64,
        }

        // `suseconds_t` is 32-bit on Apple hosts and 64-bit elsewhere.
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
        struct Rusage {
            user: Timeval,
            system: Timeval,
            _rest: [i64; 14],
        }

        extern "C" {
            fn clock_gettime(clock: c_int, out: *mut Timespec) -> c_int;
            fn clock_getres(clock: c_int, out: *mut Timespec) -> c_int;
            fn nanosleep(request: *const Timespec, remaining: *mut Timespec) -> c_int;
            fn sched_yield() -> c_int;
            fn getrusage(who: c_int, out: *mut Rusage) -> c_int;
            fn getentropy(buffer: *mut c_void, size: usize) -> c_int;
        }

        #[cfg(any(target_os = "linux", target_os = "android"))]
        fn clock_id(clock: HostClock) -> c_int {
            match clock {
                HostClock::Realtime => 0,
                HostClock::Monotonic => 1,
                HostClock::ProcessCpu => 2,
                HostClock::ThreadCpu => 3,
                HostClock::RealtimeCoarse => 5,
            }
        }

        #[cfg(target_vendor = "apple")]
        fn clock_id(clock: HostClock) -> c_int {
            match clock {
                HostClock::Realtime | HostClock::RealtimeCoarse => 0,
                HostClock::Monotonic => 6,
                HostClock::ProcessCpu => 12,
                HostClock::ThreadCpu => 16,
            }
        }

        fn errno() -> i32 {
            std::io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(super::super::host_errno::EIO)
        }

        fn read(
            clock: HostClock,
            call: unsafe extern "C" fn(c_int, *mut Timespec) -> c_int,
        ) -> Result<Time, i32> {
            let mut out = Timespec {
                seconds: 0,
                nanoseconds: 0,
            };
            // SAFETY: `out` is a live `timespec` (two 64-bit fields on these LP64 hosts).
            if unsafe { call(clock_id(clock), &mut out) } != 0 {
                return Err(errno());
            }
            Ok(Time {
                seconds: out.seconds,
                nanoseconds: out.nanoseconds,
            })
        }

        pub(in super::super) fn now(clock: HostClock) -> Result<Time, i32> {
            read(clock, clock_gettime)
        }

        pub(in super::super) fn resolution(clock: HostClock) -> Result<Time, i32> {
            read(clock, clock_getres)
        }

        /// `nanosleep`, again for what is left after a signal; another failure ends the sleep.
        pub(in super::super) fn sleep_ns(ns: u64) {
            let mut request = Timespec {
                seconds: (ns / 1_000_000_000) as i64,
                nanoseconds: (ns % 1_000_000_000) as i64,
            };
            // SAFETY: both pointers are to the same live `timespec`, which `nanosleep` may
            // overwrite with the time left.
            while unsafe { nanosleep(&request, &mut request) } != 0 {
                if errno() != super::super::host_errno::EINTR {
                    break;
                }
            }
        }

        pub(in super::super) fn yield_now() {
            // SAFETY: no arguments; gives up the CPU.
            unsafe { sched_yield() };
        }

        /// User and system CPU time in microseconds of the process (`thread` false) or the
        /// calling thread (Apple hosts have no per-thread usage: the process's).
        pub(in super::super) fn cpu_times(thread: bool) -> Option<(i64, i64)> {
            #[cfg(any(target_os = "linux", target_os = "android"))]
            let who: c_int = if thread { 1 } else { 0 }; // RUSAGE_THREAD, RUSAGE_SELF
            #[cfg(target_vendor = "apple")]
            let who: c_int = {
                let _ = thread;
                0
            };
            let empty = || Timeval {
                seconds: 0,
                microseconds: 0,
            };
            let mut usage = Rusage {
                user: empty(),
                system: empty(),
                _rest: [0; 14],
            };
            // SAFETY: `usage` is a live `struct rusage` of the host's layout.
            if unsafe { getrusage(who, &mut usage) } != 0 {
                return None;
            }
            #[allow(clippy::useless_conversion)] // `i32` on Apple hosts
            let micros = |t: &Timeval| t.seconds * 1_000_000 + i64::from(t.microseconds);
            Some((micros(&usage.user), micros(&usage.system)))
        }

        /// Fills `buffer` with random bytes from the host (`getentropy`, 256 bytes a call).
        pub(in super::super) fn random(buffer: &mut [u8]) -> Result<(), i32> {
            for chunk in buffer.chunks_mut(256) {
                // SAFETY: `chunk` is live and writable for its length, at most 256 bytes.
                if unsafe { getentropy(chunk.as_mut_ptr().cast(), chunk.len()) } != 0 {
                    return Err(errno());
                }
            }
            Ok(())
        }
    }

    #[cfg(windows)]
    mod sys {
        //! What `compat_win.c` and the C runtime's `clock_gettime` (winpthreads) did.

        use super::{HostClock, Time};
        use std::cell::Cell;
        use std::ffi::c_void;

        type Handle = *mut c_void;
        const CREATE_WAITABLE_TIMER_HIGH_RESOLUTION: u32 = 2;
        const TIMER_ALL_ACCESS: u32 = 0x1F_0003;
        const INFINITE: u32 = u32::MAX;
        const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 2;
        /// 100 ns units from 1601-01-01 to 1970-01-01.
        const UNIX_EPOCH_FILETIME: u64 = 116_444_736_000_000_000;

        #[link(name = "kernel32")]
        extern "system" {
            fn QueryPerformanceCounter(count: *mut i64) -> i32;
            fn QueryPerformanceFrequency(frequency: *mut i64) -> i32;
            fn GetSystemTimePreciseAsFileTime(time: *mut u64);
            fn GetSystemTimeAdjustment(adjustment: *mut u32, increment: *mut u32, off: *mut i32)
                -> i32;
            fn GetCurrentProcess() -> Handle;
            fn GetCurrentThread() -> Handle;
            fn GetProcessTimes(
                process: Handle,
                created: *mut u64,
                exited: *mut u64,
                kernel: *mut u64,
                user: *mut u64,
            ) -> i32;
            fn GetThreadTimes(
                thread: Handle,
                created: *mut u64,
                exited: *mut u64,
                kernel: *mut u64,
                user: *mut u64,
            ) -> i32;
            fn SwitchToThread() -> i32;
            fn Sleep(milliseconds: u32);
            fn CreateWaitableTimerExW(
                attributes: *const c_void,
                name: *const u16,
                flags: u32,
                access: u32,
            ) -> Handle;
            fn SetWaitableTimer(
                timer: Handle,
                due: *const i64,
                period: i32,
                routine: *const c_void,
                argument: *const c_void,
                resume: i32,
            ) -> i32;
            fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        }

        #[link(name = "bcrypt")]
        extern "system" {
            fn BCryptGenRandom(algorithm: Handle, buffer: *mut u8, size: u32, flags: u32) -> i32;
        }

        fn frequency() -> i64 {
            let mut frequency = 0i64;
            // SAFETY: writes one i64; cannot fail on Windows XP and later.
            unsafe { QueryPerformanceFrequency(&mut frequency) };
            frequency.max(1)
        }

        fn from_100ns(units: u64) -> Time {
            Time {
                seconds: (units / 10_000_000) as i64,
                nanoseconds: (units % 10_000_000 * 100) as i64,
            }
        }

        /// CPU time (kernel plus user) of the process or this thread, in 100 ns units.
        fn cpu_100ns(thread: bool) -> Option<(u64, u64)> {
            let (mut created, mut exited, mut kernel, mut user) = (0u64, 0u64, 0u64, 0u64);
            // SAFETY: the pseudo-handles of this process and thread; four live FILETIMEs (a
            // u64 has the layout of a FILETIME on this little-endian host).
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
            (ok != 0).then_some((user, kernel))
        }

        pub(in super::super) fn now(clock: HostClock) -> Result<Time, i32> {
            match clock {
                HostClock::Realtime | HostClock::RealtimeCoarse => {
                    let mut time = 0u64;
                    // SAFETY: writes one FILETIME.
                    unsafe { GetSystemTimePreciseAsFileTime(&mut time) };
                    Ok(from_100ns(time.saturating_sub(UNIX_EPOCH_FILETIME)))
                }
                HostClock::Monotonic => {
                    let mut count = 0i64;
                    // SAFETY: writes one i64; cannot fail on Windows XP and later.
                    unsafe { QueryPerformanceCounter(&mut count) };
                    let frequency = frequency();
                    Ok(Time {
                        seconds: count / frequency,
                        nanoseconds: ((count % frequency) as i128 * 1_000_000_000
                            / frequency as i128) as i64,
                    })
                }
                HostClock::ThreadCpu | HostClock::ProcessCpu => {
                    let (user, kernel) = cpu_100ns(clock == HostClock::ThreadCpu)
                        .ok_or(super::super::host_errno::EINVAL)?;
                    Ok(from_100ns(user + kernel))
                }
            }
        }

        pub(in super::super) fn resolution(clock: HostClock) -> Result<Time, i32> {
            if clock == HostClock::Monotonic {
                let frequency = frequency();
                let nanoseconds = (1_000_000_000 + (frequency >> 1)) / frequency;
                return Ok(Time {
                    seconds: 0,
                    nanoseconds: nanoseconds.max(1),
                });
            }
            let (mut adjustment, mut increment, mut off) = (0u32, 0u32, 0i32);
            // SAFETY: three live outputs.
            if unsafe { GetSystemTimeAdjustment(&mut adjustment, &mut increment, &mut off) } == 0 {
                return Err(super::super::host_errno::EINVAL);
            }
            Ok(from_100ns(u64::from(increment)))
        }

        thread_local! {
            /// One high-resolution timer per thread (Windows 10 1803+); a plain waitable timer
            /// before; null when neither could be made.
            static SLEEP_TIMER: Cell<Handle> = const { Cell::new(std::ptr::null_mut()) };
        }

        /// `compat_sleep_ns`: a zero sleep gives up the CPU; others wait on the thread's timer
        /// (rounded up to 100 ns), or `Sleep` rounded up to milliseconds without one.
        pub(in super::super) fn sleep_ns(ns: u64) {
            if ns == 0 {
                // SAFETY: no arguments.
                unsafe { SwitchToThread() };
                return;
            }
            let timer = SLEEP_TIMER.with(|timer| {
                if timer.get().is_null() {
                    // SAFETY: an unnamed timer with default security.
                    let mut made = unsafe {
                        CreateWaitableTimerExW(
                            std::ptr::null(),
                            std::ptr::null(),
                            CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                            TIMER_ALL_ACCESS,
                        )
                    };
                    if made.is_null() {
                        // SAFETY: as above, without the high-resolution flag.
                        made = unsafe {
                            CreateWaitableTimerExW(
                                std::ptr::null(),
                                std::ptr::null(),
                                0,
                                TIMER_ALL_ACCESS,
                            )
                        };
                    }
                    timer.set(made);
                }
                timer.get()
            });
            let due = -(ns.div_ceil(100).min(i64::MAX as u64) as i64); // relative, 100 ns units
            // SAFETY: `timer` is this thread's timer handle (or null, checked); `due` is live.
            let set = !timer.is_null()
                && unsafe {
                    SetWaitableTimer(timer, &due, 0, std::ptr::null(), std::ptr::null(), 0)
                } != 0;
            if set {
                // SAFETY: a valid timer handle that was just set.
                unsafe { WaitForSingleObject(timer, INFINITE) };
            } else {
                let ms = ns.div_ceil(1_000_000).min(u64::from(u32::MAX - 1)) as u32;
                // SAFETY: plain sleep.
                unsafe { Sleep(ms) };
            }
        }

        pub(in super::super) fn yield_now() {
            // SAFETY: plain sleep of zero: gives up the rest of the time slice.
            unsafe { Sleep(0) };
        }

        pub(in super::super) fn cpu_times(thread: bool) -> Option<(i64, i64)> {
            let (user, kernel) = cpu_100ns(thread)?;
            Some(((user / 10) as i64, (kernel / 10) as i64))
        }

        pub(in super::super) fn random(buffer: &mut [u8]) -> Result<(), i32> {
            for chunk in buffer.chunks_mut(0x1000_0000) {
                // SAFETY: `chunk` is live and writable for its length (below 2^32).
                let status = unsafe {
                    BCryptGenRandom(
                        std::ptr::null_mut(),
                        chunk.as_mut_ptr(),
                        chunk.len() as u32,
                        BCRYPT_USE_SYSTEM_PREFERRED_RNG,
                    )
                };
                if status != 0 {
                    return Err(super::super::host_errno::EIO);
                }
            }
            Ok(())
        }
    }

    pub(super) use sys::{cpu_times, random, resolution, yield_now};

    /// The host clock `clock` now, or the host errno.
    pub(super) fn now(clock: HostClock) -> Result<Time, i32> {
        sys::now(clock)
    }

    /// The monotonic clock in nanoseconds.
    pub fn monotonic_ns() -> u64 {
        let time = sys::now(HostClock::Monotonic).unwrap_or(Time {
            seconds: 0,
            nanoseconds: 0,
        });
        (time.seconds as u64)
            .wrapping_mul(1_000_000_000)
            .wrapping_add(time.nanoseconds as u64)
    }

    /// Sleeps `ns` nanoseconds (a zero sleep still enters the host's sleep, as the C did).
    pub fn sleep_ns(ns: u64) {
        sys::sleep_ns(ns)
    }

    static PROCESS_START: OnceLock<u64> = OnceLock::new();

    /// The monotonic time the guest's process time counts from: the first call (`runtime_start`
    /// makes one before guest code runs).
    pub(super) fn process_start() -> u64 {
        *PROCESS_START.get_or_init(monotonic_ns)
    }
}

/// Marks the start of the guest's process time (`sceKernelGetProcessTime` counts from here);
/// later calls change nothing. `runtime_start` calls it.
#[cfg(target_arch = "x86_64")]
pub fn mark_process_start() {
    host::process_start();
}

/// Nanoseconds since [`mark_process_start`] (or the first process-time read).
#[cfg(target_arch = "x86_64")]
pub fn process_time_ns() -> u64 {
    let start = host::process_start();
    host::monotonic_ns().saturating_sub(start)
}

/// The TSC frequency in Hz, measured once over a 20 ms sleep.
#[cfg(target_arch = "x86_64")]
pub fn tsc_frequency() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static TSC_HZ: AtomicU64 = AtomicU64::new(0);
    let known = TSC_HZ.load(Ordering::Relaxed);
    if known != 0 {
        return known;
    }
    let a = host::monotonic_ns();
    // SAFETY: `rdtsc` is available on every x86-64 CPU.
    let t0 = unsafe { std::arch::x86_64::_rdtsc() };
    host::sleep_ns(20_000_000);
    let b = host::monotonic_ns();
    // SAFETY: as above.
    let t1 = unsafe { std::arch::x86_64::_rdtsc() };
    let ns = b.wrapping_sub(a).max(1);
    let hz = (u128::from(t1.wrapping_sub(t0)) * 1_000_000_000 / u128::from(ns)) as u64;
    TSC_HZ.store(hz, Ordering::Relaxed);
    hz
}

/// The guest errno of this thread: `runtime_errno()` in the thread module (loader build), or a
/// thread-local of this module until the thread module is Rust.
#[cfg(all(target_arch = "x86_64", uncharted_loader))]
pub(crate) fn errno_slot() -> *mut i32 {
    extern "C" {
        fn runtime_errno() -> *mut i32;
    }
    // SAFETY: returns this thread's guest errno, valid for the thread's lifetime.
    unsafe { runtime_errno() }
}

#[cfg(all(target_arch = "x86_64", not(uncharted_loader)))]
thread_local! {
    static GUEST_ERRNO: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

/// The guest errno of this thread (see the loader build's variant).
#[cfg(all(target_arch = "x86_64", not(uncharted_loader)))]
pub(crate) fn errno_slot() -> *mut i32 {
    GUEST_ERRNO.with(std::cell::Cell::as_ptr)
}

#[cfg(target_arch = "x86_64")]
mod guest {
    //! The guest's libkernel and libScePosix functions (System V ABI on every host).

    use super::*;
    use crate::abi::host as c;
    use std::cell::Cell;
    use std::ffi::{c_char, c_int, c_void, CStr};
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
    use std::sync::Mutex;

    const EINVAL: i32 = 22;

    fn set_errno(value: i32) {
        // SAFETY: this thread's guest errno slot.
        unsafe { *errno_slot() = value };
    }

    fn errno() -> i32 {
        // SAFETY: this thread's guest errno slot.
        unsafe { *errno_slot() }
    }

    /// A POSIX failure: the guest errno for host errno `host`, and -1.
    fn fail_posix(host: i32) -> i32 {
        set_errno(guest_errno(host));
        -1
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub(super) struct GuestTimespec {
        pub(super) seconds: i64,
        pub(super) nanoseconds: i64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub(super) struct GuestTimeval {
        pub(super) seconds: i64,
        pub(super) microseconds: i64,
    }

    #[repr(C)]
    pub(super) struct GuestTimezone {
        pub(super) minutes_west: i32,
        pub(super) dst_time: i32,
    }

    /// `SceKernelTimesec`: `west_sec` carries the offset east of UTC in seconds, as libkernel
    /// fills it.
    #[repr(C)]
    pub(super) struct GuestTimesec {
        pub(super) t: i64,
        pub(super) west_sec: u32,
        pub(super) dst_sec: u32,
    }

    /// The guest clock `id` now, or the host errno.
    fn clock_read(id: u32) -> Result<GuestTimespec, i32> {
        let clock = host_clock(id).ok_or(host_errno::EINVAL)?;
        let time = host::now(clock)?;
        Ok(GuestTimespec {
            seconds: time.seconds,
            nanoseconds: if id == 13 { 0 } else { time.nanoseconds },
        })
    }

    pub(super) extern "sysv64" fn kernel_clock_gettime(id: u32, ts: *mut GuestTimespec) -> i32 {
        if ts.is_null() {
            return kernel_error(guest_errno(host_errno::EINVAL));
        }
        match clock_read(id) {
            Ok(now) => {
                // SAFETY: the guest passes its `timespec` (checked non-null).
                unsafe { *ts = now };
                0
            }
            Err(host) => kernel_error(guest_errno(host)),
        }
    }

    pub(super) extern "sysv64" fn posix_clock_gettime(id: u32, ts: *mut GuestTimespec) -> i32 {
        if ts.is_null() {
            return fail_posix(host_errno::EINVAL);
        }
        match clock_read(id) {
            Ok(now) => {
                // SAFETY: the guest passes its `timespec` (checked non-null).
                unsafe { *ts = now };
                0
            }
            Err(host) => fail_posix(host),
        }
    }

    pub(super) extern "sysv64" fn posix_clock_getres(id: u32, ts: *mut GuestTimespec) -> i32 {
        let Some(clock) = host_clock(id) else {
            return fail_posix(host_errno::EINVAL);
        };
        let time = match host::resolution(clock) {
            Ok(time) => time,
            Err(host) => return fail_posix(host),
        };
        // SAFETY: the guest passes null or its `timespec`.
        if let Some(ts) = unsafe { ts.as_mut() } {
            *ts = GuestTimespec {
                seconds: time.seconds,
                nanoseconds: time.nanoseconds,
            };
        }
        0
    }

    pub(super) extern "sysv64" fn process_time() -> u64 {
        process_time_ns() / 1000
    }

    pub(super) extern "sysv64" fn process_time_counter() -> u64 {
        process_time_ns()
    }

    pub(super) extern "sysv64" fn process_time_frequency() -> u64 {
        1_000_000_000
    }

    pub(super) extern "sysv64" fn read_tsc() -> u64 {
        // SAFETY: `rdtsc` is available on every x86-64 CPU.
        unsafe { std::arch::x86_64::_rdtsc() }
    }

    pub(super) extern "sysv64" fn guest_tsc_frequency() -> u64 {
        tsc_frequency()
    }

    pub(super) extern "sysv64" fn kernel_usleep(microseconds: u32) -> i32 {
        sleep_ns(u64::from(microseconds) * 1000);
        0
    }

    pub(super) extern "sysv64" fn posix_sleep(seconds: u32) -> u32 {
        sleep_ns(u64::from(seconds) * 1_000_000_000);
        0
    }

    pub(super) extern "sysv64" fn posix_nanosleep(
        request: *const GuestTimespec,
        remaining: *mut GuestTimespec,
    ) -> i32 {
        // SAFETY: the guest passes null or its `timespec`.
        let Some(&request) = (unsafe { request.as_ref() }) else {
            return fail_posix(host_errno::EINVAL);
        };
        if request.nanoseconds < 0 || request.nanoseconds >= 1_000_000_000 || request.seconds < 0
        {
            return fail_posix(host_errno::EINVAL);
        }
        sleep_ns(
            (request.seconds as u64)
                .wrapping_mul(1_000_000_000)
                .wrapping_add(request.nanoseconds as u64),
        );
        // SAFETY: the guest passes null or its `timespec`.
        if let Some(remaining) = unsafe { remaining.as_mut() } {
            *remaining = GuestTimespec {
                seconds: 0,
                nanoseconds: 0,
            };
        }
        0
    }

    pub(super) extern "sysv64" fn kernel_nanosleep(
        request: *const GuestTimespec,
        remaining: *mut GuestTimespec,
    ) -> i32 {
        if posix_nanosleep(request, remaining) != 0 {
            kernel_error(errno())
        } else {
            0
        }
    }

    fn now_seconds() -> i64 {
        host::now(HostClock::Realtime).map_or(0, |time| time.seconds)
    }

    pub(super) extern "sysv64" fn kernel_gettimezone(zone: *mut GuestTimezone) -> i32 {
        // SAFETY: the guest passes null or its `timezone`.
        let Some(zone) = (unsafe { zone.as_mut() }) else {
            return kernel_error(EINVAL);
        };
        zone.minutes_west = (-crate::time::utc_offset(now_seconds()) / 60) as i32;
        zone.dst_time = 0;
        0
    }

    pub(super) extern "sysv64" fn posix_gettimeofday(
        value: *mut GuestTimeval,
        zone: *mut GuestTimezone,
    ) -> i32 {
        let now = host::now(HostClock::Realtime).unwrap_or(host::Time {
            seconds: 0,
            nanoseconds: 0,
        });
        // SAFETY: the guest passes null or its `timeval`.
        if let Some(value) = unsafe { value.as_mut() } {
            *value = GuestTimeval {
                seconds: now.seconds,
                microseconds: now.nanoseconds / 1000,
            };
        }
        if !zone.is_null() {
            kernel_gettimezone(zone);
        }
        0
    }

    pub(super) extern "sysv64" fn kernel_gettimeofday(value: *mut GuestTimeval) -> i32 {
        if value.is_null() {
            return kernel_error(EINVAL);
        }
        posix_gettimeofday(value, std::ptr::null_mut())
    }

    pub(super) extern "sysv64" fn posix_time(out: *mut i64) -> i64 {
        let now = now_seconds();
        // SAFETY: the guest passes null or its `time_t`.
        if let Some(out) = unsafe { out.as_mut() } {
            *out = now;
        }
        now
    }

    pub(super) extern "sysv64" fn convert_utc_to_local(
        utc: i64,
        local: *mut i64,
        timesec: *mut GuestTimesec,
        dst: *mut u64,
    ) -> i32 {
        let (local_time, offset) = utc_to_local(utc, crate::time::utc_offset);
        // SAFETY: the guest passes null or its outputs, each of its type.
        unsafe {
            if let Some(local) = local.as_mut() {
                *local = local_time;
            }
            if let Some(timesec) = timesec.as_mut() {
                *timesec = GuestTimesec {
                    t: utc,
                    west_sec: offset as u32,
                    dst_sec: 0,
                };
            }
            if let Some(dst) = dst.as_mut() {
                *dst = 0;
            }
        }
        0
    }

    pub(super) extern "sysv64" fn convert_local_to_utc(
        local: i64,
        _unused: i64,
        utc: *mut i64,
        zone: *mut GuestTimezone,
        dst: *mut i32,
    ) -> i32 {
        let (utc_time, offset) = local_to_utc(local, crate::time::utc_offset);
        // SAFETY: the guest passes null or its outputs, each of its type.
        unsafe {
            if let Some(utc) = utc.as_mut() {
                *utc = utc_time;
            }
            if let Some(zone) = zone.as_mut() {
                zone.minutes_west = (-offset / 60) as i32;
                zone.dst_time = 0;
            }
            if let Some(dst) = dst.as_mut() {
                *dst = 0;
            }
        }
        0
    }

    /// Debugger range names and page locking have no effect on a PC (memory is never paged to
    /// disc).
    pub(super) extern "sysv64" fn name_range(
        _address: *mut c_void,
        _size: u64,
        _name: *const c_char,
    ) -> i32 {
        0
    }

    pub(super) extern "sysv64" fn lock_range(_address: *mut c_void, _size: u64) -> i32 {
        0
    }

    pub(super) extern "sysv64" fn get_pagesize() -> i32 {
        PAGE_SIZE
    }

    pub(super) extern "sysv64" fn get_pid() -> i32 {
        1000
    }

    pub(super) extern "sysv64" fn guest_yield() -> i32 {
        host::yield_now();
        0
    }

    pub(super) extern "sysv64" fn hard_exit(status: c_int) -> ! {
        c::out_line(&format!("Runtime: guest requested _exit({status})"));
        crate::abi::libc::runtime_report();
        c::flush();
        c::quit(status)
    }

    pub(super) extern "sysv64" fn raise_exception(code: u32, argument: u64) -> ! {
        c::err(&format!(
            "STOP: guest raised debug exception code={code:#x} argument={argument:#x}\n"
        ));
        crate::abi::libc::runtime_report();
        // SAFETY: the C `exit`, as the C module ended here (code 23).
        unsafe { c::exit(23) }
    }

    pub(super) extern "sysv64" fn print_backtrace() -> i32 {
        c::err("Runtime: guest requested a backtrace (not available)\n");
        0
    }

    /// The guest's `sigset_t`: 128 signal bits.
    #[repr(C)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct GuestSigset {
        pub(super) bits: [u32; 4],
    }

    thread_local! {
        static SIGNAL_MASK: Cell<[u32; 4]> = const { Cell::new([0; 4]) };
    }
    static HANDLERS: [AtomicUsize; 128] = [const { AtomicUsize::new(0) }; 128];

    /// `signal`: the handler is recorded and the previous one returned; host signals are not
    /// forwarded. A bad signal number is `SIG_ERR` with errno `EINVAL`.
    pub(super) extern "sysv64" fn guest_signal(signal: c_int, handler: usize) -> usize {
        if signal <= 0 || signal >= 128 {
            set_errno(EINVAL);
            return usize::MAX; // SIG_ERR
        }
        let old = HANDLERS[signal as usize].swap(handler, Ordering::AcqRel);
        c::out_line(&format!(
            "Runtime: guest signal({signal}) handler recorded (host signals are not forwarded)"
        ));
        old
    }

    pub(super) extern "sysv64" fn guest_sigprocmask(
        how: c_int,
        set: *const GuestSigset,
        old: *mut GuestSigset,
    ) -> i32 {
        let mask = SIGNAL_MASK.get();
        // SAFETY: the guest passes null or its `sigset_t`.
        if let Some(old) = unsafe { old.as_mut() } {
            old.bits = mask;
        }
        // SAFETY: the guest passes null or its `sigset_t`.
        let Some(set) = (unsafe { set.as_ref() }) else {
            return 0;
        };
        let mut next = mask;
        for (bits, &change) in next.iter_mut().zip(&set.bits) {
            match how {
                1 => *bits |= change,  // SIG_BLOCK
                2 => *bits &= !change, // SIG_UNBLOCK
                3 => *bits = change,   // SIG_SETMASK
                _ => return fail_posix(host_errno::EINVAL),
            }
        }
        SIGNAL_MASK.set(next);
        0
    }

    pub(super) extern "sysv64" fn guest_sigfillset(set: *mut GuestSigset) -> i32 {
        // SAFETY: the guest passes null or its `sigset_t`.
        let Some(set) = (unsafe { set.as_mut() }) else {
            return fail_posix(host_errno::EINVAL);
        };
        set.bits = [u32::MAX; 4];
        0
    }

    pub(super) extern "sysv64" fn guest_sigemptyset(set: *mut GuestSigset) -> i32 {
        // SAFETY: the guest passes null or its `sigset_t`.
        let Some(set) = (unsafe { set.as_mut() }) else {
            return fail_posix(host_errno::EINVAL);
        };
        set.bits = [0; 4];
        0
    }

    /// The guest's `struct rusage`: user and system time, then 14 counters (left at 0).
    #[repr(C)]
    pub(super) struct GuestRusage {
        pub(super) user: GuestTimeval,
        pub(super) system: GuestTimeval,
        pub(super) rest: [i64; 14],
    }

    pub(super) extern "sysv64" fn guest_getrusage(who: c_int, out: *mut GuestRusage) -> i32 {
        // SAFETY: the guest passes null or its `struct rusage`.
        let Some(out) = (unsafe { out.as_mut() }) else {
            return fail_posix(host_errno::EINVAL);
        };
        if who != 0 && who != 1 {
            return fail_posix(host_errno::EINVAL);
        }
        let (user, system) = host::cpu_times(who == 1).unwrap_or((0, 0));
        let timeval = |us: i64| GuestTimeval {
            seconds: us / 1_000_000,
            microseconds: us % 1_000_000,
        };
        *out = GuestRusage {
            user: timeval(user),
            system: timeval(system),
            rest: [0; 14],
        };
        0
    }

    pub(super) extern "sysv64" fn guest_sysctl(
        name: *const i32,
        name_len: u32,
        old: *mut c_void,
        old_len: *mut u64,
        new_value: *const c_void,
        _new_len: u64,
    ) -> i32 {
        if name.is_null() || name_len < 2 || !new_value.is_null() {
            return fail_posix(host_errno::EINVAL);
        }
        // SAFETY: the guest passes `name_len` (at least 2) MIB entries at `name`.
        let mib = unsafe { std::slice::from_raw_parts(name, name_len as usize) };
        if mib[0] == 1 && mib[1] == 37 {
            // kern.arandom
            if old.is_null() || old_len.is_null() {
                return fail_posix(host_errno::EINVAL);
            }
            // SAFETY: the guest's buffer holds `*old_len` bytes.
            let buffer =
                unsafe { std::slice::from_raw_parts_mut(old.cast::<u8>(), *old_len as usize) };
            return match host::random(buffer) {
                Ok(()) => 0,
                Err(host) => fail_posix(host),
            };
        }
        if mib[0] == 6 && (mib[1] == 7 || mib[1] == 3) {
            // hw.pagesize / hw.ncpu
            if old_len.is_null() {
                return fail_posix(host_errno::EINVAL);
            }
            let value: i32 = if mib[1] == 7 { PAGE_SIZE } else { 7 };
            // SAFETY: the guest passes its length and null or a buffer of that many bytes.
            unsafe {
                if !old.is_null() {
                    if *old_len < 4 {
                        return fail_posix(host_errno::ENOMEM);
                    }
                    std::ptr::write_unaligned(old.cast::<i32>(), value);
                }
                *old_len = 4;
            }
            return 0;
        }
        let shown: Vec<String> = mib.iter().take(8).map(i32::to_string).collect();
        crate::abi::libc::stop(&format!(
            "STOP: unsupported sysctl mib {}\n",
            shown.join(" ")
        ))
    }

    type OnceRoutine = Option<extern "sysv64" fn()>;

    /// `scePthreadOnce`: 0 never ran, 2 running, 1 done; other callers wait for the first.
    pub(super) extern "sysv64" fn thread_once(once: *mut i32, routine: OnceRoutine) -> i32 {
        let Some(routine) = routine else {
            return kernel_error(EINVAL);
        };
        if once.is_null() {
            return kernel_error(EINVAL);
        }
        // SAFETY: the guest's `pthread_once_t` starts with this int, aligned, and is only ever
        // used atomically.
        let state = unsafe { AtomicI32::from_ptr(once) };
        loop {
            match state.load(Ordering::Acquire) {
                1 => return 0,
                0 if state
                    .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok() =>
                {
                    routine();
                    state.store(1, Ordering::Release);
                    return 0;
                }
                _ => host::yield_now(),
            }
        }
    }

    pub(super) extern "sysv64" fn posix_once(once: *mut i32, routine: OnceRoutine) -> i32 {
        if thread_once(once, routine) != 0 {
            22
        } else {
            0
        }
    }

    const KEYS: usize = 256;
    type KeyDestructor = extern "sysv64" fn(*mut c_void);

    static KEY_USED: [AtomicBool; KEYS] = [const { AtomicBool::new(false) }; KEYS];
    static KEY_DESTRUCTOR: [AtomicUsize; KEYS] = [const { AtomicUsize::new(0) }; KEYS];
    static KEY_LOCK: Mutex<()> = Mutex::new(());

    thread_local! {
        static KEY_VALUES: [Cell<usize>; KEYS] = const { [const { Cell::new(0) }; KEYS] };
    }

    pub(super) extern "sysv64" fn key_create(
        key: *mut u32,
        destructor: Option<KeyDestructor>,
    ) -> i32 {
        if key.is_null() {
            return kernel_error(EINVAL);
        }
        let _lock = KEY_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(free) = (1..KEYS).find(|&i| !KEY_USED[i].load(Ordering::Acquire)) else {
            return kernel_error(35); // EAGAIN
        };
        KEY_DESTRUCTOR[free].store(destructor.map_or(0, |d| d as usize), Ordering::Release);
        KEY_USED[free].store(true, Ordering::Release);
        // SAFETY: the guest passes its `pthread_key_t` (checked non-null).
        unsafe { *key = free as u32 };
        0
    }

    pub(super) extern "sysv64" fn key_delete(key: u32) -> i32 {
        let key = key as usize;
        if key == 0 || key >= KEYS {
            return kernel_error(EINVAL);
        }
        let _lock = KEY_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let used = KEY_USED[key].swap(false, Ordering::AcqRel);
        KEY_DESTRUCTOR[key].store(0, Ordering::Release);
        if used {
            0
        } else {
            kernel_error(EINVAL)
        }
    }

    pub(super) extern "sysv64" fn key_set(key: u32, value: *mut c_void) -> i32 {
        let key = key as usize;
        if key == 0 || key >= KEYS || !KEY_USED[key].load(Ordering::Acquire) {
            return kernel_error(EINVAL);
        }
        KEY_VALUES.with(|values| values[key].set(value as usize));
        0
    }

    pub(super) extern "sysv64" fn key_get(key: u32) -> *mut c_void {
        let key = key as usize;
        if key == 0 || key >= KEYS {
            return std::ptr::null_mut();
        }
        KEY_VALUES.with(|values| values[key].get() as *mut c_void)
    }

    /// At a guest thread's exit: each non-null value of a key with a destructor is cleared and
    /// passed to it, for up to four rounds while destructors set new values.
    pub(super) fn thread_keys_cleanup() {
        for _round in 0..4 {
            let mut any = false;
            for key in 1..KEYS {
                let value = KEY_VALUES.with(|values| values[key].get());
                let destructor = KEY_DESTRUCTOR[key].load(Ordering::Acquire);
                if value == 0 || !KEY_USED[key].load(Ordering::Acquire) || destructor == 0 {
                    continue;
                }
                KEY_VALUES.with(|values| values[key].set(0));
                any = true;
                // SAFETY: a destructor the guest registered with `key_create`.
                let destructor: KeyDestructor = unsafe { std::mem::transmute(destructor) };
                destructor(value as *mut c_void);
            }
            if !any {
                break;
            }
        }
    }

    /// The POSIX form of a `scePthread*` result: 0, or the errno in its low 16 bits.
    fn posix_result(result: i32) -> i32 {
        if result != 0 {
            result & 0xffff
        } else {
            0
        }
    }

    pub(super) extern "sysv64" fn posix_key_create(
        key: *mut u32,
        destructor: Option<KeyDestructor>,
    ) -> i32 {
        posix_result(key_create(key, destructor))
    }

    pub(super) extern "sysv64" fn posix_key_delete(key: u32) -> i32 {
        posix_result(key_delete(key))
    }

    pub(super) extern "sysv64" fn posix_key_set(key: u32, value: *mut c_void) -> i32 {
        posix_result(key_set(key, value))
    }

    /// The function behind the symbol name `symbol`, or `None`.
    pub(super) fn by_symbol(symbol: &str) -> Option<usize> {
        let address = match symbol {
            "sceKernelClockGettime" => kernel_clock_gettime as *const () as usize,
            "clock_gettime" => posix_clock_gettime as *const () as usize,
            "clock_getres" => posix_clock_getres as *const () as usize,
            "sceKernelGetProcessTime" => process_time as *const () as usize,
            "sceKernelGetProcessTimeCounter" => process_time_counter as *const () as usize,
            "sceKernelGetProcessTimeCounterFrequency" => {
                process_time_frequency as *const () as usize
            }
            "sceKernelReadTsc" => read_tsc as *const () as usize,
            "sceKernelGetTscFrequency" => guest_tsc_frequency as *const () as usize,
            "sceKernelUsleep" | "usleep" => kernel_usleep as *const () as usize,
            "sleep" => posix_sleep as *const () as usize,
            "nanosleep" => posix_nanosleep as *const () as usize,
            "sceKernelNanosleep" => kernel_nanosleep as *const () as usize,
            "sceKernelGettimezone" => kernel_gettimezone as *const () as usize,
            "gettimeofday" => posix_gettimeofday as *const () as usize,
            "sceKernelGettimeofday" => kernel_gettimeofday as *const () as usize,
            "time" => posix_time as *const () as usize,
            "sceKernelConvertUtcToLocaltime" => convert_utc_to_local as *const () as usize,
            "sceKernelConvertLocaltimeToUtc" => convert_local_to_utc as *const () as usize,
            "sceKernelSetVirtualRangeName" => name_range as *const () as usize,
            "sceKernelMlock" => lock_range as *const () as usize,
            "getpagesize" => get_pagesize as *const () as usize,
            "getpid" => get_pid as *const () as usize,
            "sched_yield" => guest_yield as *const () as usize,
            "_exit" => hard_exit as *const () as usize,
            "sceKernelDebugRaiseException" | "sceKernelDebugRaiseExceptionOnReleaseMode" => {
                raise_exception as *const () as usize
            }
            "sceKernelPrintBacktraceWithModuleInfo" => print_backtrace as *const () as usize,
            "signal" => guest_signal as *const () as usize,
            "sigprocmask" | "_sigprocmask" => guest_sigprocmask as *const () as usize,
            "sigfillset" => guest_sigfillset as *const () as usize,
            "sigemptyset" => guest_sigemptyset as *const () as usize,
            "getrusage" => guest_getrusage as *const () as usize,
            "sysctl" => guest_sysctl as *const () as usize,
            "scePthreadOnce" => thread_once as *const () as usize,
            "pthread_once" => posix_once as *const () as usize,
            "scePthreadKeyCreate" => key_create as *const () as usize,
            "scePthreadKeyDelete" => key_delete as *const () as usize,
            "scePthreadSetspecific" => key_set as *const () as usize,
            "scePthreadGetspecific" | "pthread_getspecific" => key_get as *const () as usize,
            "pthread_key_create" => posix_key_create as *const () as usize,
            "pthread_key_delete" => posix_key_delete as *const () as usize,
            "pthread_setspecific" => posix_key_set as *const () as usize,
            _ => return None,
        };
        Some(address)
    }

    /// The function behind the scoped import `name`, found by its symbol in the NID table.
    pub(super) fn resolve(name: &CStr) -> Option<usize> {
        by_symbol(crate::abi::nid_name(name.to_str().ok()?)?)
    }
}

/// The guest function behind the scoped import `name`, or `None`. Only x86-64 hosts run guest
/// code.
#[cfg(target_arch = "x86_64")]
pub fn resolve(name: &std::ffi::CStr) -> Option<usize> {
    guest::resolve(name)
}

/// The exported C functions of this module, declared in `include/uncharted_runtime.h`.
#[cfg(target_arch = "x86_64")]
pub mod ffi {
    use std::ffi::{c_char, c_int, CStr};

    /// The address of the guest libkernel/libScePosix function behind the scoped import `name`,
    /// or 0 (the C `runtime_kernel_resolve`, now in `crates/runtime/src/kernel.rs`).
    ///
    /// # Safety
    /// `name` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_kernel_resolve(name: *const c_char) -> usize {
        if name.is_null() {
            return 0;
        }
        // SAFETY: the caller passes a NUL-terminated string.
        super::resolve(unsafe { CStr::from_ptr(name) }).unwrap_or(0)
    }

    /// The guest's (FreeBSD) errno for a host errno; unknown errors are `EIO` (5).
    #[no_mangle]
    pub extern "C" fn runtime_guest_errno(host_errno: c_int) -> i32 {
        super::guest_errno(host_errno)
    }

    /// The guest's process time in microseconds; the GPU library stamps flips and labels with
    /// it, so guest and GPU share one clock.
    #[no_mangle]
    pub extern "C" fn runtime_process_time_us() -> u64 {
        super::process_time_ns() / 1000
    }

    /// The guest's process time counter, in nanoseconds.
    #[no_mangle]
    pub extern "C" fn runtime_process_time_counter() -> u64 {
        super::process_time_ns()
    }

    /// The TSC frequency in Hz (measured once).
    #[no_mangle]
    pub extern "C" fn runtime_tsc_frequency() -> u64 {
        super::tsc_frequency()
    }

    /// Runs the guest's thread-specific-data destructors for the calling thread (at its exit).
    #[no_mangle]
    pub extern "C" fn runtime_thread_keys_cleanup() {
        super::guest::thread_keys_cleanup()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errno_table_maps_host_to_freebsd() {
        use host_errno::*;
        let table = [
            (EPERM, 1),
            (ENOENT, 2),
            (ESRCH, 3),
            (EINTR, 4),
            (EIO, 5),
            (ENXIO, 6),
            (E2BIG, 7),
            (ENOEXEC, 8),
            (EBADF, 9),
            (ECHILD, 10),
            (EDEADLK, 11),
            (ENOMEM, 12),
            (EACCES, 13),
            (EFAULT, 14),
            (EBUSY, 16),
            (EEXIST, 17),
            (EXDEV, 18),
            (ENODEV, 19),
            (ENOTDIR, 20),
            (EISDIR, 21),
            (EINVAL, 22),
            (ENFILE, 23),
            (EMFILE, 24),
            (ENOTTY, 25),
            (EFBIG, 27),
            (ENOSPC, 28),
            (ESPIPE, 29),
            (EROFS, 30),
            (EMLINK, 31),
            (EPIPE, 32),
            (ERANGE, 34),
            (EAGAIN, 35),
            (ETIMEDOUT, 60),
            (ELOOP, 62),
            (ENAMETOOLONG, 63),
            (ENOTEMPTY, 66),
            (ENOSYS, 78),
            (EOVERFLOW, 84),
            (ECANCELED, 85),
        ];
        for (host, guest) in table {
            assert_eq!(guest_errno(host), guest, "host errno {host}");
        }
        assert_eq!(guest_errno(0), 0);
        // Unmapped host errors are EIO.
        assert_eq!(guest_errno(-1), 5);
        assert_eq!(guest_errno(9999), 5);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn errno_table_uses_linux_numbers() {
        // EAGAIN and EDEADLK swap places between Linux and FreeBSD.
        assert_eq!(guest_errno(11), 35);
        assert_eq!(guest_errno(35), 11);
        assert_eq!(guest_errno(110), 60);
        assert_eq!(guest_errno(125), 85);
        assert_eq!(
            guest_errno(std::io::Error::from(std::io::ErrorKind::NotFound).raw_os_error().unwrap_or(2)),
            2
        );
    }

    #[cfg(windows)]
    #[test]
    fn errno_table_uses_ucrt_numbers() {
        assert_eq!(guest_errno(11), 35); // EAGAIN
        assert_eq!(guest_errno(36), 11); // EDEADLK
        assert_eq!(guest_errno(138), 60); // ETIMEDOUT
        assert_eq!(guest_errno(105), 85); // ECANCELED
        assert_eq!(guest_errno(41), 66); // ENOTEMPTY
    }

    #[test]
    fn kernel_errors_carry_the_guest_errno() {
        assert_eq!(kernel_error(22) as u32, 0x8002_0016);
        assert_eq!(kernel_error(35) as u32, 0x8002_0023);
    }

    #[test]
    fn guest_clock_ids() {
        for id in [0, 9, 10] {
            assert_eq!(host_clock(id), Some(HostClock::Realtime), "{id}");
        }
        for id in [4, 5, 7, 8, 11, 12, 16, 17, 18, 19] {
            assert_eq!(host_clock(id), Some(HostClock::Monotonic), "{id}");
        }
        assert_eq!(host_clock(13), Some(HostClock::RealtimeCoarse));
        assert_eq!(host_clock(14), Some(HostClock::ThreadCpu));
        assert_eq!(host_clock(2), Some(HostClock::ProcessCpu));
        assert_eq!(host_clock(15), Some(HostClock::ProcessCpu));
        for id in [1, 3, 6, 20, 99, u32::MAX] {
            assert_eq!(host_clock(id), None, "{id}");
        }
    }

    #[test]
    fn timezone_conversion() {
        // UTC+9 (Tokyo), UTC-5 (New York in winter), UTC+5:30 (India), UTC.
        for offset in [9 * 3600, -5 * 3600, 5 * 3600 + 1800, 0] {
            let zone = |_| offset;
            let utc = 1_789_000_000;
            assert_eq!(utc_to_local(utc, zone), (utc + offset, offset));
            assert_eq!(local_to_utc(utc + offset, zone), (utc, offset));
        }
        // The offset is looked up at the time given (DST: the rule changes at 1_000).
        let dst = |t: i64| if t >= 1_000 { 3600 } else { 0 };
        assert_eq!(utc_to_local(999, dst), (999, 0));
        assert_eq!(utc_to_local(1_000, dst), (4_600, 3600));
        assert_eq!(local_to_utc(4_600, dst), (1_000, 3600));
        // The host zone round-trips away from a DST change.
        let utc = 1_790_000_000;
        let (local, offset) = utc_to_local(utc, crate::time::utc_offset);
        assert_eq!(offset, crate::time::utc_offset(utc));
        assert_eq!(local - utc, offset);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn process_time_advances_and_sleep_waits() {
        mark_process_start();
        let a = process_time_ns();
        let start = std::time::Instant::now();
        sleep_ns(2_000_000);
        assert!(start.elapsed() >= std::time::Duration::from_millis(2));
        let b = process_time_ns();
        assert!(b >= a + 1_500_000, "{a} {b}");
        // A zero sleep returns.
        sleep_ns(0);
        assert!(monotonic_ns() > 0);
        assert!(ffi::runtime_process_time_counter() >= b);
        assert!(ffi::runtime_process_time_us() >= b / 1000);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn tsc_frequency_is_plausible_and_cached() {
        let hz = tsc_frequency();
        assert!((100_000_000..20_000_000_000).contains(&hz), "{hz}");
        assert_eq!(ffi::runtime_tsc_frequency(), hz);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn exported_errno_mapping() {
        assert_eq!(ffi::runtime_guest_errno(host_errno::EAGAIN), 35);
        assert_eq!(ffi::runtime_guest_errno(0), 0);
    }

    #[cfg(target_arch = "x86_64")]
    mod guest_abi {
        use super::super::guest::*;
        use super::super::*;
        use std::ffi::{c_int, c_void, CStr};

        /// The guest function behind `symbol` as the function-pointer type `F`.
        ///
        /// # Safety
        /// `F` is the `extern "sysv64"` type of that function.
        unsafe fn function<F: Copy>(symbol: &str) -> F {
            let address =
                by_symbol(symbol).unwrap_or_else(|| panic!("{symbol} does not resolve"));
            assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<usize>());
            // SAFETY: an address of a function of type `F` (caller), the size of a pointer.
            unsafe { std::mem::transmute_copy(&address) }
        }

        fn errno() -> i32 {
            // SAFETY: this thread's guest errno slot.
            unsafe { *errno_slot() }
        }

        #[test]
        fn exports_resolve_by_nid() {
            for (nid, symbol) in [
                (c"QBi7HCK03hw#p#J", "sceKernelClockGettime"),
                (c"lLMT9vJAck0#p#J", "clock_gettime"),
                (c"lLMT9vJAck0#I#J", "clock_gettime"),
                (c"4J2sUJmuHZQ#p#J", "sceKernelGetProcessTime"),
                (c"-2IRUCO--PM#p#J", "sceKernelReadTsc"),
                (c"1jfXLRVzisc#p#J", "sceKernelUsleep"),
                (c"QcteRwbsnV0#I#J", "usleep"),
                (c"yS8U2TGCe1A#p#J", "nanosleep"),
                (c"kOcnerypnQA#p#J", "sceKernelGettimezone"),
                (c"n88vx3C5nW8#p#J", "gettimeofday"),
                (c"ejekcaNQNq0#p#J", "sceKernelGettimeofday"),
                (c"wLlFkwG9UcQ#q#q", "time"),
                (c"-o5uEDpN+oY#p#J", "sceKernelConvertUtcToLocaltime"),
                (c"0NTHN1NKONI#p#J", "sceKernelConvertLocaltimeToUtc"),
                (c"k+AXqu2-eBc#I#J", "getpagesize"),
                (c"DFmMT80xcNI#p#J", "sysctl"),
                (c"14bOACANTBo#p#J", "scePthreadOnce"),
                (c"geDaqgH9lTg#p#J", "scePthreadKeyCreate"),
                (c"eoht7mQOCmo#p#J", "scePthreadGetspecific"),
            ] {
                let address = by_symbol(symbol).unwrap();
                assert_eq!(resolve(nid), Some(address), "{nid:?}");
                // SAFETY: a NUL-terminated literal; the C wrapper and the Rust function agree.
                assert_eq!(unsafe { ffi::runtime_kernel_resolve(nid.as_ptr()) }, address);
            }
            assert_eq!(resolve(c"unknown"), None);
            assert_eq!(by_symbol("sceRtcGetTick"), None);
            // SAFETY: a null name is accepted and resolves to nothing.
            assert_eq!(unsafe { ffi::runtime_kernel_resolve(std::ptr::null()) }, 0);
        }

        #[test]
        fn clocks_and_their_errors() {
            type Clock = extern "sysv64" fn(u32, *mut GuestTimespec) -> i32;
            // SAFETY: the types of these functions.
            let (kernel, posix, res): (Clock, Clock, Clock) = unsafe {
                (
                    function("sceKernelClockGettime"),
                    function("clock_gettime"),
                    function("clock_getres"),
                )
            };
            let mut ts = GuestTimespec {
                seconds: -1,
                nanoseconds: -1,
            };
            assert_eq!(kernel(0, &mut ts), 0);
            let system = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            assert!((ts.seconds - system).abs() <= 2);
            assert!((0..1_000_000_000).contains(&ts.nanoseconds));
            assert_eq!(kernel(13, &mut ts), 0);
            assert_eq!(ts.nanoseconds, 0);
            let mut a = ts;
            let mut b = ts;
            assert_eq!(posix(4, &mut a), 0);
            assert_eq!(posix(4, &mut b), 0);
            assert!((b.seconds, b.nanoseconds) >= (a.seconds, a.nanoseconds));
            assert_eq!(posix(14, &mut a), 0);
            assert_eq!(posix(2, &mut a), 0);
            assert_eq!(kernel(3, &mut ts) as u32, 0x8002_0016);
            assert_eq!(kernel(0, std::ptr::null_mut()) as u32, 0x8002_0016);
            assert_eq!(posix(3, &mut ts), -1);
            assert_eq!(errno(), 22);
            assert_eq!(res(4, &mut ts), 0);
            assert!(ts.seconds == 0 && ts.nanoseconds > 0);
            assert_eq!(res(0, std::ptr::null_mut()), 0);
            assert_eq!(res(6, &mut ts), -1);
        }

        #[test]
        fn sleeps_and_their_errors() {
            type Nanosleep = extern "sysv64" fn(*const GuestTimespec, *mut GuestTimespec) -> i32;
            type Usleep = extern "sysv64" fn(u32) -> i32;
            // SAFETY: the types of these functions.
            let (posix, kernel, usleep): (Nanosleep, Nanosleep, Usleep) = unsafe {
                (
                    function("nanosleep"),
                    function("sceKernelNanosleep"),
                    function("usleep"),
                )
            };
            let request = GuestTimespec {
                seconds: 0,
                nanoseconds: 1_000_000,
            };
            let mut left = GuestTimespec {
                seconds: 9,
                nanoseconds: 9,
            };
            let start = std::time::Instant::now();
            assert_eq!(posix(&request, &mut left), 0);
            assert!(start.elapsed() >= std::time::Duration::from_millis(1));
            assert_eq!((left.seconds, left.nanoseconds), (0, 0));
            assert_eq!(usleep(100), 0);
            let bad = GuestTimespec {
                seconds: 0,
                nanoseconds: 1_000_000_000,
            };
            assert_eq!(posix(&bad, std::ptr::null_mut()), -1);
            assert_eq!(errno(), 22);
            assert_eq!(posix(std::ptr::null(), std::ptr::null_mut()), -1);
            assert_eq!(kernel(&bad, std::ptr::null_mut()) as u32, 0x8002_0016);
            let negative = GuestTimespec {
                seconds: -1,
                nanoseconds: 0,
            };
            assert_eq!(kernel(&negative, std::ptr::null_mut()) as u32, 0x8002_0016);
            assert_eq!(kernel(&request, std::ptr::null_mut()), 0);
        }

        #[test]
        fn time_of_day_zone_and_local_time() {
            type Gettimeofday = extern "sysv64" fn(*mut GuestTimeval, *mut GuestTimezone) -> i32;
            type KernelGettimeofday = extern "sysv64" fn(*mut GuestTimeval) -> i32;
            type Gettimezone = extern "sysv64" fn(*mut GuestTimezone) -> i32;
            type Time = extern "sysv64" fn(*mut i64) -> i64;
            type ToLocal = extern "sysv64" fn(i64, *mut i64, *mut GuestTimesec, *mut u64) -> i32;
            type ToUtc =
                extern "sysv64" fn(i64, i64, *mut i64, *mut GuestTimezone, *mut i32) -> i32;
            // SAFETY: the types of these functions.
            let (posix, kernel, zone_of, time, to_local, to_utc): (
                Gettimeofday,
                KernelGettimeofday,
                Gettimezone,
                Time,
                ToLocal,
                ToUtc,
            ) = unsafe {
                (
                    function("gettimeofday"),
                    function("sceKernelGettimeofday"),
                    function("sceKernelGettimezone"),
                    function("time"),
                    function("sceKernelConvertUtcToLocaltime"),
                    function("sceKernelConvertLocaltimeToUtc"),
                )
            };
            let mut value = GuestTimeval {
                seconds: 0,
                microseconds: -1,
            };
            let mut zone = GuestTimezone {
                minutes_west: i32::MIN,
                dst_time: -1,
            };
            assert_eq!(posix(&mut value, &mut zone), 0);
            let mut now = 0i64;
            let seconds = time(&mut now);
            assert_eq!(seconds, now);
            assert!((value.seconds - now).abs() <= 2);
            assert!((0..1_000_000).contains(&value.microseconds));
            let offset = crate::time::utc_offset(now);
            assert_eq!(i64::from(zone.minutes_west), -offset / 60);
            assert_eq!(zone.dst_time, 0);
            assert_eq!(posix(std::ptr::null_mut(), std::ptr::null_mut()), 0);
            assert_eq!(kernel(&mut value), 0);
            assert_eq!(kernel(std::ptr::null_mut()) as u32, 0x8002_0016);
            assert_eq!(zone_of(&mut zone), 0);
            assert_eq!(zone_of(std::ptr::null_mut()) as u32, 0x8002_0016);
            assert_eq!(time(std::ptr::null_mut()) - now, 0.max(time(std::ptr::null_mut()) - now));

            let utc = 1_790_000_000;
            let offset = crate::time::utc_offset(utc);
            let (mut local, mut dst) = (0i64, 7u64);
            let mut timesec = GuestTimesec {
                t: 0,
                west_sec: 7,
                dst_sec: 7,
            };
            assert_eq!(to_local(utc, &mut local, &mut timesec, &mut dst), 0);
            assert_eq!(local, utc + offset);
            assert_eq!(
                (timesec.t, timesec.west_sec, timesec.dst_sec, dst),
                (utc, offset as u32, 0, 0)
            );
            assert_eq!(
                to_local(utc, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()),
                0
            );
            let (mut back, mut dst) = (0i64, 7i32);
            assert_eq!(to_utc(local, 0, &mut back, &mut zone, &mut dst), 0);
            assert_eq!(back, local - crate::time::utc_offset(local));
            assert_eq!(
                i64::from(zone.minutes_west),
                -crate::time::utc_offset(local) / 60
            );
            assert_eq!(dst, 0);
        }

        #[test]
        fn process_queries() {
            type Int = extern "sysv64" fn() -> i32;
            type U64 = extern "sysv64" fn() -> u64;
            type Range = extern "sysv64" fn(*mut c_void, u64, *const i8) -> i32;
            // SAFETY: the types of these functions.
            unsafe {
                assert_eq!(function::<Int>("getpagesize")(), 16384);
                assert_eq!(function::<Int>("getpid")(), 1000);
                assert_eq!(function::<Int>("sched_yield")(), 0);
                assert_eq!(
                    function::<U64>("sceKernelGetProcessTimeCounterFrequency")(),
                    1_000_000_000
                );
                let a = function::<U64>("sceKernelReadTsc")();
                let b = function::<U64>("sceKernelReadTsc")();
                assert!(b >= a);
                assert!(function::<U64>("sceKernelGetProcessTimeCounter")() > 0);
                assert_eq!(
                    function::<Range>("sceKernelSetVirtualRangeName")(
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null()
                    ),
                    0
                );
                assert_eq!(function::<Int>("sceKernelPrintBacktraceWithModuleInfo")(), 0);
            }
        }

        #[test]
        fn signals_are_recorded() {
            type Signal = extern "sysv64" fn(c_int, usize) -> usize;
            type Mask =
                extern "sysv64" fn(c_int, *const GuestSigset, *mut GuestSigset) -> i32;
            type Set = extern "sysv64" fn(*mut GuestSigset) -> i32;
            // SAFETY: the types of these functions.
            let (signal, mask, fill, empty): (Signal, Mask, Set, Set) = unsafe {
                (
                    function("signal"),
                    function("sigprocmask"),
                    function("sigfillset"),
                    function("sigemptyset"),
                )
            };
            assert_eq!(signal(0, 1), usize::MAX);
            assert_eq!(errno(), 22);
            assert_eq!(signal(128, 1), usize::MAX);
            let first = signal(30, 0x1234);
            assert_eq!(signal(30, first), 0x1234);
            let mut set = GuestSigset { bits: [0; 4] };
            assert_eq!(fill(&mut set), 0);
            assert_eq!(set.bits, [u32::MAX; 4]);
            assert_eq!(empty(&mut set), 0);
            assert_eq!(set.bits, [0; 4]);
            assert_eq!(fill(std::ptr::null_mut()), -1);
            // A fresh thread starts with nothing blocked.
            std::thread::spawn(move || {
                let mut old = GuestSigset { bits: [9; 4] };
                let block = GuestSigset {
                    bits: [0b1010, 0, 0, 1],
                };
                assert_eq!(mask(1, &block, &mut old), 0);
                assert_eq!(old.bits, [0; 4]);
                let unblock = GuestSigset {
                    bits: [0b0010, 0, 0, 0],
                };
                assert_eq!(mask(2, &unblock, &mut old), 0);
                assert_eq!(old.bits, [0b1010, 0, 0, 1]);
                assert_eq!(mask(3, &unblock, std::ptr::null_mut()), 0);
                assert_eq!(mask(0, std::ptr::null(), &mut old), 0);
                assert_eq!(old.bits, [0b0010, 0, 0, 0]);
                assert_eq!(mask(9, &block, &mut old), -1);
                assert_eq!(errno(), 22);
                assert_eq!(old.bits, [0b0010, 0, 0, 0]);
            })
            .join()
            .unwrap();
        }

        #[test]
        fn rusage_and_sysctl() {
            type Rusage = extern "sysv64" fn(c_int, *mut GuestRusage) -> i32;
            type Sysctl = extern "sysv64" fn(
                *const i32,
                u32,
                *mut c_void,
                *mut u64,
                *const c_void,
                u64,
            ) -> i32;
            // SAFETY: the types of these functions.
            let (rusage, sysctl): (Rusage, Sysctl) =
                unsafe { (function("getrusage"), function("sysctl")) };
            let mut usage = GuestRusage {
                user: GuestTimeval {
                    seconds: -1,
                    microseconds: -1,
                },
                system: GuestTimeval {
                    seconds: -1,
                    microseconds: -1,
                },
                rest: [-1; 14],
            };
            assert_eq!(rusage(0, &mut usage), 0);
            assert!(usage.user.seconds >= 0 && (0..1_000_000).contains(&usage.user.microseconds));
            assert_eq!(usage.rest, [0; 14]);
            assert_eq!(rusage(1, &mut usage), 0);
            assert_eq!(rusage(2, &mut usage), -1);
            assert_eq!(errno(), 22);
            assert_eq!(rusage(0, std::ptr::null_mut()), -1);

            let none = std::ptr::null();
            let mut random = [0u8; 300];
            let mut length = random.len() as u64;
            assert_eq!(
                sysctl([1, 37].as_ptr(), 2, random.as_mut_ptr().cast(), &mut length, none, 0),
                0
            );
            assert!(random.iter().any(|&b| b != 0));
            assert_eq!(
                sysctl([1, 37].as_ptr(), 2, std::ptr::null_mut(), &mut length, none, 0),
                -1
            );
            let mut value = 0i32;
            let mut length = 4u64;
            assert_eq!(
                sysctl([6, 7].as_ptr(), 2, (&raw mut value).cast(), &mut length, none, 0),
                0
            );
            assert_eq!((value, length), (16384, 4));
            assert_eq!(
                sysctl([6, 3].as_ptr(), 2, (&raw mut value).cast(), &mut length, none, 0),
                0
            );
            assert_eq!(value, 7);
            let mut length = 0u64;
            assert_eq!(
                sysctl([6, 3].as_ptr(), 2, std::ptr::null_mut(), &mut length, none, 0),
                0
            );
            assert_eq!(length, 4);
            let mut length = 2u64;
            assert_eq!(
                sysctl([6, 3].as_ptr(), 2, (&raw mut value).cast(), &mut length, none, 0),
                -1
            );
            assert_eq!(errno(), 12); // ENOMEM
            assert_eq!(
                sysctl([6].as_ptr(), 1, std::ptr::null_mut(), &mut length, none, 0),
                -1
            );
            assert_eq!(
                sysctl(
                    [6, 3].as_ptr(),
                    2,
                    std::ptr::null_mut(),
                    &mut length,
                    (&raw const value).cast(),
                    4
                ),
                -1
            );
        }

        static ONCE_RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        extern "sysv64" fn once_routine() {
            ONCE_RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }

        #[test]
        fn once_runs_once() {
            type Once = extern "sysv64" fn(*mut i32, Option<extern "sysv64" fn()>) -> i32;
            // SAFETY: the types of these functions.
            let (kernel, posix): (Once, Once) =
                unsafe { (function("scePthreadOnce"), function("pthread_once")) };
            let mut once = 0i32;
            assert_eq!(kernel(&mut once, Some(once_routine)), 0);
            assert_eq!(posix(&mut once, Some(once_routine)), 0);
            assert_eq!(once, 1);
            assert_eq!(ONCE_RUNS.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert_eq!(kernel(std::ptr::null_mut(), Some(once_routine)) as u32, 0x8002_0016);
            assert_eq!(kernel(&mut once, None) as u32, 0x8002_0016);
            assert_eq!(posix(std::ptr::null_mut(), None), 22);
        }

        static DESTROYED: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());
        extern "sysv64" fn destroy(value: *mut c_void) {
            DESTROYED.lock().unwrap().push(value as usize);
        }

        #[test]
        fn thread_specific_data() {
            type Create = extern "sysv64" fn(*mut u32, Option<extern "sysv64" fn(*mut c_void)>)
                -> i32;
            type Delete = extern "sysv64" fn(u32) -> i32;
            type SetValue = extern "sysv64" fn(u32, *mut c_void) -> i32;
            type GetValue = extern "sysv64" fn(u32) -> *mut c_void;
            // SAFETY: the types of these functions.
            let (create, delete, set, get, posix_create, posix_delete, posix_set): (
                Create,
                Delete,
                SetValue,
                GetValue,
                Create,
                Delete,
                SetValue,
            ) = unsafe {
                (
                    function("scePthreadKeyCreate"),
                    function("scePthreadKeyDelete"),
                    function("scePthreadSetspecific"),
                    function("pthread_getspecific"),
                    function("pthread_key_create"),
                    function("pthread_key_delete"),
                    function("pthread_setspecific"),
                )
            };
            let mut plain = 0u32;
            let mut with_destructor = 0u32;
            assert_eq!(create(&mut plain, None), 0);
            assert_eq!(posix_create(&mut with_destructor, Some(destroy)), 0);
            assert!(plain >= 1 && with_destructor >= 1 && plain != with_destructor);
            assert_eq!(create(std::ptr::null_mut(), None) as u32, 0x8002_0016);
            assert_eq!(set(plain, 0x10 as *mut c_void), 0);
            assert_eq!(get(plain) as usize, 0x10);
            assert_eq!(get(0), std::ptr::null_mut());
            assert_eq!(get(9999), std::ptr::null_mut());
            assert_eq!(set(0, std::ptr::null_mut()) as u32, 0x8002_0016);
            assert_eq!(posix_set(0, std::ptr::null_mut()), 0x16);
            // Values are per thread; the destructor runs at the thread's cleanup.
            std::thread::spawn(move || {
                assert_eq!(get(plain), std::ptr::null_mut());
                assert_eq!(posix_set(with_destructor, 0x20 as *mut c_void), 0);
                assert_eq!(set(plain, 0x30 as *mut c_void), 0);
                ffi::runtime_thread_keys_cleanup();
                assert_eq!(get(with_destructor), std::ptr::null_mut());
                assert_eq!(get(plain) as usize, 0x30);
            })
            .join()
            .unwrap();
            assert!(DESTROYED.lock().unwrap().contains(&0x20));
            assert_eq!(get(plain) as usize, 0x10);
            assert_eq!(delete(plain), 0);
            assert_eq!(delete(plain) as u32, 0x8002_0016);
            assert_eq!(set(plain, std::ptr::null_mut()) as u32, 0x8002_0016);
            assert_eq!(posix_delete(with_destructor), 0);
            assert_eq!(posix_delete(with_destructor), 0x16);
            assert_eq!(delete(0) as u32, 0x8002_0016);
            assert_eq!(delete(256) as u32, 0x8002_0016);
        }

        #[test]
        fn unknown_symbols_do_not_resolve() {
            assert_eq!(by_symbol("sceKernelUnknown"), None);
            assert_eq!(resolve(CStr::from_bytes_with_nul(b"\0").unwrap()), None);
        }
    }
}
