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

/// NID to symbol name for eboot and linked libc imports (diagnostics only): the C
/// `import_names.inc`, `{"bzQExy189ZI#q#q", "_init_env"}` and so on. Holds the entries of the
/// libraries already in Rust (their resolvers look their imports up here; the C table keeps the
/// rest until the port of this module moves it whole); sorted by NID so a lookup can bisect.
pub static NID_TABLE: &[(&str, &str)] = &[
    // libSceNpTrophy (crate::trophy).
    ("28xmRUFao68#l#m", "sceNpTrophyUnlockTrophy"),
    ("E1Wrwd07Lr8#l#m", "sceNpTrophyDestroyContext"),
    ("GNcF4oidY0Y#l#m", "sceNpTrophyDestroyHandle"),
    ("LHuSmO3SLd8#l#m", "sceNpTrophyGetTrophyUnlockState"),
    ("TJCAxto9SEU#l#m", "sceNpTrophyRegisterContext"),
    ("XbkjbobZlCY#l#m", "sceNpTrophyCreateContext"),
    ("YYP3f2W09og#l#m", "sceNpTrophyGetGameInfo"),
    ("q7U6tEAQf7c#l#m", "sceNpTrophyCreateHandle"),
    ("qqUVGDgQBm0#l#m", "sceNpTrophyGetTrophyInfo"),
];

/// The symbol name for `nid`, if it is in [`NID_TABLE`] (the C `runtime_symbol`).
pub fn nid_name(nid: &str) -> Option<&'static str> {
    NID_TABLE
        .binary_search_by(|(entry, _)| (*entry).cmp(nid))
        .ok()
        .map(|index| NID_TABLE[index].1)
}

/// Where a guest import `name` resolves to (the C `uintptr_t runtime_resolve(const char *name,
/// int is_data)`), asking each library's resolver in turn; `None` when nothing exports it.
/// Not `#[no_mangle]` yet: the C symbol of the same name is still linked, and the port PR
/// that deletes `src/runtime.c` adds the `extern "C"` wrapper in the same change.
pub fn runtime_resolve(name: &str, is_data: bool) -> Option<usize> {
    unimplemented!("not built yet: abi::runtime_resolve")
}

/// The runtime's ABI version. `0` meant no C symbol had moved to Rust yet; `1` since the first
/// ported module (`trophy`, 3.05). The loader checks it at start, so a stale static library
/// fails there instead of at a missing symbol.
#[no_mangle]
pub extern "C" fn uncharted_runtime_version() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_one_since_the_first_module_moved() {
        assert_eq!(uncharted_runtime_version(), 1);
    }

    #[test]
    fn the_nid_table_is_sorted_and_bisects() {
        assert!(NID_TABLE.windows(2).all(|pair| pair[0].0 < pair[1].0));
        for (nid, name) in NID_TABLE {
            assert_eq!(nid_name(nid), Some(*name));
        }
        assert_eq!(nid_name("XbkjbobZlCY#l#"), None);
        assert_eq!(nid_name(""), None);
    }
}
