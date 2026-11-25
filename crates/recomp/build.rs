//! Builds the vendored C of ADR 0006 (`apple/vendor`) for Apple arm64 targets, and nothing
//! anywhere else.
//!
//! * `libuu_zydis.a`: Zydis v4.1.1 with its bundled Zycore (MIT), the x86-64 decoder.
//! * `libuu_bbcpu.a`: bbcpu's `src/cpu` (interpreter, arm64 translator, recompiled-library
//!   loader). The host it links into provides `runtime_low_map` and `bb_image_base`.
//! * On a macOS host building for macOS, the three host tools in `$OUT_DIR/tools`: `uu-scan`
//!   (function discovery), `uu-bbrecomp` (the C generator) and `uu-fuzz-recomp` (generated C
//!   against the interpreter). `uu-recomp` finds them through `UU_RECOMP_TOOLS_DIR`.
//!
//! bbcpu's marker addresses are configuration (feasibility gate 2): set `UU_BBCPU_SENTINEL_BASE`,
//! `UU_BBCPU_RECORD_SENTINEL`, `UU_BBCPU_THREAD_SENTINEL` or `UU_BBCPU_FRAME_BASE` (decimal or
//! 0x-hex) when building to move one; the defaults are bbcpu's.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The `BB_RECOMP_API_VERSION` the vendored generator and loader were written for (upstream's
/// `tools/recomp/bbcpu_api`).
const EXPECTED_API_VERSION: &str = "4";

const MARKERS: [(&str, &str); 4] = [
    ("UU_BBCPU_SENTINEL_BASE", "BBCPU_SENTINEL_BASE"),
    ("UU_BBCPU_RECORD_SENTINEL", "BBCPU_RECORD_SENTINEL"),
    ("UU_BBCPU_THREAD_SENTINEL", "BBCPU_THREAD_SENTINEL"),
    ("UU_BBCPU_FRAME_BASE", "BBCPU_FRAME_BASE"),
];

fn sources(dir: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| extensions.contains(&e))
        })
        .collect();
    found.sort();
    found
}

fn parse_address(name: &str, value: &str) -> u64 {
    let parsed = match value.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => value.parse(),
    };
    parsed.unwrap_or_else(|_| panic!("{name}={value}: not an address"))
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let apple_arm64 = env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple")
        && env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("aarch64");
    if !apple_arm64 {
        return;
    }
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let vendor = manifest.join("../../apple/vendor");
    println!("cargo:rerun-if-changed={}", vendor.display());
    for (variable, _) in MARKERS {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());

    let bbcpu = vendor.join("bbcpu/src");
    let rc = fs::read_to_string(bbcpu.join("recomp/rc.h")).expect("rc.h");
    let version = rc
        .lines()
        .find_map(|line| line.strip_prefix("#define BB_RECOMP_API_VERSION "))
        .map(|rest| {
            rest.trim_end_matches(|c: char| c == 'u' || c.is_whitespace())
                .to_string()
        });
    assert_eq!(
        version.as_deref(),
        Some(EXPECTED_API_VERSION),
        "the vendored recompiler expects BB_RECOMP_API_VERSION {EXPECTED_API_VERSION}"
    );

    let zydis = vendor.join("zydis");
    let zycore = zydis.join("dependencies/zycore");
    let includes = [
        zydis.join("include"),
        zycore.join("include"),
        zydis.join("src"),
    ];
    let zydis_defines = [("ZYDIS_STATIC_BUILD", None), ("ZYCORE_STATIC_BUILD", None)];

    let mut z = cc::Build::new();
    z.std("c11")
        .opt_level(2)
        .debug(false)
        .warnings(false)
        .extra_warnings(false);
    for dir in &includes {
        z.include(dir);
    }
    for (name, value) in zydis_defines {
        z.define(name, value);
    }
    z.files(sources(&zydis.join("src"), &["c"]));
    z.files(sources(&zycore.join("src"), &["c"]));
    z.files(sources(&zycore.join("src/API"), &["c"]));
    z.compile("uu_zydis");

    let mut markers = Vec::new();
    for (variable, define) in MARKERS {
        if let Ok(value) = env::var(variable) {
            markers.push((
                define,
                format!("UINT64_C({:#x})", parse_address(variable, &value)),
            ));
        }
    }
    let mut b = cc::Build::new();
    b.std("gnu11")
        .opt_level(2)
        .debug(false)
        .warnings(false)
        .extra_warnings(false);
    for dir in &includes {
        b.include(dir);
    }
    b.include(&bbcpu);
    for (name, value) in zydis_defines {
        b.define(name, value);
    }
    for (name, value) in &markers {
        b.define(name, Some(value.as_str()));
    }
    b.files(sources(&bbcpu.join("cpu"), &["c", "S"]));
    b.compile("uu_bbcpu");

    println!("cargo:rustc-env=UU_RECOMP_VENDOR={}", vendor.display());
    println!("cargo:rustc-env=UU_RECOMP_LIB_DIR={}", out.display());

    // The host tools run on this machine: only when it is the target (a macOS host building
    // for macOS). An iOS or cross build has the libraries and no tools.
    let tools = out.join("tools");
    println!("cargo:rustc-env=UU_RECOMP_TOOLS_DIR={}", tools.display());
    let host = env::var("HOST").unwrap();
    let target = env::var("TARGET").unwrap();
    if host != target || env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    fs::create_dir_all(&tools).unwrap();
    let recomp = vendor.join("bloodborne-recomp");
    let tool_sources = [
        ("uu-scan", recomp.join("tools/recomp/scan.c")),
        ("uu-bbrecomp", recomp.join("tools/recomp/bbrecomp.c")),
        ("uu-fuzz-recomp", recomp.join("tests/fuzz_recomp.c")),
        // The test host (tests/recomp/host.c): a minimal bbcpu host for the end-to-end tests.
        ("uu-recomp-host", manifest.join("../../tests/recomp/host.c")),
    ];
    println!(
        "cargo:rerun-if-changed={}",
        manifest.join("../../tests/recomp").display()
    );
    let compiler = cc::Build::new().opt_level(2).debug(false).get_compiler();
    for (name, source) in tool_sources {
        let mut command: Command = compiler.to_command();
        command.arg("-std=gnu11").arg("-w");
        for dir in &includes {
            command.arg(format!("-I{}", dir.display()));
        }
        command.arg(format!("-I{}", bbcpu.display()));
        for (define, _) in zydis_defines {
            command.arg(format!("-D{define}"));
        }
        for (define, value) in &markers {
            command.arg(format!("-D{define}={value}"));
        }
        command
            .arg(&source)
            .arg(out.join("libuu_bbcpu.a"))
            .arg(out.join("libuu_zydis.a"))
            .arg("-o")
            .arg(tools.join(name));
        let status = command
            .status()
            .unwrap_or_else(|e| panic!("{name}: cannot run the C compiler: {e}"));
        assert!(status.success(), "{name}: the C compiler failed");
    }
}
