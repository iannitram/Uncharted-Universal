//! Boot image mapping, import trampoline generation, relocation application, and segment
//! protection for [`super`] (porting `allocate`, `protect`, and the mapping/relocation loop
//! from `src/probe.c` into Rust for roadmap 3.17).

use super::{is_mapped, BootImage, ProbeFormat, Segment};
use crate::abi::runtime_resolve;
use std::ffi::CString;

/// One host-mapped guest image plus its unresolved import trap pages.
pub struct MappedImage {
    base: *mut u8,
    mapped_size: usize,
    traps: *mut u8,
    traps_size: usize,
    data_traps: *mut u8,
    data_traps_size: usize,
    page_size: usize,
}

impl MappedImage {
    /// Returns the guest image base address.
    pub fn base(&self) -> u64 {
        self.base as usize as u64
    }

    /// Returns the page-rounded mapped size of the image.
    pub fn mapped_size(&self) -> usize {
        self.mapped_size
    }

    /// Allocates and populates a guest image from `boot` and `payload` (`bytes[boot.image_offset..]`),
    /// emits unresolved-import trampolines pointing to `unresolved_handler`, and applies all
    /// relocations.
    ///
    /// Segment protections remain RW until [`Self::protect_segments`] is called (after binary and
    /// runtime code patches).
    pub fn load(
        boot: &BootImage,
        payload: &[u8],
        page_size: usize,
        unresolved_handler: usize,
    ) -> Result<Self, String> {
        Self::load_with_options(boot, payload, page_size, unresolved_handler, false)
    }

    /// Same as [`Self::load`], honoring `strict_imports` when resolving linked-module bindings.
    pub fn load_with_options(
        boot: &BootImage,
        payload: &[u8],
        page_size: usize,
        unresolved_handler: usize,
        strict_imports: bool,
    ) -> Result<Self, String> {
        let image_size =
            usize::try_from(boot.size).map_err(|_| "image size overflow".to_string())?;
        if payload.len() != image_size {
            return Err("incorrect memory image size".into());
        }
        if page_size == 0 || !page_size.is_power_of_two() {
            return Err("invalid host page size".into());
        }

        let mapped_size = round_page(image_size, page_size);
        let base = allocate_rw(mapped_size)?;

        let traps_bytes = (boot.import_names.len() + 1)
            .checked_mul(32)
            .ok_or_else(|| "trap table overflow".to_string())?;
        let traps_size = round_page(traps_bytes, page_size);
        let traps = match allocate_rwx(traps_size) {
            Ok(p) => p,
            Err(e) => {
                // SAFETY: `base` was allocated by `allocate_rw` with `mapped_size`.
                unsafe { free_mem(base, mapped_size) };
                return Err(e);
            }
        };

        let data_traps_size = (boot.import_names.len() + 1)
            .checked_mul(page_size)
            .ok_or_else(|| "data trap table overflow".to_string())?;
        let data_traps = match allocate_rw(data_traps_size) {
            Ok(p) => p,
            Err(e) => {
                // SAFETY: `base` and `traps` were allocated above.
                unsafe {
                    free_mem(traps, traps_size);
                    free_mem(base, mapped_size);
                }
                return Err(e);
            }
        };
        // SAFETY: `data_traps` covers `data_traps_size` bytes; mark no-access so touching an
        // unresolved data import faults immediately.
        unsafe {
            protect_mem(data_traps, data_traps_size, 0)?;
            std::ptr::copy_nonoverlapping(payload.as_ptr(), base, image_size);
        }

        let mapped = Self {
            base,
            mapped_size,
            traps,
            traps_size,
            data_traps,
            data_traps_size,
            page_size,
        };

        mapped.emit_traps(boot.import_names.len(), unresolved_handler);
        mapped.apply_relocs(boot, strict_imports)?;
        Ok(mapped)
    }

    fn emit_traps(&self, import_count: usize, unresolved_handler: usize) {
        let handler_bytes = (unresolved_handler as u64).to_le_bytes();
        for i in 0..import_count {
            // SysV: mov rsi, rdi; mov edi, index; movabs rax, handler; jmp rax.
            let mut stub = [0u8; 20];
            stub[0..3].copy_from_slice(&[0x48, 0x89, 0xfe]);
            stub[3] = 0xbf;
            stub[4..8].copy_from_slice(&(i as u32).to_le_bytes());
            stub[8..10].copy_from_slice(&[0x48, 0xb8]);
            stub[10..18].copy_from_slice(&handler_bytes);
            stub[18..20].copy_from_slice(&[0xff, 0xe0]);
            // SAFETY: `self.traps` has `(import_count + 1) * 32` writable+executable bytes.
            unsafe {
                std::ptr::copy_nonoverlapping(stub.as_ptr(), self.traps.add(i * 32), stub.len());
            }
        }
    }

    fn apply_relocs(&self, boot: &BootImage, strict_imports: bool) -> Result<(), String> {
        let native_libc = matches!(
            boot.format,
            ProbeFormat::V3 | ProbeFormat::V4 | ProbeFormat::V5
        ) && !strict_imports
            && (boot.capabilities & 1) != 0;
        let image_base = self.base();
        let traps_base = self.traps as usize as u64;
        let data_traps_base = self.data_traps as usize as u64;

        let c_names: Vec<CString> = boot
            .import_names
            .iter()
            .map(|s| CString::new(s.as_str()).map_err(|_| "import name contains NUL".to_string()))
            .collect::<Result<_, _>>()?;

        for reloc in &boot.relocs {
            let mut value = match reloc.kind {
                2 => data_traps_base + (self.page_size as u64) * reloc.value + reloc.addend,
                1 => traps_base + 32 * reloc.value,
                _ => image_base + reloc.value,
            };

            if reloc.kind != 0 {
                let idx = reloc.value as usize;
                if let Some(resolved) = runtime_resolve(&c_names[idx], reloc.kind == 2) {
                    value = (resolved as u64) + reloc.addend;
                } else if native_libc {
                    if let Some(Some((address, kind))) = boot.bindings.get(idx) {
                        if *kind != reloc.kind
                            || !is_mapped(&boot.segments, *address, reloc.addend + 1)
                        {
                            return Err("native export kind/range mismatch".into());
                        }
                        value = image_base + *address + reloc.addend;
                    }
                }
            }

            // SAFETY: `BootImage::parse` validated `reloc.target + 8` is inside a mapped segment.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    value.to_le_bytes().as_ptr(),
                    self.base.add(reloc.target as usize),
                    8,
                );
            }
        }
        Ok(())
    }

    /// Applies final `PF_R`/`PF_W`/`PF_X` segment permissions after all patches are installed,
    /// marking the import trap table read-execute and unmapped image gaps inaccessible.
    pub fn protect_segments(&self, segments: &[Segment]) -> Result<(), String> {
        // SAFETY: `traps` and `base` cover `traps_size` and `mapped_size` bytes.
        unsafe {
            protect_mem(self.traps, self.traps_size, 5)?;
            protect_mem(self.base, self.mapped_size, 0)?;
        }
        for seg in segments {
            let addr = (self.base() + seg.address) as usize as *mut u8;
            let len = round_page(seg.size as usize, self.page_size);
            // SAFETY: `addr..addr+len` lies within the image allocated by `Self::load`.
            unsafe { protect_mem(addr, len, seg.flags) }?;
        }
        Ok(())
    }

    /// Consumes the handle without unmapping, returning `(base, mapped_size)` for guest execution.
    pub fn into_raw(self) -> (*mut u8, usize) {
        let base = self.base;
        let mapped_size = self.mapped_size;
        std::mem::forget(self);
        (base, mapped_size)
    }
}

impl Drop for MappedImage {
    fn drop(&mut self) {
        // SAFETY: all three regions were allocated with their respective sizes in `Self::load`.
        unsafe {
            free_mem(self.data_traps, self.data_traps_size);
            free_mem(self.traps, self.traps_size);
            free_mem(self.base, self.mapped_size);
        }
    }
}

fn round_page(size: usize, page_size: usize) -> usize {
    (size + page_size - 1) & !(page_size - 1)
}

#[cfg(target_os = "linux")]
fn allocate_rw(size: usize) -> Result<*mut u8, String> {
    let p = crate::memory::abi::runtime_image_map(size, libc::PROT_READ | libc::PROT_WRITE);
    if !p.is_null() {
        return Ok(p as *mut u8);
    }
    // SAFETY: anonymous private mapping for fallback when `runtime_image_map` declines.
    let p = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    if p == libc::MAP_FAILED {
        Err("mmap allocation failed".into())
    } else {
        Ok(p as *mut u8)
    }
}

#[cfg(target_os = "linux")]
fn allocate_rwx(size: usize) -> Result<*mut u8, String> {
    // SAFETY: anonymous private RWX mapping for import trampoline stubs.
    let p = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    if p == libc::MAP_FAILED {
        Err("mmap allocation failed".into())
    } else {
        Ok(p as *mut u8)
    }
}

#[cfg(target_os = "linux")]
unsafe fn protect_mem(addr: *mut u8, size: usize, flags: u64) -> Result<(), String> {
    let mut mode = 0;
    if (flags & 4) != 0 {
        mode |= libc::PROT_READ;
    }
    if (flags & 2) != 0 {
        mode |= libc::PROT_WRITE;
    }
    if (flags & 1) != 0 {
        mode |= libc::PROT_EXEC;
    }
    // SAFETY: caller guarantees `addr..addr+size` is mapped.
    if unsafe { libc::mprotect(addr as *mut libc::c_void, size, mode) } != 0 {
        Err("mprotect failed".into())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
unsafe fn free_mem(addr: *mut u8, size: usize) {
    if !addr.is_null() && size > 0 {
        // SAFETY: caller guarantees `addr..addr+size` was mapped via `mmap`.
        unsafe {
            libc::munmap(addr as *mut libc::c_void, size);
        }
    }
}

#[cfg(windows)]
fn allocate_rw(size: usize) -> Result<*mut u8, String> {
    use windows_sys::Win32::System::Memory::{
        VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE,
    };
    // SAFETY: anonymous VirtualAlloc request.
    let p = unsafe {
        VirtualAlloc(
            std::ptr::null(),
            size,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        )
    };
    if p.is_null() {
        Err("VirtualAlloc allocation failed".into())
    } else {
        Ok(p as *mut u8)
    }
}

#[cfg(windows)]
fn allocate_rwx(size: usize) -> Result<*mut u8, String> {
    use windows_sys::Win32::System::Memory::{
        VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
    };
    // SAFETY: anonymous VirtualAlloc request for import trampolines.
    let p = unsafe {
        VirtualAlloc(
            std::ptr::null(),
            size,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_EXECUTE_READWRITE,
        )
    };
    if p.is_null() {
        Err("VirtualAlloc allocation failed".into())
    } else {
        Ok(p as *mut u8)
    }
}

#[cfg(windows)]
unsafe fn protect_mem(addr: *mut u8, size: usize, flags: u64) -> Result<(), String> {
    use windows_sys::Win32::System::Memory::{
        VirtualProtect, PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_NOACCESS,
        PAGE_READONLY, PAGE_READWRITE,
    };
    let mode = match flags & 7 {
        0 => PAGE_NOACCESS,
        1 => PAGE_EXECUTE,
        4 => PAGE_READONLY,
        5 => PAGE_EXECUTE_READ,
        2 | 6 => PAGE_READWRITE,
        _ => PAGE_EXECUTE_READWRITE,
    };
    let mut old = 0u32;
    // SAFETY: caller guarantees `addr..addr+size` is committed.
    if unsafe { VirtualProtect(addr as *const _, size, mode, &mut old) } == 0 {
        Err("VirtualProtect failed".into())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
unsafe fn free_mem(addr: *mut u8, _size: usize) {
    use windows_sys::Win32::System::Memory::{VirtualFree, MEM_RELEASE};
    if !addr.is_null() {
        // SAFETY: caller guarantees `addr` is the base returned by `VirtualAlloc`.
        unsafe {
            VirtualFree(addr as *mut _, 0, MEM_RELEASE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn dummy_unresolved() {}

    #[test]
    fn maps_image_applies_image_relative_and_unresolved_relocs_and_protects() {
        let mut raw = Vec::new();
        raw.extend_from_slice(b"BBPROBE2");
        raw.extend_from_slice(&0x2000u64.to_le_bytes()); // size
        raw.extend_from_slice(&0x0u64.to_le_bytes()); // entry
        raw.extend_from_slice(&1u64.to_le_bytes()); // segments
        raw.extend_from_slice(&2u64.to_le_bytes()); // relocs
        raw.extend_from_slice(&1u64.to_le_bytes()); // imports
        raw.extend_from_slice(&0u64.to_le_bytes()); // capabilities
                                                    // Segment 0: [0, 0x2000) RW
        raw.extend_from_slice(&0u64.to_le_bytes());
        raw.extend_from_slice(&0x2000u64.to_le_bytes());
        raw.extend_from_slice(&6u64.to_le_bytes());
        // Import 0: unknown NID
        let mut nid = [0u8; 128];
        nid[..11].copy_from_slice(b"00000000000");
        raw.extend_from_slice(&nid);
        // Reloc 0: image-relative at 0x100 -> 0x400
        raw.extend_from_slice(&0x100u64.to_le_bytes());
        raw.extend_from_slice(&0u64.to_le_bytes());
        raw.extend_from_slice(&0x400u64.to_le_bytes());
        raw.extend_from_slice(&0u64.to_le_bytes());
        // Reloc 1: unresolved code import 0 at 0x108
        raw.extend_from_slice(&0x108u64.to_le_bytes());
        raw.extend_from_slice(&1u64.to_le_bytes());
        raw.extend_from_slice(&0u64.to_le_bytes());
        raw.extend_from_slice(&0u64.to_le_bytes());
        // Payload
        raw.resize(raw.len() + 0x2000, 0xCC);

        let boot = BootImage::parse(&raw, 4096).unwrap();
        let mapped = MappedImage::load(
            &boot,
            &raw[boot.image_offset..],
            4096,
            dummy_unresolved as *const () as usize,
        )
        .unwrap();

        // SAFETY: `mapped.base()` covers 0x2000 bytes.
        let (rel_ptr, trap_ptr) = unsafe {
            let p0 = std::ptr::read_unaligned((mapped.base() + 0x100) as *const u64);
            let p1 = std::ptr::read_unaligned((mapped.base() + 0x108) as *const u64);
            (p0, p1)
        };
        assert_eq!(rel_ptr, mapped.base() + 0x400);
        assert_eq!(trap_ptr, mapped.traps as usize as u64);

        mapped.protect_segments(&boot.segments).unwrap();
    }

    #[test]
    fn load_with_options_honors_strict_imports_on_bbprobe4_bindings() {
        let mut raw = Vec::new();
        raw.extend_from_slice(b"BBPROBE4");
        raw.extend_from_slice(&0x2000u64.to_le_bytes()); // size
        raw.extend_from_slice(&0x100u64.to_le_bytes()); // entry
        raw.extend_from_slice(&1u64.to_le_bytes()); // ns
        raw.extend_from_slice(&1u64.to_le_bytes()); // nr
        raw.extend_from_slice(&1u64.to_le_bytes()); // import_count
        raw.extend_from_slice(&1u64.to_le_bytes()); // capabilities (bit 0 = native_libc)
                                                    // linked module 0: [0, 0x2000), init=0x100, no tls
        raw.extend_from_slice(&0u64.to_le_bytes());
        raw.extend_from_slice(&0x2000u64.to_le_bytes());
        raw.extend_from_slice(&0x100u64.to_le_bytes());
        raw.extend_from_slice(&0u64.to_le_bytes());
        raw.extend_from_slice(&0u64.to_le_bytes());
        raw.extend_from_slice(&0u64.to_le_bytes());
        raw.extend_from_slice(&1u64.to_le_bytes()); // nb = 1
        raw.extend_from_slice(&0x200u64.to_le_bytes()); // procparam
                                                        // main_tls: 4 u64 zeros
        for _ in 0..4 {
            raw.extend_from_slice(&0u64.to_le_bytes());
        }
        // binding 0 -> index 0, address 0x300, kind 1 (func)
        raw.extend_from_slice(&0u64.to_le_bytes());
        raw.extend_from_slice(&0x300u64.to_le_bytes());
        raw.extend_from_slice(&1u64.to_le_bytes());
        // segment 0: [0, 0x2000) RWX (7)
        raw.extend_from_slice(&0u64.to_le_bytes());
        raw.extend_from_slice(&0x2000u64.to_le_bytes());
        raw.extend_from_slice(&7u64.to_le_bytes());
        // import 0: unknown NID
        let mut nid = [0u8; 128];
        nid[..11].copy_from_slice(b"unknown#a#b");
        raw.extend_from_slice(&nid);
        // reloc 0: code import 0 at 0x80
        raw.extend_from_slice(&0x80u64.to_le_bytes());
        raw.extend_from_slice(&1u64.to_le_bytes());
        raw.extend_from_slice(&0u64.to_le_bytes());
        raw.extend_from_slice(&0u64.to_le_bytes());
        // payload
        raw.resize(raw.len() + 0x2000, 0);

        let boot = BootImage::parse(&raw, 4096).unwrap();
        let non_strict = MappedImage::load_with_options(
            &boot,
            &raw[boot.image_offset..],
            4096,
            dummy_unresolved as *const () as usize,
            false,
        )
        .unwrap();
        let bound_ptr =
            unsafe { std::ptr::read_unaligned((non_strict.base() + 0x80) as *const u64) };
        assert_eq!(bound_ptr, non_strict.base() + 0x300);

        let strict = MappedImage::load_with_options(
            &boot,
            &raw[boot.image_offset..],
            4096,
            dummy_unresolved as *const () as usize,
            true,
        )
        .unwrap();
        let trap_ptr = unsafe { std::ptr::read_unaligned((strict.base() + 0x80) as *const u64) };
        assert_eq!(trap_ptr, strict.traps as usize as u64);
    }
}
