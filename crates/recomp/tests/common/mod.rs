//! Shared by the integration tests: temp folders and the test host.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use uncharted_recomp::tools;

/// A fresh folder outside any git checkout (the system temp folder).
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("uu-recomp-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

pub fn tests_recomp(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/recomp")
        .join(file)
}

/// What the test host printed.
pub struct HostRun {
    pub output: Output,
}

impl HostRun {
    pub fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.output.stdout).into_owned()
    }
    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.output.stderr).into_owned()
    }
    pub fn code(&self) -> Option<i32> {
        self.output.status.code()
    }
    /// How many functions the library replaced (`recompiled N`).
    pub fn recompiled(&self) -> usize {
        self.stdout()
            .lines()
            .find_map(|line| line.strip_prefix("recompiled "))
            .and_then(|n| n.trim().parse().ok())
            .expect("a `recompiled N` line")
    }
    /// The `call ...` and `scratch ...` lines, in order.
    pub fn results(&self) -> Vec<String> {
        self.stdout()
            .lines()
            .filter(|line| line.starts_with("call ") || line.starts_with("scratch "))
            .map(str::to_string)
            .collect()
    }
    /// The `rax` of each call, in order.
    pub fn rax(&self) -> Vec<u64> {
        self.stdout()
            .lines()
            .filter_map(|line| line.split("rax=").nth(1))
            .map(|hex| u64::from_str_radix(hex.trim(), 16).unwrap())
            .collect()
    }
}

/// Run the test host on `elf` with the interpreter only (`library` None) or with `library`.
/// `args` are host arguments (`--patch`, `--import`), then `calls` follow `--`.
pub fn run_host(elf: &Path, library: Option<&Path>, args: &[String], calls: &[String]) -> HostRun {
    let mut command = Command::new(tools::tool("uu-recomp-host").unwrap());
    command
        .env("BB_JIT", "0")
        .env_remove("BB_RECOMP_LIB")
        .arg(elf)
        .args(args)
        .arg("--")
        .args(calls);
    if let Some(library) = library {
        command.env("BB_RECOMP_LIB", library);
    }
    HostRun {
        output: command.output().expect("the test host runs"),
    }
}
