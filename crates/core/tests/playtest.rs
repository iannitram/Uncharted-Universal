use std::collections::{BTreeMap, VecDeque};
use uncharted_core::error::Result;
use uncharted_core::games::Game;
use uncharted_core::logs::Ended;
use uncharted_core::playtest::{
    run_route, validate_pad_tokens, AgentAction, Checkpoint, FailureKind, PlaytestHost,
    RecoveryPolicy, RgbFrame, Route, ScriptStep, Section, SectionBudget, SectionStatus,
    StepOutcome,
};

struct MockHost {
    outcomes: VecDeque<StepOutcome>,
    checkpoints: BTreeMap<String, RgbFrame>,
    agent_actions: VecDeque<AgentAction>,
    loaded_saves: Vec<(String, Option<String>)>,
    inputs_applied: Vec<(String, u64)>,
}

impl MockHost {
    fn new() -> Self {
        Self {
            outcomes: VecDeque::new(),
            checkpoints: BTreeMap::new(),
            agent_actions: VecDeque::new(),
            loaded_saves: Vec::new(),
            inputs_applied: Vec::new(),
        }
    }

    fn push_ok(&mut self, elapsed_ms: u64, frame_ms: f64, frames: usize, shot: Option<RgbFrame>) {
        self.outcomes.push_back(StepOutcome {
            elapsed_ms,
            frame_times_ms: vec![frame_ms; frames],
            log_lines: Vec::new(),
            process_alive: true,
            exit_code: None,
            screenshot: shot,
            screenshot_path: None,
            audio_started: true,
            trophies_unlocked: Vec::new(),
        });
    }
}

impl PlaytestHost for MockHost {
    fn load_save(&mut self, section: &Section, save: Option<&str>) -> Result<()> {
        self.loaded_saves
            .push((section.id.clone(), save.map(str::to_owned)));
        Ok(())
    }

    fn apply_input(&mut self, tokens: &str, hold_ms: u64) -> StepOutcome {
        self.inputs_applied.push((tokens.to_owned(), hold_ms));
        self.outcomes.pop_front().unwrap_or(StepOutcome {
            elapsed_ms: hold_ms,
            frame_times_ms: vec![16.6; 10],
            process_alive: true,
            audio_started: true,
            ..StepOutcome::default()
        })
    }

    fn load_checkpoint_image(&mut self, path: &str) -> Result<RgbFrame> {
        self.checkpoints
            .get(path)
            .cloned()
            .ok_or_else(|| uncharted_core::Error::Launch(format!("missing checkpoint {path}")))
    }

    fn agent_decide(
        &mut self,
        _section: &Section,
        _last: &StepOutcome,
        _attempt: u32,
    ) -> Option<AgentAction> {
        self.agent_actions.pop_front()
    }
}

#[test]
fn validates_pad_tokens_and_rejects_bad_ones() {
    assert!(validate_pad_tokens("cross l1 lx=128 ly=0 rx=255 ry=64 mx=-12.5 my=3.0").is_ok());
    assert!(validate_pad_tokens("").is_ok());
    assert!(validate_pad_tokens("replay").is_ok());
    assert!(validate_pad_tokens("jump").is_err());
    assert!(validate_pad_tokens("lx=256").is_err());
    assert!(validate_pad_tokens("ry=abc").is_err());
    assert!(validate_pad_tokens("mx=NaN").is_err());
}

#[test]
fn route_json_round_trip() {
    let mut s1 = Section::new("train-intro", "Chapter 1: Train Intro");
    s1.save = Some("u2_ch1_start".into());
    s1.steps = vec![
        ScriptStep::new("cross", 200).with_note("confirm prompt"),
        ScriptStep::new("ly=0", 1500),
    ];
    s1.checkpoint = Some(Checkpoint {
        image_path: Some("checkpoints/u2_train.png".into()),
        max_mean_diff: 8.5,
        min_luma: 3.0,
    });
    s1.budget = SectionBudget {
        timeout_ms: 15_000,
        min_avg_fps: Some(30.0),
        max_frame_ms: Some(40.0),
        stall_threshold_ms: 35.0,
        max_stalls: 2,
        max_frozen_frames: 2,
        require_audio: true,
        expected_trophies: vec!["NPWR001".into()],
    };
    s1.recovery = RecoveryPolicy {
        max_retries: 2,
        nudge_tokens: vec!["rx=160".into()],
        reload_save: true,
        agent_takeover: true,
    };

    let route = Route {
        game: Game::U2,
        name: "u2-opening".into(),
        sections: vec![s1],
    };
    let json = route.to_json();
    let parsed = Route::from_json(&json).expect("route should parse back");
    assert_eq!(parsed, route);
}

#[test]
fn clean_synthetic_run_passes_and_bridges_to_playthrough_log() {
    let mut s1 = Section::new("s1", "Train Climb");
    s1.save = Some("save_01".into());
    s1.steps = vec![ScriptStep::new("ly=0", 500), ScriptStep::new("cross", 300)];
    s1.checkpoint = Some(Checkpoint {
        image_path: Some("cp/s1.png".into()),
        max_mean_diff: 5.0,
        min_luma: 5.0,
    });
    s1.budget = SectionBudget {
        min_avg_fps: Some(50.0),
        require_audio: true,
        expected_trophies: vec!["TROPHY_1".into()],
        ..SectionBudget::default()
    };

    let route = Route {
        game: Game::U2,
        name: "u2-opening".into(),
        sections: vec![s1],
    };

    let mut host = MockHost::new();
    host.checkpoints
        .insert("cp/s1.png".into(), RgbFrame::solid(4, 4, 100, 120, 140));
    host.push_ok(500, 16.6, 30, Some(RgbFrame::solid(4, 4, 90, 110, 130)));
    host.outcomes.push_back(StepOutcome {
        elapsed_ms: 300,
        frame_times_ms: vec![16.6; 18],
        log_lines: vec!["Trophy unlocked: TROPHY_1".into()],
        process_alive: true,
        exit_code: None,
        screenshot: Some(RgbFrame::solid(4, 4, 101, 121, 141)),
        screenshot_path: Some("shots/s1_end.png".into()),
        audio_started: true,
        trophies_unlocked: vec!["TROPHY_1".into()],
    });

    let report = run_route(&mut host, &route, "v0.3.15", 1_700_000_000);
    assert!(report.overall_pass());
    assert_eq!(report.passed(), 1);
    assert_eq!(report.failed(), 0);
    assert_eq!(report.sections[0].attempts, 1);
    assert!(report.sections[0].checkpoint_diff.unwrap() < 2.0);

    let pt = report.to_playthrough_run("Balanced 60");
    assert_eq!(pt.ended, Ended::Ok);
    assert_eq!(pt.game, Game::U2);
    assert_eq!(pt.chapter.as_deref(), Some("Train Climb"));
    assert!(pt.avg_fps.unwrap() >= 59.0);
}

#[test]
fn detects_crash_hang_freeze_low_fps_and_visual_faults() {
    let mut s_crash = Section::new("crash", "Crash Section");
    s_crash.steps = vec![ScriptStep::new("cross", 200)];
    s_crash.recovery.max_retries = 0;

    let mut s_hang = Section::new("hang", "Hang Section");
    s_hang.steps = vec![ScriptStep::new("ly=0", 500)];
    s_hang.recovery.max_retries = 0;

    let mut s_freeze = Section::new("freeze", "Freeze Section");
    s_freeze.steps = vec![
        ScriptStep::new("ly=0", 100),
        ScriptStep::new("ly=0", 100),
        ScriptStep::new("ly=0", 100),
    ];
    s_freeze.budget.max_frozen_frames = 1;
    s_freeze.recovery.max_retries = 0;

    let mut s_black = Section::new("black", "Black Screen & Low FPS");
    s_black.steps = vec![ScriptStep::new("rx=180", 500)];
    s_black.budget.min_avg_fps = Some(30.0);
    s_black.budget.max_stalls = 0;
    s_black.budget.stall_threshold_ms = 40.0;
    s_black.checkpoint = Some(Checkpoint {
        image_path: Some("cp/expected.png".into()),
        max_mean_diff: 10.0,
        min_luma: 5.0,
    });
    s_black.recovery.max_retries = 0;

    let route = Route {
        game: Game::U1,
        name: "u1-health".into(),
        sections: vec![s_crash, s_hang, s_freeze, s_black],
    };

    let mut host = MockHost::new();
    host.checkpoints
        .insert("cp/expected.png".into(), RgbFrame::solid(4, 4, 80, 80, 80));

    // s_crash: log contains Crash: line and process died
    host.outcomes.push_back(StepOutcome {
        elapsed_ms: 100,
        frame_times_ms: vec![16.6; 5],
        log_lines: vec!["Crash: SIGSEGV at 0xdeadbeef".into()],
        process_alive: false,
        exit_code: Some(139),
        ..StepOutcome::default()
    });

    // s_hang: 0 frames produced during 500ms step
    host.outcomes.push_back(StepOutcome {
        elapsed_ms: 500,
        frame_times_ms: Vec::new(),
        process_alive: true,
        ..StepOutcome::default()
    });

    // s_freeze: 3 identical frames -> 2 frozen transitions > max_frozen_frames(1)
    let same = RgbFrame::solid(4, 4, 60, 60, 60);
    host.push_ok(100, 16.6, 6, Some(same.clone()));
    host.push_ok(100, 16.6, 6, Some(same.clone()));
    host.push_ok(100, 16.6, 6, Some(same));

    // s_black: 100ms frame times (10 fps, stall > 40ms) + all-black frame
    host.push_ok(500, 100.0, 5, Some(RgbFrame::solid(4, 4, 0, 0, 0)));

    let report = run_route(&mut host, &route, "v0.3.15", 1_700_000_100);
    assert!(!report.overall_pass());
    assert_eq!(report.failed(), 4);
    assert_eq!(report.sections[0].failures[0].kind, FailureKind::Crash);
    assert_eq!(report.sections[1].failures[0].kind, FailureKind::Hang);
    assert_eq!(
        report.sections[2].failures[0].kind,
        FailureKind::FrameFreeze
    );

    let black_kinds: Vec<FailureKind> =
        report.sections[3].failures.iter().map(|f| f.kind).collect();
    assert!(black_kinds.contains(&FailureKind::LowFps));
    assert!(black_kinds.contains(&FailureKind::FrameStall));
    assert!(black_kinds.contains(&FailureKind::VisualBlack));
    assert!(black_kinds.contains(&FailureKind::CheckpointMismatch));

    let json = report.to_json();
    let round_trip = uncharted_core::playtest::BuildReport::from_json(&json).unwrap();
    assert_eq!(round_trip.failed(), 4);

    let md = report.to_markdown();
    assert!(md.contains("FAIL"));
    assert!(md.contains("## Broken Sections"));
    assert!(md.contains("Crash: SIGSEGV at 0xdeadbeef"));
}

#[test]
fn retry_with_save_reload_recovers_transient_failure() {
    let mut s1 = Section::new("museum", "Museum Courtyard");
    s1.save = Some("u2_museum".into());
    s1.steps = vec![ScriptStep::new("ly=0", 200)];
    s1.checkpoint = Some(Checkpoint {
        image_path: Some("cp/museum.png".into()),
        max_mean_diff: 5.0,
        min_luma: 2.0,
    });
    s1.recovery = RecoveryPolicy {
        max_retries: 1,
        nudge_tokens: Vec::new(),
        reload_save: true,
        agent_takeover: false,
    };

    let route = Route {
        game: Game::U2,
        name: "u2-museum".into(),
        sections: vec![s1],
    };

    let mut host = MockHost::new();
    host.checkpoints
        .insert("cp/museum.png".into(), RgbFrame::solid(2, 2, 120, 120, 120));
    // Attempt 1: wrong checkpoint
    host.push_ok(200, 16.6, 12, Some(RgbFrame::solid(2, 2, 10, 10, 10)));
    // Attempt 2: matches checkpoint
    host.push_ok(200, 16.6, 12, Some(RgbFrame::solid(2, 2, 120, 120, 120)));

    let report = run_route(&mut host, &route, "v0.3.15", 1_700_000_200);
    assert!(report.overall_pass());
    assert_eq!(report.sections[0].status, SectionStatus::Pass);
    assert_eq!(report.sections[0].attempts, 2);
    assert_eq!(report.sections[0].recoveries.len(), 1);
    assert_eq!(host.loaded_saves.len(), 2);
}

#[test]
fn agent_takeover_drives_closed_loop_until_complete() {
    let mut s1 = Section::new("combat", "Courtyard Fight");
    s1.steps = vec![ScriptStep::new("l2", 100)];
    s1.recovery.agent_takeover = true;

    let route = Route {
        game: Game::U3,
        name: "u3-combat".into(),
        sections: vec![s1],
    };

    let mut host = MockHost::new();
    host.push_ok(100, 16.6, 6, Some(RgbFrame::solid(2, 2, 50, 50, 50)));
    host.agent_actions.push_back(AgentAction::Continue {
        tokens: "l2 r1".into(),
        hold_ms: 150,
    });
    host.push_ok(150, 16.6, 9, Some(RgbFrame::solid(2, 2, 70, 70, 70)));
    host.agent_actions.push_back(AgentAction::SectionComplete);

    let report = run_route(&mut host, &route, "v0.3.15", 1_700_000_300);
    assert!(report.overall_pass());
    assert_eq!(host.inputs_applied.len(), 2);
    assert_eq!(host.inputs_applied[1], ("l2 r1".into(), 150));
}

#[test]
fn decodes_png_bytes_for_checkpoint() {
    let img = image::RgbImage::from_pixel(2, 2, image::Rgb([64, 128, 192]));
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .unwrap();
    let frame = RgbFrame::from_png_bytes(&buf).unwrap();
    assert_eq!(frame.width, 2);
    assert_eq!(frame.height, 2);
    assert!(frame.mean_luma() > 100.0);
}

#[test]
fn detects_timeout_missing_audio_and_missing_trophy() {
    let mut s1 = Section::new("audio-trophy", "Audio & Trophy Check");
    s1.steps = vec![ScriptStep::new("cross", 200), ScriptStep::new("ly=0", 400)];
    s1.budget = SectionBudget {
        timeout_ms: 500,
        require_audio: true,
        expected_trophies: vec!["NPWR_GOLD".into()],
        ..SectionBudget::default()
    };
    s1.recovery.max_retries = 0;

    let route = Route {
        game: Game::U2,
        name: "u2-checks".into(),
        sections: vec![s1],
    };

    let mut host = MockHost::new();
    host.outcomes.push_back(StepOutcome {
        elapsed_ms: 250,
        frame_times_ms: vec![16.6; 15],
        process_alive: true,
        audio_started: false,
        ..StepOutcome::default()
    });
    host.outcomes.push_back(StepOutcome {
        elapsed_ms: 300,
        frame_times_ms: vec![16.6; 18],
        process_alive: true,
        audio_started: false,
        ..StepOutcome::default()
    });

    let report = run_route(&mut host, &route, "v0.3.15", 1_700_000_400);
    assert!(!report.overall_pass());
    let kinds: Vec<FailureKind> = report.sections[0].failures.iter().map(|f| f.kind).collect();
    assert!(kinds.contains(&FailureKind::Hang));
    assert!(kinds.contains(&FailureKind::AudioMissing));
    assert!(kinds.contains(&FailureKind::TrophyMissing));
}

#[test]
fn agent_abort_stops_without_retrying() {
    let mut s1 = Section::new("puzzle", "Journal Puzzle");
    s1.steps = vec![ScriptStep::new("touchpad", 100)];
    s1.recovery = RecoveryPolicy {
        max_retries: 2,
        nudge_tokens: Vec::new(),
        reload_save: true,
        agent_takeover: true,
    };

    let route = Route {
        game: Game::U3,
        name: "u3-puzzle".into(),
        sections: vec![s1],
    };

    let mut host = MockHost::new();
    host.push_ok(100, 16.6, 6, None);
    host.agent_actions.push_back(AgentAction::Abort {
        reason: "soft-locked behind closed door".into(),
    });

    let report = run_route(&mut host, &route, "v0.3.15", 1_700_000_500);
    assert!(!report.overall_pass());
    assert_eq!(report.sections[0].attempts, 1);
    assert_eq!(report.sections[0].failures[0].kind, FailureKind::Aborted);
}
