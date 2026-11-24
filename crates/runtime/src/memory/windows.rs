//! The Windows host of [`super`]: a pagefile-backed section (`CreateFileMappingW`) with
//! placeholder/view carving (`VirtualAlloc2` / `MapViewOfFile3` / `UnmapViewOfFile2`) so
//! `[USER_MIN, USER_MAX)` supports arbitrary guest mappings, splits, unmaps, and lazy commit.

use super::Host;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use windows_sys::Win32::Foundation::{
    GetLastError, SetLastError, ERROR_COMMITMENT_LIMIT, ERROR_COMMITMENT_MINIMUM,
    ERROR_NOT_ENOUGH_MEMORY, ERROR_NO_SYSTEM_RESOURCES, HANDLE, INVALID_HANDLE_VALUE, MAX_PATH,
};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, MapViewOfFile3, UnmapViewOfFile2, VirtualAlloc,
    VirtualAlloc2, VirtualFree, VirtualProtect, VirtualQuery, FILE_MAP_ALL_ACCESS,
    MEMORY_BASIC_INFORMATION, MEMORY_MAPPED_VIEW_ADDRESS, MEM_COMMIT, MEM_FREE,
    MEM_PRESERVE_PLACEHOLDER, MEM_RELEASE, MEM_REPLACE_PLACEHOLDER, MEM_RESERVE,
    MEM_RESERVE_PLACEHOLDER, PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
    PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, SEC_RESERVE,
};
use windows_sys::Win32::System::ProcessStatus::K32GetMappedFileNameA;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

const MEM_COALESCE_PLACEHOLDERS: u32 = 0x0000_0001;

#[derive(Clone, Copy)]
struct HostRegion {
    start: u64,
    end: u64,
    phys: u64,
    mapped: bool,
}

#[derive(Clone, Copy)]
struct ProtRun {
    start: u64,
    end: u64,
    protect: u32,
}

static LOW_NEXT: AtomicU64 = AtomicU64::new(super::LOW_MIN);

fn win_prot(prot: i32) -> u32 {
    const PROT_READ: i32 = 1;
    const PROT_WRITE: i32 = 2;
    const PROT_EXEC: i32 = 4;
    if prot & PROT_EXEC != 0 {
        if prot & PROT_WRITE != 0 {
            PAGE_EXECUTE_READWRITE
        } else if prot & PROT_READ != 0 {
            PAGE_EXECUTE_READ
        } else {
            PAGE_EXECUTE
        }
    } else if prot & PROT_WRITE != 0 {
        PAGE_READWRITE
    } else if prot & PROT_READ != 0 {
        PAGE_READONLY
    } else {
        PAGE_NOACCESS
    }
}

fn win_fail(what: &str, address: u64, size: u64) -> ! {
    // SAFETY: GetLastError has no preconditions.
    let err = unsafe { GetLastError() };
    eprintln!("STOP: {what} failed at 0x{address:x} (+0x{size:x}), error {err}");
    std::process::exit(21);
}

fn region_index(regions: &[HostRegion], addr: u64) -> usize {
    regions.partition_point(|r| r.end <= addr)
}

fn save_protections(start: u64, end: u64, runs: &mut Vec<ProtRun>) {
    let mut at = start;
    while at < end {
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: info is valid and writable.
        let queried = unsafe {
            VirtualQuery(
                at as *const c_void,
                &mut info,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if queried == 0 {
            win_fail("VirtualQuery", at, 0);
        }
        let mut next = info.BaseAddress as u64 + info.RegionSize as u64;
        if next > end {
            next = end;
        }
        runs.push(ProtRun {
            start: at,
            end: next,
            protect: info.Protect,
        });
        at = next;
    }
}

/// The Windows backing section and placeholder region table.
pub struct WinHost {
    section: AtomicUsize,
    backing: AtomicUsize,
    regions: Mutex<Vec<HostRegion>>,
}

impl WinHost {
    pub const fn new() -> Self {
        Self {
            section: AtomicUsize::new(0),
            backing: AtomicUsize::new(0),
            regions: Mutex::new(Vec::new()),
        }
    }

    /// Host address of backing byte `phys` (0 before the pool exists).
    pub fn backing(&self, phys: u64) -> usize {
        self.backing.load(Ordering::Acquire) + phys as usize
    }

    /// Reserves `[USER_MIN, USER_MAX)` as a placeholder, retrying up to 8 times if an injected
    /// DLL or heap landed in the range.
    pub fn reserve(&self) {
        let mut regions = self.regions.lock().unwrap_or_else(PoisonError::into_inner);
        if !regions.is_empty() {
            return;
        }
        if std::env::var_os("BB_TEST_OCCUPY").is_some()
            && std::env::var_os("BB_RESERVE_TRY").is_none()
        {
            // SAFETY: test hook reserving a dummy block inside the PS4 user range.
            unsafe {
                VirtualAlloc(
                    0x14_0000_0000usize as *const c_void,
                    0x1_0000,
                    MEM_RESERVE,
                    PAGE_NOACCESS,
                );
            }
        }
        // SAFETY: reserving [USER_MIN, USER_MAX) as a placeholder.
        let reserved = unsafe {
            VirtualAlloc2(
                ptr::null_mut(),
                super::USER_MIN as *const c_void,
                (super::USER_MAX - super::USER_MIN) as usize,
                MEM_RESERVE | MEM_RESERVE_PLACEHOLDER,
                PAGE_NOACCESS,
                ptr::null_mut(),
                0,
            )
        };
        if reserved.is_null() {
            drop(regions);
            reserve_failed();
        }
        regions.push(HostRegion {
            start: super::USER_MIN,
            end: super::USER_MAX,
            phys: 0,
            mapped: false,
        });
    }

    fn view_map(&self, r: &HostRegion, protect: u32) {
        let initial = if protect == PAGE_NOACCESS {
            PAGE_READWRITE
        } else {
            protect
        };
        let sec = self.section.load(Ordering::Acquire) as HANDLE;
        // SAFETY: placeholder `[r.start, r.end)` was carved out and `sec` covers `r.phys`.
        let mapped = unsafe {
            MapViewOfFile3(
                sec,
                GetCurrentProcess(),
                r.start as *const c_void,
                r.phys,
                (r.end - r.start) as usize,
                MEM_REPLACE_PLACEHOLDER,
                initial,
                ptr::null_mut(),
                0,
            )
        };
        if mapped.Value.is_null() {
            win_fail("MapViewOfFile3", r.start, r.end - r.start);
        }
        if initial != protect {
            let mut old = 0u32;
            // SAFETY: restoring PAGE_NOACCESS on the newly mapped view.
            let ok = unsafe {
                VirtualProtect(
                    r.start as *const c_void,
                    (r.end - r.start) as usize,
                    protect,
                    &mut old,
                )
            };
            if ok == 0 {
                win_fail("VirtualProtect", r.start, r.end - r.start);
            }
        }
    }

    fn carve_host(&self, regions: &mut Vec<HostRegion>, a: u64, b: u64) -> usize {
        let first = region_index(regions, a);
        if first == regions.len() || regions[first].start > a {
            win_fail("guest range outside the reservation", a, b - a);
        }
        let mut last = first;
        while last < regions.len() && regions[last].start < b {
            last += 1;
        }
        if regions[last - 1].end < b {
            win_fail("guest range outside the reservation", a, b - a);
        }
        let lo = regions[first].start;
        let hi = regions[last - 1].end;
        let left = regions[first];
        let right = regions[last - 1];
        let keep_left = left.mapped && lo < a;
        let keep_right = right.mapped && b < hi;
        if first == last - 1 && !left.mapped && lo == a && hi == b {
            return first;
        }
        let mut runs = Vec::new();
        if keep_left {
            save_protections(lo, a, &mut runs);
        }
        if keep_right {
            save_protections(b, hi, &mut runs);
        }
        for r in &regions[first..last] {
            if r.mapped {
                // SAFETY: unmapping a view into a placeholder.
                let ok = unsafe {
                    UnmapViewOfFile2(
                        GetCurrentProcess(),
                        MEMORY_MAPPED_VIEW_ADDRESS {
                            Value: r.start as *mut c_void,
                        },
                        MEM_PRESERVE_PLACEHOLDER,
                    )
                };
                if ok == 0 {
                    win_fail("UnmapViewOfFile2", r.start, r.end - r.start);
                }
            }
        }
        if last - first > 1 {
            // SAFETY: coalescing contiguous placeholders `[lo, hi)`.
            let ok = unsafe {
                VirtualFree(
                    lo as *mut c_void,
                    (hi - lo) as usize,
                    MEM_RELEASE | MEM_COALESCE_PLACEHOLDERS,
                )
            };
            if ok == 0 {
                win_fail("coalescing placeholders", lo, hi - lo);
            }
        }
        regions.drain(first..last);
        regions.insert(
            first,
            HostRegion {
                start: lo,
                end: hi,
                phys: 0,
                mapped: false,
            },
        );
        let mut at = first;
        if lo < a {
            // SAFETY: splitting `[lo, a)` out of placeholder `[lo, hi)`.
            let ok = unsafe {
                VirtualFree(
                    lo as *mut c_void,
                    (a - lo) as usize,
                    MEM_RELEASE | MEM_PRESERVE_PLACEHOLDER,
                )
            };
            if ok == 0 {
                win_fail("splitting a placeholder", lo, a - lo);
            }
            regions[at].end = a;
            regions.insert(
                at + 1,
                HostRegion {
                    start: a,
                    end: hi,
                    phys: 0,
                    mapped: false,
                },
            );
            if keep_left {
                regions[at].phys = left.phys;
                regions[at].mapped = true;
                let region = regions[at];
                self.view_map(&region, PAGE_READWRITE);
            }
            at += 1;
        }
        if b < hi {
            // SAFETY: splitting `[a, b)` out of placeholder `[a, hi)`.
            let ok = unsafe {
                VirtualFree(
                    a as *mut c_void,
                    (b - a) as usize,
                    MEM_RELEASE | MEM_PRESERVE_PLACEHOLDER,
                )
            };
            if ok == 0 {
                win_fail("splitting a placeholder", a, b - a);
            }
            regions[at].end = b;
            regions.insert(
                at + 1,
                HostRegion {
                    start: b,
                    end: hi,
                    phys: 0,
                    mapped: false,
                },
            );
            if keep_right {
                regions[at + 1].phys = right.phys + (b - right.start);
                regions[at + 1].mapped = true;
                let region = regions[at + 1];
                self.view_map(&region, PAGE_READWRITE);
            }
        }
        for run in runs {
            if run.protect != PAGE_READWRITE {
                let mut old = 0u32;
                // SAFETY: restoring prior page protections on the split views.
                let ok = unsafe {
                    VirtualProtect(
                        run.start as *const c_void,
                        (run.end - run.start) as usize,
                        run.protect,
                        &mut old,
                    )
                };
                if ok == 0 {
                    win_fail("restoring page protection", run.start, run.end - run.start);
                }
            }
        }
        at
    }
}

impl Default for WinHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Host for WinHost {
    fn pool(&mut self, bytes: u64) -> bool {
        if self.backing.load(Ordering::Acquire) != 0 {
            return true;
        }
        self.reserve();
        let hi = (bytes >> 32) as u32;
        let lo = bytes as u32;
        // SAFETY: reserving a pagefile-backed section of `bytes`.
        let sec = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                ptr::null(),
                PAGE_EXECUTE_READWRITE | SEC_RESERVE,
                hi,
                lo,
                ptr::null(),
            )
        };
        if sec.is_null() {
            // SAFETY: GetLastError has no preconditions.
            let err = unsafe { GetLastError() };
            eprintln!(
                "Runtime: cannot create {} MiB of guest memory (error {err}); is the page file large enough?",
                bytes >> 20
            );
            return false;
        }
        // SAFETY: mapping the whole section as the host backing view.
        let view = unsafe { MapViewOfFile(sec, FILE_MAP_ALL_ACCESS, 0, 0, bytes as usize) };
        if view.Value.is_null() {
            return false;
        }
        self.section.store(sec as usize, Ordering::Release);
        self.backing.store(view.Value as usize, Ordering::Release);
        true
    }

    fn commit(&self, phys: u64, size: u64) -> bool {
        let base = self.backing.load(Ordering::Acquire);
        if base == 0 {
            return false;
        }
        for attempt in 0..=40 {
            // SAFETY: committing `[base + phys, base + phys + size)` inside the section view.
            let p = unsafe {
                VirtualAlloc(
                    (base + phys as usize) as *const c_void,
                    size as usize,
                    MEM_COMMIT,
                    PAGE_READWRITE,
                )
            };
            if !p.is_null() {
                if attempt > 0 {
                    println!(
                        "Runtime: committed {} MiB of guest memory after {attempt} retries (page file grew)",
                        size >> 20
                    );
                }
                return true;
            }
            // SAFETY: GetLastError has no preconditions.
            let err = unsafe { GetLastError() };
            let retryable = matches!(
                err,
                ERROR_COMMITMENT_LIMIT
                    | ERROR_NOT_ENOUGH_MEMORY
                    | ERROR_NO_SYSTEM_RESOURCES
                    | ERROR_COMMITMENT_MINIMUM
            );
            if attempt == 40 || !retryable {
                eprintln!(
                    "Runtime: cannot commit {} MiB of guest memory (error {err}); is the page file large enough?",
                    size >> 20
                );
                return false;
            }
            if attempt == 0 {
                println!(
                    "Runtime: commit limit reached ({} MiB of guest memory, error {err}): waiting for the page file to grow",
                    size >> 20
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        false
    }

    fn map(&self, address: u64, size: u64, prot: i32, reserved: bool, phys: u64) -> bool {
        let mut regions = self.regions.lock().unwrap_or_else(PoisonError::into_inner);
        let idx = self.carve_host(&mut regions, address, address + size);
        if reserved {
            return true;
        }
        regions[idx].phys = phys;
        regions[idx].mapped = true;
        let region = regions[idx];
        self.view_map(&region, win_prot(prot));
        true
    }

    fn unmap(&self, address: u64, size: u64) -> bool {
        let mut regions = self.regions.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = self.carve_host(&mut regions, address, address + size);
        true
    }

    fn protect(&self, address: u64, size: u64, prot: i32) -> bool {
        let regions = self.regions.lock().unwrap_or_else(PoisonError::into_inner);
        let protect = win_prot(prot);
        let end = address + size;
        let mut i = region_index(&regions, address);
        while i < regions.len() && regions[i].start < end {
            if regions[i].mapped {
                let a = regions[i].start.max(address);
                let b = regions[i].end.min(end);
                let mut old = 0u32;
                // SAFETY: `[a, b)` is inside a mapped view of the guest section.
                let ok = unsafe {
                    VirtualProtect(a as *const c_void, (b - a) as usize, protect, &mut old)
                };
                if ok == 0 {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    fn discard(&self, phys: u64, size: u64) {
        let base = self.backing.load(Ordering::Acquire);
        if base != 0 {
            // SAFETY: `[base + phys, base + phys + size)` is committed backing memory.
            unsafe { ptr::write_bytes((base + phys as usize) as *mut u8, 0, size as usize) };
        }
    }
}

fn reserve_failed() -> ! {
    // SAFETY: GetLastError has no preconditions.
    let error = unsafe { GetLastError() };
    let mut listed = 0;
    let mut at = super::USER_MIN;
    while at < super::USER_MAX && listed < 8 {
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: info is valid and writable.
        let queried = unsafe {
            VirtualQuery(
                at as *const c_void,
                &mut info,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if queried == 0 {
            break;
        }
        if info.State != MEM_FREE {
            let mut name = [0u8; MAX_PATH as usize];
            // SAFETY: current process handle and stack buffer.
            let len = unsafe {
                K32GetMappedFileNameA(
                    GetCurrentProcess(),
                    info.BaseAddress,
                    name.as_mut_ptr(),
                    name.len() as u32,
                )
            };
            let label = if len > 0 {
                String::from_utf8_lossy(&name[..len as usize]).into_owned()
            } else {
                String::from("no file")
            };
            println!(
                "Runtime: the PS4 address range is not free: {:p} +0x{:x} ({label})",
                info.BaseAddress, info.RegionSize
            );
            listed += 1;
        }
        at = info.BaseAddress as u64 + info.RegionSize as u64;
    }
    let tries = std::env::var("BB_RESERVE_TRY")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    if tries < 8 {
        std::env::set_var("BB_RESERVE_TRY", (tries + 1).to_string());
        println!("Runtime: starting again (try {} of 8)", tries + 1);
        if let Ok(exe) = std::env::current_exe() {
            if let Ok(status) = std::process::Command::new(exe)
                .args(std::env::args_os().skip(1))
                .status()
            {
                std::process::exit(status.code().unwrap_or(21));
            }
        }
        // SAFETY: restore the original reservation error if respawning failed.
        unsafe { SetLastError(error) };
    }
    win_fail(
        "reserving the PS4 address range (link with --disable-high-entropy-va)",
        super::USER_MIN,
        super::USER_MAX - super::USER_MIN,
    );
}

pub fn low_map(size: u64, prot: i32) -> *mut c_void {
    let mut next = LOW_NEXT.load(Ordering::Relaxed);
    let mut mapped = ptr::null_mut();
    let wprot = win_prot(prot);
    while next.wrapping_add(size) <= super::USER_MIN {
        // SAFETY: allocating an uncommitted/committed low region in `[LOW_MIN, USER_MIN)`.
        mapped = unsafe {
            VirtualAlloc(
                next as *const c_void,
                size as usize,
                MEM_RESERVE | MEM_COMMIT,
                wprot,
            )
        };
        next = next.wrapping_add(size + 0x1_0000);
        if !mapped.is_null() {
            break;
        }
    }
    LOW_NEXT.store(next, Ordering::Relaxed);
    mapped
}

pub fn image_map(size: u64, prot: i32) -> *mut c_void {
    let wprot = win_prot(prot);
    let mut at = super::IMAGE_MIN;
    while at.wrapping_add(size) <= super::IMAGE_LIMIT {
        // SAFETY: reserving and committing executable image memory below 2 GiB.
        let mapped = unsafe {
            VirtualAlloc(
                at as *const c_void,
                size as usize,
                MEM_RESERVE | MEM_COMMIT,
                wprot,
            )
        };
        if !mapped.is_null() {
            return mapped;
        }
        at += 0x10_0000;
    }
    ptr::null_mut()
}
