//! Where the host tools and the generated code's headers are.

use std::path::PathBuf;

/// The folder holding `uu-scan`, `uu-bbrecomp` and `uu-fuzz-recomp`: `UU_RECOMP_TOOLS` if set,
/// else the one `build.rs` filled when this crate was built.
pub fn tools_dir() -> PathBuf {
    std::env::var_os("UU_RECOMP_TOOLS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("UU_RECOMP_TOOLS_DIR")))
}

/// The vendored sources: `UU_RECOMP_VENDOR` if set, else `apple/vendor` of the checkout this
/// crate was built from.
pub fn vendor_dir() -> PathBuf {
    std::env::var_os("UU_RECOMP_VENDOR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("UU_RECOMP_VENDOR")))
}

/// The include folders generated C is compiled against: bbcpu's `src` (`recomp/rc.h`) and the
/// Zydis and Zycore headers its `cpu_internal.h` includes.
pub fn include_dirs() -> Vec<PathBuf> {
    let vendor = vendor_dir();
    vec![
        vendor.join("bbcpu/src"),
        vendor.join("zydis/include"),
        vendor.join("zydis/dependencies/zycore/include"),
    ]
}

/// One host tool by name; `Err` names the missing file.
pub fn tool(name: &str) -> Result<PathBuf, String> {
    let path = tools_dir().join(name);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!(
            "{} is missing: the host tools are built for a macOS host only (cargo build -p uncharted_recomp)",
            path.display()
        ))
    }
}
