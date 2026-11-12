//! Benchmark reports into the three cards (`uncharted_core::tune`, issues #173 and #174).

use serde_json::{json, Value};
use uncharted_core::tune::{
    bench_from, cards_from, parse_bench, parse_probe, pick_for, Bench, BenchReport,
};

/// A report in the format of #173, as `benchmark::Report::to_json` in the runtime writes it.
fn report(avg_fps: f64, ended: &str) -> Value {
    json!({
        "game": "u2", "seconds": 90,
        "settings": {"BB_RESOLUTION": "1280x720", "BB_UPSCALE_QUALITY": "quality"},
        "frame_ms": {"p50": 16.4, "p95": 19.8, "p99": 24.0},
        "avg_fps": avg_fps, "vram_peak_mib": 5120, "stalls_over_40ms": 3, "ended": ended,
    })
}

fn text(value: &Value) -> String {
    value.to_string()
}

/// The Strix Halo tablet's probe report at 120 Hz.
const STRIX_HALO: &str = r#"{"vendor":"amd", "name":"AMD Radeon 8060S Graphics (RADV STRIX_HALO)",
    "generation":"rdna3", "vram_mib":32768, "shared_memory":true, "integrated_gpu":true,
    "hybrid_graphics":false, "refresh_hz":120, "cpu_cores":16, "cpu_supported":true}"#;

#[test]
fn the_report_of_173_parses() {
    let got = parse_bench(&text(&report(59.1, "ok"))).unwrap();
    assert_eq!(
        got,
        BenchReport {
            game: "u2".into(),
            seconds: 90,
            avg_fps: 59.1,
            finished: true,
        }
    );
    assert!(!parse_bench(&text(&report(12.0, "crash"))).unwrap().finished);
}

#[test]
fn extra_keys_are_ignored() {
    let mut value = report(30.0, "ok");
    value["future"] = json!({"x": 1});
    assert_eq!(parse_bench(&text(&value)).unwrap().avg_fps, 30.0);
}

#[test]
fn a_damaged_report_is_a_fixed_launch_error() {
    let mut broken = vec![
        "{not json".to_owned(),
        String::new(),
        "[]".to_owned(),
        "5".to_owned(),
    ];
    let edits: [(&str, Value); 13] = [
        ("game", json!("u4")),
        ("game", json!(2)),
        ("seconds", json!(0)),
        ("seconds", json!(-5)),
        ("seconds", json!(1.5)),
        ("avg_fps", json!(0.0)),
        ("avg_fps", json!(-1.0)),
        ("avg_fps", json!("59")),
        ("ended", json!("credits")),
        ("frame_ms", json!({"p50": 16.0, "p95": 18.0})),
        ("settings", json!([])),
        ("vram_peak_mib", json!(-1)),
        ("stalls_over_40ms", json!(null)),
    ];
    for (key, value) in edits {
        let mut edited = report(60.0, "ok");
        edited[key] = value;
        broken.push(text(&edited));
    }
    for key in [
        "game",
        "seconds",
        "settings",
        "frame_ms",
        "avg_fps",
        "vram_peak_mib",
        "stalls_over_40ms",
        "ended",
    ] {
        let mut missing = report(60.0, "ok");
        missing.as_object_mut().unwrap().remove(key);
        broken.push(text(&missing));
    }
    for json in broken {
        let err = parse_bench(&json).unwrap_err();
        assert_eq!(err.code(), "launch", "{json}");
        assert!(err.to_string().contains("benchmark report is damaged"));
        assert!(!err.to_string().contains("BB_RESOLUTION"));
    }
}

#[test]
fn one_report_per_card_fills_the_bench() {
    let (h, b, m) = (
        text(&report(31.0, "ok")),
        text(&report(52.5, "ok")),
        text(&report(61.0, "ok")),
    );
    assert_eq!(
        bench_from(Some(&h), Some(&b), Some(&m)).unwrap(),
        Bench {
            handheld30: Some(31.0),
            balanced60: Some(52.5),
            max: Some(61.0),
        }
    );
    assert_eq!(bench_from(None, None, None).unwrap(), Bench::default());
    assert_eq!(
        bench_from(None, Some(&b), None).unwrap(),
        Bench {
            balanced60: Some(52.5),
            ..Bench::default()
        }
    );
}

#[test]
fn a_crashed_run_is_not_benchmarked() {
    let crashed = text(&report(12.0, "crash"));
    assert_eq!(
        bench_from(Some(&crashed), None, None).unwrap(),
        Bench::default()
    );
}

#[test]
fn a_damaged_report_fails_the_bench() {
    let good = text(&report(60.0, "ok"));
    let err = bench_from(Some(&good), Some("{not json"), None).unwrap_err();
    assert!(err.to_string().contains("benchmark report is damaged"));
}

#[test]
fn reports_of_different_games_fail_the_bench() {
    let u2 = text(&report(60.0, "ok"));
    let mut u3 = report(60.0, "ok");
    u3["game"] = json!("u3");
    let err = bench_from(Some(&u2), None, Some(&text(&u3))).unwrap_err();
    assert_eq!(err.code(), "launch");
    assert!(err.to_string().contains("different games"));
}

#[test]
fn probe_then_bench_then_pick_for() {
    let slow = text(&report(50.0, "ok"));
    let cards = cards_from(STRIX_HALO, None, Some(&slow), None).unwrap();
    let expected = pick_for(
        &parse_probe(STRIX_HALO).unwrap(),
        &Bench {
            balanced60: Some(50.0),
            ..Bench::default()
        },
    );
    assert_eq!(cards, expected);
    // 50 is below 95% of 60: Balanced 60 steps from the shared-memory "balanced" down one.
    assert!(cards.balanced60.stepped_down);
    assert_eq!(cards.balanced60.upscale_preset, "performance");
    assert!(!cards.handheld30.stepped_down && !cards.max.stepped_down);
    assert_eq!(cards.max.anti_aliasing, "fsr3");
}

#[test]
fn a_damaged_probe_fails_before_the_bench() {
    let err = cards_from("{}", Some("{not json"), None, None).unwrap_err();
    assert!(err.to_string().contains("hardware probe report is damaged"));
}
