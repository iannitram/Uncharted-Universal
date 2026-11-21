//! The Linux host of [`super`]: one sparse memfd behind the direct pool and the flexible span,
//! and the C symbols `src/runtime_memory.c` exported (it is no longer built on Linux).
//!
//! The guest functions (`sceKernelMapDirectMemory` and the rest) are not linker symbols, as the
//! C ones were `static`: [`runtime_memory_resolve`] hands out their addresses by NID. The
//! `runtime_memory_*`, `runtime_*_map` and `runtime_disabled_optimizations*` symbols are the
//! loader's and the GPU library's (`gpu/shim/bbgpu.cpp`). `runtime_fault_recover` is a C
//! thread-local, which Rust cannot export: it is in `src/runtime_fault.c`.

use super::*;
use std::ffi::{c_int, c_void};
use std::ptr;

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

static LOW_NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(LOW_MIN);
const MAP_FIXED_NOREPLACE: c_int = 0x10_0000;

pub fn low_map(size: u64, prot: c_int) -> *mut c_void {
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
}

pub fn image_map(size: u64, prot: c_int) -> *mut c_void {
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
}

unsafe fn anonymous(address: u64, size: u64, prot: c_int) -> *mut c_void {
    // SAFETY: MAP_FIXED_NOREPLACE leaves existing mappings alone.
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
