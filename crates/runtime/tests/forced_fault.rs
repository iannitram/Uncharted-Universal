//! The forced-fault hook of `uncharted-run` prints exactly one `Crash:` line (#168).

use std::process::Command;

#[test]
fn forced_fault_prints_exactly_one_crash_line_on_stderr() {
    let out = Command::new(env!("CARGO_BIN_EXE_uncharted-run"))
        .arg("--force-fault")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    let crashes: Vec<_> = stderr.lines().filter(|l| l.starts_with("Crash:")).collect();
    assert_eq!(
        crashes,
        ["Crash: kind=fault where=0x1234 text=read access to 0x5678 (forced test fault)"]
    );
    assert_eq!(stderr.lines().count(), 1);
}
