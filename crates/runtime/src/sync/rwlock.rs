//! Rwlocks behind the PS4's pointer-to-handle ABI (`scePthreadRwlock*`, the deleted
//! `src/runtime_rwlock.c`). A guest rwlock variable holds a handle: 0 is a statically initialized
//! rwlock, created on first lock, 1 a destroyed one, anything else an rwlock made here. Handles
//! are never reused, so a stale one fails with `EINVAL`.
//!
//! The registry lock guards lifetime, static initialization and ownership; waits happen on each
//! rwlock's condition variable. Readers are preferred, as glibc's default rwlock (which the C
//! file used) prefers them: a read lock is granted whenever no writer holds the rwlock. A thread
//! may hold several read locks; asking for a lock it cannot get without its own release is
//! `EDEADLK` (`EBUSY` for the try forms).

use super::{errno, lock, sce, this_thread, wait_until, GuestTimespec, Status};
use crate::abi::host;
use std::collections::HashMap;
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};
use std::time::Instant;

/// The value a destroyed rwlock's variable holds.
const DESTROYED: usize = 1;

/// What one thread holds of an rwlock.
struct Holder {
    thread: u64,
    readers: u32,
    writer: bool,
}

struct Rwlock {
    holders: Vec<Holder>,
    readers: u32,
    writer: bool,
    /// Threads waiting for this rwlock; it cannot be destroyed under them.
    inflight: u32,
    released: Arc<Condvar>,
}

impl Rwlock {
    fn available(&self, writer: bool) -> bool {
        !self.writer && (!writer || self.readers == 0)
    }
}

struct Registry {
    locks: HashMap<usize, Rwlock>,
    next_handle: usize,
    created: usize,
    reads: usize,
    writes: usize,
    unlocks: usize,
}

impl Registry {
    fn create(&mut self) -> usize {
        let handle = self.next_handle;
        self.next_handle += 0x10;
        self.locks.insert(
            handle,
            Rwlock {
                holders: Vec::new(),
                readers: 0,
                writer: false,
                inflight: 0,
                released: Arc::new(Condvar::new()),
            },
        );
        self.created += 1;
        handle
    }
}

/// Handles start past any small integer a guest could leave in its variable by mistake.
static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| {
    Mutex::new(Registry {
        locks: HashMap::new(),
        next_handle: 0x1_0000,
        created: 0,
        reads: 0,
        writes: 0,
        unlocks: 0,
    })
});

/// The guest's handle variable as an atomic word (read and written under the registry lock).
///
/// # Safety
/// `slot` is non-null, aligned and valid while the call runs.
unsafe fn word<'a>(slot: *mut usize) -> &'a AtomicUsize {
    // SAFETY: the caller's guarantee.
    unsafe { AtomicUsize::from_ptr(slot) }
}

extern "sysv64" fn rw_init(out: *mut usize, attr: *const *mut c_void, _name: *const c_char) -> i32 {
    // SAFETY: the guest passes null or a pointer to its attribute variable.
    if out.is_null() || (!attr.is_null() && unsafe { *attr }.is_null()) {
        return sce(Err(errno::EINVAL));
    }
    if !attr.is_null() {
        host::err("STOP: non-default PS4 rwlock attributes are not implemented\n");
        host::quit(21);
    }
    let handle = lock(&REGISTRY).create();
    // SAFETY: the guest passes a pointer to its rwlock variable; POSIX allows it to be
    // uninitialized, so it is written, not read.
    unsafe { out.write(handle) };
    0
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Wait {
    /// `scePthreadRwlockRdlock` or `Wrlock`: until it is granted.
    Block,
    /// The try forms: `EBUSY` at once.
    Attempt,
    /// The timed forms: until the wall-clock time (validated only when the lock is busy).
    Until(*const GuestTimespec),
}

fn acquire(slot: *mut usize, writer: bool, wait: Wait) -> Status {
    if slot.is_null() {
        return Err(errno::EINVAL);
    }
    // SAFETY: the guest passes a pointer to its rwlock variable.
    let word = unsafe { word(slot) };
    let mut registry = lock(&REGISTRY);
    let mut handle = word.load(Ordering::Relaxed);
    if handle == 0 {
        handle = registry.create();
        word.store(handle, Ordering::Relaxed);
    }
    let me = this_thread();
    let rwlock = registry.locks.get_mut(&handle).ok_or(errno::EINVAL)?;
    if let Some(mine) = rwlock.holders.iter().find(|holder| holder.thread == me) {
        if mine.writer || (writer && mine.readers > 0) {
            return Err(if wait == Wait::Attempt {
                errno::EBUSY
            } else {
                errno::EDEADLK
            });
        }
        if mine.readers == u32::MAX {
            return Err(errno::EAGAIN);
        }
    }
    if !rwlock.available(writer) {
        let deadline = match wait {
            Wait::Attempt => return Err(errno::EBUSY),
            Wait::Block => None,
            Wait::Until(time) if time.is_null() => return Err(errno::EINVAL),
            // SAFETY: the guest passes a pointer to its timespec. A time past `Instant`'s range
            // is no deadline.
            Wait::Until(time) => unsafe { time.read_unaligned() }.deadline()?,
        };
        registry = wait_for(registry, handle, writer, deadline)?;
    }
    let rwlock = registry
        .locks
        .get_mut(&handle)
        .expect("an rwlock in use stays registered");
    let index = match rwlock.holders.iter().position(|holder| holder.thread == me) {
        Some(index) => index,
        None => {
            rwlock.holders.push(Holder {
                thread: me,
                readers: 0,
                writer: false,
            });
            rwlock.holders.len() - 1
        }
    };
    let mine = &mut rwlock.holders[index];
    if writer {
        mine.writer = true;
        rwlock.writer = true;
        registry.writes += 1;
    } else {
        mine.readers += 1;
        rwlock.readers += 1;
        registry.reads += 1;
    }
    Ok(())
}

/// Waits until rwlock `handle` is available for `writer`, or `ETIMEDOUT` at the deadline.
fn wait_for(
    mut registry: MutexGuard<'static, Registry>,
    handle: usize,
    writer: bool,
    deadline: Option<Instant>,
) -> Result<MutexGuard<'static, Registry>, u32> {
    let rwlock = registry.locks.get_mut(&handle).expect("a found rwlock");
    let released = Arc::clone(&rwlock.released);
    rwlock.inflight += 1;
    let mut result = Ok(());
    loop {
        let timed_out;
        (registry, timed_out) = wait_until(&released, registry, deadline);
        // Not destroyed while this thread is in flight.
        let rwlock = &registry.locks[&handle];
        if rwlock.available(writer) {
            break;
        }
        if timed_out {
            result = Err(errno::ETIMEDOUT);
            break;
        }
    }
    let rwlock = registry.locks.get_mut(&handle).expect("a found rwlock");
    rwlock.inflight -= 1;
    result.map(|()| registry)
}

extern "sysv64" fn rw_read(rwlock: *mut usize) -> i32 {
    sce(acquire(rwlock, false, Wait::Block))
}

extern "sysv64" fn rw_write(rwlock: *mut usize) -> i32 {
    sce(acquire(rwlock, true, Wait::Block))
}

extern "sysv64" fn rw_tryread(rwlock: *mut usize) -> i32 {
    sce(acquire(rwlock, false, Wait::Attempt))
}

extern "sysv64" fn rw_trywrite(rwlock: *mut usize) -> i32 {
    sce(acquire(rwlock, true, Wait::Attempt))
}

extern "sysv64" fn rw_timedread(rwlock: *mut usize, time: *const GuestTimespec) -> i32 {
    sce(acquire(rwlock, false, Wait::Until(time)))
}

extern "sysv64" fn rw_timedwrite(rwlock: *mut usize, time: *const GuestTimespec) -> i32 {
    sce(acquire(rwlock, true, Wait::Until(time)))
}

fn unlock(slot: *mut usize) -> Status {
    if slot.is_null() {
        return Err(errno::EINVAL);
    }
    // SAFETY: the guest passes a pointer to its rwlock variable.
    let word = unsafe { word(slot) };
    let mut registry = lock(&REGISTRY);
    let handle = word.load(Ordering::Relaxed);
    let rwlock = registry.locks.get_mut(&handle).ok_or(errno::EINVAL)?;
    let me = this_thread();
    let index = rwlock
        .holders
        .iter()
        .position(|holder| holder.thread == me)
        .ok_or(errno::EPERM)?;
    let mine = &mut rwlock.holders[index];
    if mine.writer {
        mine.writer = false;
        rwlock.writer = false;
    } else {
        mine.readers -= 1;
        rwlock.readers -= 1;
    }
    if !mine.writer && mine.readers == 0 {
        rwlock.holders.swap_remove(index);
    }
    rwlock.released.notify_all();
    registry.unlocks += 1;
    Ok(())
}

extern "sysv64" fn rw_unlock(rwlock: *mut usize) -> i32 {
    sce(unlock(rwlock))
}

fn destroy(slot: *mut usize) -> Status {
    if slot.is_null() {
        return Err(errno::EINVAL);
    }
    // SAFETY: the guest passes a pointer to its rwlock variable.
    let word = unsafe { word(slot) };
    let mut registry = lock(&REGISTRY);
    let handle = word.load(Ordering::Relaxed);
    if handle == 0 {
        return Ok(());
    }
    let rwlock = registry.locks.get(&handle).ok_or(errno::EINVAL)?;
    if rwlock.inflight > 0 || !rwlock.holders.is_empty() {
        return Err(errno::EBUSY);
    }
    registry.locks.remove(&handle);
    word.store(DESTROYED, Ordering::Relaxed);
    Ok(())
}

extern "sysv64" fn rw_destroy(rwlock: *mut usize) -> i32 {
    sce(destroy(rwlock))
}

/// The function for scoped NID `name` (libkernel only).
pub(super) fn resolve(name: &[u8]) -> Option<usize> {
    let address = match name {
        b"6ULAa0fq4jA#p#J" => rw_init as *const () as usize,
        b"BB+kb08Tl9A#p#J" => rw_destroy as *const () as usize,
        b"Ox9i0c7L5w0#p#J" => rw_read as *const () as usize,
        b"mqdNorrB+gI#p#J" => rw_write as *const () as usize,
        b"XD3mDeybCnk#p#J" => rw_tryread as *const () as usize,
        b"bIHoZCTomsI#p#J" => rw_trywrite as *const () as usize,
        b"+L98PIbGttk#p#J" => rw_unlock as *const () as usize,
        b"iPtZRWICjrM#p#J" => rw_timedread as *const () as usize,
        b"adh--6nIqTk#p#J" => rw_timedwrite as *const () as usize,
        _ => return None,
    };
    Some(address)
}

pub(super) fn report() {
    let registry = lock(&REGISTRY);
    host::out_line(&format!(
        "Runtime: rwlocks created={}, reads={}, writes={}, unlocks={}",
        registry.created, registry.reads, registry.writes, registry.unlocks
    ));
}

#[cfg(test)]
mod tests {
    //! `tests/test_runtime.c`'s `rw_lifecycle`, `rw_concurrency` and `rw_timeouts`.

    use super::*;
    use std::mem::transmute;
    use std::sync::atomic::AtomicBool;
    use std::sync::Barrier;
    use std::time::Duration;

    type Init = extern "sysv64" fn(*mut usize, *const *mut c_void, *const c_char) -> i32;
    type HandleOp = extern "sysv64" fn(*mut usize) -> i32;
    type Timed = extern "sysv64" fn(*mut usize, *const GuestTimespec) -> i32;

    #[derive(Clone, Copy)]
    struct Api {
        init: Init,
        read: HandleOp,
        write: HandleOp,
        tryread: HandleOp,
        trywrite: HandleOp,
        unlock: HandleOp,
        destroy: HandleOp,
        timedread: Timed,
        timedwrite: Timed,
    }

    fn api() -> Api {
        let get = |name: &[u8]| resolve(name).expect("rwlock NID");
        // SAFETY: the functions behind these NIDs have these signatures.
        unsafe {
            Api {
                init: transmute::<usize, Init>(get(b"6ULAa0fq4jA#p#J")),
                read: transmute::<usize, HandleOp>(get(b"Ox9i0c7L5w0#p#J")),
                write: transmute::<usize, HandleOp>(get(b"mqdNorrB+gI#p#J")),
                tryread: transmute::<usize, HandleOp>(get(b"XD3mDeybCnk#p#J")),
                trywrite: transmute::<usize, HandleOp>(get(b"bIHoZCTomsI#p#J")),
                unlock: transmute::<usize, HandleOp>(get(b"+L98PIbGttk#p#J")),
                destroy: transmute::<usize, HandleOp>(get(b"BB+kb08Tl9A#p#J")),
                timedread: transmute::<usize, Timed>(get(b"iPtZRWICjrM#p#J")),
                timedwrite: transmute::<usize, Timed>(get(b"adh--6nIqTk#p#J")),
            }
        }
    }

    fn hex(value: i32) -> u32 {
        value as u32
    }

    /// A guest rwlock variable that threads share.
    fn shared_slot() -> &'static AtomicUsize {
        Box::leak(Box::new(AtomicUsize::new(0)))
    }

    #[test]
    fn lifecycle() {
        let rw = api();
        let mut lock = 0usize;
        assert_eq!(
            hex((rw.init)(
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null()
            )),
            0x8002_0016
        );
        let cleared: *mut c_void = std::ptr::null_mut();
        assert_eq!(
            hex((rw.init)(&mut lock, &cleared, std::ptr::null())),
            0x8002_0016
        );
        lock = 0xdead; // never read by init
        assert_eq!(
            (rw.init)(&mut lock, std::ptr::null(), c"rw-test".as_ptr()),
            0
        );
        assert_eq!((rw.read)(&mut lock), 0);
        assert_eq!((rw.read)(&mut lock), 0);
        assert_eq!(hex((rw.trywrite)(&mut lock)), 0x8002_0010);
        assert_eq!(hex((rw.write)(&mut lock)), 0x8002_000b);
        assert_eq!(hex((rw.destroy)(&mut lock)), 0x8002_0010);
        assert_eq!((rw.unlock)(&mut lock), 0);
        assert_eq!((rw.unlock)(&mut lock), 0);
        assert_eq!(hex((rw.unlock)(&mut lock)), 0x8002_0001);
        assert_eq!((rw.write)(&mut lock), 0);
        assert_eq!(hex((rw.tryread)(&mut lock)), 0x8002_0010);
        assert_eq!(hex((rw.read)(&mut lock)), 0x8002_000b);
        assert_eq!(hex((rw.write)(&mut lock)), 0x8002_000b);
        assert_eq!((rw.unlock)(&mut lock), 0);
        assert_eq!((rw.destroy)(&mut lock), 0);
        assert_eq!(lock, DESTROYED);
        assert_eq!(hex((rw.read)(&mut lock)), 0x8002_0016);
        assert_eq!(hex((rw.destroy)(&mut lock)), 0x8002_0016);
        lock = 0;
        assert_eq!((rw.destroy)(&mut lock), 0);
        assert_eq!((rw.read)(&mut lock), 0); // static initialization
        assert_ne!(lock, 0);
        assert_eq!((rw.unlock)(&mut lock), 0);
        assert_eq!((rw.destroy)(&mut lock), 0);
        lock = 0xdead;
        assert_eq!(hex((rw.tryread)(&mut lock)), 0x8002_0016);
        assert_eq!(hex((rw.unlock)(&mut lock)), 0x8002_0016);
        assert_eq!(hex((rw.read)(std::ptr::null_mut())), 0x8002_0016);
        assert_eq!(hex((rw.unlock)(std::ptr::null_mut())), 0x8002_0016);
        assert_eq!(hex((rw.destroy)(std::ptr::null_mut())), 0x8002_0016);
        let past = GuestTimespec {
            seconds: 0,
            nanoseconds: 0,
        };
        assert_eq!(
            hex((rw.timedwrite)(std::ptr::null_mut(), &past)),
            0x8002_0016
        );
    }

    #[test]
    fn readers_share_and_writers_wait() {
        let rw = api();
        let slot = shared_slot();
        let value: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
        let ready = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let writer_ready = Arc::new(Barrier::new(2));
        assert_eq!((rw.read)(slot.as_ptr()), 0);
        let reader = {
            let (ready, release) = (Arc::clone(&ready), Arc::clone(&release));
            std::thread::spawn(move || {
                assert_eq!((rw.read)(slot.as_ptr()), 0);
                ready.wait();
                release.wait();
                assert!(!value.load(Ordering::SeqCst));
                assert_eq!((rw.unlock)(slot.as_ptr()), 0);
            })
        };
        ready.wait(); // two threads hold read locks at once
        assert_eq!((rw.unlock)(slot.as_ptr()), 0);
        let writer = {
            let writer_ready = Arc::clone(&writer_ready);
            std::thread::spawn(move || {
                assert_eq!(hex((rw.trywrite)(slot.as_ptr())), 0x8002_0010);
                writer_ready.wait();
                assert_eq!((rw.write)(slot.as_ptr()), 0);
                value.store(true, Ordering::SeqCst);
                assert_eq!((rw.unlock)(slot.as_ptr()), 0);
            })
        };
        writer_ready.wait();
        assert_eq!(hex((rw.destroy)(slot.as_ptr())), 0x8002_0010);
        assert!(!value.load(Ordering::SeqCst));
        release.wait();
        reader.join().unwrap();
        writer.join().unwrap();
        assert!(value.load(Ordering::SeqCst));
        assert_eq!((rw.destroy)(slot.as_ptr()), 0);
    }

    #[test]
    fn timeouts() {
        let rw = api();
        let slot = shared_slot();
        let ready = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let writer = {
            let (ready, release) = (Arc::clone(&ready), Arc::clone(&release));
            std::thread::spawn(move || {
                assert_eq!((rw.write)(slot.as_ptr()), 0);
                ready.wait();
                release.wait();
                assert_eq!((rw.unlock)(slot.as_ptr()), 0);
            })
        };
        ready.wait();
        let expired = GuestTimespec {
            seconds: 0,
            nanoseconds: 0,
        };
        let invalid = GuestTimespec {
            seconds: 0,
            nanoseconds: 1_000_000_000,
        };
        assert_eq!(hex((rw.timedread)(slot.as_ptr(), &expired)), 0x8002_003c);
        assert_eq!(hex((rw.timedwrite)(slot.as_ptr(), &expired)), 0x8002_003c);
        assert_eq!(hex((rw.timedread)(slot.as_ptr(), &invalid)), 0x8002_0016);
        assert_eq!(
            hex((rw.timedread)(slot.as_ptr(), std::ptr::null())),
            0x8002_0016
        );
        assert_eq!(hex((rw.unlock)(slot.as_ptr())), 0x8002_0001);
        // A deadline 30 ms ahead is waited for.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            + Duration::from_millis(30);
        let soon = GuestTimespec {
            seconds: now.as_secs() as i64,
            nanoseconds: now.subsec_nanos().into(),
        };
        let start = Instant::now();
        assert_eq!(hex((rw.timedwrite)(slot.as_ptr(), &soon)), 0x8002_003c);
        assert!(start.elapsed() >= Duration::from_millis(20));
        release.wait();
        writer.join().unwrap();
        // An available rwlock ignores the deadline, even an invalid one.
        assert_eq!((rw.timedread)(slot.as_ptr(), &invalid), 0);
        assert_eq!((rw.unlock)(slot.as_ptr()), 0);
        assert_eq!((rw.timedwrite)(slot.as_ptr(), std::ptr::null()), 0);
        assert_eq!((rw.unlock)(slot.as_ptr()), 0);
        assert_eq!((rw.destroy)(slot.as_ptr()), 0);
    }

    #[test]
    fn a_timed_wait_is_granted_on_release() {
        let rw = api();
        let slot = shared_slot();
        assert_eq!((rw.write)(slot.as_ptr()), 0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            + Duration::from_secs(10);
        let later = GuestTimespec {
            seconds: now.as_secs() as i64,
            nanoseconds: 0,
        };
        let waiter = std::thread::spawn(move || {
            let result = (rw.timedread)(slot.as_ptr(), &later);
            assert_eq!((rw.unlock)(slot.as_ptr()), 0);
            result
        });
        // Wait until the reader is in flight, then release.
        loop {
            let registry = lock(&REGISTRY);
            if registry.locks[&slot.load(Ordering::Relaxed)].inflight == 1 {
                break;
            }
            drop(registry);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(hex((rw.destroy)(slot.as_ptr())), 0x8002_0010);
        assert_eq!((rw.unlock)(slot.as_ptr()), 0);
        assert_eq!(waiter.join().unwrap(), 0);
        assert_eq!((rw.destroy)(slot.as_ptr()), 0);
    }

    #[test]
    fn report_runs() {
        // The report prints through C stdio; here it must only run.
        report();
    }
}
