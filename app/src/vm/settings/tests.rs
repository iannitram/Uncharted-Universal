use super::*;
use serde_json::json;
use std::collections::BTreeMap;
use uncharted_core::games::Game;

fn desktop() -> &'static DeviceProfile {
    profile::by_id("desktop").unwrap()
}

fn handheld() -> &'static DeviceProfile {
    profile::by_id("amd-handheld").unwrap()
}

/// A synthetic probe report (the #171 format).
fn probe(
    vendor: &str,
    generation: &str,
    name: &str,
    vram_mib: u32,
    refresh: u32,
) -> HardwareReport {
    tune::parse_probe(
        &json!({
            "vendor": vendor,
            "name": name,
            "generation": generation,
            "vram_mib": vram_mib,
            "shared_memory": false,
            "integrated_gpu": false,
            "hybrid_graphics": false,
            "refresh_hz": refresh,
            "cpu_cores": 16,
            "cpu_supported": true,
        })
        .to_string(),
    )
    .unwrap()
}

fn rtx30() -> HardwareReport {
    probe("nvidia", "rtx30", "Synthetic RTX 30", 24_576, 144)
}

fn env_of(stored: &Stored, profile: &DeviceProfile) -> BTreeMap<String, String> {
    run_env::build_env(
        Game::U2,
        &run_settings(stored, profile),
        profile,
        &BTreeMap::new(),
    )
    .unwrap()
    .into_iter()
    .collect()
}

#[test]
fn each_card_writes_the_pickers_settings() {
    let bench = Bench {
        handheld30: Some(31.0),
        balanced60: Some(40.0),
        max: None,
    };
    let mut vm = SettingsVm::new(Stored::default(), desktop(), Some((rtx30(), bench.clone())));
    let cards = tune::pick_for(&rtx30(), &bench);
    for (i, card) in [&cards.handheld30, &cards.balanced60, &cards.max]
        .into_iter()
        .enumerate()
    {
        vm.focus = Grid::at(0, 0);
        for _ in 0..i {
            vm.handle(Input::Move(Nav::Right));
        }
        let SettingsCommand::Save(stored) = vm.handle(Input::Activate) else {
            panic!("card {i} saved nothing");
        };
        assert_eq!(stored.with_card(card), stored, "card {i}");
        assert!(vm.view().cards[i].in_use);
        let env = env_of(&stored, desktop());
        assert_eq!(env["BB_VBLANK_HZ"], card.frame_rate);
        assert_eq!(env["BB_RESOLUTION"], card.render_resolution);
        assert_eq!(env["BB_UPSCALER"], card.anti_aliasing);
        assert_eq!(env["BB_UPSCALE_QUALITY"], card.upscale_preset);
        assert_eq!(env["BB_FRAME_GEN"], card.frame_generation);
    }
    // The balanced card missed 60 and stepped down; its line says so.
    let view = vm.view();
    assert!(cards.balanced60.stepped_down);
    assert!(view.cards[1]
        .lines
        .contains(&"Quality stepped down: the benchmark missed 60 FPS".to_owned()));
    assert!(view.cards[0]
        .lines
        .contains(&"Last benchmark 31 FPS".to_owned()));
    assert!(view.cards[2]
        .lines
        .contains(&"Not benchmarked yet".to_owned()));
}

#[test]
fn the_amd_handheld_caps_the_frame_rate_like_steam_deck_mode() {
    // tests/test_deck_framerate.py: 30 and 60 stay, anything else is 30.
    for (chosen, expected) in [
        ("30", "30"),
        ("60", "60"),
        ("120", "30"),
        ("144", "30"),
        ("", "30"),
    ] {
        let stored = Stored {
            frame_rate: chosen.into(),
            ..Stored::default()
        };
        assert_eq!(
            env_of(&stored, handheld())["BB_VBLANK_HZ"],
            expected,
            "{chosen}"
        );
    }
}

#[test]
fn the_amd_handheld_never_runs_what_its_gpu_cannot() {
    // DECK_UNAVAILABLE in launcher/uncharted_linux.py.
    let stored = Stored {
        anti_aliasing: "dlss".into(),
        upscale_preset: "quality".into(),
        frame_generation: "dlss".into(),
        ..Stored::default()
    };
    let env = env_of(&stored, handheld());
    assert_eq!(env["BB_UPSCALER"], "off");
    assert_eq!(env["BB_UPSCALE_QUALITY"], "off");
    assert_eq!(env["BB_FRAME_GEN"], "off");
    for value in ["fsr4", "fsr411", "dlss"] {
        assert!(!profile_allows(handheld(), "anti_aliasing", value));
        assert!(profile_allows(desktop(), "anti_aliasing", value));
    }
    assert!(profile_allows(handheld(), "anti_aliasing", "fsr3"));
}

#[test]
fn fsr1_is_its_own_switch() {
    let stored = Stored {
        frame_rate: "60".into(),
        render_resolution: "2560x1440".into(),
        anti_aliasing: "fsr1".into(),
        upscale_preset: "balanced".into(),
        frame_generation: "off".into(),
        ..Stored::default()
    };
    let env = env_of(&stored, desktop());
    assert_eq!(env["BB_FSR1"], "1");
    assert_eq!(env["BB_UPSCALER"], "off");
    assert_eq!(env["BB_UPSCALE_QUALITY"], "balanced");
    // With no upscaler the preset does nothing.
    let off = Stored {
        anti_aliasing: "off".into(),
        ..stored
    };
    let env = env_of(&off, desktop());
    assert_eq!(env["BB_UPSCALE_QUALITY"], "off");
    assert!(!env.contains_key("BB_FSR1"));
}

#[test]
fn empty_values_become_the_devices_defaults() {
    let vm = SettingsVm::new(
        Stored::default(),
        desktop(),
        Some((rtx30(), Bench::default())),
    );
    let stored = vm.stored();
    assert_eq!(stored.device_profile, "auto");
    assert_eq!(stored.frame_rate, "60");
    assert_eq!(stored.render_resolution, "1920x1080");
    assert_eq!(stored.anti_aliasing, "dlss");
    assert_eq!(stored.upscale_preset, "quality");
    assert_eq!(stored.frame_generation, "off");

    let vm = SettingsVm::new(Stored::default(), handheld(), None);
    assert_eq!(vm.stored().frame_rate, "30");
    assert_eq!(vm.stored().anti_aliasing, "off");
}

#[test]
fn choices_offer_only_what_the_gpu_and_profile_allow() {
    // An RDNA 2 handheld GPU with 4 GB: no FSR 4, no DLSS, no frame generation, 1080p at most.
    let small = probe("amd", "rdna2", "Synthetic RDNA 2", 4_096, 60);
    let vm = SettingsVm::new(
        Stored::default(),
        handheld(),
        Some((small, Bench::default())),
    );
    let values = |choice| {
        vm.options(choice)
            .into_iter()
            .map(|(v, _)| v)
            .collect::<Vec<_>>()
    };
    assert_eq!(values(Choice::FrameRate), ["30", "60"]);
    assert_eq!(values(Choice::Upscaler), ["off", "taa", "fsr1", "fsr3"]);
    assert_eq!(values(Choice::FrameGeneration), ["off"]);
    assert_eq!(
        values(Choice::Resolution),
        ["1280x720", "1600x900", "1920x1080"]
    );
    let notes = vm.view().notes;
    assert!(notes.contains(&"GPU: Synthetic RDNA 2".to_owned()));
    assert!(notes.contains(&"DLSS: needs a GeForce RTX.".to_owned()));
    assert!(notes.contains(&"FSR 3 Frame Generation: needs 6 GB of video memory.".to_owned()));
    assert!(notes.contains(&"Resolutions above 1920 x 1080: need 6 GB of video memory.".to_owned()));
    assert!(notes.iter().any(|n| n.starts_with("Power: 20 W")));

    // An RTX 40 on the handheld profile: DLSS is the GPU's, the profile still leaves it out.
    let rtx40 = probe("nvidia", "rtx40", "Synthetic RTX 40", 16_384, 144);
    let vm = SettingsVm::new(
        Stored::default(),
        handheld(),
        Some((rtx40, Bench::default())),
    );
    assert!(!vm
        .options(Choice::Upscaler)
        .iter()
        .any(|(v, _)| v == "dlss"));
    assert!(vm
        .view()
        .notes
        .contains(&"DLSS: not with the AMD handheld profile.".to_owned()));
    // Its cards keep to the profile too.
    let cards = vm.cards();
    assert_eq!(cards.max.anti_aliasing, "off");
    assert_eq!(cards.max.frame_generation, "off");
}

#[test]
fn left_and_right_change_a_choice_and_a_cycles_it() {
    let mut vm = SettingsVm::new(Stored::default(), desktop(), None);
    // Down past the cards and Tune to the frame rate.
    vm.handle(Input::Move(Nav::Down));
    vm.handle(Input::Move(Nav::Down));
    vm.handle(Input::Move(Nav::Down));
    assert_eq!(vm.focus(), Grid::at(3, 0));
    assert_eq!(vm.view().choices[1].value, "60 FPS");
    let SettingsCommand::Save(stored) = vm.handle(Input::Move(Nav::Right)) else {
        panic!("no save");
    };
    assert_eq!(stored.frame_rate, "120");
    // At the end Right does nothing; A goes round.
    assert_eq!(vm.handle(Input::Move(Nav::Right)), SettingsCommand::None);
    assert!(!vm.view().choices[1].can_next);
    vm.handle(Input::Activate);
    assert_eq!(vm.stored().frame_rate, "30");
    vm.handle(Input::Move(Nav::Left));
    assert_eq!(vm.stored().frame_rate, "30");
    // A click on a choice row's arrows.
    assert!(matches!(vm.click(3, 1), SettingsCommand::Save(_)));
    assert_eq!(vm.stored().frame_rate, "60");
}

#[test]
fn a_profile_change_fits_the_values_to_it() {
    let stored = Stored {
        frame_rate: "120".into(),
        ..Stored::default()
    };
    let mut vm = SettingsVm::new(stored, desktop(), None);
    assert_eq!(vm.stored().frame_rate, "120");
    vm.focus = Grid::at(2, 0);
    // auto, amd-handheld, ...
    vm.handle(Input::Move(Nav::Right));
    assert_eq!(vm.stored().device_profile, "amd-handheld");
    assert_eq!(vm.profile().id, "amd-handheld");
    assert_eq!(vm.stored().frame_rate, "30");
    assert_eq!(vm.view().choices[0].value, "AMD handheld");
    // An unknown profile id is auto.
    let vm = SettingsVm::new(
        Stored {
            device_profile: "toaster".into(),
            ..Stored::default()
        },
        desktop(),
        None,
    );
    assert_eq!(vm.stored().device_profile, "auto");
    assert_eq!(vm.view().choices[0].value, "Auto (Desktop)");
}

#[test]
fn tune_runs_once_and_takes_the_result() {
    let mut vm = SettingsVm::new(Stored::default(), desktop(), None);
    assert!(vm
        .view()
        .notes
        .iter()
        .any(|n| n.starts_with("Not tuned yet")));
    // The untuned cards stay at 1080p without frame generation.
    assert_eq!(vm.cards().max.render_resolution, "1920x1080");
    vm.handle(Input::Move(Nav::Down));
    assert_eq!(vm.handle(Input::Activate), SettingsCommand::Tune);
    assert!(vm.view().tuning);
    assert_eq!(vm.handle(Input::Activate), SettingsCommand::None);
    vm.tuned(Ok((rtx30(), Bench::default())));
    assert!(!vm.is_tuning());
    assert_eq!(vm.view().status, "Tuned for Synthetic RTX 30.");
    assert_eq!(vm.cards().max.render_resolution, "3840x2160");
    assert_eq!(vm.cards().max.anti_aliasing, "dlss");

    vm.handle(Input::Activate);
    vm.tuned(Err("no Vulkan".into()));
    assert_eq!(vm.view().status, "Could not tune this device: no Vulkan");
    // The last good tune stays.
    assert_eq!(vm.cards().max.anti_aliasing, "dlss");
}

#[test]
fn benchmarks_match_their_card() {
    let report = rtx30();
    let cards = tune::pick_for(&report, &Bench::default());
    let run = |card: &Card, fps: f64, ended: &str| {
        json!({
            "game": "u2",
            "ended": ended,
            "avg_fps": fps,
            "settings": {
                "BB_VBLANK_HZ": card.frame_rate,
                "BB_RESOLUTION": card.render_resolution,
                "BB_UPSCALE_QUALITY": card_quality(card),
            },
        })
    };
    let reports = [
        run(&cards.handheld30, 33.0, "ok"),
        run(&cards.max, 58.0, "ok"),
        run(&cards.max, 52.0, "ok"),
        run(&cards.max, 10.0, "crash"),
        json!({"ended": "ok", "avg_fps": 99.0, "settings": {}}),
    ];
    let bench = bench_from_reports(&report, &reports);
    assert_eq!(bench.handheld30, Some(33.0));
    assert_eq!(bench.balanced60, None);
    assert_eq!(bench.max, Some(52.0));
}

#[test]
fn back_and_tab() {
    let mut vm = SettingsVm::new(Stored::default(), desktop(), None);
    assert_eq!(vm.handle(Input::Back), SettingsCommand::Back);
    // The defaults are the Balanced 60 card: focus starts there.
    assert_eq!(vm.focus(), Grid::at(0, 1));
    assert!(vm.view().cards[1].in_use);
    vm.handle(Input::Tab { back: false });
    assert_eq!(vm.focus(), Grid::at(0, 2));
    vm.handle(Input::Tab { back: true });
    vm.handle(Input::Tab { back: true });
    vm.handle(Input::Tab { back: true });
    assert_eq!(vm.focus(), Grid::at(7, 0));
    // LB and RB do nothing here.
    assert_eq!(vm.handle(Input::NextGame), SettingsCommand::None);
}
