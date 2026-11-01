//! The C ABI of the runtime: what `src/runtime.h` and `src/runtime.c` (266 lines) hold today,
//! the NID resolution table (`import_names.inc`) and `runtime_resolve`. Replaces them first (order
//! 1 in docs/code-map.md); `test_runtime.c` becomes this module's Rust tests.
//!
//! # The strangler plan (ADR 0002)
//!
//! This crate builds as a static library linked into the loader binary while C remains. Each
//! `src/runtime_*.c` is replaced by a Rust module that exports the SAME `extern "C"` symbols
//! (the header comes from `cbindgen` into `crates/runtime/include/`, added by piece 3.01), and
//! the C file is deleted in the same PR; its C test becomes a Rust test. Leaves first, loader
//! (`probe.c`) last, and `bash build.sh` plus the boot smoke test stay green at every step. No
//! behaviour changes in a port PR; a bug found while porting is its own issue.
//!
//! Only this crate has `unsafe` and FFI; every `unsafe` block carries a `// SAFETY:` line.

/// The runtime's ABI version. `0` means no C symbol has moved to Rust yet; the first ported
/// module raises it and the loader checks it at start. Exists so the static library links and a
/// test can call across the C ABI from day one.
#[no_mangle]
pub extern "C" fn uncharted_runtime_version() -> u32 {
    0
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_zero_until_a_module_moves() {
        assert_eq!(super::uncharted_runtime_version(), 0);
    }
}
