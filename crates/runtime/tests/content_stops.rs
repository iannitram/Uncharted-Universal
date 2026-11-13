//! The content module through its C symbols and the guest functions they hand out
//! (`tests/test_content.c`'s `--missing` case and its lifecycle): the cases that end the process
//! run in a child copy of this test binary, which must exit with the loader's code and its
//! message on stderr.
#![cfg(target_arch = "x86_64")]

use std::ffi::{c_char, c_void, CStr};
use std::process::Command;
use uncharted_runtime::content::ffi::{runtime_content_configure, runtime_content_resolve};

const CASE: &str = "UNCHARTED_CONTENT_STOP_CASE";
const APP_CONTENT: u32 = 0xb4;

type ModuleFn = extern "sysv64" fn(u32) -> i32;
type InitFn = extern "sysv64" fn(*const u8, *mut u8) -> i32;
type ParamFn = extern "sysv64" fn(u32, *mut i32) -> i32;
type ListFn = extern "sysv64" fn(u32, *mut c_void, u32, *mut u32) -> i32;

fn get(name: &CStr) -> usize {
    // SAFETY: `name` is a NUL-terminated string.
    let address = unsafe { runtime_content_resolve(name.as_ptr()) };
    assert_ne!(address, 0, "{name:?} does not resolve");
    address
}

fn load() -> ModuleFn {
    // SAFETY: `sceSysmoduleLoadModule`'s contract.
    unsafe { std::mem::transmute::<usize, ModuleFn>(get(c"g8cM39EUZ6o#M#N")) }
}

/// In the child: runs `case`, which must not return. In the parent: runs the child and checks it.
fn expect_stop(test: &str, case: fn(), code: i32, message: &str) {
    if std::env::var_os(CASE).is_some_and(|value| value == test) {
        case();
        std::process::exit(99);
    }
    let out = Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env(CASE, test)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(code), "{stderr}");
    assert!(stderr.contains(message), "{stderr}");
}

#[test]
fn app_content_without_a_profile_stops() {
    expect_stop(
        "app_content_without_a_profile_stops",
        || {
            load()(APP_CONTENT);
        },
        21,
        "STOP: AppContent needs --content-profile",
    );
}

#[test]
fn an_invalid_profile_ends_the_process() {
    expect_stop(
        "an_invalid_profile_ends_the_process",
        || {
            let values = [2u32, 0, 0, 0, 0];
            // SAFETY: five values.
            unsafe { runtime_content_configure(values.as_ptr()) };
        },
        1,
        "ERROR: invalid or late content profile",
    );
}

#[test]
fn a_missing_profile_ends_the_process() {
    expect_stop(
        "a_missing_profile_ends_the_process",
        // SAFETY: null is the documented "no profile".
        || unsafe { runtime_content_configure(std::ptr::null()) },
        1,
        "ERROR: invalid or late content profile",
    );
}

#[test]
fn a_query_before_initialization_stops() {
    expect_stop(
        "a_query_before_initialization_stops",
        || {
            let values = [1u32, 13, 0, 0, 0];
            // SAFETY: five values.
            unsafe { runtime_content_configure(values.as_ptr()) };
            assert_eq!(load()(APP_CONTENT), 0);
            // SAFETY: `sceAppContentAppParamGetInt`'s contract.
            let param: ParamFn = unsafe { std::mem::transmute(get(c"99b82IKXpH4#c#d")) };
            let mut value = 0i32;
            param(1, &mut value);
        },
        21,
        "STOP: AppContent query before initialization",
    );
}

/// The whole lifecycle through the guest functions (in this process: no other test here
/// touches the provider's state outside a child).
#[test]
fn lifecycle_through_the_guest_functions() {
    if std::env::var_os(CASE).is_some() {
        return;
    }
    // SAFETY: each NID's contract (`sceSysmodule*` and `sceAppContent*`).
    let (loaded, unload, init, param, list) = unsafe {
        (
            std::mem::transmute::<usize, ModuleFn>(get(c"fMP5NHUOaMk#M#N")),
            std::mem::transmute::<usize, ModuleFn>(get(c"eR2bZFAAU0Q#M#N")),
            std::mem::transmute::<usize, InitFn>(get(c"R9lA82OraNs#c#d")),
            std::mem::transmute::<usize, ParamFn>(get(c"99b82IKXpH4#c#d")),
            std::mem::transmute::<usize, ListFn>(get(c"xnd8BJzAxmk#c#d")),
        )
    };
    // SAFETY: a null name is answered with 0.
    assert_eq!(
        unsafe { runtime_content_resolve(std::ptr::null::<c_char>()) },
        0
    );
    assert_eq!(
        // SAFETY: a NUL-terminated name of another library.
        unsafe { runtime_content_resolve(c"g8cM39EUZ6o#I#J".as_ptr()) },
        0
    );

    let values = [3u32, 13, 0, 0, 0xffff_ffff];
    // SAFETY: five values.
    unsafe { runtime_content_configure(values.as_ptr()) };
    assert_eq!(load()(APP_CONTENT), 0);
    assert_eq!(loaded(APP_CONTENT), 0);
    // Only the low 16 bits of the module id count.
    assert_eq!(loaded(0x1_0000 | APP_CONTENT), 0);

    let zero = [0u8; 32];
    // Unaligned on purpose: the guest's buffers need not be aligned.
    let mut boot = [0xaau8; 41];
    assert_eq!(init(zero.as_ptr(), boot[1..].as_mut_ptr()), 0);
    assert!(boot[1..].iter().all(|&byte| byte == 0));
    assert_eq!(boot[0], 0xaa);
    assert_eq!(
        init(zero.as_ptr(), boot[1..].as_mut_ptr()),
        0x80d9_0003_u32 as i32
    );

    let mut value = 0i32;
    assert_eq!(param(1, &mut value), 0);
    assert_eq!(value, 13);
    assert_eq!(param(4, &mut value), 0);
    assert_eq!(value, -1);
    assert_eq!(param(5, &mut value), 0x80d9_0002_u32 as i32);
    assert_eq!(param(0, std::ptr::null_mut()), 0x80d9_0002_u32 as i32);

    let mut hits = 7u32;
    assert_eq!(list(0, std::ptr::null_mut(), 0, &mut hits), 0);
    assert_eq!(hits, 0);
    assert_eq!(
        list(0, std::ptr::null_mut(), 0, std::ptr::null_mut()),
        0x80d9_0002_u32 as i32
    );

    assert_eq!(unload(APP_CONTENT), 0);
    assert_eq!(loaded(APP_CONTENT), 0x805a_1001_u32 as i32);
    assert_eq!(unload(APP_CONTENT), 0x805a_1001_u32 as i32);
    assert_eq!(loaded(0), 0x805a_1000_u32 as i32);
    uncharted_runtime::content::ffi::runtime_content_report();
}
