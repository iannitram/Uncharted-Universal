//! What [`super::libc`] calls on the C side: C stdio (the loader's stdout is unbuffered C stdio,
//! so messages go through it in order, with the C runtime's line endings; the Rust modules print
//! through [`out_line`] too), the C memory primitives the guest asked for, `exit`, and, in the
//! loader build (`cfg(uncharted_loader)`), the C modules' resolvers and reports and
//! `runtime_quit`. The modules already in Rust (`savedata`) keep their place in the chain and
//! the reports. Without the loader the chain and the reports are those Rust modules only, and
//! quitting flushes stdio and ends the process at once, as `tests/test_runtime.c`'s
//! `runtime_quit` did.

use std::ffi::{c_char, c_int, c_void, CStr, CString};

type File = c_void;

extern "C" {
    fn puts(text: *const c_char) -> c_int;
    fn fputs(text: *const c_char, stream: *mut File) -> c_int;
    pub(super) fn exit(code: c_int) -> !;
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
pub(super) fn err(text: &str) {
    let text = c_string(text);
    // SAFETY: `text` is a NUL-terminated string and `stderr()` the C library's stream.
    unsafe { fputs(text.as_ptr(), stderr()) };
}

#[cfg(uncharted_loader)]
mod loader {
    use std::ffi::{c_char, c_int};

    extern "C" {
        pub fn runtime_mutex_resolve(name: *const c_char) -> usize;
        pub fn runtime_thread_resolve(name: *const c_char) -> usize;
        pub fn runtime_content_resolve(name: *const c_char) -> usize;
        pub fn runtime_time_resolve(name: *const c_char) -> usize;
        pub fn runtime_sema_resolve(name: *const c_char) -> usize;
        pub fn runtime_rwlock_resolve(name: *const c_char) -> usize;
        pub fn runtime_memory_resolve(name: *const c_char) -> usize;
        pub fn runtime_kernel_resolve(name: *const c_char) -> usize;
        pub fn runtime_trophy_resolve(name: *const c_char) -> usize;
        pub fn runtime_services_resolve(name: *const c_char) -> usize;
        pub fn runtime_ajm_resolve(name: *const c_char) -> usize;
        pub fn runtime_audio_resolve(name: *const c_char) -> usize;
        pub fn runtime_pad_resolve(name: *const c_char) -> usize;
        pub fn runtime_rtc_resolve(name: *const c_char) -> usize;
        pub fn runtime_file_resolve(name: *const c_char) -> usize;
        pub fn runtime_fiber_resolve(name: *const c_char) -> usize;
        pub fn bbgpu_resolve(scoped_nid: *const c_char) -> usize;

        pub fn runtime_thread_report();
        pub fn runtime_mutex_report();
        pub fn runtime_rwlock_report();
        pub fn runtime_sema_report();
        pub fn runtime_content_report();
        pub fn runtime_memory_report();
        pub fn runtime_ajm_report();
        pub fn runtime_audio_report();
        pub fn runtime_pad_report();
        pub fn runtime_file_report();
        pub fn runtime_fiber_report();

        pub fn runtime_quit(code: c_int) -> !;
    }
}

/// The C modules' resolvers in `runtime.c`'s order, then the GPU library: the first that knows
/// `name` answers.
#[cfg(uncharted_loader)]
pub(super) fn resolve_chain(name: &CStr) -> Option<usize> {
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
        runtime_trophy_resolve,
        runtime_services_resolve,
        runtime_ajm_resolve,
        runtime_audio_resolve,
        runtime_pad_resolve,
        runtime_rtc_resolve,
        crate::savedata::ffi::runtime_savedata_resolve,
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

#[cfg(not(uncharted_loader))]
pub(super) fn resolve_chain(name: &CStr) -> Option<usize> {
    crate::savedata::resolve(name)
}

/// The C modules' report lines, in `runtime.c`'s order.
#[cfg(uncharted_loader)]
pub(super) fn report_modules() {
    use loader::*;
    let reports: [unsafe extern "C" fn(); 12] = [
        runtime_thread_report,
        runtime_mutex_report,
        runtime_rwlock_report,
        runtime_sema_report,
        runtime_content_report,
        runtime_memory_report,
        runtime_ajm_report,
        runtime_audio_report,
        runtime_pad_report,
        crate::savedata::ffi::runtime_savedata_report,
        runtime_file_report,
        runtime_fiber_report,
    ];
    for report in reports {
        // SAFETY: the reports take nothing and print their module's counters.
        unsafe { report() };
    }
}

#[cfg(not(uncharted_loader))]
pub(super) fn report_modules() {
    crate::savedata::report();
}

/// Ends the process: the loader's `runtime_quit` (log flushed, shader cache saved, no
/// destructors), or without the loader stdio flushed and `_exit`.
#[cfg(uncharted_loader)]
pub(super) fn quit(code: c_int) -> ! {
    // SAFETY: `runtime_quit` takes any exit code and does not return.
    unsafe { loader::runtime_quit(code) }
}

#[cfg(not(uncharted_loader))]
pub(super) fn quit(code: c_int) -> ! {
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
