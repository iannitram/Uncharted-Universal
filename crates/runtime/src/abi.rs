//! The C ABI of the runtime, the first module ported from C (piece 3.01; order 1 in
//! docs/code-map.md): the NID table (`src/import_names.inc`), `runtime_resolve`, and the narrow
//! PS4 libc contracts of the deleted `src/runtime.c` in [`libc`]. `tests/test_runtime.c`'s cases
//! for that file are this module's tests now.
//!
//! # The strangler plan (ADR 0002)
//!
//! This crate builds as a static library linked into the loader binary while C remains. Each
//! `src/runtime_*.c` is replaced by a Rust module that exports the SAME `extern "C"` symbols
//! (declared for C in `crates/runtime/include/uncharted_runtime.h`, which `cbindgen` writes and
//! `tests/header.rs` keeps current), and the C file is deleted in the same PR; its C test becomes a
//! Rust test. Leaves first, loader (`probe.c`) last, and `bash build.sh` plus the boot smoke test
//! (`tests/test_boot_smoke.py`) stay green at every step. No behaviour changes in a port PR; a
//! bug found while porting is its own issue.
//!
//! `runtime_resolve` asks this module's own contracts first, then the C modules in the order
//! `runtime.c` did, then the GPU library. Those calls exist only in the loader build
//! (`cfg(uncharted_loader)`, set by `build.rs`); elsewhere the chain ends here.
//!
//! Only this crate has `unsafe` and FFI; every `unsafe` block carries a `// SAFETY:` line.

use std::ffi::{c_char, c_void, CStr};

// The C functions the runtime calls, not ones it exports.
/// cbindgen:ignore
#[cfg(target_arch = "x86_64")]
mod host;
#[cfg(target_arch = "x86_64")]
pub mod libc;

/// The runtime's C ABI version. The loader compares [`uncharted_runtime_version`] with the value
/// its copy of the header has and refuses to start on a mismatch (a stale static library).
/// Raised when a symbol moves, changes or goes away. 1: `src/runtime.c` moved here (3.01).
pub const UNCHARTED_RUNTIME_ABI_VERSION: u32 = 1;

include!(concat!(env!("OUT_DIR"), "/nid_table.rs"));

/// The symbol name for `nid`, if it is in [`NID_TABLE`] (the C `runtime_symbol`).
pub fn nid_name(nid: &str) -> Option<&'static str> {
    nid_entry(nid.as_bytes()).and_then(|symbol| symbol.to_str().ok())
}

fn nid_entry(nid: &[u8]) -> Option<&'static CStr> {
    NID_TABLE
        .binary_search_by(|(entry, _)| entry.to_bytes().cmp(nid))
        .ok()
        .map(|i| NID_TABLE[i].1)
}

/// Where a guest import `name` resolves to (`is_data`: a data symbol, not a function): this
/// module's libc contracts, then each C library's resolver in turn and the GPU library (loader
/// build only); `None` when nothing exports it or the runtime is not enabled (`runtime_start`
/// without capability 1). Only x86-64 hosts run guest code; elsewhere this is always `None`.
pub fn runtime_resolve(name: &CStr, is_data: bool) -> Option<usize> {
    #[cfg(target_arch = "x86_64")]
    {
        libc::resolve(name, is_data)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (name, is_data);
        None
    }
}

/// One entry of a C module's export table: a symbol name and the host function behind it.
/// `RUNTIME_LOOKUP` in `src/runtime.h` searches such a table through [`ffi::runtime_lookup`].
#[repr(C)]
pub struct RuntimeExport {
    pub name: *const c_char,
    pub function: *mut c_void,
}

/// The exported C functions of this module, declared in `include/uncharted_runtime.h`.
pub mod ffi {
    use super::{nid_entry, RuntimeExport};
    use std::ffi::{c_char, c_int, CStr};

    /// The runtime's ABI version ([`super::UNCHARTED_RUNTIME_ABI_VERSION`]); the loader checks it
    /// at start.
    #[no_mangle]
    pub extern "C" fn uncharted_runtime_version() -> u32 {
        super::UNCHARTED_RUNTIME_ABI_VERSION
    }

    /// `name` as a C string, or `None` for a null pointer.
    ///
    /// # Safety
    /// `name` is null or a NUL-terminated string that outlives the returned reference.
    unsafe fn c_str<'a>(name: *const c_char) -> Option<&'a CStr> {
        // SAFETY: the caller passes null or a valid NUL-terminated string.
        (!name.is_null()).then(|| unsafe { CStr::from_ptr(name) })
    }

    /// The address a guest import resolves to, or 0 (`uintptr_t runtime_resolve(const char *name,
    /// int is_data)`).
    ///
    /// # Safety
    /// `name` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_resolve(name: *const c_char, is_data: c_int) -> usize {
        // SAFETY: forwarded from the caller.
        let Some(name) = (unsafe { c_str(name) }) else {
            return 0;
        };
        super::runtime_resolve(name, is_data != 0).unwrap_or(0)
    }

    /// The symbol name for a scoped NID, or null when this eboot and libc never import it.
    ///
    /// # Safety
    /// `nid` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_symbol(nid: *const c_char) -> *const c_char {
        // SAFETY: forwarded from the caller.
        unsafe { c_str(nid) }
            .and_then(|nid| nid_entry(nid.to_bytes()))
            .map_or(std::ptr::null(), CStr::as_ptr)
    }

    /// The function of the entry in `table` whose name is the symbol of `nid`, or 0. The same
    /// symbol name means the same contract whether imported from libkernel or libScePosix, so the
    /// C modules resolve by name.
    ///
    /// # Safety
    /// `table` points to `count` entries whose names are null or NUL-terminated strings; `nid` is
    /// null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_lookup(
        table: *const RuntimeExport,
        count: usize,
        nid: *const c_char,
    ) -> usize {
        // SAFETY: forwarded from the caller.
        let Some(symbol) = (unsafe { c_str(nid) }).and_then(|nid| nid_entry(nid.to_bytes())) else {
            return 0;
        };
        if table.is_null() {
            return 0;
        }
        // SAFETY: the caller passes `count` valid entries at `table`.
        let entries = unsafe { std::slice::from_raw_parts(table, count) };
        entries
            .iter()
            // SAFETY: each entry's name is null or a NUL-terminated string (caller).
            .find(|entry| unsafe { c_str(entry.name) } == Some(symbol))
            .map_or(0, |entry| entry.function as usize)
    }

    /// The symbol name of `name` for messages, or a pointer to where hints are.
    ///
    /// # Safety
    /// `name` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_import_name(name: *const c_char) -> *const c_char {
        // SAFETY: forwarded from the caller.
        let symbol = unsafe { runtime_symbol(name) };
        if symbol.is_null() {
            c"name not resolved; see analysis.json import_name_hints".as_ptr()
        } else {
            symbol
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_raised_with_the_first_module() {
        assert_eq!(ffi::uncharted_runtime_version(), 1);
        assert_eq!(
            ffi::uncharted_runtime_version(),
            UNCHARTED_RUNTIME_ABI_VERSION
        );
    }

    #[test]
    fn nid_table_is_sorted_unique_and_complete() {
        assert!(NID_TABLE
            .windows(2)
            .all(|pair| pair[0].0.to_bytes() < pair[1].0.to_bytes()));
        let source = include_str!("../../../src/import_names.inc");
        let lines = source.lines().filter(|line| line.starts_with('{')).count();
        assert_eq!(NID_TABLE.len(), lines);
        assert!(NID_TABLE.len() > 900);
    }

    #[test]
    fn nid_name_finds_every_entry_and_nothing_else() {
        assert_eq!(nid_name("bzQExy189ZI#q#q"), Some("_init_env"));
        assert_eq!(nid_name("8G2LB+A3rzg#q#q"), Some("atexit"));
        assert_eq!(nid_name("ejekcaNQNq0#p#J"), Some("sceKernelGettimeofday"));
        for (nid, symbol) in NID_TABLE {
            assert_eq!(nid_name(nid.to_str().unwrap()), symbol.to_str().ok());
        }
        assert_eq!(nid_name("bzQExy189ZI#wrong#module"), None);
        assert_eq!(nid_name(""), None);
    }

    #[test]
    fn symbol_lookup_and_import_name_follow_the_c_contract() {
        // SAFETY: every pointer below is a NUL-terminated literal or null.
        unsafe {
            let symbol = ffi::runtime_symbol(c"8G2LB+A3rzg#q#q".as_ptr());
            assert_eq!(CStr::from_ptr(symbol), c"atexit");
            assert!(ffi::runtime_symbol(c"unknown".as_ptr()).is_null());
            assert!(ffi::runtime_symbol(std::ptr::null()).is_null());
            assert_eq!(
                CStr::from_ptr(ffi::runtime_import_name(c"bzQExy189ZI#q#q".as_ptr())),
                c"_init_env"
            );
            assert_eq!(
                CStr::from_ptr(ffi::runtime_import_name(c"unknown".as_ptr())),
                c"name not resolved; see analysis.json import_name_hints"
            );
            let mut a = 0u8;
            let mut b = 0u8;
            let table = [
                RuntimeExport {
                    name: c"_init_env".as_ptr(),
                    function: std::ptr::addr_of_mut!(a).cast(),
                },
                RuntimeExport {
                    name: c"atexit".as_ptr(),
                    function: std::ptr::addr_of_mut!(b).cast(),
                },
            ];
            // `atexit` is the symbol of this NID in libkernel and in libc alike.
            assert_eq!(
                ffi::runtime_lookup(table.as_ptr(), 2, c"8G2LB+A3rzg#q#q".as_ptr()),
                std::ptr::addr_of_mut!(b) as usize
            );
            assert_eq!(
                ffi::runtime_lookup(table.as_ptr(), 1, c"8G2LB+A3rzg#q#q".as_ptr()),
                0
            );
            assert_eq!(
                ffi::runtime_lookup(table.as_ptr(), 2, c"unknown".as_ptr()),
                0
            );
        }
    }

    #[test]
    fn nothing_resolves_before_the_runtime_is_enabled_or_off_x86_64() {
        // A fresh process has capability 0 until `runtime_start(1)`; tests that enable it hold the
        // lock in `libc::tests`. A name nothing exports never resolves either way.
        assert_eq!(runtime_resolve(c"unknown", false), None);
        // SAFETY: a null name is accepted and resolves to nothing.
        assert_eq!(unsafe { ffi::runtime_resolve(std::ptr::null(), 0) }, 0);
    }
}
