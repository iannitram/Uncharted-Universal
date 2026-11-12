//! Kernel semaphores (`sceKernel*Sema`, the deleted `src/runtime_sema.c`): 32-bit ids that are
//! never reused, counted tokens and FIFO waiters. A waiter whose request fits the count is served
//! even when an earlier, larger request still waits (no tokens are taken in part).
//! Priority-ordered semaphores (attribute 2) are served FIFO as well: there is no PS4 priority
//! scheduler, host threads run at equal priority.

use super::{errno, lock, orbis, sce, wait_until, Status};
use crate::abi::host;
use std::collections::VecDeque;
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// A blocked `sceKernelWaitSema`. `done` and `result` change under the registry lock only.
struct Waiter {
    need: i32,
    done: AtomicBool,
    result: AtomicU32,
    event: Condvar,
}

impl Waiter {
    /// Ends the wait with `errno` (0: the tokens were taken for it).
    fn finish(&self, errno: u32) {
        self.result.store(errno, Ordering::Relaxed);
        self.done.store(true, Ordering::Relaxed);
        self.event.notify_one();
    }
}

struct Sema {
    id: u32,
    count: i32,
    initial: i32,
    maximum: i32,
    waiters: VecDeque<Arc<Waiter>>,
}

impl Sema {
    /// Ends every wait with `errno` and says how many there were.
    fn wake_all(&mut self, errno: u32) -> i32 {
        let woken = self.waiters.len();
        for waiter in self.waiters.drain(..) {
            waiter.finish(errno);
        }
        woken as i32
    }
}

struct Registry {
    semas: Vec<Sema>,
    next_id: u32,
    created: usize,
    deleted: usize,
    acquired: usize,
    signaled: usize,
    timed_out: usize,
}

impl Registry {
    fn find(&mut self, id: u32) -> Result<&mut Sema, u32> {
        self.semas
            .iter_mut()
            .find(|sema| sema.id == id)
            .ok_or(errno::ESRCH)
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    semas: Vec::new(),
    next_id: 1,
    created: 0,
    deleted: 0,
    acquired: 0,
    signaled: 0,
    timed_out: 0,
});

extern "sysv64" fn sem_create(
    out: *mut u32,
    name: *const c_char,
    attr: u32,
    initial: i32,
    maximum: i32,
    options: *const c_void,
) -> i32 {
    if out.is_null()
        || name.is_null()
        || attr > 2
        || initial < 0
        || maximum <= 0
        || initial > maximum
    {
        return orbis(errno::EINVAL);
    }
    if !options.is_null() {
        host::err("STOP: semaphore options are not implemented\n");
        host::quit(21);
    }
    let mut registry = lock(&REGISTRY);
    if registry.next_id == u32::MAX {
        return orbis(errno::ENOSPC);
    }
    let id = registry.next_id;
    registry.next_id += 1;
    registry.semas.push(Sema {
        id,
        count: initial,
        initial,
        maximum,
        waiters: VecDeque::new(),
    });
    registry.created += 1;
    // SAFETY: the guest passes a pointer to its id variable.
    unsafe { out.write_unaligned(id) };
    host::out_line(&format!(
        "Runtime: semaphore id={id}, attr={attr}, initial={initial}, max={maximum}"
    ));
    0
}

/// `sceKernelWaitSema` (`block`) or `sceKernelPollSema`. `timeout` (microseconds, null: no limit)
/// is set to the time left, 0 after a failed wait; an immediate success leaves it alone.
fn wait_count(id: u32, need: i32, timeout: *mut u32, block: bool) -> Status {
    let mut registry = lock(&REGISTRY);
    let sema = registry.find(id)?;
    if need <= 0 || need > sema.maximum {
        return Err(errno::EINVAL);
    }
    if sema.count >= need {
        sema.count -= need;
        registry.acquired += 1;
        return Ok(());
    }
    if !block {
        return Err(errno::EBUSY);
    }
    // SAFETY: the guest passes null or a pointer to its timeout variable.
    let micros = (!timeout.is_null()).then(|| unsafe { timeout.read_unaligned() });
    if micros == Some(0) {
        registry.timed_out += 1;
        return Err(errno::ETIMEDOUT);
    }
    if std::env::var_os("BB_TRACE_SEMA").is_some() {
        // Deadlock diagnosis.
        host::err(&format!(
            "Runtime: blocking wait on semaphore {id} (need {need}, count {})\n",
            sema.count
        ));
    }
    let waiter = Arc::new(Waiter {
        need,
        done: AtomicBool::new(false),
        result: AtomicU32::new(0),
        event: Condvar::new(),
    });
    sema.waiters.push_back(Arc::clone(&waiter));
    let deadline = micros.and_then(|micros| super::after_micros(micros.into()));
    while !waiter.done.load(Ordering::Relaxed) {
        let timed_out;
        (registry, timed_out) = wait_until(&waiter.event, registry, deadline);
        if timed_out && !waiter.done.load(Ordering::Relaxed) {
            // Not served, cancelled or deleted, so the semaphore is there with this waiter queued.
            if let Ok(sema) = registry.find(id) {
                sema.waiters.retain(|queued| !Arc::ptr_eq(queued, &waiter));
            }
            waiter.result.store(errno::ETIMEDOUT, Ordering::Relaxed);
            waiter.done.store(true, Ordering::Relaxed);
            registry.timed_out += 1;
        }
    }
    let result = match waiter.result.load(Ordering::Relaxed) {
        0 => Ok(()),
        errno => Err(errno),
    };
    if result.is_ok() {
        registry.acquired += 1;
    }
    if let Some(micros) = micros {
        let left = match (result, deadline) {
            (Ok(()), Some(deadline)) => deadline.saturating_duration_since(Instant::now()),
            (Ok(()), None) => Duration::from_micros(micros.into()),
            (Err(_), _) => Duration::ZERO,
        };
        // SAFETY: as above; `left` is at most `micros`.
        unsafe { timeout.write_unaligned(left.as_micros() as u32) };
    }
    result
}

extern "sysv64" fn sem_wait(id: u32, need: i32, timeout: *mut u32) -> i32 {
    sce(wait_count(id, need, timeout, true))
}

extern "sysv64" fn sem_poll(id: u32, need: i32) -> i32 {
    sce(wait_count(id, need, std::ptr::null_mut(), false))
}

fn signal(id: u32, count: i32) -> Status {
    let mut registry = lock(&REGISTRY);
    let sema = registry.find(id)?;
    if count <= 0 || count > sema.maximum - sema.count {
        return Err(errno::EINVAL);
    }
    sema.count += count;
    let mut available = sema.count;
    sema.waiters.retain(|waiter| {
        if waiter.need > available {
            return true;
        }
        available -= waiter.need;
        waiter.finish(0);
        false
    });
    sema.count = available;
    registry.signaled += 1;
    Ok(())
}

extern "sysv64" fn sem_signal(id: u32, count: i32) -> i32 {
    sce(signal(id, count))
}

/// `sceKernelCancelSema`: every waiter fails with `ECANCELED` and the count becomes `count`
/// (negative: the initial count).
fn cancel(id: u32, count: i32, waiters: *mut i32) -> Status {
    let mut registry = lock(&REGISTRY);
    let sema = registry.find(id)?;
    if count > sema.maximum {
        return Err(errno::EINVAL);
    }
    let woken = sema.wake_all(errno::ECANCELED);
    if !waiters.is_null() {
        // SAFETY: the guest passes null or a pointer to its count variable.
        unsafe { waiters.write_unaligned(woken) };
    }
    sema.count = if count < 0 { sema.initial } else { count };
    Ok(())
}

extern "sysv64" fn sem_cancel(id: u32, count: i32, waiters: *mut i32) -> i32 {
    sce(cancel(id, count, waiters))
}

/// `sceKernelDeleteSema`: every waiter fails with `EACCES`.
fn delete(id: u32) -> Status {
    let mut registry = lock(&REGISTRY);
    let index = registry
        .semas
        .iter()
        .position(|sema| sema.id == id)
        .ok_or(errno::ESRCH)?;
    let mut sema = registry.semas.swap_remove(index);
    sema.wake_all(errno::EACCES);
    registry.deleted += 1;
    Ok(())
}

extern "sysv64" fn sem_delete(id: u32) -> i32 {
    sce(delete(id))
}

/// How many threads wait on semaphore `id` now (0 for an unknown id).
pub(super) fn waiters(id: u32) -> u32 {
    let mut registry = lock(&REGISTRY);
    registry
        .find(id)
        .map_or(0, |sema| sema.waiters.len() as u32)
}

/// The function for scoped NID `name` (libkernel only).
pub(super) fn resolve(name: &[u8]) -> Option<usize> {
    let address = match name {
        b"188x57JYp0g#p#J" => sem_create as *const () as usize,
        b"Zxa0VhQVTsk#p#J" => sem_wait as *const () as usize,
        b"4czppHBiriw#p#J" => sem_signal as *const () as usize,
        b"12wOHk8ywb0#p#J" => sem_poll as *const () as usize,
        b"4DM06U2BNEY#p#J" => sem_cancel as *const () as usize,
        b"R1Jvn8bSCW8#p#J" => sem_delete as *const () as usize,
        _ => return None,
    };
    Some(address)
}

pub(super) fn report() {
    let registry = lock(&REGISTRY);
    host::out_line(&format!(
        "Runtime: semaphores created={}, deleted={}, acquired={}, signals={}, timeouts={}",
        registry.created,
        registry.deleted,
        registry.acquired,
        registry.signaled,
        registry.timed_out
    ));
}

#[cfg(test)]
mod tests {
    //! `tests/test_sema.c`: the lifecycle, FIFO concurrency, cancellation and priority cases.

    use super::*;
    use std::mem::transmute;
    use std::thread::JoinHandle;

    type Create = extern "sysv64" fn(*mut u32, *const c_char, u32, i32, i32, *const c_void) -> i32;
    type CountOp = extern "sysv64" fn(u32, i32) -> i32;
    type Wait = extern "sysv64" fn(u32, i32, *mut u32) -> i32;
    type Delete = extern "sysv64" fn(u32) -> i32;
    type Cancel = extern "sysv64" fn(u32, i32, *mut i32) -> i32;

    struct Api {
        create: Create,
        poll: CountOp,
        signal: CountOp,
        wait: Wait,
        delete: Delete,
        cancel: Cancel,
    }

    fn api() -> Api {
        let get = |name: &[u8]| resolve(name).expect("semaphore NID");
        // SAFETY: the functions behind these NIDs have these signatures.
        unsafe {
            Api {
                create: transmute::<usize, Create>(get(b"188x57JYp0g#p#J")),
                poll: transmute::<usize, CountOp>(get(b"12wOHk8ywb0#p#J")),
                signal: transmute::<usize, CountOp>(get(b"4czppHBiriw#p#J")),
                wait: transmute::<usize, Wait>(get(b"Zxa0VhQVTsk#p#J")),
                delete: transmute::<usize, Delete>(get(b"R1Jvn8bSCW8#p#J")),
                cancel: transmute::<usize, Cancel>(get(b"4DM06U2BNEY#p#J")),
            }
        }
    }

    fn hex(value: i32) -> u32 {
        value as u32
    }

    fn create(api: &Api, attr: u32, initial: i32, maximum: i32) -> u32 {
        let mut id = 0;
        let name = c"test".as_ptr();
        assert_eq!(
            (api.create)(&mut id, name, attr, initial, maximum, std::ptr::null()),
            0
        );
        id
    }

    /// A thread that waits for `need` tokens with a 5 s timeout: (result, timeout left).
    fn waiter(id: u32, need: i32) -> JoinHandle<(i32, u32)> {
        std::thread::spawn(move || {
            let mut timeout = 5_000_000;
            let result = (api().wait)(id, need, &mut timeout);
            (result, timeout)
        })
    }

    /// Waits until `count` threads wait on `id`.
    fn enrolled(id: u32, count: u32) {
        for _ in 0..2000 {
            if waiters(id) == count {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("waiter enrollment timed out");
    }

    #[test]
    fn lifecycle() {
        let api = api();
        let name = c"test".as_ptr();
        let null = std::ptr::null();
        #[repr(C)]
        struct Slot {
            handle: u32,
            canary: u32,
        }
        let mut s = Slot {
            handle: 0,
            canary: 0xdead_beef,
        };
        assert_eq!(
            hex((api.create)(std::ptr::null_mut(), name, 1, 0, 1, null)),
            0x8002_0016
        );
        assert_eq!(
            hex((api.create)(&mut s.handle, name, 1, 2, 1, null)),
            0x8002_0016
        );
        assert_eq!(
            hex((api.create)(&mut s.handle, std::ptr::null(), 1, 0, 1, null)),
            0x8002_0016
        );
        assert_eq!(
            hex((api.create)(&mut s.handle, name, 3, 0, 1, null)),
            0x8002_0016
        );
        assert_eq!(
            hex((api.create)(&mut s.handle, name, 1, -1, 1, null)),
            0x8002_0016
        );
        assert_eq!(
            hex((api.create)(&mut s.handle, name, 1, 0, 0, null)),
            0x8002_0016
        );
        assert_eq!((api.create)(&mut s.handle, name, 2, 2, 3, null), 0);
        assert_eq!(s.canary, 0xdead_beef);
        let old = s.handle;
        let mut timeout = 1_000_000;
        assert_eq!((api.wait)(s.handle, 2, &mut timeout), 0);
        assert_eq!(timeout, 1_000_000);
        assert_eq!(hex((api.poll)(s.handle, 1)), 0x8002_0010);
        timeout = 0;
        assert_eq!(hex((api.wait)(s.handle, 1, &mut timeout)), 0x8002_003c);
        assert_eq!(hex((api.signal)(s.handle, 4)), 0x8002_0016);
        assert_eq!(hex((api.poll)(s.handle, 0)), 0x8002_0016);
        assert_eq!(hex((api.poll)(s.handle, 4)), 0x8002_0016);
        assert_eq!(hex((api.signal)(s.handle, -1)), 0x8002_0016);
        assert_eq!((api.signal)(s.handle, 3), 0);
        assert_eq!(hex((api.signal)(s.handle, i32::MAX)), 0x8002_0016);
        assert_eq!((api.poll)(s.handle, 3), 0);
        let mut n = -1;
        assert_eq!((api.cancel)(s.handle, -1, &mut n), 0);
        assert_eq!(n, 0);
        assert_eq!(hex((api.cancel)(s.handle, 4, &mut n)), 0x8002_0016);
        assert_eq!((api.poll)(s.handle, 2), 0);
        assert_eq!((api.delete)(s.handle), 0);
        assert_eq!(hex((api.delete)(s.handle)), 0x8002_0003);
        assert_eq!(hex((api.signal)(s.handle, 1)), 0x8002_0003);
        assert_eq!(hex((api.cancel)(s.handle, 0, &mut n)), 0x8002_0003);
        assert_eq!(
            hex((api.wait)(s.handle, 1, std::ptr::null_mut())),
            0x8002_0003
        );
        assert_eq!((api.create)(&mut s.handle, name, 1, 0, 1, null), 0);
        assert_ne!(s.handle, old);
        assert_eq!(hex((api.poll)(old, 1)), 0x8002_0003);
        timeout = 1000;
        assert_eq!(hex((api.wait)(s.handle, 1, &mut timeout)), 0x8002_003c);
        assert_eq!(timeout, 0);
        assert_eq!(waiters(s.handle), 0);
        assert_eq!((api.signal)(s.handle, 1), 0);
        assert_eq!((api.poll)(s.handle, 1), 0);
        assert_eq!((api.delete)(s.handle), 0);
        assert_eq!(waiters(s.handle), 0);
    }

    #[test]
    fn fifo_and_smaller_requests_passing() {
        let api = api();
        let id = create(&api, 1, 0, 3);
        let first = waiter(id, 1);
        enrolled(id, 1);
        let second = waiter(id, 1);
        enrolled(id, 2);
        assert_eq!((api.signal)(id, 1), 0);
        let (result, left) = first.join().unwrap();
        assert_eq!(result, 0);
        assert!(left <= 5_000_000 && left > 0);
        assert_eq!(waiters(id), 1);
        assert_eq!(hex((api.poll)(id, 1)), 0x8002_0010);
        assert_eq!((api.signal)(id, 1), 0);
        assert_eq!(second.join().unwrap().0, 0);
        // A request that fits passes an earlier, larger one; no tokens are taken in part.
        let large = waiter(id, 2);
        enrolled(id, 1);
        let small = waiter(id, 1);
        enrolled(id, 2);
        assert_eq!((api.signal)(id, 1), 0);
        assert_eq!(small.join().unwrap().0, 0);
        assert_eq!(waiters(id), 1);
        assert_eq!((api.signal)(id, 2), 0);
        assert_eq!(large.join().unwrap().0, 0);
        assert_eq!((api.delete)(id), 0);
    }

    #[test]
    fn cancel_and_delete_wake_waiters() {
        let api = api();
        let id = create(&api, 1, 0, 2);
        let a = waiter(id, 1);
        enrolled(id, 1);
        let b = waiter(id, 1);
        enrolled(id, 2);
        let mut n = -1;
        assert_eq!((api.cancel)(id, 1, &mut n), 0);
        assert_eq!(n, 2);
        assert_eq!(a.join().unwrap(), (0x8002_0055_u32 as i32, 0));
        assert_eq!(b.join().unwrap(), (0x8002_0055_u32 as i32, 0));
        assert_eq!((api.poll)(id, 1), 0);
        let a = waiter(id, 1);
        enrolled(id, 1);
        let b = waiter(id, 1);
        enrolled(id, 2);
        assert_eq!((api.delete)(id), 0);
        assert_eq!(hex(a.join().unwrap().0), 0x8002_000d);
        assert_eq!(hex(b.join().unwrap().0), 0x8002_000d);
        assert_eq!(hex((api.poll)(id, 1)), 0x8002_0003);
    }

    #[test]
    fn priority_semaphores_wait_in_fifo_order() {
        let api = api();
        let id = create(&api, 2, 0, 1);
        let a = waiter(id, 1);
        enrolled(id, 1);
        assert_eq!((api.signal)(id, 1), 0);
        assert_eq!(a.join().unwrap().0, 0);
        assert_eq!((api.delete)(id), 0);
    }

    #[test]
    fn timed_wait_without_signal_times_out_and_leaves_the_queue() {
        let api = api();
        let id = create(&api, 1, 0, 1);
        let mut timeout = 20_000;
        let start = Instant::now();
        assert_eq!(hex((api.wait)(id, 1, &mut timeout)), 0x8002_003c);
        assert!(start.elapsed() >= Duration::from_millis(20));
        assert_eq!(timeout, 0);
        assert_eq!(waiters(id), 0);
        // An untimed wait (null timeout) is served by a later signal.
        let wait = api.wait;
        let blocked = std::thread::spawn(move || wait(id, 1, std::ptr::null_mut()));
        enrolled(id, 1);
        assert_eq!((api.signal)(id, 1), 0);
        assert_eq!(blocked.join().unwrap(), 0);
        assert_eq!((api.delete)(id), 0);
    }

    #[test]
    fn report_runs() {
        // The report prints through C stdio; here it must only run.
        report();
    }
}
