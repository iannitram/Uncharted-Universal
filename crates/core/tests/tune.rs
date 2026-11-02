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
