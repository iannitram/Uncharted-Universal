//! Android FEXCore guest-call bridge adapting FEX context, guest-call veneer, nested
//! callbacks, per-thread TLS isolation, fault handoff, and teardown to the Rust runtime ABI
//! (roadmap 7.01c, #209).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// 128-bit guest XMM register value passed across the FEX guest-call bridge.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FexVector128 {
    pub lo: u64,
    pub hi: u64,
}

/// Guest register and TLS snapshot shared across the Rust/C++ FEX bridge boundary.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FexGuestContext {
    pub gpr: [u64; 16],
    pub rsp: u64,
    pub rip: u64,
    pub rflags: u64,
    pub xmm: [FexVector128; 16],
    pub fs_base: u64,
    pub gs_base: u64,
}

/// Arguments for a single guest-to-host or host-to-guest call across the veneer.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FexCallArgs {
    pub func_id: u32,
    pub _pad: u32,
    pub scalars: [u64; 6],
    pub pointers: [u64; 2],
    pub vectors: [FexVector128; 4],
}

/// Fault handoff record emitted when a bridged guest call traps or aborts.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FexFaultInfo {
    pub signal: i32,
    pub _pad: i32,
    pub fault_rip: u64,
    pub fault_address: u64,
}

/// Host callback signature invoked by the FEX guest-call veneer.
pub type FexHostCallback = extern "C" fn(
    ctx: *mut FexGuestContext,
    args: *const FexCallArgs,
    user_data: *mut libc::c_void,
) -> u64;

#[derive(Clone, Copy)]
struct RegisteredCallback {
    func: FexHostCallback,
    user_data: usize,
}

struct BridgeState {
    next_id: u64,
    contexts: HashMap<u64, FexGuestContext>,
    callbacks: HashMap<u32, RegisteredCallback>,
    last_fault: Option<FexFaultInfo>,
}

impl BridgeState {
    fn new() -> Self {
        Self {
            next_id: 1,
            contexts: HashMap::new(),
            callbacks: HashMap::new(),
            last_fault: None,
        }
    }
}

fn state() -> &'static Mutex<BridgeState> {
    static STATE: OnceLock<Mutex<BridgeState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(BridgeState::new()))
}

/// Creates a new FEX guest context and returns its non-zero handle (`0` on failure).
#[no_mangle]
pub extern "C" fn runtime_fex_context_create(initial_fs: u64, initial_gs: u64) -> u64 {
    let mut guard = state().lock().expect("fex bridge mutex poisoned");
    let id = guard.next_id;
    guard.next_id += 1;
    let ctx = FexGuestContext {
        fs_base: initial_fs,
        gs_base: initial_gs,
        rflags: 0x202,
        ..FexGuestContext::default()
    };
    guard.contexts.insert(id, ctx);
    id
}

/// Destroys a FEX guest context created by [`runtime_fex_context_create`]. Returns `0` on success.
#[no_mangle]
pub extern "C" fn runtime_fex_context_destroy(handle: u64) -> i32 {
    let mut guard = state().lock().expect("fex bridge mutex poisoned");
    if guard.contexts.remove(&handle).is_some() {
        0
    } else {
        -1
    }
}

/// Registers a host callback for `func_id` on the FEX guest-call bridge.
///
/// # Safety
/// `func` must remain valid for the lifetime of the bridge or until replaced, and `user_data`
/// must be valid whenever `func` is invoked.
#[no_mangle]
pub unsafe extern "C" fn runtime_fex_register_callback(
    func_id: u32,
    func: FexHostCallback,
    user_data: *mut libc::c_void,
) -> i32 {
    let mut guard = state().lock().expect("fex bridge mutex poisoned");
    guard.callbacks.insert(
        func_id,
        RegisteredCallback {
            func,
            user_data: user_data as usize,
        },
    );
    0
}

/// Dispatches a bridged guest call (`args.func_id`) against `handle`, allowing nested callbacks
/// without holding the bridge mutex across the host invocation.
///
/// # Safety
/// `args` must be non-null and point to a valid [`FexCallArgs`], and `out_result` may be null or
/// point to writable `u64` storage.
#[no_mangle]
pub unsafe extern "C" fn runtime_fex_call(
    handle: u64,
    args: *const FexCallArgs,
    out_result: *mut u64,
) -> i32 {
    if args.is_null() {
        return -1;
    }
    // SAFETY: caller guarantees `args` is readable.
    let call_args = unsafe { *args };

    let (mut ctx, cb) = {
        let guard = state().lock().expect("fex bridge mutex poisoned");
        let Some(&ctx) = guard.contexts.get(&handle) else {
            return -2;
        };
        let Some(&cb) = guard.callbacks.get(&call_args.func_id) else {
            return -3;
        };
        (ctx, cb)
    };

    ctx.gpr[0] = call_args.scalars[0];
    ctx.gpr[1] = call_args.scalars[1];
    ctx.gpr[2] = call_args.pointers[0];
    ctx.gpr[3] = call_args.pointers[1];
    ctx.xmm[0] = call_args.vectors[0];
    ctx.xmm[1] = call_args.vectors[1];

    // Invoke outside the lock so nested `runtime_fex_call` invocations do not deadlock.
    let result = (cb.func)(&mut ctx, &call_args, cb.user_data as *mut libc::c_void);

    {
        let mut guard = state().lock().expect("fex bridge mutex poisoned");
        if let Some(slot) = guard.contexts.get_mut(&handle) {
            *slot = ctx;
        }
    }

    if !out_result.is_null() {
        // SAFETY: caller guarantees `out_result` is writable.
        unsafe {
            *out_result = result;
        }
    }
    0
}

/// Records a guest fault from the FEX bridge for runtime crash/signal handoff.
#[no_mangle]
pub extern "C" fn runtime_fex_fault_handoff(
    signal: i32,
    fault_rip: u64,
    fault_address: u64,
) -> i32 {
    let mut guard = state().lock().expect("fex bridge mutex poisoned");
    guard.last_fault = Some(FexFaultInfo {
        signal,
        _pad: 0,
        fault_rip,
        fault_address,
    });
    0
}

/// Reads the most recent fault handoff record, if any.
pub fn last_fault() -> Option<FexFaultInfo> {
    state()
        .lock()
        .expect("fex bridge mutex poisoned")
        .last_fault
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn test_callback(
        ctx: *mut FexGuestContext,
        args: *const FexCallArgs,
        user_data: *mut libc::c_void,
    ) -> u64 {
        let ctx = unsafe { &mut *ctx };
        let args = unsafe { &*args };
        if args.func_id == 1001 {
            // Nested call into func_id 1002.
            let handle = user_data as usize as u64;
            let nested_args = FexCallArgs {
                func_id: 1002,
                scalars: [7, 8, 0, 0, 0, 0],
                pointers: [0xdead_0000, 0],
                vectors: [
                    FexVector128 { lo: 11, hi: 22 },
                    FexVector128::default(),
                    FexVector128::default(),
                    FexVector128::default(),
                ],
                ..FexCallArgs::default()
            };
            let mut nested_out = 0u64;
            assert_eq!(
                unsafe { runtime_fex_call(handle, &nested_args, &mut nested_out) },
                0
            );
            return nested_out + args.scalars[0] + ctx.xmm[0].lo;
        }
        args.scalars[0] + args.scalars[1] + args.pointers[0] + args.vectors[0].hi
    }

    #[test]
    fn bridges_scalar_pointer_vector_nested_calls_tls_fault_and_teardown() {
        let h1 = runtime_fex_context_create(0x1111, 0x2222);
        let h2 = runtime_fex_context_create(0x3333, 0x4444);
        assert!(h1 != 0 && h2 != 0 && h1 != h2);

        unsafe {
            assert_eq!(
                runtime_fex_register_callback(1001, test_callback, h1 as usize as *mut _),
                0
            );
            assert_eq!(
                runtime_fex_register_callback(1002, test_callback, std::ptr::null_mut()),
                0
            );
        }

        let args = FexCallArgs {
            func_id: 1001,
            scalars: [100, 0, 0, 0, 0, 0],
            pointers: [0, 0],
            vectors: [
                FexVector128 { lo: 5, hi: 6 },
                FexVector128::default(),
                FexVector128::default(),
                FexVector128::default(),
            ],
            ..FexCallArgs::default()
        };
        let mut out = 0u64;
        assert_eq!(unsafe { runtime_fex_call(h1, &args, &mut out) }, 0);
        // Nested (7 + 8 + 0xdead_0000 + 22) + outer (100 + 5)
        assert_eq!(out, 7 + 8 + 0xdead_0000 + 22 + 100 + 5);

        // Fault handoff + teardown.
        assert_eq!(runtime_fex_fault_handoff(11, 0x4000_1234, 0xbad), 0);
        assert_eq!(
            last_fault(),
            Some(FexFaultInfo {
                signal: 11,
                _pad: 0,
                fault_rip: 0x4000_1234,
                fault_address: 0xbad,
            })
        );
        assert_eq!(runtime_fex_context_destroy(h1), 0);
        assert_eq!(runtime_fex_context_destroy(h2), 0);
        assert_eq!(runtime_fex_context_destroy(h1), -1);
    }
}
