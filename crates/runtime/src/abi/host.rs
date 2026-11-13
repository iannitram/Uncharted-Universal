//! What [`super::libc`] calls on the C side: C stdio (the loader's stdout is unbuffered C stdio,
//! so messages go through it in order, with the C runtime's line endings), the C memory
//! primitives the guest asked for, `exit`, and, in the loader build (`cfg(uncharted_loader)`),
//! the C modules' resolvers and reports and `runtime_quit`. The modules already in Rust
//! ([`crate::content`], [`crate::time`], [`crate::rtc`], [`crate::audio`] and [`crate::file`])
//! keep their place in the chain and the reports. Without the loader the chain holds only those,
//! and quitting flushes stdio and ends the process at once, as
//! `tests/test_runtime.c`'s `runtime_quit` did.

use std::ffi::{c_char, c_int, c_void, CStr, CString};

type File = c_void;

extern "C" {
    fn puts(text: *const c_char) -> c_int;
    fn fputs(text: *const c_char, stream: *mut File) -> c_int;
    pub(crate) fn exit(code: c_int) -> !;
    pub(super) fn calloc(count: usize, size: usize) -> *mut c_void;
    pub(super) fn memset(dst: *mut c_void, value: c_int, size: usize) -> *mut c_void;
    pub(super) fn memcpy(dst: *mut c_void, src: *const c_void, size: usize) -> *mut c_void;
    pub(super) fn memmove(dst: *mut c_void, src: *const c_void, size: usize) -> *mut c_void;
    pub(super) fn memcmp(a: *const c_void, b: *const c_void, size: usize) -> c_int;
    pub(super) fn strlen(text: *const c_char) -> usize;
}

#[cfg(windows)]
fn stderr() -> *mut File {
    extern "C" {
        fn __acrt_iob_func(index: u32) -> *mut File;
    }
    // SAFETY: the UCRT's accessor for its standard streams; index 2 is stderr.
    unsafe { __acrt_iob_func(2) }
}

#[cfg(all(unix, any(target_vendor = "apple", target_os = "freebsd")))]
fn stderr() -> *mut File {
    extern "C" {
        static __stderrp: *mut File;
    }
    // SAFETY: the C library initializes its stderr stream before any Rust code runs.
    unsafe { __stderrp }
}

#[cfg(all(unix, not(any(target_vendor = "apple", target_os = "freebsd"))))]
fn stderr() -> *mut File {
    extern "C" {
        static stderr: *mut File;
    }
    // SAFETY: the C library initializes its stderr stream before any Rust code runs.
    unsafe { stderr }
}

fn c_string(text: &str) -> CString {
    CString::new(text).unwrap_or_else(|_| CString::from(c"(message with a NUL byte)"))
}

/// `puts`: one line on the C stdout (the newline is added).
pub(crate) fn out_line(text: &str) {
    let line = c_string(text);
    // SAFETY: `line` is a NUL-terminated string.
    unsafe { puts(line.as_ptr()) };
}

/// `fputs(text, stderr)`; `text` carries its own newline.
pub(crate) fn err(text: &str) {
    let text = c_string(text);
    // SAFETY: `text` is a NUL-terminated string and `stderr()` the C library's stream.
    unsafe { fputs(text.as_ptr(), stderr()) };
}

#[cfg(uncharted_loader)]
mod loader {
    use std::ffi::{c_char, c_int};

    // Rust on Linux and Android since 3.12 (`crate::memory`); Windows keeps the C file.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub use crate::memory::linux::{runtime_memory_report, runtime_memory_resolve};

    extern "C" {
        pub fn runtime_mutex_resolve(name: *const c_char) -> usize;
        pub fn runtime_thread_resolve(name: *const c_char) -> usize;
        pub fn runtime_sema_resolve(name: *const c_char) -> usize;
        pub fn runtime_rwlock_resolve(name: *const c_char) -> usize;
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        pub fn runtime_memory_resolve(name: *const c_char) -> usize;
        pub fn runtime_kernel_resolve(name: *const c_char) -> usize;
        pub fn runtime_services_resolve(name: *const c_char) -> usize;
        pub fn runtime_ajm_resolve(name: *const c_char) -> usize;
        pub fn runtime_pad_resolve(name: *const c_char) -> usize;
        pub fn runtime_savedata_resolve(name: *const c_char) -> usize;
        pub fn runtime_fiber_resolve(name: *const c_char) -> usize;
        pub fn bbgpu_resolve(scoped_nid: *const c_char) -> usize;

        pub fn runtime_thread_report();
        pub fn runtime_mutex_report();
        pub fn runtime_rwlock_report();
        pub fn runtime_sema_report();
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        pub fn runtime_memory_report();
        pub fn runtime_ajm_report();
        pub fn runtime_pad_report();
        pub fn runtime_savedata_report();
        pub fn runtime_fiber_report();

        pub fn runtime_quit(code: c_int) -> !;
    }
}

/// The modules' resolvers in `runtime.c`'s order (the C modules and those already in Rust:
/// `content`, piece 3.04, `time` and `rtc`, piece 3.02, `audio`, piece 3.07, and `file`, piece
/// 3.09), then the GPU library: the first that knows `name` answers.
#[cfg(uncharted_loader)]
pub(super) fn resolve_chain(name: &CStr) -> Option<usize> {
    use crate::content::ffi::runtime_content_resolve;
    use crate::file::ffi::runtime_file_resolve;
    use crate::rtc::ffi::runtime_rtc_resolve;
    use crate::time::ffi::runtime_time_resolve;
    use loader::*;
    let resolvers: [unsafe extern "C" fn(*const c_char) -> usize; 18] = [
        runtime_mutex_resolve,
        runtime_thread_resolve,
        runtime_content_resolve,
        runtime_time_resolve,
        runtime_sema_resolve,
        runtime_rwlock_resolve,
        runtime_memory_resolve,
        runtime_kernel_resolve,
        // Rust since 3.05 (`crate::trophy`).
        crate::trophy::runtime_trophy_resolve,
        runtime_services_resolve,
        runtime_ajm_resolve,
        crate::audio::runtime_audio_resolve,
        runtime_pad_resolve,
        runtime_rtc_resolve,
        runtime_savedata_resolve,
        runtime_file_resolve,
        runtime_fiber_resolve,
        bbgpu_resolve,
    ];
    resolvers
        .iter()
        // SAFETY: each resolver takes a NUL-terminated name and only reads it.
        .map(|resolve| unsafe { resolve(name.as_ptr()) })
        .find(|&address| address != 0)
}

/// Without the loader only the Rust modules answer, in the same order.
#[cfg(not(uncharted_loader))]
pub(super) fn resolve_chain(name: &CStr) -> Option<usize> {
    crate::content::resolve(name)
        .or_else(|| crate::time::resolve(name))
        .or_else(|| crate::rtc::resolve(name))
        .or_else(|| crate::audio::resolve(name))
        .or_else(|| crate::file::resolve(name))
}

/// The modules' report lines, in `runtime.c`'s order.
#[cfg(uncharted_loader)]
pub(super) fn report_modules() {
    use crate::content::ffi::runtime_content_report;
    use crate::file::ffi::runtime_file_report;
    use loader::*;
    let reports: [unsafe extern "C" fn(); 12] = [
        runtime_thread_report,
        runtime_mutex_report,
        runtime_rwlock_report,
        runtime_sema_report,
        runtime_content_report,
        runtime_memory_report,
        runtime_ajm_report,
        crate::audio::runtime_audio_report,
        runtime_pad_report,
        runtime_savedata_report,
        runtime_file_report,
        runtime_fiber_report,
    ];
    for report in reports {
        // SAFETY: the reports take nothing and print their module's counters.
        unsafe { report() };
    }
}

/// Without the loader only the modules already in Rust report, in the same order.
#[cfg(not(uncharted_loader))]
pub(super) fn report_modules() {
    crate::content::ffi::runtime_content_report();
    crate::audio::runtime_audio_report();
    crate::file::report();
}

/// Ends the process: the loader's `runtime_quit` (log flushed, shader cache saved, no
/// destructors), or without the loader stdio flushed and `_exit`.
#[cfg(uncharted_loader)]
pub(crate) fn quit(code: c_int) -> ! {
    // SAFETY: `runtime_quit` takes any exit code and does not return.
    unsafe { loader::runtime_quit(code) }
}

#[cfg(not(uncharted_loader))]
pub(crate) fn quit(code: c_int) -> ! {
    extern "C" {
        fn fflush(stream: *mut File) -> c_int;
        fn _exit(code: c_int) -> !;
    }
    // SAFETY: flushing every C stream and ending the process take no Rust state.
    unsafe {
        fflush(std::ptr::null_mut());
        _exit(code)
    }
}
