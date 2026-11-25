use std::process::Command;

#[test]
fn uncharted_run_probe_force_fault_and_list_missing() {
    let bin = env!("CARGO_BIN_EXE_uncharted-run");

    // 1. Bad args exit 2
    let bad = Command::new(bin).output().unwrap();
    assert_eq!(bad.status.code(), Some(2));

    // 2. --probe succeeds with JSON on Vulkan-capable hosts, or exits 1 with a clean
    //    diagnostic on headless runners without a Vulkan loader/device.
    let probe = Command::new(bin).arg("--probe").output().unwrap();
    match probe.status.code() {
        Some(0) => {
            let stdout = String::from_utf8_lossy(&probe.stdout);
            assert!(
                stdout.trim().starts_with('{'),
                "expected JSON probe report, got: {stdout}"
            );
        }
        Some(1) => {
            let stderr = String::from_utf8_lossy(&probe.stderr);
            assert!(
                stderr.contains("uncharted-run:"),
                "expected clean probe diagnostic on stderr, got: {stderr}"
            );
        }
        other => panic!(
            "unexpected --probe exit code {other:?}; stderr: {}",
            String::from_utf8_lossy(&probe.stderr)
        ),
    }

    // 3. --force-fault exits non-zero and prints exactly one Crash: line
    let fault = Command::new(bin).arg("--force-fault").output().unwrap();
    assert!(!fault.status.success());
    let stderr = String::from_utf8_lossy(&fault.stderr);
    let crash_lines: Vec<&str> = stderr.lines().filter(|l| l.starts_with("Crash:")).collect();
    assert_eq!(crash_lines.len(), 1, "stderr: {stderr}");

    // 4. --list-missing on a synthetic BBPROBE2 boot image with 0 missing imports
    let mut img = Vec::new();
    img.extend_from_slice(b"BBPROBE2");
    img.extend_from_slice(&4096u64.to_le_bytes()); // size
    img.extend_from_slice(&0u64.to_le_bytes()); // entry
    img.extend_from_slice(&1u64.to_le_bytes()); // n_segments
    img.extend_from_slice(&0u64.to_le_bytes()); // n_relocs
    img.extend_from_slice(&0u64.to_le_bytes()); // n_imports
    img.extend_from_slice(&1u64.to_le_bytes()); // capabilities
    img.extend_from_slice(&0u64.to_le_bytes()); // seg 0 offset
    img.extend_from_slice(&4096u64.to_le_bytes()); // seg 0 size
    img.extend_from_slice(&7u64.to_le_bytes()); // seg 0 prot (rwx)
    img.resize(img.len() + 4096, 0);

    let path = std::env::temp_dir().join(format!("uncharted-run-test-{}.bin", std::process::id()));
    std::fs::write(&path, &img).unwrap();
    let list = Command::new(bin)
        .args([path.to_str().unwrap(), "--list-missing"])
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(list.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&list.stdout).contains("0 of 0 imports missing"));
}

// The BB_BENCHMARK check is in the guest boot path, which only x86-64 Linux and Windows build;
// elsewhere uncharted-run exits 3 before reaching it.
#[cfg(all(
    target_arch = "x86_64",
    any(target_os = "linux", target_os = "windows")
))]
#[test]
fn uncharted_run_rejects_invalid_bb_benchmark() {
    let bin = env!("CARGO_BIN_EXE_uncharted-run");
    let mut img = Vec::new();
    img.extend_from_slice(b"BBPROBE2");
    img.extend_from_slice(&4096u64.to_le_bytes()); // size
    img.extend_from_slice(&0u64.to_le_bytes()); // entry
    img.extend_from_slice(&1u64.to_le_bytes()); // n_segments
    img.extend_from_slice(&0u64.to_le_bytes()); // n_relocs
    img.extend_from_slice(&0u64.to_le_bytes()); // n_imports
    img.extend_from_slice(&1u64.to_le_bytes()); // capabilities
    img.extend_from_slice(&0u64.to_le_bytes()); // seg 0 offset
    img.extend_from_slice(&4096u64.to_le_bytes()); // seg 0 size
    img.extend_from_slice(&7u64.to_le_bytes()); // seg 0 prot (rwx)
    img.resize(img.len() + 4096, 0);

    let path = std::env::temp_dir().join(format!(
        "uncharted-run-bench-invalid-{}.bin",
        std::process::id()
    ));
    let user_dir =
        std::env::temp_dir().join(format!("uncharted-run-bench-user-{}", std::process::id()));
    std::fs::write(&path, &img).unwrap();
    let out = Command::new(bin)
        .args([
            path.to_str().unwrap(),
            "--cpu-only",
            "--user",
            user_dir.to_str().unwrap(),
        ])
        .env("BB_BENCHMARK", "0")
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir_all(&user_dir);

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("BB_BENCHMARK is set but cannot run"),
        "expected benchmark startup failure, got: {stderr}"
    );
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
#[test]
fn uncharted_run_starts_valid_bb_benchmark_and_exits_on_guest_fault() {
    let bin = env!("CARGO_BIN_EXE_uncharted-run");
    let mut img = Vec::new();
    img.extend_from_slice(b"BBPROBE2");
    img.extend_from_slice(&4096u64.to_le_bytes()); // size
    img.extend_from_slice(&0u64.to_le_bytes()); // entry
    img.extend_from_slice(&1u64.to_le_bytes()); // n_segments
    img.extend_from_slice(&0u64.to_le_bytes()); // n_relocs
    img.extend_from_slice(&0u64.to_le_bytes()); // n_imports
    img.extend_from_slice(&1u64.to_le_bytes()); // capabilities
    img.extend_from_slice(&0u64.to_le_bytes()); // seg 0 offset
    img.extend_from_slice(&4096u64.to_le_bytes()); // seg 0 size
    img.extend_from_slice(&7u64.to_le_bytes()); // seg 0 prot (rwx)
                                                // ud2 at entry -> SIGILL (128 + 4 = 132)
    let mut code = vec![0x0fu8, 0x0b];
    code.resize(4096, 0);
    img.extend_from_slice(&code);

    let path = std::env::temp_dir().join(format!(
        "uncharted-run-bench-valid-{}.bin",
        std::process::id()
    ));
    let user_dir = std::env::temp_dir().join(format!(
        "uncharted-run-bench-valid-user-{}",
        std::process::id()
    ));
    std::fs::write(&path, &img).unwrap();
    let out = Command::new(bin)
        .args([
            path.to_str().unwrap(),
            "--cpu-only",
            "--user",
            user_dir.to_str().unwrap(),
        ])
        .env("BB_BENCHMARK", "5")
        .env("BB_EXE", "u2")
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir_all(&user_dir);

    assert_eq!(out.status.code(), Some(132));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Benchmark: on (BB_BENCHMARK)"),
        "expected benchmark startup banner, got: {stdout}"
    );
}
