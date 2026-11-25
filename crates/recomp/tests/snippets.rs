//! The snippet fuzz: authored encodings (tests/recomp/encodings.txt) through `uu-bbrecomp
//! --snippets`, compiled, then each one run from random states through the interpreter and
//! through its generated C (`uu-fuzz-recomp`). Zero mismatches, zero crashes.
#![cfg(all(target_vendor = "apple", target_arch = "aarch64"))]

mod common;

use common::{temp_dir, tests_recomp};
use std::process::Command;
use uncharted_recomp::tools;

#[test]
fn generated_c_matches_the_interpreter_instruction_by_instruction() {
    let work = temp_dir("snippets");
    let encodings = tests_recomp("encodings.txt");
    let lines = std::fs::read_to_string(&encodings)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .count();
    assert!(lines >= 150, "the corpus is {lines} lines");

    let c = work.join("snippets.c");
    let generated = Command::new(tools::tool("uu-bbrecomp").unwrap())
        .env("BB_JIT", "0")
        .args(["--snippets"])
        .arg(&encodings)
        .arg(&c)
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let said = String::from_utf8_lossy(&generated.stdout).into_owned();
    let translated: usize = said
        .lines()
        .find_map(|l| {
            l.split(" instructions translated natively")
                .next()
                .filter(|_| l.contains("natively"))
        })
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or_else(|| panic!("no count in {said:?}"));
    assert!(
        translated >= 100,
        "only {translated} instructions were translated natively"
    );

    let library = work.join("libsnippets.dylib");
    let mut compile = Command::new("clang");
    compile
        .args([
            "-dynamiclib",
            "-std=gnu11",
            "-O1",
            "-Wno-unused-label",
            "-Wno-unused-variable",
        ])
        .args([
            "-ffp-contract=off",
            "-fno-strict-aliasing",
            "-DZYDIS_STATIC_BUILD",
            "-DZYCORE_STATIC_BUILD",
        ])
        .args(
            tools::include_dirs()
                .iter()
                .map(|d| format!("-I{}", d.display())),
        )
        .arg(&c)
        .arg("-o")
        .arg(&library);
    let compiled = compile.output().unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let fuzz = Command::new(tools::tool("uu-fuzz-recomp").unwrap())
        .env("BB_JIT", "0")
        .arg(&encodings)
        .arg(&library)
        .args(["300", "20"])
        .output()
        .unwrap();
    let out =
        String::from_utf8_lossy(&fuzz.stdout).into_owned() + &String::from_utf8_lossy(&fuzz.stderr);
    assert!(
        fuzz.status.success(),
        "the fuzz reported mismatches:\n{out}"
    );
    assert!(
        out.contains(&format!(
            "{translated} instructions tested, 0 failed, 0 crashed"
        )),
        "{out}"
    );
}
