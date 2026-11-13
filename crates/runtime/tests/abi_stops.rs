//! The guest requests that end the process (`tests/test_runtime.c`'s `--guard-recursion`,
//! `--bad-tls` and `--stack-failure`): each case runs in a child copy of this test binary, which
//! must exit with the loader's code and its `STOP` line on stderr.
#![cfg(target_arch = "x86_64")]

use std::ffi::{c_int, c_void, CStr};
use std::process::Command;
use uncharted_runtime::abi::{libc, runtime_resolve};

const CASE: &str = "UNCHARTED_ABI_STOP_CASE";

fn get(name: &CStr) -> usize {
    runtime_resolve(name, false).unwrap_or_else(|| panic!("{name:?} does not resolve"))
}

/// In the child: runs `case`, which must not return. In the parent: runs the child and checks it.
fn expect_stop(test: &str, case: fn(), code: i32, message: &str) {
    if std::env::var_os(CASE).is_some_and(|value| value == test) {
        libc::runtime_start(1);
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
fn recursive_guard_fails_explicitly() {
    expect_stop(
        "recursive_guard_fails_explicitly",
        || {
            // SAFETY: the `__cxa_guard_acquire` contract.
            let acquire: extern "sysv64" fn(*mut u64) -> c_int =
                unsafe { std::mem::transmute(get(c"3GPpjQdAMTw#q#q")) };
            let mut guard = 0u64;
            acquire(&mut guard);
            acquire(&mut guard);
        },
        21,
        "STOP: recursive/concurrent static initialization is not supported yet",
    );
}

#[test]
fn tls_bounds_are_checked() {
    expect_stop(
        "tls_bounds_are_checked",
        || {
            static DATA: [u8; 4] = [0; 4];
            libc::runtime_set_libc_tls(DATA.as_ptr().cast::<c_void>(), 4, 8);
            // SAFETY: the `__tls_get_addr` contract.
            let tls: extern "sysv64" fn(*const u64) -> *mut c_void =
                unsafe { std::mem::transmute(get(c"vNe1w4diLCs#p#J")) };
            tls([2, 8].as_ptr());
        },
        21,
        "STOP: unsupported TLS module/offset",
    );
}

#[test]
fn unknown_tls_module_is_refused() {
    expect_stop(
        "unknown_tls_module_is_refused",
        || libc::runtime_set_module_tls(8, std::ptr::null(), 0, 0),
        21,
        "STOP: unsupported TLS module id",
    );
}

#[test]
fn stack_corruption_is_not_ignored() {
    expect_stop(
        "stack_corruption_is_not_ignored",
        || {
            // SAFETY: `__stack_chk_fail` takes nothing and does not return.
            let fail: extern "sysv64" fn() =
                unsafe { std::mem::transmute(get(c"Ou3iL1abvng#p#J")) };
            fail();
        },
        22,
        "STOP: guest stack protector detected corruption",
    );
}

#[test]
fn guest_exit_runs_handlers_reports_and_quits_with_its_status() {
    expect_stop(
        "guest_exit_runs_handlers_reports_and_quits_with_its_status",
        || {
            extern "sysv64" fn handler() {
                eprintln!("exit handler ran");
            }
            type Register = extern "sysv64" fn(extern "sysv64" fn()) -> c_int;
            type Exit = extern "sysv64" fn(c_int);
            // SAFETY: the `atexit` and `exit` contracts.
            let (register, exit) = unsafe {
                (
                    std::mem::transmute::<usize, Register>(get(c"8G2LB+A3rzg#q#q")),
                    std::mem::transmute::<usize, Exit>(get(c"uMei1W9uyNo#q#q")),
                )
            };
            assert_eq!(register(handler), 0);
            exit(7);
        },
        7,
        "exit handler ran",
    );
}
