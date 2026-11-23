//! End-to-end integration tests for `BB_BENCHMARK` lifecycle, pad wiring, and `benchmark.json`.

use std::ffi::CString;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use uncharted_core::tune;
use uncharted_runtime::benchmark_run::{
    session_from, uncharted_benchmark_active, uncharted_benchmark_crash, uncharted_benchmark_frame,
    uncharted_benchmark_pad, uncharted_benchmark_start, BenchmarkPad, Progress,
};
use uncharted_runtime::pad::camera::{Camera, Knobs};
use uncharted_runtime::pad::host;
use uncharted_runtime::pad::input::Settings;
use uncharted_runtime::pad::read::Pad;
use uncharted_runtime::pad::EntryScript;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("uu-bench-lifecycle-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn c_exports_and_pad_host_hook_follow_bb_benchmark_lifecycle() {
    let dir = TempDir::new();
    let saves = CString::new(dir.0.to_str().unwrap()).unwrap();

    // 1. Unset BB_BENCHMARK: inactive, Pad does not take benchmark path.
    std::env::remove_var("BB_BENCHMARK");
    std::env::remove_var("BB_EXE");
    assert_eq!(unsafe { uncharted_benchmark_start(saves.as_ptr()) }, 0);
    assert_eq!(uncharted_benchmark_active(), 0);
    let mut pad_out = BenchmarkPad {
        buttons: 0x1234,
        ..Default::default()
    };
    assert_eq!(unsafe { uncharted_benchmark_pad(&mut pad_out) }, 0);
    assert_eq!(pad_out.buttons, 0x1234);
    assert_eq!(uncharted_benchmark_frame(512), 0);

    // 2. Invalid BB_BENCHMARK: fails launch (-1) and leaves benchmark inactive.
    std::env::set_var("BB_BENCHMARK", "0");
    assert_eq!(unsafe { uncharted_benchmark_start(saves.as_ptr()) }, -1);
    assert_eq!(uncharted_benchmark_active(), 0);

    // 3. Valid BB_BENCHMARK: activates C exports and routes Pad::read_state through
    //    Host::benchmark_pad -> uncharted_benchmark_pad.
    std::env::set_var("BB_BENCHMARK", "5");
    std::env::set_var("BB_EXE", "u3");
    assert_eq!(unsafe { uncharted_benchmark_start(saves.as_ptr()) }, 1);
    assert_eq!(uncharted_benchmark_active(), 1);

    let mut pad = Pad::new(
        host::system(),
        Settings::default(),
        Arc::new(Mutex::new(Camera::new(Knobs::default()))),
    );
    assert_eq!(pad.init(), 0);
    assert_eq!(pad.open(1, 0, 0), 1);
    let data = pad.read_state(1).unwrap();
    assert_eq!(data.connected, 1);
    assert_eq!(data.connected_count, 1);
    assert_eq!(data.buttons, 0);
    assert_eq!(
        (data.left_x, data.left_y, data.right_x, data.right_y),
        (128, 128, 128, 128)
    );

    // Frame during lead-in does not finish or write benchmark.json.
    assert_eq!(uncharted_benchmark_frame(2048), 0);
    uncharted_benchmark_crash();
    assert!(!dir.0.join("benchmark.json").exists());

    // 4. Re-calling start with BB_BENCHMARK unset resets active state cleanly.
    std::env::remove_var("BB_BENCHMARK");
    std::env::remove_var("BB_EXE");
    assert_eq!(unsafe { uncharted_benchmark_start(saves.as_ptr()) }, 0);
    assert_eq!(uncharted_benchmark_active(), 0);
    assert_eq!(unsafe { uncharted_benchmark_pad(&mut pad_out) }, 0);
}

#[test]
fn session_writes_tune_compatible_benchmark_json_on_expiry_and_crash() {
    let dir = TempDir::new();
    let mut session = session_from(
        Some("10"),
        Some("eboot.bin"),
        &dir.0,
        [
            ("BB_RESOLUTION".into(), "1920x1080".into()),
            ("BB_UPSCALER".into(), "fsr3".into()),
            ("BB_UPSCALE_QUALITY".into(), "balanced".into()),
            ("BB_USER_DIR".into(), "/private/user/path".into()),
        ],
    )
    .unwrap()
    .unwrap();

    let origin = 1_000_000u64;
    session.pad(origin);
    let measure_start = origin + EntryScript::DONE_MS * 1_000;
    assert_eq!(
        session.frame(measure_start - 10_000, 9_999).unwrap(),
        Progress::Running
    );

    for i in 0..60u64 {
        let now = measure_start + i * 16_667;
        let _ = session.pad(now);
        let vram = if i == 30 { 4_096 } else { 2_048 };
        let now = if i == 45 { now + 30_000 } else { now };
        if i > 0 {
            assert_eq!(session.frame(now, vram).unwrap(), Progress::Running);
        }
    }

    assert_eq!(
        session.frame(measure_start + 10_000_000, 1).unwrap(),
        Progress::Finished
    );
    let report_bytes = fs::read(dir.0.join("benchmark.json")).unwrap();
    let report_str = std::str::from_utf8(&report_bytes).unwrap();
    let parsed = tune::parse_bench(report_str).unwrap();
    assert_eq!(parsed.game, "u1");
    assert_eq!(parsed.seconds, 10);
    assert!(parsed.finished);
    assert!(parsed.avg_fps > 0.0);

    let json: serde_json::Value = serde_json::from_str(report_str).unwrap();
    assert_eq!(json["vram_peak_mib"], 4_096);
    assert_eq!(json["stalls_over_40ms"], 1);
    assert_eq!(
        json["settings"],
        serde_json::json!({
            "BB_RESOLUTION": "1920x1080",
            "BB_UPSCALER": "fsr3",
            "BB_UPSCALE_QUALITY": "balanced",
        })
    );

    // Crash after sampling overwrites with ended: "crash" and tune rejects it as unfinished.
    let crash_dir = TempDir::new();
    let mut crashed = session_from(Some("10"), Some("u2"), &crash_dir.0, [])
        .unwrap()
        .unwrap();
    crashed.pad(origin);
    for i in 0..5u64 {
        crashed.frame(measure_start + i * 16_000, 1_536).unwrap();
    }
    crashed.crash().unwrap();
    let crash_str = fs::read_to_string(crash_dir.0.join("benchmark.json")).unwrap();
    let crash_parsed = tune::parse_bench(&crash_str).unwrap();
    assert_eq!(crash_parsed.game, "u2");
    assert!(!crash_parsed.finished);
}
