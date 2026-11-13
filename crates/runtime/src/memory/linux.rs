//! The Linux host of [`super`]: one sparse memfd behind the direct pool and the flexible span,
//! and the C symbols `src/runtime_memory.c` exported (it is no longer built on Linux).
//!
//! The guest functions (`sceKernelMapDirectMemory` and the rest) are not linker symbols, as the
//! C ones were `static`: [`runtime_memory_resolve`] hands out their addresses by NID. The
//! `runtime_memory_*`, `runtime_*_map` and `runtime_disabled_optimizations*` symbols are the
//! loader's and the GPU library's (`gpu/shim/bbgpu.cpp`). `runtime_fault_recover` is a C
//! thread-local, which Rust cannot export: it is in `src/runtime_fault.c`.

use super::*;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::ptr;
use std::sync::atomic::AtomicBool;
use std::sync::OnceLock;

/// The direct pool and the flexible span as one memfd, plus a second view of all of it: host
/// writes through the view bypass guest and GPU page protection.
pub struct MemfdHost {
    fd: c_int,
    backing: usize,
}

impl MemfdHost {
    pub const fn new() -> Self {
        Self { fd: -1, backing: 0 }
    }

    /// The host address of backing byte `phys` (0 before the pool exists).
    pub fn backing(&self, phys: u64) -> usize {
        self.backing + phys as usize
    }
}

impl Default for MemfdHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Host for MemfdHost {
    fn pool(&mut self, bytes: u64) -> bool {
        // As the C: a memfd that exists counts as the pool, even if sizing it failed.
        if self.fd >= 0 {
            return true;
        }
        // SAFETY: the name is a NUL-terminated string; the flags are valid.
        self.fd = unsafe { libc::memfd_create(c"bb-guest-memory".as_ptr(), libc::MFD_CLOEXEC) };
        // SAFETY: `ftruncate` on the descriptor just made (or a failing -1).
        if self.fd < 0 || unsafe { libc::ftruncate(self.fd, bytes as libc::off_t) } != 0 {
            return false;
        }
        // SAFETY: a new shared mapping of the whole memfd at an address the kernel picks.
        let view = unsafe {
            libc::mmap(
                ptr::null_mut(),
                bytes as usize,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_NORESERVE,
                self.fd,
                0,
            )
        };
        if view == libc::MAP_FAILED {
            return false;
        }
        self.backing = view as usize;
        true
    }

    fn map(&self, address: u64, size: u64, prot: i32, reserved: bool, phys: u64) -> bool {
        // SAFETY: the guest chose this range of its own address space (the PS4 user range, or
        // MAP_FIXED where it asked), and replacing whatever was mapped there is the contract,
        // as in the C. `phys` and `size` lie inside the memfd (checked against the blocks).
        let mapped = unsafe {
            if reserved {
                libc::mmap(
                    address as *mut c_void,
                    size as usize,
                    libc::PROT_NONE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE | libc::MAP_FIXED,
                    -1,
                    0,
                )
            } else {
                libc::mmap(
                    address as *mut c_void,
                    size as usize,
                    prot,
                    libc::MAP_SHARED | libc::MAP_FIXED,
                    self.fd,
                    phys as libc::off_t,
                )
            }
        };
        mapped != libc::MAP_FAILED
    }

    fn unmap(&self, address: u64, size: u64) -> bool {
        // SAFETY: a range the region table holds as guest memory.
        unsafe { libc::munmap(address as *mut c_void, size as usize) == 0 }
    }

    fn protect(&self, address: u64, size: u64, prot: i32) -> bool {
        // SAFETY: a range the region table holds as guest memory; only its access changes.
        unsafe { libc::mprotect(address as *mut c_void, size as usize, prot) == 0 }
    }

    fn discard(&self, phys: u64, size: u64) {
        let mode = libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE;
        // SAFETY: frees pages of the memfd; a failure leaves the old data, as in the C.
        unsafe { libc::fallocate(self.fd, mode, phys as libc::off_t, size as libc::off_t) };
    }
}

static POOL: OnceLock<u64> = OnceLock::new();
static MEMORY: OnceLock<Memory<MemfdHost>> = OnceLock::new();
/// The next address `runtime_low_map` tries; changed under the table lock.
static LOW_NEXT: AtomicU64 = AtomicU64::new(LOW_MIN);
static WATCHER: AtomicBool = AtomicBool::new(false);
/// Linux 4.17's `MAP_FIXED_NOREPLACE` (the same value on every architecture); the libc crate
/// has no Android definition.
const MAP_FIXED_NOREPLACE: c_int = 0x10_0000;

fn env(name: &str) -> Option<String> {
    std::env::var_os(name).map(|value| value.to_string_lossy().into_owned())
}

/// `BB_DMEM_MB`, read once.
fn pool() -> u64 {
    *POOL.get_or_init(|| pool_size(env("BB_DMEM_MB").as_deref()))
}

/// The process's address space; `BB_MEMORY_LOG=1` traces every mapping call.
fn memory() -> &'static Memory<MemfdHost> {
    MEMORY.get_or_init(|| {
        let trace = env("BB_MEMORY_LOG").is_some_and(|value| value.starts_with('1'));
        Memory::new(Space::new(pool(), MemfdHost::new()).with_trace(trace))
    })
}

fn trace() -> bool {
    memory().read(Space::trace)
}

/// Writes `value` to a guest output, when the guest passed one.
///
/// # Safety
/// `out` is null or valid for an unaligned write.
unsafe fn put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        // SAFETY: non-null, and the caller guarantees it is writable.
        unsafe { out.write_unaligned(value) };
    }
}

/// Reads a guest in-out address (0 for a null pointer).
///
/// # Safety
/// `inout` is null or valid for an unaligned read.
unsafe fn get(inout: *const u64) -> u64 {
    if inout.is_null() {
        0
    } else {
        // SAFETY: non-null, and the caller guarantees it is readable.
        unsafe { inout.read_unaligned() }
    }
}

fn code(result: &Result<u64, i32>) -> i32 {
    match result {
        Ok(_) => 0,
        Err(error) => *error,
    }
}

extern "C" fn direct_size() -> u64 {
    pool()
}

unsafe extern "C" fn direct_allocate(
    low: i64,
    high: i64,
    size: u64,
    alignment: u64,
    memory_type: c_int,
    out: *mut i64,
) -> i32 {
    if out.is_null() {
        return INVALID;
    }
    let (low_start, end, alignment) =
        match direct_window(pool(), low, high, size, alignment, memory_type) {
            Ok(window) => window,
            Err(NO_SPACE) => {
                say(&refusal_line(pool(), low, high, size));
                return NO_SPACE;
            }
            Err(error) => return error,
        };
    let start =
        match memory().write(|s| s.allocate_direct(low_start, end, size, alignment, memory_type)) {
            Ok(start) => start,
            Err(error) => return error,
        };
    // SAFETY: the guest's output, checked non-null above.
    unsafe { put(out, start as i64) };
    say(&format!(
        "Runtime: direct allocation offset=0x{start:x}, size={size} bytes, type={memory_type}"
    ));
    0
}

unsafe extern "C" fn direct_map(
    out: *mut u64,
    size: u64,
    prot: c_int,
    flags: c_int,
    physical: i64,
    alignment: u64,
) -> i32 {
    // SAFETY: the guest's in-out address, or null.
    let requested = unsafe { get(out) };
    let result = if out.is_null() {
        Err(INVALID)
    } else {
        memory().write(|s| s.map_direct(requested, size, prot, flags, physical, alignment))
    };
    if let Ok(address) = result {
        // SAFETY: the guest's in-out address, non-null here.
        unsafe { put(out, address) };
    }
    memory().flush_hooks();
    // SAFETY: as above.
    let mapped = unsafe { get(out) };
    if trace() {
        say(&format!(
            "Memory: map direct want={} size=0x{size:x} prot=0x{:x} flags=0x{:x} phys=0x{:x} align=0x{alignment:x} -> {} result=0x{:x}",
            pointer(requested),
            prot as u32,
            flags as u32,
            physical as u64,
            pointer(mapped),
            code(&result) as u32
        ));
    } else if result.is_ok() {
        say(&format!(
            "Runtime: mapped direct memory {}, size={size}, prot=0x{:x}",
            pointer(mapped),
            prot as u32
        ));
    }
    code(&result)
}

unsafe extern "C" fn direct_map_named(
    out: *mut u64,
    size: u64,
    prot: c_int,
    flags: c_int,
    physical: i64,
    alignment: u64,
    _name: *const c_char,
) -> i32 {
    // SAFETY: the same arguments, from the guest.
    unsafe { direct_map(out, size, prot, flags, physical, alignment) }
}

extern "C" fn direct_unmap(address: u64, size: u64) -> i32 {
    // The GPU forgets the range before it disappears (as shadPS4's early GPU unmap).
    if let Some(hook) = memory().read(Space::unmap_hook) {
        if address.is_multiple_of(PAGE) && size != 0 {
            hook(address as usize, align_up(size, PAGE));
        }
    }
    let result = memory().write(|s| {
        let before = s.pending_hooks();
        let result = s.unmap(address, size);
        s.forget_hooks_after(before); // already notified above
        result
    });
    if trace() {
        say(&format!(
            "Memory: unmap {} size=0x{size:x} result=0x{:x}",
            pointer(address),
            result as u32
        ));
    }
    result
}

extern "C" fn direct_release(start: u64, size: u64) -> i32 {
    let result = memory().write(|s| s.release_direct(start, size));
    if result != 0 || size == 0 {
        return result;
    }
    memory().flush_hooks();
    if trace() {
        say(&format!(
            "Memory: release direct phys=0x{start:x} size=0x{size:x}"
        ));
    }
    0
}

unsafe extern "C" fn map_flexible(inout: *mut u64, size: u64, prot: c_int, flags: c_int) -> i32 {
    // SAFETY: the guest's in-out address, or null.
    let want = unsafe { get(inout) };
    if trace() {
        say(&format!(
            "Memory: map flexible want={} size=0x{size:x} prot=0x{:x} flags=0x{:x}",
            pointer(want),
            prot as u32,
            flags as u32
        ));
    }
    let result = if inout.is_null() {
        Err(INVALID)
    } else {
        memory().write(|s| s.map_flexible(want, size, prot, flags))
    };
    if let Ok(address) = result {
        // SAFETY: the guest's in-out address, non-null here.
        unsafe { put(inout, address) };
    }
    memory().flush_hooks();
    match result {
        Ok(address) => say(&format!(
            "Runtime: mapped flexible memory {}, size={size}, prot=0x{:x}",
            pointer(address),
            prot as u32
        )),
        Err(error) => eprintln!(
            "Runtime: flexible map of {size} bytes failed (0x{:x}), in use={}",
            error as u32,
            memory().read(|s| s.stats().flexible_bytes)
        ),
    }
    code(&result)
}

unsafe extern "C" fn map_flexible_named(
    inout: *mut u64,
    size: u64,
    prot: c_int,
    flags: c_int,
    _name: *const c_char,
) -> i32 {
    // SAFETY: the same arguments, from the guest.
    unsafe { map_flexible(inout, size, prot, flags) }
}

extern "C" fn release_flexible(address: u64, size: u64) -> i32 {
    direct_unmap(address, size)
}

unsafe extern "C" fn reserve_range(
    inout: *mut u64,
    size: u64,
    flags: c_int,
    alignment: u64,
) -> i32 {
    if inout.is_null() {
        return INVALID;
    }
    // SAFETY: the guest's in-out address, non-null.
    let want = unsafe { get(inout) };
    match memory().write(|s| s.reserve(want, size, flags, alignment)) {
        Ok(address) => {
            // SAFETY: as above.
            unsafe { put(inout, address) };
            say(&format!(
                "Runtime: reserved virtual range {}, size={size}",
                pointer(address)
            ));
            0
        }
        Err(error) => error,
    }
}

extern "C" fn kernel_mprotect(address: u64, size: u64, prot: c_int) -> i32 {
    let result = memory().write(|s| s.protect(address, size, prot, -1));
    memory().flush_hooks();
    if trace() {
        say(&format!(
            "Memory: mprotect {} size=0x{size:x} prot=0x{:x} result=0x{:x}",
            pointer(address),
            prot as u32,
            result as u32
        ));
    }
    result
}

unsafe extern "C" fn query_protection(
    address: u64,
    start: *mut u64,
    end: *mut u64,
    prot: *mut u32,
) -> i32 {
    match memory().read(|s| s.query_protection(address)) {
        Ok((first, last, protection)) => {
            // SAFETY: the guest's outputs, each null or writable.
            unsafe {
                put(start, first);
                put(end, last);
                put(prot, protection);
            }
            0
        }
        Err(error) => error,
    }
}

unsafe extern "C" fn virtual_query(
    address: u64,
    flags: c_int,
    info: *mut VirtualQueryInfo,
    info_size: u64,
) -> i32 {
    if info.is_null() || info_size != std::mem::size_of::<VirtualQueryInfo>() as u64 {
        return INVALID;
    }
    match memory().read(|s| s.virtual_query(address, flags)) {
        Ok(result) => {
            // SAFETY: the guest's output, non-null and `info_size` bytes long.
            unsafe { put(info, result) };
            0
        }
        Err(error) => error,
    }
}

unsafe extern "C" fn direct_memory_type(
    phys: u64,
    memory_type: *mut c_int,
    start: *mut u64,
    end: *mut u64,
) -> i32 {
    match memory().read(|s| s.direct_memory_type(phys)) {
        Ok((found, first, last)) => {
            // SAFETY: the guest's outputs, each null or writable.
            unsafe {
                put(memory_type, found);
                put(start, first);
                put(end, last);
            }
            0
        }
        Err(error) => error,
    }
}

unsafe extern "C" fn batch_map2(
    entries: *mut BatchEntry,
    count: c_int,
    processed: *mut c_int,
    flags: c_int,
) -> i32 {
    if entries.is_null() || count < 0 {
        return INVALID;
    }
    // The mapped addresses are written to the guest's entries after the lock is released (a
    // write to a GPU-tracked page faults, and the fault handler must not wait behind this lock).
    let mut starts = Vec::with_capacity(count as usize);
    let result = memory().write(|s| {
        while starts.len() < count as usize {
            // SAFETY: the guest passed `count` entries; each is read as the loop reaches it.
            let entry = unsafe { entries.add(starts.len()).read_unaligned() };
            if s.trace() {
                say(&format!(
                    "Memory: batch op={} start={} length=0x{:x} offset=0x{:x} prot=0x{:x} type={} flags=0x{:x}",
                    entry.operation,
                    pointer(entry.start),
                    entry.length,
                    entry.offset,
                    entry.prot as u8,
                    entry.memory_type,
                    flags as u32
                ));
            }
            match s.batch_entry(&entry, flags) {
                Ok(address) => starts.push(address),
                Err(error) => return error,
            }
        }
        0
    });
    for (i, &address) in starts.iter().enumerate() {
        // SAFETY: entry `i` was read above; its `start` field is written back.
        unsafe { put(ptr::addr_of_mut!((*entries.add(i)).start), address) };
    }
    memory().flush_hooks();
    // SAFETY: the guest's output, null or writable.
    unsafe { put(processed, starts.len() as c_int) };
    result
}

unsafe extern "C" fn batch_map(
    entries: *mut BatchEntry,
    count: c_int,
    processed: *mut c_int,
) -> i32 {
    // SAFETY: the same arguments, from the guest.
    unsafe { batch_map2(entries, count, processed, MAP_FIXED) }
}

/// The guest function a symbol of [`SYMBOLS`] names (the C `exports` table).
fn function(symbol: &str) -> usize {
    match symbol {
        "sceKernelGetDirectMemorySize" => direct_size as *const () as usize,
        "sceKernelAllocateDirectMemory" => direct_allocate as *const () as usize,
        "sceKernelMapDirectMemory" => direct_map as *const () as usize,
        "sceKernelMapNamedDirectMemory" => direct_map_named as *const () as usize,
        "sceKernelReleaseDirectMemory" => direct_release as *const () as usize,
        "sceKernelMunmap" | "munmap" => direct_unmap as *const () as usize,
        "sceKernelMapFlexibleMemory" => map_flexible as *const () as usize,
        "sceKernelMapNamedFlexibleMemory" => map_flexible_named as *const () as usize,
        "sceKernelReleaseFlexibleMemory" => release_flexible as *const () as usize,
        "sceKernelReserveVirtualRange" => reserve_range as *const () as usize,
        "sceKernelMprotect" => kernel_mprotect as *const () as usize,
        "sceKernelQueryMemoryProtection" => query_protection as *const () as usize,
        "sceKernelVirtualQuery" => virtual_query as *const () as usize,
        "sceKernelGetDirectMemoryType" => direct_memory_type as *const () as usize,
        "sceKernelBatchMap" => batch_map as *const () as usize,
        "sceKernelBatchMap2" => batch_map2 as *const () as usize,
        _ => 0,
    }
}

/// The guest function for an import NID, or 0 (`runtime_resolve` asks each module in turn).
///
/// # Safety
/// `name` is a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn runtime_memory_resolve(name: *const c_char) -> usize {
    // SAFETY: the caller guarantees a NUL-terminated string.
    let nid = unsafe { CStr::from_ptr(name) };
    nid_symbol(nid.to_bytes()).map_or(0, function)
}

/// Guest memory for the GPU library's own data (movie frames scaled to the display size,
/// `gpu/shim/bbport_movie.cpp`): flexible memory, CPU and GPU read/write, or null.
#[no_mangle]
pub extern "C" fn runtime_map_guest_memory(size: u64) -> *mut c_void {
    let mut address = GUEST_SCRATCH;
    // SAFETY: a local in-out address.
    let result = unsafe { map_flexible(&mut address, align_up(size, PAGE), 0x33, 0) };
    if result == 0 {
        address as *mut c_void
    } else {
        ptr::null_mut()
    }
}

/// Host-owned memory the guest can see (stacks, trampolines) in `[LOW_MIN, USER_MIN)`, with an
/// unmapped page after each piece to catch overruns; null when nothing is free.
#[no_mangle]
pub extern "C" fn runtime_low_map(size: usize, prot: c_int) -> *mut c_void {
    let size = align_up(size as u64, PAGE);
    memory().write(|_| {
        let mut next = LOW_NEXT.load(Ordering::Relaxed);
        let mut mapped = libc::MAP_FAILED;
        while next.wrapping_add(size) <= USER_MIN {
            // SAFETY: MAP_FIXED_NOREPLACE never replaces an existing mapping.
            mapped = unsafe { anonymous(next, size, prot) };
            next = next.wrapping_add(size + PAGE);
            if mapped != libc::MAP_FAILED {
                break;
            }
        }
        LOW_NEXT.store(next, Ordering::Relaxed);
        if mapped == libc::MAP_FAILED {
            ptr::null_mut()
        } else {
            mapped
        }
    })
}

/// The main executable where the PS4 loads it (0x400000), else as low as possible below 2 GiB
/// (Uncharted's `TGenericOffsetPtr` tables); null when nothing is free.
#[no_mangle]
pub extern "C" fn runtime_image_map(size: usize, prot: c_int) -> *mut c_void {
    let size = align_up(size as u64, 0x1_0000);
    memory().write(|_| {
        let mut at = IMAGE_MIN;
        while at.wrapping_add(size) <= IMAGE_LIMIT {
            // SAFETY: MAP_FIXED_NOREPLACE never replaces an existing mapping.
            let mapped = unsafe { anonymous(at, size, prot) };
            if mapped != libc::MAP_FAILED {
                return mapped;
            }
            at += 0x10_0000;
        }
        ptr::null_mut()
    })
}

/// A private anonymous mapping exactly at `address`, or `MAP_FAILED` when anything is there.
///
/// # Safety
/// None beyond `mmap`'s: MAP_FIXED_NOREPLACE leaves existing mappings alone.
unsafe fn anonymous(address: u64, size: u64, prot: c_int) -> *mut c_void {
    // SAFETY: see the function.
    unsafe {
        libc::mmap(
            address as *mut c_void,
            size as usize,
            prot,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | MAP_FIXED_NOREPLACE,
            -1,
            0,
        )
    }
}

/// Windows reserves the PS4 user range here (3.15); Linux needs nothing.
#[no_mangle]
pub extern "C" fn runtime_memory_reserve() {}

/// Re-reads the `BB_TOGGLE_FILE` numbers every 250 ms and prints each change.
fn watch_toggles(path: std::ffi::OsString) {
    let (mut last, mut last2) = (u64::MAX, u64::MAX);
    loop {
        let text = std::fs::read(&path).unwrap_or_default();
        let (value, value2) = parse_toggles(&String::from_utf8_lossy(&text));
        if (value, value2) != (last, last2) {
            DISABLED_OPTIMIZATIONS.store(value, Ordering::Release);
            DISABLED_OPTIMIZATIONS2.store(value2, Ordering::Release);
            say(&format!(
                "Runtime: disabled optimizations mask={value} {value2}"
            ));
            (last, last2) = (value, value2);
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// The GPU library's notifications (mapped, about to be unmapped, protection changed); every
/// mapping the guest already has is announced. Also starts the toggle watcher
/// (`BB_TOGGLE_FILE`) and takes `BB_TOGGLE2`, the second toggle number's starting value.
#[no_mangle]
pub extern "C" fn runtime_memory_set_gpu_hooks(
    map: Option<GpuRange>,
    unmap: Option<GpuRange>,
    invalidate: Option<GpuRange>,
) {
    if let Some(start2) = env("BB_TOGGLE2").filter(|value| !value.is_empty()) {
        DISABLED_OPTIMIZATIONS2.store(strtoull(&start2, 0), Ordering::Release);
    }
    if let Some(path) = std::env::var_os("BB_TOGGLE_FILE") {
        if !WATCHER.swap(true, Ordering::AcqRel) {
            let _ = std::thread::Builder::new()
                .name("bb-toggles".into())
                .spawn(move || watch_toggles(path));
        }
    }
    memory().write(|s| s.set_hooks(map, unmap, invalidate));
    memory().flush_hooks();
}

/// Writes through the backing view (past guest and GPU page protection); 0 when part of the
/// range has no backing (the part before it is written).
///
/// # Safety
/// `data` is readable for `size` bytes.
#[no_mangle]
pub unsafe extern "C" fn runtime_memory_write_backing(
    address: usize,
    data: *const c_void,
    size: u64,
) -> c_int {
    let data = data.cast::<u8>();
    memory().read(|s| {
        let written = s.for_each_backing(address as u64, size, |phys, offset, n| {
            // SAFETY: the source is the caller's `size` bytes; the target is backing of a
            // mapped region (a mapped region implies the pool exists), inside the memfd view.
            unsafe {
                ptr::copy_nonoverlapping(
                    data.add(offset as usize),
                    s.host().backing(phys) as *mut u8,
                    n as usize,
                )
            };
        });
        c_int::from(written)
    })
}

/// Mapped length from `address`, up to `size`, across adjacent mappings.
#[no_mangle]
pub extern "C" fn runtime_memory_clamp(address: usize, size: u64) -> u64 {
    memory().clamp(address as u64, size)
}

/// The region around `address`: a mapping (`mapped` 1) or the gap before the next one; 0 past
/// the last mapping (the outputs are then left alone).
///
/// # Safety
/// `start`, `end` and `mapped` are writable.
#[no_mangle]
pub unsafe extern "C" fn runtime_memory_region(
    address: usize,
    start: *mut usize,
    end: *mut usize,
    mapped: *mut c_int,
) -> c_int {
    match memory().region(address as u64) {
        Some((first, last, is_mapped)) => {
            // SAFETY: the caller guarantees the outputs are writable.
            unsafe {
                start.write(first as usize);
                end.write(last as usize);
                mapped.write(c_int::from(is_mapped));
            }
            1
        }
        None => 0,
    }
}

/// GPU page tracking: restricts host access to `[address, address + size)`, never beyond the
/// guest's own protection.
#[no_mangle]
pub extern "C" fn runtime_memory_gpu_protect(address: usize, size: u64, read: c_int, write: c_int) {
    memory().read(|s| s.gpu_protect(address as u64, size, read != 0, write != 0));
}

/// 1 when `[address, address + size)` is all mapped guest memory.
#[no_mangle]
pub extern "C" fn runtime_memory_is_mapped(address: usize, size: u64) -> c_int {
    c_int::from(memory().is_mapped(address as u64, size))
}

/// The two counter lines at exit.
#[no_mangle]
pub extern "C" fn runtime_memory_report() {
    for line in memory().read(Space::report) {
        say(&line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// The tests share the process's address space: one at a time, each releasing what it took.
    fn serial() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn resolve(nid: &CStr) -> usize {
        // SAFETY: a NUL-terminated literal.
        let address = unsafe { runtime_memory_resolve(nid.as_ptr()) };
        assert_ne!(address, 0, "{nid:?}");
        address
    }

    type Allocate = unsafe extern "C" fn(i64, i64, u64, u64, c_int, *mut i64) -> i32;
    type Map = unsafe extern "C" fn(*mut u64, u64, c_int, c_int, i64, u64) -> i32;
    type Unmap = extern "C" fn(u64, u64) -> i32;
    type Release = extern "C" fn(u64, u64) -> i32;

    /// `test_runtime.c` `direct_memory`, through the resolver as the guest calls it: the
    /// mappings share real backing, and released pages read as zero when used again.
    #[test]
    fn direct_memory_through_the_resolver_as_the_c_test() {
        let _serial = serial();
        // SAFETY: the resolver returns these functions for these NIDs (the NID test checks the
        // names); the signatures are the ones above.
        let (alloc, map, unmap, release) = unsafe {
            (
                std::mem::transmute::<usize, Allocate>(resolve(c"rTXw65xmLIA#p#J")),
                std::mem::transmute::<usize, Map>(resolve(c"L-Q3LEjIbgA#p#J")),
                std::mem::transmute::<usize, Unmap>(resolve(c"cQke9UuBQOk#p#J")),
                std::mem::transmute::<usize, Release>(resolve(c"MBuItvba6z8#p#J")),
            )
        };
        let (mut a, mut b, mut c) = (-1i64, -1i64, -1i64);
        let (pool, length) = (536_870_912i64, 16384u64);
        // SAFETY: every pointer below is a local or a mapping this test made.
        let last = unsafe {
            assert_eq!(alloc(0, pool, 1, 0, 0, &mut a), INVALID);
            assert_eq!(a, -1);
            assert_eq!(alloc(0, 8192, length, 0, 0, &mut a), NO_SPACE);
            assert_eq!((alloc(0, pool, length, 2 << 20, 0, &mut a), a), (0, 0));
            assert_eq!(
                (alloc(0, pool, length, 2 << 20, 0, &mut b), b),
                (0, 2 << 20)
            );
            assert_eq!((alloc(0, pool, length, 16384, 0, &mut c), c), (0, 16384));
            let (mut x, mut y, mut invalid) = (0u64, 0u64, 0u64);
            assert_eq!(map(&mut invalid, length * 2, 3, 0, b, 0), INVALID);
            assert_eq!(map(&mut x, length, 3, 0, a, 2 << 20), 0);
            assert_eq!(x % (2 << 20), 0);
            assert_eq!(map(&mut y, length, 3, 0, a, 0), 0);
            assert_eq!((x as *const u64).read_volatile(), 0);
            (x as *mut u64).write_volatile(0xabcd_ef01_2345_6789);
            assert_eq!((y as *const u64).read_volatile(), 0xabcd_ef01_2345_6789);
            assert_eq!(unmap(x + 4096, length), INVALID);
            assert_eq!((unmap(x, length), unmap(y, length)), (0, 0));
            for start in [a, b, c] {
                assert_eq!(release(start as u64, length), 0);
            }
            assert_eq!((alloc(0, pool, length, 0, 0, &mut a), a), (0, 0));
            x = 0;
            assert_eq!(map(&mut x, length, 3, 0, a, 0), 0);
            assert_eq!(
                (x as *const u64).read_volatile(),
                0,
                "released data does not leak"
            );
            assert_eq!(
                release(a as u64, length),
                0,
                "release also unmaps the mapping"
            );
            x
        };
        assert!(!memory().read(|s| s.is_mapped(last, 1)));
    }

    /// #121: the second direct block, mapped in 1 MiB MAP_FIXED steps up to 0x11bec00000, is
    /// backed to its last byte, and the host's backing writes stop at its end.
    #[test]
    fn write_backing_at_the_second_block_edge_of_issue_121() {
        let _serial = serial();
        let end = 0x11_bec0_0000u64;
        let (mut first, mut second) = (0i64, 0i64);
        let base = end - 64 * (1 << 20);
        // SAFETY: locals, and mappings this test made and releases.
        unsafe {
            assert_eq!(
                direct_allocate(0, RETAIL_POOL as i64, 256 << 20, 0, 0, &mut first),
                0
            );
            assert_eq!(
                direct_allocate(0, RETAIL_POOL as i64, 64 << 20, 0, 0, &mut second),
                0
            );
            for step in 0..64u64 {
                let mut at = base + step * (1 << 20);
                let phys = second + (step << 20) as i64;
                assert_eq!(direct_map(&mut at, 1 << 20, 0x33, MAP_FIXED, phys, 0), 0);
            }
            let tail = [0x5au8; 8];
            assert_eq!(
                runtime_memory_write_backing((end - 8) as usize, tail.as_ptr().cast(), 8),
                1
            );
            assert_eq!(
                ((end - 8) as *const u64).read_volatile(),
                0x5a5a_5a5a_5a5a_5a5a
            );
            assert_eq!(
                runtime_memory_write_backing((end - 4) as usize, tail.as_ptr().cast(), 8),
                0
            );
            assert_eq!(
                ((end - 8) as *const u64).read_volatile(),
                0x5a5a_5a5a_5a5a_5a5a
            );
            assert_eq!(runtime_memory_clamp((end - 8) as usize, 64), 8);
            assert_eq!(runtime_memory_is_mapped((end - 1) as usize, 1), 1);
            assert_eq!(runtime_memory_is_mapped(end as usize, 1), 0);
            assert_eq!(runtime_memory_is_mapped(0x11_cff8_3f88, 8), 0);
            let (mut start, mut stop, mut mapped) = (0usize, 0usize, 0);
            assert_eq!(
                runtime_memory_region((end - 1) as usize, &mut start, &mut stop, &mut mapped),
                1
            );
            assert_eq!((stop as u64, mapped), (end, 1));
            // Protection blocks guest writes; the backing view does not care.
            assert_eq!(kernel_mprotect(end - (1 << 20), 1 << 20, 1), 0);
            assert_eq!(
                runtime_memory_write_backing((end - 8) as usize, [0u8; 8].as_ptr().cast(), 8),
                1
            );
            assert_eq!(((end - 8) as *const u64).read_volatile(), 0);
            assert_eq!(direct_release(first as u64, 256 << 20), 0);
            assert_eq!(direct_release(second as u64, 64 << 20), 0);
        }
        assert!(!memory().read(|s| s.is_mapped(base, 1)));
    }

    #[test]
    fn flexible_queries_and_batches_through_the_exports() {
        let _serial = serial();
        // SAFETY: locals, and mappings this test made and releases.
        unsafe {
            let mut address = 0u64;
            assert_eq!(map_flexible(&mut address, 4 * PAGE, 3, 0), 0);
            let mut info = std::mem::MaybeUninit::<VirtualQueryInfo>::zeroed();
            assert_eq!(virtual_query(address, 0, info.as_mut_ptr(), 71), INVALID);
            assert_eq!(virtual_query(address, 0, info.as_mut_ptr(), 72), 0);
            let info = info.assume_init();
            assert_eq!(
                (info.start, info.end, info.flags),
                (address, address + 4 * PAGE, 17)
            );
            let (mut start, mut end, mut prot) = (0u64, 0u64, 0u32);
            assert_eq!(kernel_mprotect(address + PAGE, PAGE, 1), 0);
            assert_eq!(
                query_protection(address + PAGE, &mut start, &mut end, &mut prot),
                0
            );
            assert_eq!((start, end, prot), (address + PAGE, address + 2 * PAGE, 1));
            assert_eq!(
                query_protection(address, ptr::null_mut(), ptr::null_mut(), ptr::null_mut()),
                0
            );
            let mut entries = [
                BatchEntry {
                    start: address,
                    offset: 0,
                    length: PAGE,
                    prot: 3,
                    memory_type: 0,
                    reserved: 0,
                    operation: 2,
                },
                BatchEntry {
                    start: 0,
                    offset: 0,
                    length: PAGE,
                    prot: 3,
                    memory_type: 0,
                    reserved: 0,
                    operation: 3,
                },
                BatchEntry {
                    start: 0,
                    offset: 0,
                    length: PAGE,
                    prot: 0,
                    memory_type: 0,
                    reserved: 0,
                    operation: 9,
                },
            ];
            let mut processed = -1;
            assert_eq!(
                batch_map2(entries.as_mut_ptr(), 3, &mut processed, 0),
                INVALID
            );
            assert_eq!(processed, 2);
            let flexible = entries[1].start;
            assert_ne!(flexible, 0, "the mapped address is written back");
            assert_eq!(release_flexible(flexible, PAGE), 0);
            assert_eq!(release_flexible(address, 4 * PAGE), 0);
            assert_eq!(batch_map(ptr::null_mut(), 1, &mut processed), INVALID);
        }
    }

    #[test]
    fn low_and_image_maps_stay_in_their_windows() {
        let _serial = serial();
        let low = runtime_low_map(100, libc::PROT_READ | libc::PROT_WRITE) as u64;
        assert!((LOW_MIN..USER_MIN).contains(&low), "{low:#x}");
        let next = runtime_low_map(PAGE as usize, libc::PROT_READ) as u64;
        assert!(next >= low + 2 * PAGE, "one unmapped page between pieces");
        let image = runtime_image_map(0x1234, libc::PROT_READ) as u64;
        assert!((IMAGE_MIN..IMAGE_LIMIT).contains(&image), "{image:#x}");
        assert_eq!(image % 0x10_0000, 0x40_0000 % 0x10_0000);
        // SAFETY: the three mappings above, made by this test.
        unsafe {
            libc::munmap(low as *mut c_void, PAGE as usize);
            libc::munmap(next as *mut c_void, PAGE as usize);
            libc::munmap(image as *mut c_void, 0x1_0000);
        }
        assert_eq!(direct_size(), pool());
        // SAFETY: an unknown NID literal.
        assert_eq!(
            unsafe { runtime_memory_resolve(c"n88vx3C5nW8#q#q".as_ptr()) },
            0
        );
    }
}
