//! Guest synchronization (piece 3.03; order 3 in docs/code-map.md): the PS4's mutexes and
//! condition variables ([`mutex`], the deleted `src/runtime_mutex.c`), kernel semaphores
//! ([`sema`], `src/runtime_sema.c`) and rwlocks ([`rwlock`], `src/runtime_rwlock.c`). Their C
//! symbols (`runtime_mutex_resolve`, `runtime_sema_report`, ...) are exported by [`ffi`] and
//! declared in `include/uncharted_runtime.h`; [`crate::abi`] resolves guest imports through them
//! in the place `src/runtime.c` gave them. The C tests of those files (`tests/test_sema.c`, the
//! mutex and rwlock cases of `tests/test_runtime.c`) are the unit tests here.
//!
//! The C files wrapped host pthread objects; this module builds the same contracts on Rust's
//! `std::sync`, so it is the same code on every host OS (winpthreads' rwlock and condition
//! variable clocks no longer differ from glibc's). Guest code calls these functions with the
//! System V ABI (`extern "sysv64"`), so the module exists on x86-64 hosts only, as the guest code
//! does. Error values are the Orbis kernel's (`0x8002_0000 | errno`, FreeBSD errno numbers) for
//! the `scePthread*` and `sceKernel*` functions and the bare errno for the POSIX ones.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub mod mutex;
pub mod rwlock;
pub mod sema;

/// FreeBSD errno values, as the guest sees them (Rust-only: the host C library has its own).
/// cbindgen:ignore
mod errno {
    pub const EPERM: u32 = 1;
    pub const ESRCH: u32 = 3;
    pub const EDEADLK: u32 = 11;
    pub const EACCES: u32 = 13;
    pub const EBUSY: u32 = 16;
    pub const EINVAL: u32 = 22;
    pub const ENOSPC: u32 = 28;
    pub const EAGAIN: u32 = 35;
    pub const ETIMEDOUT: u32 = 60;
    pub const ECANCELED: u32 = 85;
}

/// A guest error: the FreeBSD errno of a failed call.
type Status = Result<(), u32>;

/// The Orbis kernel's error value for `errno`: `0x8002_0000 | errno`.
const fn orbis(errno: u32) -> i32 {
    (0x8002_0000 | errno) as i32
}

/// What a `scePthread*` or `sceKernel*` function returns: 0 or the Orbis error.
fn sce(status: Status) -> i32 {
    status.map_or_else(orbis, |()| 0)
}

/// What a POSIX function (`pthread_*` from libScePosix) returns: 0 or the bare errno.
fn posix(status: Status) -> i32 {
    status.map_or_else(|errno| errno as i32, |()| 0)
}

/// `std::sync::Mutex::lock` that ignores poisoning: no guest code runs under these locks, and the
/// state they guard is whole after every update.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Waits on `condvar` until `deadline` (forever for `None`). `true` when the deadline has passed;
/// the caller checks its condition again either way (wakeups may be spurious).
fn wait_until<'a, T>(
    condvar: &Condvar,
    guard: MutexGuard<'a, T>,
    deadline: Option<Instant>,
) -> (MutexGuard<'a, T>, bool) {
    let Some(deadline) = deadline else {
        return (
            condvar
                .wait(guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            false,
        );
    };
    let now = Instant::now();
    if now >= deadline {
        return (guard, true);
    }
    let (guard, _) = condvar
        .wait_timeout(guard, deadline - now)
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    (guard, Instant::now() >= deadline)
}

/// The deadline `micros` microseconds from now (`None`, no deadline, past `Instant`'s range).
fn after_micros(micros: u64) -> Option<Instant> {
    Instant::now().checked_add(Duration::from_micros(micros))
}

/// A PS4 `struct timespec` (FreeBSD x86-64: two 64-bit fields, as on Linux x86-64).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GuestTimespec {
    pub seconds: i64,
    pub nanoseconds: i64,
}

impl GuestTimespec {
    /// The `Instant` at which the wall clock (`CLOCK_REALTIME`, as the C code's pthread waits
    /// used) reads this time; `Err(EINVAL)` for nanoseconds outside 0..1e9, `Ok(None)` past
    /// `Instant`'s range (no deadline). A time already past is now.
    fn deadline(self) -> Result<Option<Instant>, u32> {
        if !(0..1_000_000_000).contains(&self.nanoseconds) {
            return Err(errno::EINVAL);
        }
        let nanos = Duration::from_nanos(self.nanoseconds as u64);
        let at = if self.seconds >= 0 {
            UNIX_EPOCH
                .checked_add(Duration::from_secs(self.seconds as u64))
                .and_then(|time| time.checked_add(nanos))
        } else {
            UNIX_EPOCH
                .checked_sub(Duration::from_secs(self.seconds.unsigned_abs()))
                .and_then(|time| time.checked_add(nanos))
        };
        let Some(at) = at else {
            return Ok(if self.seconds < 0 {
                Some(Instant::now())
            } else {
                None
            });
        };
        let left = at
            .duration_since(SystemTime::now())
            .unwrap_or(Duration::ZERO);
        Ok(Instant::now().checked_add(left))
    }
}

thread_local! {
    /// This thread's owner tag for mutexes and rwlocks, 0 until its first lock. No destructor, so
    /// it can be read at any time in the thread's life.
    static THREAD: Cell<u64> = const { Cell::new(0) };
}
static NEXT_THREAD: AtomicU64 = AtomicU64::new(1);

/// The calling thread's owner tag (the C code's `pthread_self()`): unique for the process's life.
fn this_thread() -> u64 {
    THREAD.with(|tag| {
        if tag.get() == 0 {
            tag.set(NEXT_THREAD.fetch_add(1, Ordering::Relaxed));
        }
        tag.get()
    })
}

/// The address of this module's function for scoped NID `name`, or `None` (the C resolvers, in
/// `src/runtime.c`'s order, are [`ffi::runtime_mutex_resolve`], [`ffi::runtime_sema_resolve`] and
/// [`ffi::runtime_rwlock_resolve`]).
pub fn resolve(name: &[u8]) -> Option<usize> {
    mutex::resolve(name)
        .or_else(|| sema::resolve(name))
        .or_else(|| rwlock::resolve(name))
}

/// The C symbols of `src/runtime_mutex.c`, `runtime_sema.c` and `runtime_rwlock.c`, declared in
/// `include/uncharted_runtime.h`.
pub mod ffi {
    use std::ffi::{c_char, CStr};

    /// # Safety
    /// `name` is null or a NUL-terminated string.
    unsafe fn name<'a>(name: *const c_char) -> Option<&'a [u8]> {
        // SAFETY: the caller passes null or a NUL-terminated string.
        (!name.is_null()).then(|| unsafe { CStr::from_ptr(name) }.to_bytes())
    }

    /// The mutex, condition variable and attribute function for scoped NID `name`, or 0.
    ///
    /// # Safety
    /// `name` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_mutex_resolve(name: *const c_char) -> usize {
        // SAFETY: forwarded from the caller.
        unsafe { self::name(name) }
            .and_then(super::mutex::resolve)
            .unwrap_or(0)
    }

    /// The mutex and condition variable counters, two lines on stdout.
    #[no_mangle]
    pub extern "C" fn runtime_mutex_report() {
        super::mutex::report();
    }

    /// The kernel semaphore function for scoped NID `name`, or 0.
    ///
    /// # Safety
    /// `name` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_sema_resolve(name: *const c_char) -> usize {
        // SAFETY: forwarded from the caller.
        unsafe { self::name(name) }
            .and_then(super::sema::resolve)
            .unwrap_or(0)
    }

    /// The semaphore counters, one line on stdout.
    #[no_mangle]
    pub extern "C" fn runtime_sema_report() {
        super::sema::report();
    }

    /// How many threads wait on semaphore `id` now (0 for an unknown id). For tests and
    /// diagnostics; guest code cannot call it.
    #[no_mangle]
    pub extern "C" fn runtime_sema_waiters(id: u32) -> u32 {
        super::sema::waiters(id)
    }

    /// The rwlock function for scoped NID `name`, or 0.
    ///
    /// # Safety
    /// `name` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_rwlock_resolve(name: *const c_char) -> usize {
        // SAFETY: forwarded from the caller.
        unsafe { self::name(name) }
            .and_then(super::rwlock::resolve)
            .unwrap_or(0)
    }

    /// The rwlock counters, one line on stdout.
    #[no_mangle]
    pub extern "C" fn runtime_rwlock_report() {
        super::rwlock::report();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_orbis_or_bare_errno() {
        assert_eq!(sce(Ok(())), 0);
        assert_eq!(sce(Err(errno::EINVAL)) as u32, 0x8002_0016);
        assert_eq!(sce(Err(errno::ETIMEDOUT)) as u32, 0x8002_003c);
        assert_eq!(posix(Err(errno::EINVAL)), 22);
        assert_eq!(posix(Err(errno::EDEADLK)), 11);
        assert_eq!(posix(Ok(())), 0);
    }

    #[test]
    fn thread_tags_differ_between_threads_and_stay_put() {
        let here = this_thread();
        assert_eq!(here, this_thread());
        let there = std::thread::spawn(this_thread).join().unwrap();
        assert_ne!(here, there);
        assert_ne!(there, 0);
    }

    #[test]
    fn wall_clock_deadlines() {
        let past = GuestTimespec {
            seconds: 0,
            nanoseconds: 0,
        };
        assert!(past.deadline().unwrap().unwrap() <= Instant::now());
        let before_epoch = GuestTimespec {
            seconds: -5,
            nanoseconds: 0,
        };
        assert!(before_epoch.deadline().unwrap().unwrap() <= Instant::now());
        for nanoseconds in [-1, 1_000_000_000] {
            let bad = GuestTimespec {
                seconds: 0,
                nanoseconds,
            };
            assert_eq!(bad.deadline(), Err(errno::EINVAL));
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let soon = GuestTimespec {
            seconds: now.as_secs() as i64 + 2,
            nanoseconds: 0,
        };
        let at = soon.deadline().unwrap().unwrap();
        assert!(at > Instant::now() + Duration::from_secs(1));
        assert!(at < Instant::now() + Duration::from_secs(3));
        let far = GuestTimespec {
            seconds: i64::MAX,
            nanoseconds: 999_999_999,
        };
        assert_eq!(far.deadline(), Ok(None));
    }

    #[test]
    fn every_nid_resolves_in_one_module_only() {
        for name in [
            &b"9UK1vLZQft4#p#J"[..],
            b"Op8TBGY5KHg#I#J",
            b"188x57JYp0g#p#J",
            b"Ox9i0c7L5w0#p#J",
        ] {
            let found = [
                mutex::resolve(name),
                sema::resolve(name),
                rwlock::resolve(name),
            ];
            assert_eq!(found.iter().flatten().count(), 1, "{name:?}");
            assert_eq!(resolve(name), found.into_iter().flatten().next());
        }
        assert_eq!(resolve(b"9UK1vLZQft4#q#q"), None);
        assert_eq!(resolve(b"unknown"), None);
    }

    #[test]
    fn exported_resolvers_take_c_strings() {
        // SAFETY: NUL-terminated literals and null.
        unsafe {
            assert_ne!(ffi::runtime_mutex_resolve(c"9UK1vLZQft4#p#J".as_ptr()), 0);
            assert_eq!(ffi::runtime_mutex_resolve(c"188x57JYp0g#p#J".as_ptr()), 0);
            assert_ne!(ffi::runtime_sema_resolve(c"188x57JYp0g#p#J".as_ptr()), 0);
            assert_eq!(ffi::runtime_sema_resolve(c"188x57JYp0g#I#J".as_ptr()), 0);
            assert_ne!(ffi::runtime_rwlock_resolve(c"Ox9i0c7L5w0#p#J".as_ptr()), 0);
            assert_eq!(ffi::runtime_rwlock_resolve(std::ptr::null()), 0);
        }
        // The reports print through C stdio; here they must only run.
        ffi::runtime_mutex_report();
        ffi::runtime_sema_report();
        ffi::runtime_rwlock_report();
    }
}
