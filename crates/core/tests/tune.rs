//! The settings picker for the three display cards (`uncharted_core::tune`, issue #174).

use uncharted_core::tune::{pick, Bench, Card, Cards, Device, Vendor};

fn dev(
    vendor: Vendor,
    rtx: bool,
    vram_mib: u32,
    shared_memory: bool,
    refresh_hz: Option<u32>,
) -> Device {
    Device {
        vendor,
        rtx,
        vram_mib,
        shared_memory,
        refresh_hz,
    }
}

/// An AMD card with 8 GiB of its own memory at 60 Hz: the device most cases change one field of.
fn base() -> Device {
    dev(Vendor::Amd, false, 8192, false, Some(60))
}

fn bench(handheld30: Option<f64>, balanced60: Option<f64>, max: Option<f64>) -> Bench {
    Bench {
        handheld30,
        balanced60,
        max,
    }
}

fn cards(device: &Device) -> Cards {
    pick(device, &Bench::default())
}

fn all(c: &Cards) -> [&Card; 3] {
    [&c.handheld30, &c.balanced60, &c.max]
}

fn basics(c: &Card) -> (&str, &str, &str, &str) {
    (
        &c.frame_rate,
        &c.render_resolution,
        &c.upscale_preset,
        &c.frame_generation,
    )
}

#[test]
fn every_value_is_filled_and_no_card_starts_stepped_down() {
    for c in all(&cards(&base())) {
        for text in [
            &c.frame_rate,
            &c.render_resolution,
            &c.anti_aliasing,
            &c.upscale_preset,
            &c.frame_generation,
        ] {
            assert!(!text.is_empty());
        }
        assert!(!c.stepped_down);
    }
}

#[test]
fn the_upscaler_is_dlss_for_rtx_and_fsr3_otherwise() {
    for c in all(&cards(&dev(Vendor::Nvidia, true, 8192, false, Some(60)))) {
        assert_eq!(c.anti_aliasing, "dlss");
    }
    let others = [
        dev(Vendor::Nvidia, false, 8192, false, Some(60)),
        dev(Vendor::Intel, false, 8192, false, Some(60)),
        base(),
    ];
    for device in others {
        for c in all(&cards(&device)) {
            assert_eq!(c.anti_aliasing, "fsr3");
        }
    }
}

#[test]
fn handheld30_is_the_same_on_every_device() {
    let devices = [
        base(),
        dev(Vendor::Amd, false, 2048, true, Some(60)),
        dev(Vendor::Nvidia, true, 16384, false, Some(60)),
    ];
    for d in devices {
        assert_eq!(
            basics(&cards(&d).handheld30),
            ("30", "1280x720", "quality", "off")
        );
    }
}

#[test]
fn balanced60_preset_follows_shared_memory() {
    assert_eq!(
        basics(&cards(&base()).balanced60),
        ("60", "1920x1080", "quality", "off")
    );
    let shared = Device {
        shared_memory: true,
        ..base()
    };
    let c = cards(&shared).balanced60;
    assert_eq!(
        (c.render_resolution.as_str(), c.upscale_preset.as_str()),
        ("1920x1080", "balanced")
    );
}

#[test]
fn max_resolution_tiers() {
    let res = |vram_mib: u32, shared_memory: bool| {
        cards(&Device {
            vram_mib,
            shared_memory,
            ..base()
        })
        .max
        .render_resolution
    };
    assert_eq!(res(10240, false), "3840x2160");
    assert_eq!(res(10239, false), "2560x1440");
    assert_eq!(res(6144, false), "2560x1440");
    assert_eq!(res(6143, false), "1920x1080");
    assert_eq!(res(24576, true), "1920x1080");
    assert_eq!(res(8192, true), "1920x1080");
}

#[test]
fn max_frame_rate_and_preset() {
    let c = cards(&Device {
        shared_memory: true,
        ..base()
    })
    .max;
    assert_eq!(
        (c.frame_rate.as_str(), c.upscale_preset.as_str()),
        ("60", "quality")
    );
}

#[test]
fn max_frame_generation() {
    let fg = |d: Device| cards(&d).max.frame_generation;
    assert_eq!(fg(dev(Vendor::Nvidia, true, 8192, false, Some(60))), "dlss");
    assert_eq!(
        fg(Device {
            refresh_hz: Some(120),
            ..base()
        }),
        "fsr3"
    );
    assert_eq!(
        fg(Device {
            refresh_hz: Some(144),
            vram_mib: 6144,
            ..base()
        }),
        "fsr3"
    );
    assert_eq!(
        fg(Device {
            refresh_hz: Some(119),
            ..base()
        }),
        "off"
    );
    assert_eq!(
        fg(Device {
            refresh_hz: None,
            ..base()
        }),
        "off"
    );
}

#[test]
fn low_video_memory_turns_frame_generation_off_and_caps_the_size() {
    let devices = [
        dev(Vendor::Nvidia, true, 4096, false, Some(240)),
        dev(Vendor::Amd, false, 6143, false, Some(240)),
        dev(Vendor::Nvidia, true, 2048, true, Some(60)),
    ];
    for d in devices {
        let got = cards(&d);
        for c in all(&got) {
            assert_eq!(c.frame_generation, "off");
        }
        assert_eq!(got.max.render_resolution, "1920x1080");
        assert_eq!(got.handheld30.render_resolution, "1280x720");
    }
}

#[test]
fn a_benchmark_below_95_percent_of_the_target_steps_the_preset_down() {
    let got = pick(&base(), &bench(Some(28.4), Some(56.9), Some(20.0)));
    assert_eq!(got.handheld30.upscale_preset, "balanced");
    assert_eq!(got.balanced60.upscale_preset, "balanced");
    assert_eq!(got.max.upscale_preset, "balanced");
    for c in all(&got) {
        assert!(c.stepped_down);
    }
}

#[test]
fn a_benchmark_at_or_above_the_threshold_keeps_the_preset() {
    let shared = Device {
        shared_memory: true,
        ..base()
    };
    let got = pick(&shared, &bench(Some(28.6), Some(57.1), Some(61.0)));
    assert_eq!(got.handheld30.upscale_preset, "quality");
    assert_eq!(got.balanced60.upscale_preset, "balanced");
    assert_eq!(got.max.upscale_preset, "quality");
    for c in all(&got) {
        assert!(!c.stepped_down);
    }
}

#[test]
fn stepping_down_from_balanced_goes_to_performance() {
    let shared = Device {
        shared_memory: true,
        ..base()
    };
    let got = pick(&shared, &bench(None, Some(30.0), None));
    assert_eq!(got.balanced60.upscale_preset, "performance");
    assert!(got.balanced60.stepped_down);
}

#[test]
fn a_partial_benchmark_only_touches_the_named_cards() {
    let got = pick(&base(), &bench(None, None, Some(10.0)));
    assert!(got.max.stepped_down);
    assert!(!got.handheld30.stepped_down);
    assert!(!got.balanced60.stepped_down);
    assert_eq!(got.balanced60.upscale_preset, "quality");
}

#[test]
fn the_target_differs_per_card() {
    let got = pick(&base(), &bench(Some(40.0), Some(40.0), None));
    assert!(!got.handheld30.stepped_down);
    assert!(got.balanced60.stepped_down);
}

/// Synthetic probe output only: no device queries or game bytes. The module name makes
/// these cases run under `cargo test -p uncharted_core tune`.
mod tune_probe {
    use super::*;
    use serde_json::{json, Value};
    use uncharted_core::tune::{choices, parse_probe, Generation, HardwareReport};

    const FOUR_GB: &str = r#"{
        "vendor":"nvidia", "name":"GeForce GTX 1650", "generation":"older",
        "vram_mib":4096, "shared_memory":false, "integrated_gpu":false,
        "hybrid_graphics":false, "refresh_hz":144, "cpu_cores":8, "cpu_supported":true
    }"#;
    const RTX_3090_TI: &str = r#"{
        "vendor":"nvidia", "name":"GeForce RTX 3090 Ti", "generation":"rtx30",
        "vram_mib":24576, "shared_memory":false, "integrated_gpu":false,
        "hybrid_graphics":false, "refresh_hz":120, "cpu_cores":16, "cpu_supported":true
    }"#;
    const RADEON_780M: &str = r#"{
        "vendor":"amd", "name":"Radeon 780M", "generation":"rdna3",
        "vram_mib":4096, "shared_memory":true, "integrated_gpu":true,
        "hybrid_graphics":false, "refresh_hz":60, "cpu_cores":8, "cpu_supported":true
    }"#;
    const INTEL_NVIDIA: &str = r#"{
        "vendor":"intel", "name":"Intel integrated graphics", "generation":"older",
        "vram_mib":2048, "shared_memory":true, "integrated_gpu":true,
        "hybrid_graphics":true, "refresh_hz":144, "cpu_cores":12, "cpu_supported":true
    }"#;

    fn changed(source: &str, key: &str, value: Value) -> String {
        let mut report: Value = serde_json::from_str(source).unwrap();
        report[key] = value;
        report.to_string()
    }

    fn read(source: &str) -> HardwareReport {
        parse_probe(source).unwrap()
    }

    #[test]
    fn four_gb_hides_frame_generation_and_above_1080p_and_dlss() {
        let report = read(FOUR_GB);
        let offered = choices(&report);
        assert_eq!(report.device.vendor, Vendor::Nvidia);
        assert_eq!(report.generation, Generation::Older);
        assert!(!offered.frame_generation);
        assert!(!offered.above_1080p);
        assert!(!offered.dlss);
        assert_eq!(offered.integrated_gpu_warning, None);
        let presets = cards(&report.device);
        for card in all(&presets) {
            assert_eq!(card.frame_generation, "off");
            assert_ne!(card.anti_aliasing, "dlss");
        }
        assert_eq!(presets.max.render_resolution, "1920x1080");
    }

    #[test]
    fn rtx_3090_ti_offers_dlss_and_has_memory_for_larger_sizes() {
        let report = read(RTX_3090_TI);
        assert_eq!(report.name, "GeForce RTX 3090 Ti");
        assert_eq!(report.generation, Generation::Rtx30);
        assert_eq!(report.device.vram_mib, 24576);
        assert!(!report.device.shared_memory);
        assert_eq!(report.device.refresh_hz, Some(120));
        assert_eq!(report.cpu_cores, 16);
        assert!(report.cpu_supported);
        let offered = choices(&report);
        assert!(offered.frame_generation);
        assert!(offered.above_1080p);
        assert!(offered.dlss);
        assert_eq!(offered.integrated_gpu_warning, None);
        let presets = cards(&report.device);
        assert_eq!(presets.max.render_resolution, "3840x2160");
        assert_eq!(presets.max.anti_aliasing, "dlss");
    }

    #[test]
    fn radeon_780m_preserves_shared_heap_size_and_has_no_laptop_warning() {
        let report = read(RADEON_780M);
        assert_eq!(report.name, "Radeon 780M");
        assert_eq!(report.generation, Generation::Rdna3);
        assert!(report.device.shared_memory);
        assert_eq!(report.device.vram_mib, 4096);
        let offered = choices(&report);
        assert!(!offered.frame_generation);
        assert!(!offered.above_1080p);
        assert!(!offered.dlss);
        assert_eq!(offered.integrated_gpu_warning, None);
        assert_eq!(cards(&report.device).balanced60.upscale_preset, "balanced");
    }

    #[test]
    fn intel_nvidia_laptop_gates_the_selected_gpu_and_warns_on_integrated_selection() {
        let offered = choices(&read(INTEL_NVIDIA));
        assert!(offered.integrated_gpu_warning.is_some());
        assert!(!offered.dlss);
        assert!(!offered.frame_generation);
        assert!(!offered.above_1080p);
        // Same topology, but now the discrete adapter and its heap are selected.
        let discrete = changed(RTX_3090_TI, "hybrid_graphics", json!(true));
        let offered = choices(&read(&discrete));
        assert_eq!(offered.integrated_gpu_warning, None);
        assert!(offered.dlss);
        assert!(offered.frame_generation);
        assert!(offered.above_1080p);
    }

    #[test]
    fn six_gib_boundary_applies_to_rtx_and_shared_memory_too() {
        for fixture in [RTX_3090_TI, RADEON_780M] {
            for memory in [0, 6143, 6144] {
                let report = read(&changed(fixture, "vram_mib", json!(memory)));
                let offered = choices(&report);
                assert_eq!(offered.frame_generation, memory >= 6144);
                assert_eq!(offered.above_1080p, memory >= 6144);
                assert_eq!(offered.dlss, report.device.vendor == Vendor::Nvidia);
                if memory < 6144 {
                    assert_eq!(cards(&report.device).max.frame_generation, "off");
                }
            }
        }
    }

    #[test]
    fn generation_is_preserved_without_guessing_from_a_marketing_name() {
        for (vendor, generation, expected) in [
            ("amd", "rdna2", Generation::Rdna2),
            ("amd", "rdna3", Generation::Rdna3),
            ("amd", "rdna4", Generation::Rdna4),
            ("nvidia", "rtx20", Generation::Rtx20),
            ("nvidia", "rtx30", Generation::Rtx30),
            ("nvidia", "rtx40", Generation::Rtx40),
            ("nvidia", "rtx50", Generation::Rtx50),
            ("intel", "intel_arc", Generation::IntelArc),
            ("other", "unknown", Generation::Unknown),
        ] {
            let fixture = changed(FOUR_GB, "vendor", json!(vendor));
            let fixture = changed(&fixture, "generation", json!(generation));
            let report = read(&fixture);
            assert_eq!(report.generation, expected);
            assert_eq!(report.device.rtx, vendor == "nvidia");
        }
        let fixture = changed(RTX_3090_TI, "generation", json!("unknown"));
        assert!(!choices(&read(&fixture)).dlss);
    }

    #[test]
    fn unknown_refresh_and_a_failed_cpu_instruction_check_survive_parsing() {
        let fixture = changed(FOUR_GB, "refresh_hz", Value::Null);
        let fixture = changed(&fixture, "cpu_supported", json!(false));
        let report = read(&fixture);
        assert_eq!(report.device.refresh_hz, None);
        assert!(!report.cpu_supported);
    }

    #[test]
    fn damaged_or_incomplete_reports_fail_without_echoing_the_input() {
        for fixture in ["not JSON", "[]", "null", "{}"] {
            assert!(parse_probe(fixture).is_err());
        }
        for (key, value) in [
            ("vendor", json!("unrecognised")),
            ("vendor", json!("amd")), // Inconsistent with the RTX generation.
            ("generation", json!("future_generation")),
            ("name", json!("  ")),
            ("vram_mib", json!(-1)),
            ("vram_mib", json!(4294967296_u64)),
            ("vram_mib", json!(4096.5)),
            ("shared_memory", json!("false")),
            ("integrated_gpu", Value::Null),
            ("hybrid_graphics", json!(1)),
            ("refresh_hz", json!(0)),
            ("refresh_hz", json!("60")),
            ("cpu_cores", json!(0)),
            ("cpu_supported", json!("private probe contents")),
        ] {
            let error = parse_probe(&changed(RTX_3090_TI, key, value)).unwrap_err();
            assert_eq!(error.code(), "launch");
            assert_eq!(
                error,
                uncharted_core::Error::Launch("The hardware probe report is damaged".into())
            );
        }
        let mut report: Value = serde_json::from_str(FOUR_GB).unwrap();
        let keys: Vec<_> = report.as_object().unwrap().keys().cloned().collect();
        for key in keys {
            let removed = report.as_object_mut().unwrap().remove(&key).unwrap();
            assert!(parse_probe(&report.to_string()).is_err(), "missing {key}");
            report[key] = removed;
        }
        report["future_field"] = json!(true);
        assert!(parse_probe(&report.to_string()).is_ok());
    }
}
