//! Mutexes and condition variables behind the PS4's pointer-to-handle ABI (the deleted
//! `src/runtime_mutex.c`): `scePthreadMutex*`, `scePthreadCond*` and their attributes from
//! libkernel, and the POSIX forms from libScePosix, which return bare errno values.
//!
//! A guest mutex variable holds a handle: 0 or 1 is a statically initialized mutex, created on
//! first use (once, even under contention), 2 a destroyed one, anything else a mutex made here.
//! Condition variables are the same, except that destroying one clears the variable to 0.
//! Priority protocols need a PS4 scheduler: the protocol is validated, not used.

use super::{errno, lock, orbis, posix, sce, this_thread, wait_until, GuestTimespec, Status};
use crate::abi::host;
use std::ffi::{c_char, c_int, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Instant;

/// Statically initialized handles.
const STATIC: usize = 2;
/// The value a destroyed mutex's variable holds.
const DESTROYED: usize = 2;

static MUTEXES: AtomicUsize = AtomicUsize::new(0);
static LOCKS: AtomicUsize = AtomicUsize::new(0);
static UNLOCKS: AtomicUsize = AtomicUsize::new(0);
static CONDS: AtomicUsize = AtomicUsize::new(0);
static WAITS: AtomicUsize = AtomicUsize::new(0);
static WAKEUPS: AtomicUsize = AtomicUsize::new(0);
/// Serializes the creation of statically initialized mutexes and condition variables.
static STATIC_INIT: Mutex<()> = Mutex::new(());

/// The PS4 mutex types (`SCE_PTHREAD_MUTEX_*`): 1 error-checking (the default), 2 recursive,
/// 3 normal, 4 adaptive (error-checking here, as on the host before).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    ErrorCheck,
    Recursive,
    Normal,
}

impl Kind {
    fn from_guest(kind: c_int) -> Option<Self> {
        match kind {
            1 | 4 => Some(Kind::ErrorCheck),
            2 => Some(Kind::Recursive),
            3 => Some(Kind::Normal),
            _ => None,
        }
    }
}

/// A mutex attribute object: only the type is kept.
pub struct MutexAttr {
    kind: c_int,
}

/// Who holds a mutex: a thread tag (0: nobody) and how many times (recursive mutexes).
struct Owner {
    thread: u64,
    depth: u32,
}

pub struct GuestMutex {
    kind: Kind,
    owner: Mutex<Owner>,
    released: Condvar,
}

impl GuestMutex {
    fn new(kind: Kind) -> Self {
        GuestMutex {
            kind,
            owner: Mutex::new(Owner {
                thread: 0,
                depth: 0,
            }),
            released: Condvar::new(),
        }
    }

    /// `pthread_mutex_lock` (no deadline), `pthread_mutex_timedlock` or, with `attempt`,
    /// `pthread_mutex_trylock`, as glibc's mutex types answer them.
    fn acquire(&self, attempt: bool, deadline: Option<Instant>) -> Status {
        let me = this_thread();
        let mut owner = lock(&self.owner);
        if owner.thread == me {
            match self.kind {
                Kind::Recursive => {
                    owner.depth = owner.depth.checked_add(1).ok_or(errno::EAGAIN)?;
                    return Ok(());
                }
                Kind::ErrorCheck if attempt => return Err(errno::EBUSY),
                Kind::ErrorCheck => return Err(errno::EDEADLK),
                Kind::Normal if attempt => return Err(errno::EBUSY),
                // A normal mutex locked again by its owner waits (forever, or to the deadline).
                Kind::Normal => {}
            }
        }
        while owner.thread != 0 {
            if attempt {
                return Err(errno::EBUSY);
            }
            let timed_out;
            (owner, timed_out) = wait_until(&self.released, owner, deadline);
            if timed_out && owner.thread != 0 {
                return Err(errno::ETIMEDOUT);
            }
        }
        owner.thread = me;
        owner.depth = 1;
        Ok(())
    }

    /// `pthread_mutex_unlock`: the owner only, except for a normal mutex, which anyone unlocks.
    fn release(&self) -> Status {
        let mut owner = lock(&self.owner);
        if self.kind != Kind::Normal && owner.thread != this_thread() {
            return Err(errno::EPERM);
        }
        owner.depth = owner.depth.saturating_sub(1);
        if owner.depth == 0 || self.kind == Kind::Normal {
            owner.thread = 0;
            owner.depth = 0;
            self.released.notify_one();
        }
        Ok(())
    }

    /// Releases the mutex completely for a condition wait and returns its depth; the caller must
    /// own it. FreeBSD's libthr (the PS4's) does the same with a recursive mutex.
    fn release_all(&self) -> Result<u32, u32> {
        let mut owner = lock(&self.owner);
        if owner.thread != this_thread() {
            return Err(errno::EPERM);
        }
        let depth = owner.depth;
        owner.thread = 0;
        owner.depth = 0;
        self.released.notify_one();
        Ok(depth)
    }

    /// Takes the mutex back after a condition wait, at `depth`.
    fn reacquire(&self, depth: u32) {
        let me = this_thread();
        let mut owner = lock(&self.owner);
        while owner.thread != 0 {
            (owner, _) = wait_until(&self.released, owner, None);
        }
        owner.thread = me;
        owner.depth = depth;
    }
}

/// The guest's handle variable as an atomic word.
///
/// # Safety
/// `slot` is non-null, aligned and valid for the process's life (guest memory).
unsafe fn word<'a>(slot: *mut usize) -> &'a AtomicUsize {
    // SAFETY: the caller's guarantee; all access to the variable from here is atomic.
    unsafe { AtomicUsize::from_ptr(slot) }
}

/// A mutex made here, as a reference that lives as long as the guest does not destroy it (a
/// mutex destroyed while another thread uses it is the guest's error, as it was in C).
///
/// # Safety
/// `handle` came from [`create_mutex`] and was not destroyed.
unsafe fn mutex_at<'a>(handle: usize) -> &'a GuestMutex {
    // SAFETY: the caller's guarantee.
    unsafe { &*(handle as *const GuestMutex) }
}

fn create_mutex(kind: Kind) -> usize {
    MUTEXES.fetch_add(1, Ordering::Relaxed);
    Box::into_raw(Box::new(GuestMutex::new(kind))) as usize
}

fn attr_kind(attr: *mut *mut MutexAttr) -> Result<Kind, u32> {
    if attr.is_null() {
        return Ok(Kind::ErrorCheck);
    }
    // SAFETY: the guest passes null or a pointer to its attribute variable.
    let attr = unsafe { *attr };
    if attr.is_null() {
        return Err(errno::EINVAL);
    }
    // SAFETY: a non-null attribute variable holds an attribute made by `attr_init`.
    Kind::from_guest(unsafe { (*attr).kind }).ok_or(errno::EINVAL)
}

fn init(out: *mut usize, attr: *mut *mut MutexAttr) -> Status {
    if out.is_null() {
        return Err(errno::EINVAL);
    }
    let kind = attr_kind(attr)?;
    // SAFETY: the guest passes a pointer to its mutex variable; POSIX allows it to be
    // uninitialized, so it is written, not read.
    unsafe { out.write(create_mutex(kind)) };
    Ok(())
}

/// The mutex in the guest variable `slot`, created now if statically initialized.
fn ensure_mutex<'a>(slot: *mut usize) -> Result<&'a GuestMutex, u32> {
    if slot.is_null() {
        return Err(errno::EINVAL);
    }
    // SAFETY: the guest passes a pointer to its mutex variable.
    let word = unsafe { word(slot) };
    let mut handle = word.load(Ordering::Acquire);
    if handle == DESTROYED {
        return Err(errno::EINVAL);
    }
    if handle < STATIC {
        let _creating = lock(&STATIC_INIT);
        handle = word.load(Ordering::Acquire);
        if handle < STATIC {
            handle = create_mutex(Kind::ErrorCheck);
            word.store(handle, Ordering::Release);
        }
    }
    // SAFETY: a handle past the static and destroyed values came from `create_mutex`.
    Ok(unsafe { mutex_at(handle) })
}

/// The mutex in `slot` for an unlock or a condition wait: never created here.
fn existing_mutex<'a>(slot: *mut usize, unlocked: u32) -> Result<&'a GuestMutex, u32> {
    if slot.is_null() {
        return Err(errno::EINVAL);
    }
    // SAFETY: the guest passes a pointer to its mutex variable.
    match unsafe { word(slot) }.load(Ordering::Acquire) {
        DESTROYED => Err(errno::EINVAL),
        handle if handle < STATIC => Err(unlocked),
        // SAFETY: a handle past the static and destroyed values came from `create_mutex`.
        handle => Ok(unsafe { mutex_at(handle) }),
    }
}

fn lock_mutex(slot: *mut usize, attempt: bool, deadline: Option<Instant>) -> Status {
    ensure_mutex(slot)?.acquire(attempt, deadline)?;
    LOCKS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

fn unlock_mutex(slot: *mut usize) -> Status {
    existing_mutex(slot, errno::EPERM)?.release()?;
    UNLOCKS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

fn destroy_mutex(slot: *mut usize) -> Status {
    if slot.is_null() {
        return Err(errno::EINVAL);
    }
    // SAFETY: the guest passes a pointer to its mutex variable.
    let word = unsafe { word(slot) };
    let handle = word.load(Ordering::Acquire);
    if handle == DESTROYED {
        return Err(errno::EINVAL);
    }
    if handle < STATIC {
        return Ok(());
    }
    // SAFETY: a handle past the static and destroyed values came from `create_mutex`.
    if lock(&unsafe { mutex_at(handle) }.owner).thread != 0 {
        return Err(errno::EBUSY);
    }
    word.store(DESTROYED, Ordering::Release);
    // SAFETY: made by `Box::into_raw` in `create_mutex`; the variable no longer names it.
    drop(unsafe { Box::from_raw(handle as *mut GuestMutex) });
    Ok(())
}

extern "sysv64" fn attr_init(out: *mut *mut MutexAttr) -> i32 {
    if out.is_null() {
        return orbis(errno::EINVAL);
    }
    // SAFETY: the guest passes a pointer to its attribute variable.
    unsafe { out.write(Box::into_raw(Box::new(MutexAttr { kind: 1 }))) };
    0
}

extern "sysv64" fn attr_type(attr: *mut *mut MutexAttr, kind: c_int) -> i32 {
    // SAFETY: the guest passes null or a pointer to its attribute variable.
    let object = (!attr.is_null()).then(|| unsafe { *attr });
    match object {
        Some(object) if !object.is_null() && Kind::from_guest(kind).is_some() => {
            // SAFETY: a non-null attribute variable holds an attribute made by `attr_init`.
            unsafe { (*object).kind = kind };
            0
        }
        _ => orbis(errno::EINVAL),
    }
}

/// Priority protocols need a PS4 scheduler; the value is validated only.
extern "sysv64" fn attr_protocol(attr: *mut *mut MutexAttr, protocol: c_int) -> i32 {
    // SAFETY: the guest passes null or a pointer to its attribute variable.
    let valid = !attr.is_null() && !unsafe { *attr }.is_null();
    if valid && (0..=2).contains(&protocol) {
        0
    } else {
        orbis(errno::EINVAL)
    }
}

extern "sysv64" fn attr_destroy(attr: *mut *mut MutexAttr) -> i32 {
    // SAFETY: the guest passes null or a pointer to its attribute variable.
    let object = (!attr.is_null()).then(|| unsafe { *attr });
    match object {
        Some(object) if !object.is_null() => {
            // SAFETY: made by `Box::into_raw` in `attr_init`; the variable is cleared.
            unsafe {
                drop(Box::from_raw(object));
                attr.write(std::ptr::null_mut());
            }
            0
        }
        _ => orbis(errno::EINVAL),
    }
}

extern "sysv64" fn mutex_init(
    out: *mut usize,
    attr: *mut *mut MutexAttr,
    _name: *const c_char,
) -> i32 {
    sce(init(out, attr))
}

extern "sysv64" fn mutex_lock(mutex: *mut usize) -> i32 {
    sce(lock_mutex(mutex, false, None))
}

extern "sysv64" fn mutex_trylock(mutex: *mut usize) -> i32 {
    sce(lock_mutex(mutex, true, None))
}

/// `scePthreadMutexTimedlock`: `micros` from now.
extern "sysv64" fn mutex_timedlock(mutex: *mut usize, micros: u32) -> i32 {
    sce(lock_mutex(mutex, false, super::after_micros(micros.into())))
}

extern "sysv64" fn mutex_unlock(mutex: *mut usize) -> i32 {
    sce(unlock_mutex(mutex))
}

extern "sysv64" fn mutex_destroy(mutex: *mut usize) -> i32 {
    sce(destroy_mutex(mutex))
}

/// A condition variable: a wakeup sequence number under its own lock.
pub struct GuestCond {
    sequence: Mutex<u64>,
    changed: Condvar,
}

fn create_cond() -> usize {
    CONDS.fetch_add(1, Ordering::Relaxed);
    let cond = GuestCond {
        sequence: Mutex::new(0),
        changed: Condvar::new(),
    };
    Box::into_raw(Box::new(cond)) as usize
}

/// # Safety
/// `handle` came from [`create_cond`] and was not destroyed.
unsafe fn cond_at<'a>(handle: usize) -> &'a GuestCond {
    // SAFETY: the caller's guarantee.
    unsafe { &*(handle as *const GuestCond) }
}

fn ensure_cond<'a>(slot: *mut usize) -> Result<&'a GuestCond, u32> {
    if slot.is_null() {
        return Err(errno::EINVAL);
    }
    // SAFETY: the guest passes a pointer to its condition variable.
    let word = unsafe { word(slot) };
    let mut handle = word.load(Ordering::Acquire);
    if handle < STATIC {
        let _creating = lock(&STATIC_INIT);
        handle = word.load(Ordering::Acquire);
        if handle < STATIC {
            handle = create_cond();
            word.store(handle, Ordering::Release);
        }
    }
    // SAFETY: a handle past the static values came from `create_cond`.
    Ok(unsafe { cond_at(handle) })
}

/// `pthread_cond_wait` (no deadline) or `pthread_cond_timedwait`: releases `mutex` (which the
/// caller must own), waits for a signal or the deadline, and takes `mutex` back either way.
/// Wakeups may be spurious, as POSIX allows.
fn wait(cond: *mut usize, mutex: *mut usize, deadline: Option<Instant>) -> Status {
    let cond = ensure_cond(cond)?;
    let mutex = existing_mutex(mutex, errno::EINVAL)?;
    WAITS.fetch_add(1, Ordering::Relaxed);
    let mut sequence = lock(&cond.sequence);
    let ticket = *sequence;
    // Released while the sequence is locked: a signal after the release cannot be missed.
    let depth = mutex.release_all()?;
    let mut result = Ok(());
    while *sequence == ticket {
        let timed_out;
        (sequence, timed_out) = wait_until(&cond.changed, sequence, deadline);
        if timed_out && *sequence == ticket {
            result = Err(errno::ETIMEDOUT);
            break;
        }
    }
    drop(sequence);
    mutex.reacquire(depth);
    result
}

fn wake(cond: *mut usize, all: bool) -> Status {
    let cond = ensure_cond(cond)?;
    WAKEUPS.fetch_add(1, Ordering::Relaxed);
    let mut sequence = lock(&cond.sequence);
    *sequence = sequence.wrapping_add(1);
    if all {
        cond.changed.notify_all();
    } else {
        cond.changed.notify_one();
    }
    Ok(())
}

fn destroy_cond(slot: *mut usize) -> Status {
    if slot.is_null() {
        return Err(errno::EINVAL);
    }
    // SAFETY: the guest passes a pointer to its condition variable.
    let word = unsafe { word(slot) };
    let handle = word.load(Ordering::Acquire);
    if handle >= STATIC {
        word.store(0, Ordering::Release);
        // SAFETY: made by `Box::into_raw` in `create_cond`; the variable no longer names it.
        drop(unsafe { Box::from_raw(handle as *mut GuestCond) });
    }
    Ok(())
}

fn init_cond(out: *mut usize) -> Status {
    if out.is_null() {
        return Err(errno::EINVAL);
    }
    // SAFETY: the guest passes a pointer to its condition variable (written, not read).
    unsafe { out.write(create_cond()) };
    Ok(())
}

fn abs_wait(cond: *mut usize, mutex: *mut usize, time: *const GuestTimespec) -> Status {
    if time.is_null() {
        return Err(errno::EINVAL);
    }
    // SAFETY: the guest passes a pointer to its timespec.
    let deadline = unsafe { time.read_unaligned() }.deadline()?;
    wait(cond, mutex, deadline)
}

extern "sysv64" fn cond_init(
    out: *mut usize,
    _attr: *mut *mut c_void,
    _name: *const c_char,
) -> i32 {
    sce(init_cond(out))
}

extern "sysv64" fn cond_destroy(cond: *mut usize) -> i32 {
    sce(destroy_cond(cond))
}

extern "sysv64" fn cond_wait(cond: *mut usize, mutex: *mut usize) -> i32 {
    sce(wait(cond, mutex, None))
}

/// `scePthreadCondTimedwait`: `micros` from now.
extern "sysv64" fn cond_timedwait(cond: *mut usize, mutex: *mut usize, micros: u32) -> i32 {
    sce(wait(cond, mutex, super::after_micros(micros.into())))
}

extern "sysv64" fn cond_signal(cond: *mut usize) -> i32 {
    sce(wake(cond, false))
}

extern "sysv64" fn cond_broadcast(cond: *mut usize) -> i32 {
    sce(wake(cond, true))
}

/// Condition attributes carry nothing the condition variables use (clock and process sharing
/// are fixed), so every attribute is this one object.
static CONDATTR: u8 = 0;

fn condattr() -> *mut c_void {
    std::ptr::addr_of!(CONDATTR).cast_mut().cast()
}

extern "sysv64" fn condattr_init(out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return orbis(errno::EINVAL);
    }
    // SAFETY: the guest passes a pointer to its attribute variable.
    unsafe { out.write(condattr()) };
    0
}

extern "sysv64" fn condattr_destroy(attr: *mut *mut c_void) -> i32 {
    // SAFETY: the guest passes null or a pointer to its attribute variable.
    if attr.is_null() || unsafe { *attr } != condattr() {
        return orbis(errno::EINVAL);
    }
    // SAFETY: as above.
    unsafe { attr.write(std::ptr::null_mut()) };
    0
}

/// The POSIX forms return bare errno values (the low 16 bits of the Orbis error).
fn bare(result: i32) -> i32 {
    if result == 0 {
        0
    } else {
        (result as u32 & 0xffff) as i32
    }
}

extern "sysv64" fn posix_attr_init(out: *mut *mut MutexAttr) -> i32 {
    bare(attr_init(out))
}

extern "sysv64" fn posix_attr_type(attr: *mut *mut MutexAttr, kind: c_int) -> i32 {
    bare(attr_type(attr, kind))
}

extern "sysv64" fn posix_attr_destroy(attr: *mut *mut MutexAttr) -> i32 {
    bare(attr_destroy(attr))
}

extern "sysv64" fn posix_mutex_init(out: *mut usize, attr: *mut *mut MutexAttr) -> i32 {
    posix(init(out, attr))
}

extern "sysv64" fn posix_mutex_destroy(mutex: *mut usize) -> i32 {
    posix(destroy_mutex(mutex))
}

extern "sysv64" fn posix_mutex_lock(mutex: *mut usize) -> i32 {
    posix(lock_mutex(mutex, false, None))
}

extern "sysv64" fn posix_mutex_trylock(mutex: *mut usize) -> i32 {
    posix(lock_mutex(mutex, true, None))
}

extern "sysv64" fn posix_mutex_unlock(mutex: *mut usize) -> i32 {
    posix(unlock_mutex(mutex))
}

extern "sysv64" fn posix_condattr_init(out: *mut *mut c_void) -> i32 {
    bare(condattr_init(out))
}

extern "sysv64" fn posix_condattr_destroy(attr: *mut *mut c_void) -> i32 {
    bare(condattr_destroy(attr))
}

extern "sysv64" fn posix_cond_init(out: *mut usize, _attr: *mut *mut c_void) -> i32 {
    posix(init_cond(out))
}

extern "sysv64" fn posix_cond_destroy(cond: *mut usize) -> i32 {
    posix(destroy_cond(cond))
}

extern "sysv64" fn posix_cond_wait(cond: *mut usize, mutex: *mut usize) -> i32 {
    posix(wait(cond, mutex, None))
}

/// `pthread_cond_timedwait`: an absolute wall-clock time.
extern "sysv64" fn posix_cond_timedwait(
    cond: *mut usize,
    mutex: *mut usize,
    time: *const GuestTimespec,
) -> i32 {
    posix(abs_wait(cond, mutex, time))
}

extern "sysv64" fn posix_cond_signal(cond: *mut usize) -> i32 {
    posix(wake(cond, false))
}

extern "sysv64" fn posix_cond_broadcast(cond: *mut usize) -> i32 {
    posix(wake(cond, true))
}

/// The function for scoped NID `name`: `#p#J` is libkernel, `#I#J` libScePosix.
pub(super) fn resolve(name: &[u8]) -> Option<usize> {
    let address = match name {
        b"m5-2bsNfv7s#p#J" => condattr_init as *const () as usize,
        b"waPcxYiR3WA#p#J" => condattr_destroy as *const () as usize,
        b"mKoTx03HRWA#I#J" => posix_condattr_init as *const () as usize,
        b"dJcuQVn6-Iw#I#J" => posix_condattr_destroy as *const () as usize,
        b"0TyVk4MSLt0#I#J" => posix_cond_init as *const () as usize,
        b"RXXqi4CtF8w#I#J" => posix_cond_destroy as *const () as usize,
        b"Op8TBGY5KHg#I#J" => posix_cond_wait as *const () as usize,
        b"27bAgiJmOh0#I#J" => posix_cond_timedwait as *const () as usize,
        b"2MOy+rUfuhQ#I#J" => posix_cond_signal as *const () as usize,
        b"mkx2fVhNMsg#I#J" => posix_cond_broadcast as *const () as usize,
        b"2Tb92quprl0#p#J" => cond_init as *const () as usize,
        b"g+PZd2hiacg#p#J" => cond_destroy as *const () as usize,
        b"WKAXJ4XBPQ4#p#J" => cond_wait as *const () as usize,
        b"BmMjYxmew1w#p#J" => cond_timedwait as *const () as usize,
        b"kDh-NfxgMtE#p#J" => cond_signal as *const () as usize,
        b"JGgj7Uvrl+A#p#J" => cond_broadcast as *const () as usize,
        b"IafI2PxcPnQ#p#J" => mutex_timedlock as *const () as usize,
        b"dQHWEsJtoE4#I#J" => posix_attr_init as *const () as usize,
        b"mDmgMOGVUqg#I#J" => posix_attr_type as *const () as usize,
        b"HF7lK46xzjY#I#J" => posix_attr_destroy as *const () as usize,
        b"ttHNfU+qDBU#I#J" => posix_mutex_init as *const () as usize,
        b"ltCfaGr2JGE#I#J" => posix_mutex_destroy as *const () as usize,
        b"7H0iTOciTLo#I#J" => posix_mutex_lock as *const () as usize,
        b"K-jXhbt2gn4#I#J" => posix_mutex_trylock as *const () as usize,
        b"2Z+PpY6CaJg#I#J" => posix_mutex_unlock as *const () as usize,
        b"F8bUHwAG284#p#J" => attr_init as *const () as usize,
        b"iMp8QpE+XO4#p#J" => attr_type as *const () as usize,
        b"smWEktiyyG0#p#J" => attr_destroy as *const () as usize,
        b"1FGvU0i9saQ#p#J" => attr_protocol as *const () as usize,
        b"cmo1RIYva9o#p#J" => mutex_init as *const () as usize,
        b"9UK1vLZQft4#p#J" => mutex_lock as *const () as usize,
        b"upoVrzMHFeE#p#J" => mutex_trylock as *const () as usize,
        b"tn3VlD0hG60#p#J" => mutex_unlock as *const () as usize,
        b"2Of0f+3mhhE#p#J" => mutex_destroy as *const () as usize,
        _ => return None,
    };
    Some(address)
}

pub(super) fn report() {
    host::out_line(&format!(
        "Runtime: mutexes created={}, locks={}, unlocks={}",
        MUTEXES.load(Ordering::Relaxed),
        LOCKS.load(Ordering::Relaxed),
        UNLOCKS.load(Ordering::Relaxed)
    ));
    host::out_line(&format!(
        "Runtime: condition variables created={}, waits={}, wakeups={}",
        CONDS.load(Ordering::Relaxed),
        WAITS.load(Ordering::Relaxed),
        WAKEUPS.load(Ordering::Relaxed)
    ));
}

#[cfg(test)]
mod tests {
    //! `tests/test_runtime.c`'s `mutexes` and `posix_mutexes`, and the condition variable
    //! contracts the C file had no test for.

    use super::*;
    use std::mem::transmute;
    use std::ptr::null_mut;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    type AttrInit = extern "sysv64" fn(*mut *mut c_void) -> i32;
    type AttrType = extern "sysv64" fn(*mut *mut c_void, c_int) -> i32;
    type Init = extern "sysv64" fn(*mut usize, *mut *mut c_void, *const c_char) -> i32;
    type PosixInit = extern "sysv64" fn(*mut usize, *mut *mut c_void) -> i32;
    type HandleOp = extern "sysv64" fn(*mut usize) -> i32;
    type AttrOp = extern "sysv64" fn(*mut *mut c_void) -> i32;
    type Timed = extern "sysv64" fn(*mut usize, u32) -> i32;
    type CondWait = extern "sysv64" fn(*mut usize, *mut usize) -> i32;
    type CondTimed = extern "sysv64" fn(*mut usize, *mut usize, u32) -> i32;
    type CondAbs = extern "sysv64" fn(*mut usize, *mut usize, *const GuestTimespec) -> i32;

    fn get(name: &[u8]) -> usize {
        resolve(name).unwrap_or_else(|| panic!("{} does not resolve", name.escape_ascii()))
    }

    macro_rules! guest {
        ($kind:ty, $name:literal) => {{
            // SAFETY: the function behind this NID has this signature.
            unsafe { transmute::<usize, $kind>(get($name)) }
        }};
    }

    fn hex(value: i32) -> u32 {
        value as u32
    }

    #[test]
    fn mutex_errors() {
        let attr_init = guest!(AttrInit, b"F8bUHwAG284#p#J");
        let kind = guest!(AttrType, b"iMp8QpE+XO4#p#J");
        let attr_destroy = guest!(AttrOp, b"smWEktiyyG0#p#J");
        let init = guest!(Init, b"cmo1RIYva9o#p#J");
        let lock = guest!(HandleOp, b"9UK1vLZQft4#p#J");
        let unlock = guest!(HandleOp, b"tn3VlD0hG60#p#J");
        let trylock = guest!(HandleOp, b"upoVrzMHFeE#p#J");
        let destroy = guest!(HandleOp, b"2Of0f+3mhhE#p#J");
        let (mut attr, mut mutex): (*mut c_void, usize) = (null_mut(), 0);
        assert_eq!(hex(attr_init(null_mut())), 0x8002_0016);
        assert_eq!(attr_init(&mut attr), 0);
        assert_eq!(hex(kind(&mut attr, 99)), 0x8002_0016);
        assert_eq!(kind(&mut attr, 2), 0);
        assert_eq!(init(&mut mutex, &mut attr, c"recursive-test".as_ptr()), 0);
        assert_eq!(attr_destroy(&mut attr), 0);
        assert!(attr.is_null());
        assert_eq!(lock(&mut mutex), 0);
        assert_eq!(trylock(&mut mutex), 0);
        assert_eq!(hex(destroy(&mut mutex)), 0x8002_0010);
        assert_eq!(unlock(&mut mutex), 0);
        assert_eq!(unlock(&mut mutex), 0);
        assert_eq!(destroy(&mut mutex), 0);
        assert_eq!(hex(lock(&mut mutex)), 0x8002_0016);
        mutex = 0;
        assert_eq!(lock(&mut mutex), 0); // static initialization
        assert_eq!(hex(lock(&mut mutex)), 0x8002_000b);
        assert_eq!(hex(trylock(&mut mutex)), 0x8002_0010);
        assert_eq!(unlock(&mut mutex), 0);
        assert_eq!(destroy(&mut mutex), 0);
        // Beyond the C test: the null and unlocked cases and attribute misuse.
        assert_eq!(hex(lock(null_mut())), 0x8002_0016);
        assert_eq!(hex(destroy(null_mut())), 0x8002_0016);
        mutex = 0;
        assert_eq!(hex(unlock(&mut mutex)), 0x8002_0001);
        assert_eq!(destroy(&mut mutex), 0);
        mutex = 1;
        assert_eq!(destroy(&mut mutex), 0);
        assert_eq!(mutex, 1);
        assert_eq!(
            hex(init(null_mut(), null_mut(), std::ptr::null())),
            0x8002_0016
        );
        let mut cleared: *mut c_void = null_mut();
        assert_eq!(
            hex(init(&mut mutex, &mut cleared, std::ptr::null())),
            0x8002_0016
        );
        assert_eq!(hex(attr_destroy(&mut cleared)), 0x8002_0016);
        assert_eq!(hex(kind(&mut cleared, 1)), 0x8002_0016);
        let protocol = guest!(AttrType, b"1FGvU0i9saQ#p#J");
        assert_eq!(attr_init(&mut attr), 0);
        assert_eq!(protocol(&mut attr, 2), 0);
        assert_eq!(hex(protocol(&mut attr, 3)), 0x8002_0016);
        assert_eq!(hex(protocol(&mut cleared, 0)), 0x8002_0016);
        assert_eq!(attr_destroy(&mut attr), 0);
    }

    #[test]
    fn posix_mutexes() {
        let init = guest!(AttrInit, b"dQHWEsJtoE4#I#J");
        let kind = guest!(AttrType, b"mDmgMOGVUqg#I#J");
        let destroy_attr = guest!(AttrOp, b"HF7lK46xzjY#I#J");
        let mutex_init = guest!(PosixInit, b"ttHNfU+qDBU#I#J");
        let lock = guest!(HandleOp, b"7H0iTOciTLo#I#J");
        let unlock = guest!(HandleOp, b"2Z+PpY6CaJg#I#J");
        let trylock = guest!(HandleOp, b"K-jXhbt2gn4#I#J");
        let destroy = guest!(HandleOp, b"ltCfaGr2JGE#I#J");
        let (mut a, mut m): (*mut c_void, usize) = (null_mut(), 0);
        assert_eq!(init(null_mut()), 22);
        assert_eq!(init(&mut a), 0);
        assert_eq!(kind(&mut a, 99), 22);
        assert_eq!(kind(&mut a, 2), 0);
        assert_eq!(mutex_init(&mut m, &mut a), 0);
        assert_eq!(destroy_attr(&mut a), 0);
        assert_eq!(lock(&mut m), 0);
        assert_eq!(trylock(&mut m), 0);
        assert_eq!(destroy(&mut m), 16);
        assert_eq!(unlock(&mut m), 0);
        assert_eq!(unlock(&mut m), 0);
        assert_eq!(destroy(&mut m), 0);
        m = 0;
        assert_eq!(lock(&mut m), 0);
        assert_eq!(lock(&mut m), 11);
        assert_eq!(trylock(&mut m), 16);
        assert_eq!(unlock(&mut m), 0);
        assert_eq!(destroy(&mut m), 0);
        assert_eq!(resolve(b"dQHWEsJtoE4#q#q"), None);
    }

    #[test]
    fn ownership_and_normal_mutexes() {
        let attr_init = guest!(AttrInit, b"F8bUHwAG284#p#J");
        let kind = guest!(AttrType, b"iMp8QpE+XO4#p#J");
        let attr_destroy = guest!(AttrOp, b"smWEktiyyG0#p#J");
        let init = guest!(Init, b"cmo1RIYva9o#p#J");
        let lock = guest!(HandleOp, b"9UK1vLZQft4#p#J");
        let unlock = guest!(HandleOp, b"tn3VlD0hG60#p#J");
        let trylock = guest!(HandleOp, b"upoVrzMHFeE#p#J");
        let destroy = guest!(HandleOp, b"2Of0f+3mhhE#p#J");
        let timed = guest!(Timed, b"IafI2PxcPnQ#p#J");
        // An error-checking mutex refuses an unlock from another thread.
        let mut checked = 0usize;
        assert_eq!(lock(&mut checked), 0);
        let address = &mut checked as *mut usize as usize;
        let other = std::thread::spawn(move || {
            let slot = address as *mut usize;
            (unlock(slot), trylock(slot), timed(slot, 1000))
        })
        .join()
        .unwrap();
        assert_eq!(
            (hex(other.0), hex(other.1), hex(other.2)),
            (0x8002_0001, 0x8002_0010, 0x8002_003c)
        );
        // The owner's timed lock of its own error-checking mutex is a deadlock, not a wait.
        assert_eq!(hex(timed(&mut checked, 1_000_000)), 0x8002_000b);
        assert_eq!(unlock(&mut checked), 0);
        assert_eq!(timed(&mut checked, 0), 0); // free: the deadline does not matter
        assert_eq!(unlock(&mut checked), 0);
        assert_eq!(destroy(&mut checked), 0);
        // A normal mutex: trylock by its owner is busy, its timed relock times out, and anyone may
        // unlock it.
        let mut attr: *mut c_void = null_mut();
        let mut normal = 0usize;
        assert_eq!(attr_init(&mut attr), 0);
        assert_eq!(kind(&mut attr, 3), 0);
        assert_eq!(init(&mut normal, &mut attr, std::ptr::null()), 0);
        assert_eq!(attr_destroy(&mut attr), 0);
        assert_eq!(lock(&mut normal), 0);
        assert_eq!(hex(trylock(&mut normal)), 0x8002_0010);
        let start = Instant::now();
        assert_eq!(hex(timed(&mut normal, 20_000)), 0x8002_003c);
        assert!(start.elapsed() >= Duration::from_millis(20));
        let address = &mut normal as *mut usize as usize;
        let other = std::thread::spawn(move || unlock(address as *mut usize));
        assert_eq!(other.join().unwrap(), 0);
        assert_eq!(trylock(&mut normal), 0);
        assert_eq!(unlock(&mut normal), 0);
        assert_eq!(destroy(&mut normal), 0);
    }

    #[test]
    fn contention_excludes_and_waits() {
        let lock = guest!(HandleOp, b"9UK1vLZQft4#p#J");
        let unlock = guest!(HandleOp, b"tn3VlD0hG60#p#J");
        let destroy = guest!(HandleOp, b"2Of0f+3mhhE#p#J");
        // A statically initialized mutex shared by threads that race to create it.
        let slot: &'static AtomicUsize = Box::leak(Box::new(AtomicUsize::new(0)));
        let counter: &'static AtomicUsize = Box::leak(Box::new(AtomicUsize::new(0)));
        let inside: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(move || {
                    for _ in 0..500 {
                        assert_eq!(lock(slot.as_ptr()), 0);
                        assert!(!inside.swap(true, Ordering::SeqCst));
                        counter.fetch_add(1, Ordering::Relaxed);
                        inside.store(false, Ordering::SeqCst);
                        assert_eq!(unlock(slot.as_ptr()), 0);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(counter.load(Ordering::Relaxed), 2000);
        assert_eq!(destroy(slot.as_ptr()), 0);
        assert_eq!(slot.load(Ordering::Relaxed), DESTROYED);
    }

    #[test]
    fn condition_variables() {
        let lock = guest!(HandleOp, b"9UK1vLZQft4#p#J");
        let unlock = guest!(HandleOp, b"tn3VlD0hG60#p#J");
        let cond_init = guest!(Init, b"2Tb92quprl0#p#J");
        let destroy = guest!(HandleOp, b"g+PZd2hiacg#p#J");
        let wait = guest!(CondWait, b"WKAXJ4XBPQ4#p#J");
        let timed = guest!(CondTimed, b"BmMjYxmew1w#p#J");
        let signal = guest!(HandleOp, b"kDh-NfxgMtE#p#J");
        let broadcast = guest!(HandleOp, b"JGgj7Uvrl+A#p#J");
        let mut cond = 0usize;
        let mut mutex = 0usize;
        assert_eq!(
            hex(cond_init(null_mut(), null_mut(), std::ptr::null())),
            0x8002_0016
        );
        assert_eq!(cond_init(&mut cond, null_mut(), c"cond".as_ptr()), 0);
        // The mutex must exist and be owned.
        assert_eq!(hex(wait(&mut cond, &mut mutex)), 0x8002_0016);
        assert_eq!(hex(wait(&mut cond, null_mut())), 0x8002_0016);
        assert_eq!(lock(&mut mutex), 0);
        assert_eq!(unlock(&mut mutex), 0);
        assert_eq!(hex(wait(&mut cond, &mut mutex)), 0x8002_0001);
        // A timed wait with nobody signalling times out and holds the mutex again.
        assert_eq!(lock(&mut mutex), 0);
        let start = Instant::now();
        assert_eq!(hex(timed(&mut cond, &mut mutex, 20_000)), 0x8002_003c);
        assert!(start.elapsed() >= Duration::from_millis(20));
        assert_eq!(hex(lock(&mut mutex)), 0x8002_000b);
        assert_eq!(unlock(&mut mutex), 0);
        // Signal and broadcast wake waiters that hold the mutex again on return. A waiter counts
        // itself under the mutex and releases it only inside `wait`, so once the count is 2 with
        // the mutex held here, both are waiting.
        let shared: &'static [AtomicUsize; 4] = Box::leak(Box::new([
            AtomicUsize::new(cond),
            AtomicUsize::new(mutex),
            AtomicUsize::new(0),
            AtomicUsize::new(0),
        ]));
        let waiters: Vec<_> = (0..2)
            .map(|_| {
                std::thread::spawn(move || {
                    let [cond, mutex, flag, waiting] = shared;
                    assert_eq!(lock(mutex.as_ptr()), 0);
                    waiting.fetch_add(1, Ordering::SeqCst);
                    while flag.load(Ordering::SeqCst) == 0 {
                        assert_eq!(wait(cond.as_ptr(), mutex.as_ptr()), 0);
                    }
                    assert_eq!(hex(lock(mutex.as_ptr())), 0x8002_000b); // owned again
                    assert_eq!(unlock(mutex.as_ptr()), 0);
                })
            })
            .collect();
        let [cond_word, mutex_word, flag, waiting] = shared;
        loop {
            assert_eq!(lock(mutex_word.as_ptr()), 0);
            if waiting.load(Ordering::SeqCst) == 2 {
                break;
            }
            assert_eq!(unlock(mutex_word.as_ptr()), 0);
            std::thread::sleep(Duration::from_millis(1));
        }
        flag.store(1, Ordering::SeqCst);
        assert_eq!(signal(cond_word.as_ptr()), 0);
        assert_eq!(broadcast(cond_word.as_ptr()), 0);
        assert_eq!(unlock(mutex_word.as_ptr()), 0);
        for waiter in waiters {
            waiter.join().unwrap();
        }
        // A recursive mutex is released completely for the wait and restored after it.
        let mut attr: *mut c_void = null_mut();
        let mut recursive = 0usize;
        assert_eq!(guest!(AttrInit, b"F8bUHwAG284#p#J")(&mut attr), 0);
        assert_eq!(guest!(AttrType, b"iMp8QpE+XO4#p#J")(&mut attr, 2), 0);
        assert_eq!(
            guest!(Init, b"cmo1RIYva9o#p#J")(&mut recursive, &mut attr, std::ptr::null()),
            0
        );
        assert_eq!(lock(&mut recursive), 0);
        assert_eq!(lock(&mut recursive), 0);
        assert_eq!(hex(timed(&mut cond, &mut recursive, 1000)), 0x8002_003c);
        assert_eq!(unlock(&mut recursive), 0);
        assert_eq!(unlock(&mut recursive), 0);
        assert_eq!(hex(unlock(&mut recursive)), 0x8002_0001);
        // Destroy clears the variable, which then initializes statically again.
        assert_eq!(destroy(&mut cond), 0);
        assert_eq!(cond, 0);
        assert_eq!(destroy(&mut cond), 0);
        assert_eq!(signal(&mut cond), 0);
        assert_ne!(cond, 0);
        assert_eq!(destroy(&mut cond), 0);
        assert_eq!(hex(signal(null_mut())), 0x8002_0016);
        assert_eq!(hex(destroy(null_mut())), 0x8002_0016);
    }

    #[test]
    fn posix_condition_variables_and_attributes() {
        let attr_init = guest!(AttrOp, b"m5-2bsNfv7s#p#J");
        let attr_destroy = guest!(AttrOp, b"waPcxYiR3WA#p#J");
        let posix_attr_init = guest!(AttrOp, b"mKoTx03HRWA#I#J");
        let posix_attr_destroy = guest!(AttrOp, b"dJcuQVn6-Iw#I#J");
        let init = guest!(PosixInit, b"0TyVk4MSLt0#I#J");
        let destroy = guest!(HandleOp, b"RXXqi4CtF8w#I#J");
        let wait = guest!(CondWait, b"Op8TBGY5KHg#I#J");
        let timed = guest!(CondAbs, b"27bAgiJmOh0#I#J");
        let signal = guest!(HandleOp, b"2MOy+rUfuhQ#I#J");
        let broadcast = guest!(HandleOp, b"mkx2fVhNMsg#I#J");
        let lock = guest!(HandleOp, b"7H0iTOciTLo#I#J");
        let unlock = guest!(HandleOp, b"2Z+PpY6CaJg#I#J");
        let mut attr: *mut c_void = null_mut();
        assert_eq!(hex(attr_init(null_mut())), 0x8002_0016);
        assert_eq!(attr_init(&mut attr), 0);
        assert_eq!(attr_destroy(&mut attr), 0);
        assert!(attr.is_null());
        assert_eq!(hex(attr_destroy(&mut attr)), 0x8002_0016);
        let mut other = std::ptr::dangling_mut::<c_void>(); // not the condition attribute
        assert_eq!(posix_attr_destroy(&mut other), 22);
        assert_eq!(posix_attr_init(null_mut()), 22);
        assert_eq!(posix_attr_init(&mut attr), 0);
        let (mut cond, mut mutex) = (0usize, 0usize);
        assert_eq!(init(&mut cond, &mut attr), 0);
        assert_eq!(posix_attr_destroy(&mut attr), 0);
        assert_eq!(init(null_mut(), null_mut()), 22);
        assert_eq!(wait(&mut cond, &mut mutex), 22);
        assert_eq!(lock(&mut mutex), 0);
        let past = GuestTimespec {
            seconds: 0,
            nanoseconds: 0,
        };
        let invalid = GuestTimespec {
            seconds: 0,
            nanoseconds: 1_000_000_000,
        };
        assert_eq!(timed(&mut cond, &mut mutex, &past), 60);
        assert_eq!(timed(&mut cond, &mut mutex, &invalid), 22);
        assert_eq!(timed(&mut cond, &mut mutex, std::ptr::null()), 22);
        assert_eq!(signal(&mut cond), 0);
        assert_eq!(broadcast(&mut cond), 0);
        assert_eq!(unlock(&mut mutex), 0);
        assert_eq!(wait(&mut cond, &mut mutex), 1);
        assert_eq!(destroy(&mut cond), 0);
        assert_eq!(destroy(null_mut()), 22);
        assert_eq!(guest!(HandleOp, b"ltCfaGr2JGE#I#J")(&mut mutex), 0);
    }

    #[test]
    fn report_runs() {
        // The report prints through C stdio; here it must only run.
        report();
    }
}
