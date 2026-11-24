//! The Apple host of [`super`] (macOS and iOS): guest memory by `mmap`, never `MAP_JIT`.
//!
//! The direct pool and the flexible span are one unlinked file, so any mapping of any
//! physical range aliases the same storage (the role of Linux's memfd) and, being file
//! backed, clean pages can leave memory: an iOS app's memory limit is far under the retail
//! pool. A second view of the whole file lets the host write past guest and GPU page
//! protection. iOS forbids writable-and-executable memory and offers no JIT, so execute
//! permission is dropped there (guest code runs in an interpreter); macOS passes it through.
//!
//! [`reserve`], [`commit`], [`protect`] and [`release`] are the primitives over an anonymous
//! range; [`AppleHost`] is the [`Host`] the region table runs on.

use super::*;
use std::ffi::{c_int, c_void, CString};
use std::io;
use std::path::Path;
use std::ptr;

/// The host's page (16 KiB on Apple silicon, 4 KiB on an Intel Mac).
pub fn host_page() -> u64 {
    // SAFETY: `sysconf` with a valid name.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page > 0 {
        page as u64
    } else {
        4096
    }
}

/// Guest-facing protection bits to `PROT_*`. Execute is dropped on iOS (no RWX, no JIT).
pub fn prot_bits(prot: i32) -> c_int {
    let mut bits = libc::PROT_NONE;
    if prot & HOST_READ != 0 {
        bits |= libc::PROT_READ;
    }
    if prot & HOST_WRITE != 0 {
        bits |= libc::PROT_WRITE;
    }
    if prot & HOST_EXEC != 0 && cfg!(target_os = "macos") {
        bits |= libc::PROT_EXEC;
    }
    bits
}

fn check(ptr: *mut c_void) -> io::Result<usize> {
    if ptr == libc::MAP_FAILED {
        Err(io::Error::last_os_error())
    } else {
        Ok(ptr as usize)
    }
}

/// Reserves `size` bytes of inaccessible address space. With `at` it maps exactly there,
/// replacing whatever was mapped (`MAP_FIXED`, for the layouts that demand an address); with
/// `None` the kernel picks the base.
pub fn reserve(at: Option<usize>, size: usize) -> io::Result<usize> {
    let mut flags = libc::MAP_PRIVATE | libc::MAP_ANON;
    if at.is_some() {
        flags |= libc::MAP_FIXED;
    }
    // SAFETY: an anonymous PROT_NONE mapping; a fixed address is the caller's own range.
    check(unsafe {
        libc::mmap(
            at.unwrap_or(0) as *mut c_void,
            size,
            libc::PROT_NONE,
            flags,
            -1,
            0,
        )
    })
}

/// Makes a reserved range usable with `prot` (`HOST_*` bits). Pages are zero on first use.
///
/// # Safety
/// `[base, base + size)` must lie inside a mapping of the caller's.
pub unsafe fn commit(base: usize, size: usize, prot: i32) -> io::Result<()> {
    // SAFETY: the caller's range.
    unsafe { protect(base, size, prot) }
}

/// Changes the access of a mapped range.
///
/// # Safety
/// `[base, base + size)` must lie inside a mapping of the caller's.
pub unsafe fn protect(base: usize, size: usize, prot: i32) -> io::Result<()> {
    // SAFETY: the caller's range.
    if unsafe { libc::mprotect(base as *mut c_void, size, prot_bits(prot)) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Returns a range to the system.
///
/// # Safety
/// `[base, base + size)` must be the caller's, with nothing still using it.
pub unsafe fn release(base: usize, size: usize) -> io::Result<()> {
    // SAFETY: the caller's range.
    if unsafe { libc::munmap(base as *mut c_void, size) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Maps `[offset, offset + size)` of `fd` shared, at `at` (replacing what is there) or at a
/// base the kernel picks.
pub fn map_file(
    fd: c_int,
    offset: u64,
    size: usize,
    prot: i32,
    at: Option<usize>,
) -> io::Result<usize> {
    let mut flags = libc::MAP_SHARED;
    if at.is_some() {
        flags |= libc::MAP_FIXED;
    }
    // SAFETY: a shared file mapping; a fixed address is the caller's own range.
    check(unsafe {
        libc::mmap(
            at.unwrap_or(0) as *mut c_void,
            size,
            prot_bits(prot),
            flags,
            fd,
            offset as libc::off_t,
        )
    })
}

/// The direct pool and the flexible span as one unlinked file, plus a second view of all of
/// it: host writes through the view bypass guest and GPU page protection.
pub struct AppleHost {
    dir: std::path::PathBuf,
    fd: c_int,
    backing: usize,
    bytes: u64,
    /// The block [`AppleHost::claim`] reserved: `(base, size)`.
    claimed: Option<(usize, usize)>,
}

impl AppleHost {
    /// A host whose backing file is made in the temporary directory.
    pub fn new() -> Self {
        Self::in_dir(std::env::temp_dir())
    }

    /// A host whose backing file is made in `dir` (an iOS app passes its caches directory).
    pub fn in_dir(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            fd: -1,
            backing: 0,
            bytes: 0,
            claimed: None,
        }
    }

    /// Reserves, as inaccessible memory, a block for the windows of `template` (a layout at
    /// base 0, such as [`Layout::IOS`]) at an address the kernel picks, and returns the
    /// template rebased onto it. Nothing already mapped (the dyld shared cache, the
    /// executable, the heap) can be inside the block, so the host's `MAP_FIXED` maps within
    /// it replace only the host's own reservation. Once claimed, `map` refuses addresses
    /// outside the block. The block is released when the host drops, and a second claim
    /// returns the first one's layout only when the template is the same.
    pub fn claim(&mut self, template: Layout) -> Option<Layout> {
        if !template.is_valid() {
            return None;
        }
        let page = PAGE as usize;
        let size = template.span() as usize;
        if let Some((base, held)) = self.claimed {
            let layout = template.rebased(base.next_multiple_of(page) as u64);
            return (held >= size + page).then_some(layout);
        }
        // Over-reserve by a guest page so the base can be aligned up to it (the host page
        // may be smaller than the guest's).
        let block = reserve(None, size + page).ok()?;
        self.claimed = Some((block, size + page));
        Some(template.rebased(block.next_multiple_of(page) as u64))
    }

    /// The host address of backing byte `phys` (0 before the pool exists).
    pub fn backing(&self, phys: u64) -> usize {
        if self.backing == 0 {
            0
        } else {
            self.backing + phys as usize
        }
    }

    /// The backing file's descriptor (-1 before the pool exists).
    pub fn fd(&self) -> c_int {
        self.fd
    }
}

impl Default for AppleHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AppleHost {
    fn drop(&mut self) {
        if self.backing != 0 {
            // SAFETY: the view made in `pool`, unused once the host is gone.
            unsafe { libc::munmap(self.backing as *mut c_void, self.bytes as usize) };
        }
        if self.fd >= 0 {
            // SAFETY: the descriptor made in `pool`.
            unsafe { libc::close(self.fd) };
        }
        if let Some((base, size)) = self.claimed {
            // SAFETY: the block made in `claim`, unused once the host is gone.
            unsafe { libc::munmap(base as *mut c_void, size) };
        }
    }
}

impl Host for AppleHost {
    fn pool(&mut self, bytes: u64) -> bool {
        if self.fd >= 0 {
            return true;
        }
        let Ok(template) = CString::new(
            self.dir
                .join("uu-guest-memory-XXXXXX")
                .to_string_lossy()
                .into_owned(),
        ) else {
            return false;
        };
        let mut name = template.into_bytes_with_nul();
        // SAFETY: `name` is a writable NUL-terminated template; the file is unlinked at once,
        // so only the descriptor keeps it.
        let fd = unsafe { libc::mkstemp(name.as_mut_ptr().cast()) };
        if fd < 0 {
            return false;
        }
        // SAFETY: the path of the file just made.
        unsafe { libc::unlink(name.as_ptr().cast()) };
        // SAFETY: `fd` is ours; close-on-exec keeps it from children.
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
        // SAFETY: sizing the file just made (sparse).
        if unsafe { libc::ftruncate(fd, bytes as libc::off_t) } != 0 {
            // SAFETY: the descriptor just made.
            unsafe { libc::close(fd) };
            return false;
        }
        match map_file(fd, 0, bytes as usize, HOST_READ | HOST_WRITE, None) {
            Ok(view) => {
                self.fd = fd;
                self.backing = view;
                self.bytes = bytes;
                true
            }
            Err(_) => {
                // SAFETY: the descriptor just made.
                unsafe { libc::close(fd) };
                false
            }
        }
    }

    fn map(&self, address: u64, size: u64, prot: i32, reserved: bool, phys: u64) -> bool {
        // The guest chose this range of its own address space; replacing what is mapped
        // there is the contract, as on Linux.
        let (at, size) = (Some(address as usize), size as usize);
        if let Some((base, held)) = self.claimed {
            // Only the block this host reserved may be replaced.
            if (address as usize) < base || address as usize + size > base + held {
                return false;
            }
        }
        if reserved {
            reserve(at, size).is_ok()
        } else {
            map_file(self.fd, phys, size, prot, at).is_ok()
        }
    }

    fn unmap(&self, address: u64, size: u64) -> bool {
        // SAFETY: a range the region table holds as guest memory.
        unsafe { release(address as usize, size as usize) }.is_ok()
    }

    fn protect(&self, address: u64, size: u64, prot: i32) -> bool {
        // SAFETY: a range the region table holds as guest memory; only its access changes.
        unsafe { protect(address as usize, size as usize, prot) }.is_ok()
    }

    fn discard(&self, phys: u64, size: u64) {
        if self.fd < 0 {
            return;
        }
        let hole = libc::fpunchhole_t {
            fp_flags: 0,
            reserved: 0,
            fp_offset: phys as libc::off_t,
            fp_length: size as libc::off_t,
        };
        // SAFETY: frees blocks of the backing file (APFS); the descriptor and struct are ours.
        if unsafe { libc::fcntl(self.fd, libc::F_PUNCHHOLE, &hole) } == 0 {
            return;
        }
        // No hole punching here: zero the range through the view, so it reads as zero.
        if self.backing != 0 {
            // SAFETY: `[phys, phys + size)` lies in the backing view (checked against the blocks).
            unsafe {
                ptr::write_bytes((self.backing + phys as usize) as *mut u8, 0, size as usize)
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: usize = 1 << 20;

    #[test]
    fn reserve_commit_protect_release_at_a_chosen_base() {
        let size = 4 * MIB;
        let base = reserve(None, size).unwrap();
        assert_eq!(base as u64 % host_page(), 0);
        // SAFETY: the range was just reserved by this test.
        unsafe {
            commit(base, size, HOST_READ | HOST_WRITE).unwrap();
            let p = base as *mut u8;
            assert_eq!(*p.add(size - 1), 0);
            *p = 7;
            *p.add(size - 1) = 9;
            protect(base, size, HOST_READ).unwrap();
            assert_eq!((*p, *p.add(size - 1)), (7, 9));
            protect(base, size, HOST_READ | HOST_WRITE).unwrap();
            *p = 8;
            assert_eq!(*p, 8);
            release(base, size).unwrap();
        }
        // The base can be taken again, fixed, now that it is free.
        let again = reserve(Some(base), size).unwrap();
        assert_eq!(again, base);
        // SAFETY: reserved above.
        unsafe { release(again, size).unwrap() };
    }

    #[test]
    fn file_backed_views_alias_and_discard_reads_zero() {
        let mut host = AppleHost::new();
        let total = (8 * MIB) as u64;
        assert!(host.pool(total));
        assert!(host.pool(total), "a second call keeps the pool");
        let view_a = reserve(None, 2 * MIB).unwrap() as u64;
        let view_b = reserve(None, 2 * MIB).unwrap() as u64;
        let phys = 4 * MIB as u64;
        let rw = HOST_READ | HOST_WRITE;
        assert!(host.map(view_a, 2 * MIB as u64, rw, false, phys));
        assert!(host.map(view_b, 2 * MIB as u64, rw, false, phys));
        // SAFETY: three live mappings of the same file range made above.
        unsafe {
            *(view_a as *mut u64) = 0x1122_3344_5566_7788;
            assert_eq!(*(view_b as *const u64), 0x1122_3344_5566_7788);
            assert_eq!(*(host.backing(phys) as *const u64), 0x1122_3344_5566_7788);
            *(host.backing(phys + 8) as *mut u64) = 5;
            assert_eq!(*((view_a + 8) as *const u64), 5);
        }
        assert!(host.protect(view_a, 2 * MIB as u64, HOST_READ));
        host.discard(phys, 2 * MIB as u64);
        // SAFETY: as above.
        unsafe {
            assert_eq!(*(view_a as *const u64), 0);
            assert_eq!(*(view_b as *const u64), 0);
        }
        assert!(host.unmap(view_a, 2 * MIB as u64));
        assert!(host.unmap(view_b, 2 * MIB as u64));
    }

    #[test]
    fn host_map_reserved_then_unmap() {
        let mut host = AppleHost::new();
        assert!(host.pool(MIB as u64));
        let at = reserve(None, MIB).unwrap();
        assert!(host.map(at as u64, MIB as u64, 0, true, 0));
        assert!(host.unmap(at as u64, MIB as u64));
    }

    #[test]
    fn ios_never_executes_and_macos_may() {
        assert_eq!(
            prot_bits(HOST_READ | HOST_WRITE),
            libc::PROT_READ | libc::PROT_WRITE
        );
        let exec = prot_bits(HOST_READ | HOST_EXEC) & libc::PROT_EXEC != 0;
        assert_eq!(exec, cfg!(target_os = "macos"));
    }

    #[test]
    fn layouts_stay_within_a_budget() {
        for layout in [
            Layout::DEFAULT,
            Layout::IOS,
            Layout::IOS.rebased(0x7_0000_0000),
        ] {
            assert!(layout.is_valid());
        }
        const { assert!(Layout::IOS.footprint() <= IOS_BUDGET) };
        const { assert!(Layout::IOS.pool_size < RETAIL_POOL) };
        assert_eq!(Layout::for_budget(IOS_BUDGET), Some(Layout::IOS));
        assert_eq!(Layout::for_budget(1 << 40), Some(Layout::DEFAULT));
        assert_eq!(Layout::for_budget(1 << 30), None);
        let ios = Layout::IOS;
        assert_eq!(Layout::for_budget(ios.footprint()), Some(ios));
        assert_eq!(Layout::for_budget(ios.footprint() - 1), None);
        assert_eq!(ios.rebased(0x1000_0000).footprint(), ios.footprint());
        assert!(ios.image_limit <= ios.low_min && ios.low_min < ios.user_min);
        assert_eq!(Layout::DEFAULT.pool_size, RETAIL_POOL);
    }

    extern "C" {
        fn _dyld_get_image_header(index: u32) -> *const c_void;
    }

    /// `SHARED_REGION_BASE_ARM64` and `_SIZE_ARM64` (mach/shared_region.h).
    const SHARED_REGION: std::ops::Range<u64> = 0x1_8000_0000..0x2_8000_0000;

    #[test]
    fn a_claimed_ios_layout_avoids_the_shared_cache_and_the_running_image() {
        let mut host = AppleHost::new();
        let layout = host.claim(Layout::IOS).unwrap();
        assert!(layout.is_valid());
        assert_eq!(layout.footprint(), Layout::IOS.footprint());
        let windows = layout.image_min..layout.user_max;
        assert_eq!(layout.image_min % PAGE, 0);
        assert!(windows.start >= SHARED_REGION.end || windows.end <= SHARED_REGION.start);
        // SAFETY: image 0 is the main executable; its header address is read, not followed.
        let exe = unsafe { _dyld_get_image_header(0) } as u64;
        assert!(!windows.contains(&exe));
        // The block is the host's: a fixed map inside it works, one outside is refused.
        assert!(host.map(layout.user_min, 1 << 20, 0, true, 0));
        assert!(!host.map(layout.user_max, 1 << 20, 0, true, 0));
        assert!(!host.map(layout.image_min - PAGE, PAGE, 0, true, 0));
    }

    #[test]
    fn a_space_on_an_ios_layout_places_inside_its_user_range() {
        let mut host = AppleHost::new();
        let layout = host.claim(Layout::IOS).unwrap();
        let mut space = Space::with_layout(layout, host);
        assert_eq!(space.pool_size(), layout.pool_size);
        let at = space.reserve(0, 64 << 20, 0, 0).unwrap();
        assert!(at >= layout.user_min && at + (64 << 20) <= layout.user_max);
        // Past the end of the user range is refused.
        assert!(space
            .reserve(0, layout.user_max - layout.user_min, 0, 0)
            .is_err());
    }

    #[test]
    fn flexible_memory_round_trips_through_a_real_host() {
        let mut host = AppleHost::new();
        let layout = Layout {
            pool_size: 64 << 20,
            flex_span: 32 << 20,
            flexible_size: 16 << 20,
            ..Layout::IOS
        };
        let layout = host.claim(layout).unwrap();
        let mut space = Space::with_layout(layout, host);
        let at = space.map_flexible(0, 1 << 20, 3, 0).unwrap();
        assert!(at >= layout.user_min);
        // The guest range is a real mapping of the file: it is writable and zero.
        // SAFETY: just mapped read-write by the space.
        unsafe {
            assert_eq!(*(at as *const u8), 0);
            *(at as *mut u8) = 1;
        }
        assert_eq!(space.map_flexible(0, 32 << 20, 3, 0), Err(NO_MEMORY));
    }
}
