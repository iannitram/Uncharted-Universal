//! The upscaler and frame-generation rules (`uncharted_core::tune::upscalers`, issue #147),
//! fed made-up hardware reports with the device names the drivers report.

use uncharted_core::tune::{
    parse_probe, pick, pick_for, upscalers, Bench, Card, Cards, HardwareReport, Hidden, Upscalers,
    FSR3_VALIDATED_DEVICES,
};

/// The page's lists, in order (`ANTI_ALIASING` and `FRAME_GENERATION` in the launcher).
const ANTI_ALIASING: [&str; 6] = ["off", "taa", "fsr1", "fsr3", "fsr4", "dlss"];
const FRAME_GENERATION: [&str; 3] = ["off", "fsr3", "dlss"];

const NEEDS_RTX: &str = "needs a GeForce RTX";
const NEEDS_FSR4_GPU: &str = "needs a Radeon RX 7000 or 9000, or a GeForce RTX 40 or 50";
const NEEDS_RTX_40: &str = "needs a GeForce RTX 40 or 50";
const NEEDS_6_GB: &str = "needs 6 GB of video memory";

fn report(name: &str, vendor: &str, generation: &str, vram_mib: u32, shared: bool) -> String {
    format!(
        r#"{{"vendor":"{vendor}", "name":"{name}", "generation":"{generation}",
            "vram_mib":{vram_mib}, "shared_memory":{shared}, "integrated_gpu":{shared},
            "hybrid_graphics":false, "refresh_hz":120, "cpu_cores":8, "cpu_supported":true}}"#
    )
}

fn rules(json: &str) -> Upscalers {
    let report: HardwareReport = parse_probe(json).unwrap();
    upscalers(&report)
}

fn rtx_3090_ti() -> String {
    report(
        "NVIDIA GeForce RTX 3090 Ti",
        "nvidia",
        "rtx30",
        24576,
        false,
    )
}
fn rtx_4070() -> String {
    report("NVIDIA GeForce RTX 4070", "nvidia", "rtx40", 12282, false)
}
fn rtx_5080() -> String {
    report("NVIDIA GeForce RTX 5080", "nvidia", "rtx50", 16303, false)
}
fn radeon_8060s() -> String {
    report(
        "AMD Radeon 8060S Graphics (RADV STRIX_HALO)",
        "amd",
        "rdna3",
        32768,
        true,
    )
}
fn radeon_780m() -> String {
    report(
        "AMD Radeon 780M Graphics (RADV PHOENIX)",
        "amd",
        "rdna3",
        4096,
        true,
    )
}
fn intel() -> String {
    report("Intel(R) UHD Graphics 770", "intel", "older", 2048, true)
}

fn hidden(setting: &'static str, value: &'static str, reason: &'static str) -> Hidden {
    Hidden {
        setting,
        value,
        reason,
    }
}

#[test]
fn upscalers_rtx_3090_ti_has_dlss_super_resolution_but_fsr3_frame_generation() {
    let got = rules(&rtx_3090_ti());
    assert_eq!(got.offered, ["off", "taa", "fsr1", "fsr3", "dlss"]);
    assert_eq!(got.frame_generation, ["off", "fsr3"]);
    assert!(got.dlss_multipliers.is_empty());
    assert_eq!(
        got.hidden,
        [
            hidden("anti_aliasing", "fsr4", NEEDS_FSR4_GPU),
            hidden("frame_generation", "dlss", NEEDS_RTX_40),
        ]
    );
    assert_eq!(got.default, "dlss");
}

#[test]
fn upscalers_rtx_4070_has_dlss_and_dlss_frame_generation_at_2x() {
    let got = rules(&rtx_4070());
    assert_eq!(got.offered, ANTI_ALIASING);
    assert_eq!(got.frame_generation, FRAME_GENERATION);
    assert_eq!(got.dlss_multipliers, [2]);
    assert!(got.hidden.is_empty());
    assert_eq!(got.default, "dlss");
}

#[test]
fn upscalers_rtx_50_has_dlss_frame_generation_up_to_4x() {
    let got = rules(&rtx_5080());
    assert_eq!(got.frame_generation, FRAME_GENERATION);
    assert_eq!(got.dlss_multipliers, [2, 3, 4]);
    assert_eq!(got.default, "dlss");
}

#[test]
fn upscalers_radeon_8060s_defaults_to_fsr3_with_no_dlss() {
    let got = rules(&radeon_8060s());
    assert_eq!(got.offered, ["off", "taa", "fsr1", "fsr3", "fsr4"]);
    assert_eq!(got.frame_generation, ["off", "fsr3"]);
    assert!(got.dlss_multipliers.is_empty());
    assert_eq!(
        got.hidden,
        [
            hidden("anti_aliasing", "dlss", NEEDS_RTX),
            hidden("frame_generation", "dlss", NEEDS_RTX_40),
        ]
    );
    assert_eq!(got.default, "fsr3");
}

#[test]
fn upscalers_radeon_780m_defaults_to_fsr3_and_has_no_frame_generation_in_4_gb() {
    let got = rules(&radeon_780m());
    assert_eq!(got.offered, ["off", "taa", "fsr1", "fsr3", "fsr4"]);
    assert_eq!(got.frame_generation, ["off"]);
    assert_eq!(
        got.hidden,
        [
            hidden("anti_aliasing", "dlss", NEEDS_RTX),
            hidden("frame_generation", "fsr3", NEEDS_6_GB),
            hidden("frame_generation", "dlss", NEEDS_RTX_40),
        ]
    );
    assert_eq!(got.default, "fsr3");
}

#[test]
fn upscalers_intel_has_no_dlss_or_fsr4_and_defaults_to_off() {
    let got = rules(&intel());
    assert_eq!(got.offered, ["off", "taa", "fsr1", "fsr3"]);
    assert_eq!(got.frame_generation, ["off"]);
    assert_eq!(
        got.hidden,
        [
            hidden("anti_aliasing", "fsr4", NEEDS_FSR4_GPU),
            hidden("anti_aliasing", "dlss", NEEDS_RTX),
            hidden("frame_generation", "fsr3", NEEDS_6_GB),
            hidden("frame_generation", "dlss", NEEDS_RTX_40),
        ]
    );
    assert_eq!(got.default, "off");
}

/// The game turns an unmeasured FSR 3.1 off by default (`DriverDefaultUpscaler`), so the
/// launcher's default does the same: the same GPUs under the Windows driver, or another
/// RADV GPU, start with Off.
#[test]
fn upscalers_fsr3_default_only_on_the_device_names_the_game_measured() {
    for name in [
        "AMD Radeon(TM) 780M Graphics",
        "AMD Radeon(TM) 8060S Graphics",
        "AMD Radeon 780M Graphics (RADV PHOENIX2)",
        "AMD Radeon RX 7900 XTX (RADV NAVI31)",
    ] {
        let got = rules(&report(name, "amd", "rdna3", 4096, true));
        assert_eq!(got.default, "off", "{name}");
        assert!(got.offered.contains(&"fsr3"), "{name} still offers FSR 3.1");
    }
    for name in [
        "AMD Radeon Graphics (RADV PHOENIX)",
        "AMD Radeon Graphics (RADV STRIX_HALO)",
    ] {
        assert_eq!(
            rules(&report(name, "amd", "rdna3", 4096, true)).default,
            "fsr3"
        );
    }
}

#[test]
fn upscalers_fsr4_follows_the_generation() {
    for (vendor, generation, fsr4) in [
        ("amd", "rdna2", false),
        ("amd", "rdna3", true),
        ("amd", "rdna4", true),
        ("nvidia", "rtx20", false),
        ("nvidia", "rtx30", false),
        ("nvidia", "rtx40", true),
        ("nvidia", "rtx50", true),
        ("intel", "intel_arc", false),
        ("amd", "older", false),
        ("nvidia", "unknown", false),
    ] {
        let got = rules(&report("A made-up GPU", vendor, generation, 8192, false));
        assert_eq!(got.offered.contains(&"fsr4"), fsr4, "{vendor} {generation}");
    }
}

#[test]
fn upscalers_dlss_needs_an_rtx_generation_never_the_vendor_alone() {
    for (generation, dlss, dlss_fg) in [
        ("rtx20", true, false),
        ("rtx30", true, false),
        ("rtx40", true, true),
        ("rtx50", true, true),
        ("older", false, false),
        ("unknown", false, false),
    ] {
        let got = rules(&report("NVIDIA GeForce", "nvidia", generation, 8192, false));
        assert_eq!(got.offered.contains(&"dlss"), dlss, "{generation}");
        assert_eq!(
            got.frame_generation.contains(&"dlss"),
            dlss_fg,
            "{generation}"
        );
        assert_eq!(
            got.default,
            if dlss { "dlss" } else { "off" },
            "{generation}"
        );
    }
}

/// Under 6 GB (#171's gate) no frame generation is offered, even on an RTX 40.
#[test]
fn upscalers_frame_generation_needs_6_gb_even_on_an_rtx_40() {
    let got = rules(&report(
        "NVIDIA GeForce RTX 4050 Laptop GPU",
        "nvidia",
        "rtx40",
        6143,
        false,
    ));
    assert_eq!(got.frame_generation, ["off"]);
    assert!(got.dlss_multipliers.is_empty());
    assert_eq!(
        got.hidden,
        [
            hidden("frame_generation", "fsr3", NEEDS_6_GB),
            hidden("frame_generation", "dlss", NEEDS_6_GB),
        ]
    );
    let enough = rules(&report(
        "NVIDIA GeForce RTX 4050",
        "nvidia",
        "rtx40",
        6144,
        false,
    ));
    assert_eq!(enough.frame_generation, FRAME_GENERATION);
}

/// Every value of both lists is either offered or hidden, once, in the page's order; Off,
/// TAA, FSR 1 and FSR 3.1 are always offered; the default is always offered.
/// The named devices and a made-up GPU of every other generation.
fn fixtures() -> Vec<String> {
    let mut fixtures = vec![
        rtx_3090_ti(),
        rtx_4070(),
        rtx_5080(),
        radeon_8060s(),
        radeon_780m(),
        intel(),
    ];
    for generation in ["rdna2", "rdna4", "rtx20", "intel_arc", "unknown"] {
        let vendor = match generation {
            "rdna2" | "rdna4" => "amd",
            "intel_arc" => "intel",
            _ => "nvidia",
        };
        fixtures.push(report("A made-up GPU", vendor, generation, 8192, false));
    }
    fixtures
}

#[test]
fn upscalers_every_value_is_offered_or_hidden_once() {
    for json in &fixtures() {
        let got = rules(json);
        for (setting, list, offered) in [
            ("anti_aliasing", &ANTI_ALIASING[..], &got.offered),
            (
                "frame_generation",
                &FRAME_GENERATION[..],
                &got.frame_generation,
            ),
        ] {
            let left_out: Vec<_> = got
                .hidden
                .iter()
                .filter(|h| h.setting == setting)
                .map(|h| h.value)
                .collect();
            let mut both: Vec<_> = offered.iter().chain(&left_out).copied().collect();
            both.sort_by_key(|v| list.iter().position(|l| l == v));
            assert_eq!(both, list, "{setting} in {json}");
            let in_order: Vec<_> = list.iter().filter(|v| offered.contains(v)).collect();
            assert_eq!(in_order.len(), offered.len(), "{setting} order in {json}");
            assert!(offered.iter().zip(in_order).all(|(a, b)| a == b));
        }
        for always in ["off", "taa", "fsr1", "fsr3"] {
            assert!(got.offered.contains(&always), "{always} in {json}");
        }
        assert_eq!(got.frame_generation[0], "off");
        assert!(got.offered.contains(&got.default), "default in {json}");
        assert!(got.hidden.iter().all(|h| !h.reason.is_empty()));
    }
}

/// The launcher's FSR 3.1 list and the game's are one list.
#[test]
fn upscalers_fsr3_list_matches_the_game_shim() {
    let shim = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../gpu/shim/bbport_settings.cpp"
    ))
    .unwrap();
    let start = shim.find("bool FsrDeviceValidated(").unwrap();
    let body = &shim[start..];
    let list = &body[body.find("validated[] = {").unwrap()..];
    let list = &list[..list.find("};").unwrap()];
    let names: Vec<&str> = list.split('"').skip(1).step_by(2).collect();
    assert_eq!(names, FSR3_VALIDATED_DEVICES);
}

/// The three cards of `tune::pick_for`, the entry point the Settings page uses.
fn cards(json: &str) -> Cards {
    pick_for(&parse_probe(json).unwrap(), &Bench::default())
}

fn all(c: &Cards) -> [&Card; 3] {
    [&c.handheld30, &c.balanced60, &c.max]
}

/// The same report with another display refresh rate.
fn at_hz(json: &str, refresh_hz: u32) -> String {
    json.replace(
        r#""refresh_hz":120"#,
        &format!(r#""refresh_hz":{refresh_hz}"#),
    )
}

/// #147: an RTX 3090 Ti gets DLSS Super Resolution and FSR 3 Frame Generation, never DLSS's
/// (the game would fall back to FSR 3.1 itself: `vk_frame_generation.cpp`).
#[test]
fn cards_rtx_3090_ti_max_has_fsr3_frame_generation_not_dlss() {
    let got = cards(&rtx_3090_ti());
    assert_eq!(got.max.frame_generation, "fsr3");
    for card in all(&got) {
        assert_eq!(card.anti_aliasing, "dlss");
        assert_ne!(card.frame_generation, "dlss");
    }
}

#[test]
fn cards_rtx_4070_and_rtx_50_max_keep_dlss_frame_generation() {
    for json in [rtx_4070(), rtx_5080()] {
        let got = cards(&json);
        assert_eq!(got.max.frame_generation, "dlss", "{json}");
        assert_eq!(got.max.anti_aliasing, "dlss", "{json}");
    }
}

/// The replacement follows the FSR 3 rule of the cards (#174): a display of 120 Hz or more.
#[test]
fn cards_rtx_30_below_120_hz_turn_frame_generation_off() {
    for (refresh_hz, frame_generation) in [(60, "off"), (119, "off"), (120, "fsr3"), (144, "fsr3")]
    {
        let got = cards(&at_hz(&rtx_3090_ti(), refresh_hz));
        assert_eq!(
            got.max.frame_generation, frame_generation,
            "{refresh_hz} Hz"
        );
    }
    let rtx_20 = report("NVIDIA GeForce RTX 2080", "nvidia", "rtx20", 8192, false);
    assert_eq!(cards(&rtx_20).max.frame_generation, "fsr3");
    assert_eq!(cards(&at_hz(&rtx_20, 60)).max.frame_generation, "off");
}

/// The cards start with the GPU's default upscaler: FSR 3.1 only where the game measured it.
#[test]
fn cards_use_the_default_upscaler_of_the_gpu() {
    for (json, upscaler) in [
        (rtx_3090_ti(), "dlss"),
        (radeon_8060s(), "fsr3"),
        (radeon_780m(), "fsr3"),
        (intel(), "off"),
        (
            report(
                "AMD Radeon RX 7900 XTX (RADV NAVI31)",
                "amd",
                "rdna3",
                24560,
                false,
            ),
            "off",
        ),
    ] {
        for card in all(&cards(&json)) {
            assert_eq!(card.anti_aliasing, upscaler, "{json}");
        }
    }
}

/// No card ever picks a value the Display page hides, at any refresh rate or benchmark; the
/// rest of each card is `pick`'s.
#[test]
fn cards_never_pick_a_hidden_value() {
    let benches = [
        Bench::default(),
        Bench {
            handheld30: Some(20.0),
            balanced60: Some(40.0),
            max: Some(30.0),
        },
    ];
    for json in fixtures() {
        for refresh_hz in [60, 120, 240] {
            let json = at_hz(&json, refresh_hz);
            let report = parse_probe(&json).unwrap();
            let rules = upscalers(&report);
            for bench in &benches {
                let got = pick_for(&report, bench);
                let base = pick(&report.device, bench);
                for (card, base) in all(&got).into_iter().zip(all(&base)) {
                    let aa = card.anti_aliasing.as_str();
                    let fg = card.frame_generation.as_str();
                    assert!(rules.offered.contains(&aa), "{aa} in {json}");
                    assert!(rules.frame_generation.contains(&fg), "{fg} in {json}");
                    assert!(
                        !rules
                            .hidden
                            .iter()
                            .any(|h| (h.setting, h.value) == ("anti_aliasing", aa)
                                || (h.setting, h.value) == ("frame_generation", fg)),
                        "{json}"
                    );
                    assert_eq!(aa, rules.default, "{json}");
                    assert_eq!(
                        (
                            &card.frame_rate,
                            &card.render_resolution,
                            &card.upscale_preset,
                            card.stepped_down
                        ),
                        (
                            &base.frame_rate,
                            &base.render_resolution,
                            &base.upscale_preset,
                            base.stepped_down
                        ),
                        "{json}"
                    );
                }
            }
        }
    }
}
