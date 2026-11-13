//! The Settings page (roadmap 2.06, issue #177): the three display cards of auto-tune
//! (`core::tune::pick_for`: Handheld 30, Balanced 60, Max quality), "Tune this device", and the
//! display choices one by one: device profile, frame rate, resolution, upscaler, quality and
//! frame generation. A card sets all five values at once; a choice row changes one. Every change
//! is saved straight away ([`SettingsCommand::Save`]), and [`run_settings`] turns the saved
//! values into what `core::run_env::build_env` starts the game with.
//!
//! What a choice row offers comes from the hardware report (`tune::upscalers`, `tune::choices`)
//! and the device profile ([`profile_allows`]: the AMD handheld keeps Steam Deck mode's rules,
//! 30 or 60 FPS and no upscaler or frame generation its GPU cannot run). A value left out says
//! why in the notes under the rows. Nothing here reads a file or runs the probe: `main.rs` does,
//! and hands the result to [`SettingsVm::tuned`].

use crate::focus::{Grid, Nav};
use crate::screen::Input;
use serde_json::Value;
use uncharted_core::profile::{self, DeviceProfile};
use uncharted_core::run_env;
use uncharted_core::tune::{self, Bench, Card, Cards, Device, Generation, HardwareReport, Vendor};

/// The render resolutions offered (`RENDER_RESOLUTIONS` of the Python launcher).
pub const RESOLUTIONS: [&str; 7] = [
    "1280x720",
    "1600x900",
    "1920x1080",
    "2560x1440",
    "2880x1620",
    "3200x1800",
    "3840x2160",
];

/// The upscaling presets (`UPSCALE_PRESETS`): the resolution is the output, the game draws
/// smaller by the factor and the upscaler makes it up.
pub const PRESETS: [(&str, &str); 5] = [
    ("off", "Native"),
    ("quality", "Quality (1.5x)"),
    ("balanced", "Balanced (1.7x)"),
    ("performance", "Performance (2x)"),
    ("ultra", "Ultra performance (3x)"),
];

/// The settings the page keeps (`<config>/settings.json`, `settings_store.rs`). An empty value
/// is not set yet: [`SettingsVm::new`] fills it with the device's default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stored {
    /// "auto" (detected) or a built-in profile id (`core::profile`).
    pub device_profile: String,
    /// "30", "60", ...: `BB_VBLANK_HZ`.
    pub frame_rate: String,
    /// "WxH": `BB_RESOLUTION`.
    pub render_resolution: String,
    /// The upscaler: off, taa, fsr1, fsr3, fsr4, dlss (`BB_UPSCALER`, `BB_FSR1`).
    pub anti_aliasing: String,
    /// A [`PRESETS`] value: `BB_UPSCALE_QUALITY`.
    pub upscale_preset: String,
    /// off, fsr3, dlss: `BB_FRAME_GEN`.
    pub frame_generation: String,
}

impl Stored {
    /// The five display values of a card.
    pub fn with_card(&self, card: &Card) -> Stored {
        Stored {
            device_profile: self.device_profile.clone(),
            frame_rate: card.frame_rate.clone(),
            render_resolution: card.render_resolution.clone(),
            anti_aliasing: card.anti_aliasing.clone(),
            upscale_preset: card.upscale_preset.clone(),
            frame_generation: card.frame_generation.clone(),
        }
    }

    fn matches(&self, card: &Card) -> bool {
        self.with_card(card) == *self
    }

    fn get(&self, choice: Choice) -> &str {
        match choice {
            Choice::Profile => &self.device_profile,
            Choice::FrameRate => &self.frame_rate,
            Choice::Resolution => &self.render_resolution,
            Choice::Upscaler => &self.anti_aliasing,
            Choice::Quality => &self.upscale_preset,
            Choice::FrameGeneration => &self.frame_generation,
        }
    }

    fn set(&mut self, choice: Choice, value: String) {
        *match choice {
            Choice::Profile => &mut self.device_profile,
            Choice::FrameRate => &mut self.frame_rate,
            Choice::Resolution => &mut self.render_resolution,
            Choice::Upscaler => &mut self.anti_aliasing,
            Choice::Quality => &mut self.upscale_preset,
            Choice::FrameGeneration => &mut self.frame_generation,
        } = value;
    }
}

/// The frame rate a profile starts with (its `BB_VBLANK_HZ`), "60" when it has none.
pub fn profile_rate(profile: &DeviceProfile) -> &'static str {
    profile
        .env
        .iter()
        .find(|(name, _)| *name == "BB_VBLANK_HZ")
        .map_or("60", |(_, value)| value)
}

/// Whether `profile` allows `value` for `setting` (a settings key). The frame rate must be one
/// the profile offers; the AMD handheld keeps Steam Deck mode's rules (`DECK_UNAVAILABLE` in
/// `launcher/uncharted_linux.py`): no FSR 4, FSR 4.1.1 or DLSS, and no DLSS Frame Generation.
pub fn profile_allows(profile: &DeviceProfile, setting: &str, value: &str) -> bool {
    match (profile.id, setting) {
        (_, "frame_rate") => profile
            .frame_rates
            .iter()
            .any(|rate| rate.to_string() == value),
        ("amd-handheld", "anti_aliasing") => !matches!(value, "fsr4" | "fsr411" | "dlss"),
        ("amd-handheld", "frame_generation") => value != "dlss",
        _ => true,
    }
}

/// The settings `build_env` starts the game with, from the saved values and the device profile:
/// the Python launcher's mapping (`build_environment`). `BB_UPSCALER` is the upscaler, `off` for
/// FSR 1, which is `BB_FSR1`; `BB_UPSCALE_QUALITY` is the preset, `off` with no upscaler (the game
/// draws at the resolution). A frame rate the profile does not offer becomes its own (Steam Deck
/// mode's 30/60 cap), and a value the profile does not allow becomes `off`.
// The play command (roadmap 2.02) reads settings.json through this; until then only the
// tests run it against `build_env`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn run_settings(stored: &Stored, profile: &DeviceProfile) -> run_env::Settings {
    let or_off = |setting: &str, value: &str| {
        if value.is_empty() || !profile_allows(profile, setting, value) {
            "off".to_owned()
        } else {
            value.to_owned()
        }
    };
    let upscaler = or_off("anti_aliasing", &stored.anti_aliasing);
    let rate = if profile_allows(profile, "frame_rate", &stored.frame_rate) {
        stored.frame_rate.clone()
    } else {
        profile_rate(profile).to_owned()
    };
    let mut settings = run_env::Settings {
        resolution: (!stored.render_resolution.is_empty())
            .then(|| stored.render_resolution.clone()),
        upscale_quality: Some(if upscaler == "off" || stored.upscale_preset.is_empty() {
            "off".to_owned()
        } else {
            stored.upscale_preset.clone()
        }),
        ..run_env::Settings::default()
    };
    let extra = &mut settings.extra;
    extra.insert("BB_VBLANK_HZ".into(), rate);
    if upscaler == "fsr1" {
        extra.insert("BB_FSR1".into(), "1".into());
    }
    extra.insert(
        "BB_UPSCALER".into(),
        if upscaler == "fsr1" {
            "off".into()
        } else {
            upscaler
        },
    );
    extra.insert(
        "BB_FRAME_GEN".into(),
        or_off("frame_generation", &stored.frame_generation),
    );
    settings
}

/// The report for a device not tuned yet: an unknown GPU with no video memory known, so the
/// cards stay at 1080p without frame generation until the probe says more.
pub fn unknown_gpu() -> HardwareReport {
    HardwareReport {
        device: Device {
            vendor: Vendor::Other,
            rtx: false,
            vram_mib: 0,
            shared_memory: false,
            refresh_hz: None,
        },
        name: String::new(),
        generation: Generation::Unknown,
        integrated_gpu: false,
        hybrid_graphics: false,
        cpu_cores: 1,
        cpu_supported: true,
    }
}

/// The `BB_UPSCALE_QUALITY` a card runs with.
fn card_quality(card: &Card) -> &str {
    if card.anti_aliasing == "off" {
        "off"
    } else {
        &card.upscale_preset
    }
}

/// The benchmark results of the cards, from the runtime's `benchmark.json` reports (#173): a
/// report counts for a card when the run ended well and ran the card's frame rate, resolution
/// and quality (the cards `pick_for` gives before any step-down). The lowest average of the
/// matching reports counts, so one good run does not hide a bad one.
pub fn bench_from_reports(report: &HardwareReport, reports: &[Value]) -> Bench {
    let cards = tune::pick_for(report, &Bench::default());
    let fps = |card: &Card| {
        reports
            .iter()
            .filter(|r| r["ended"] == "ok")
            .filter(|r| {
                let set = &r["settings"];
                set["BB_VBLANK_HZ"] == card.frame_rate.as_str()
                    && set["BB_RESOLUTION"] == card.render_resolution.as_str()
                    && set["BB_UPSCALE_QUALITY"] == card_quality(card)
            })
            .filter_map(|r| r["avg_fps"].as_f64())
            .fold(None, |low: Option<f64>, fps| {
                Some(low.map_or(fps, |low| low.min(fps)))
            })
    };
    Bench {
        handheld30: fps(&cards.handheld30),
        balanced60: fps(&cards.balanced60),
        max: fps(&cards.max),
    }
}

/// A choice row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Profile,
    FrameRate,
    Resolution,
    Upscaler,
    Quality,
    FrameGeneration,
}

impl Choice {
    pub const ALL: [Choice; 6] = [
        Choice::Profile,
        Choice::FrameRate,
        Choice::Resolution,
        Choice::Upscaler,
        Choice::Quality,
        Choice::FrameGeneration,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Choice::Profile => "Device profile",
            Choice::FrameRate => "Frame rate",
            Choice::Resolution => "Resolution",
            Choice::Upscaler => "Upscaler",
            Choice::Quality => "Quality",
            Choice::FrameGeneration => "Frame generation",
        }
    }

    /// The settings key, as `profile_allows` and `tune::Hidden::setting` name it.
    fn key(self) -> &'static str {
        match self {
            Choice::Profile => "device_profile",
            Choice::FrameRate => "frame_rate",
            Choice::Resolution => "render_resolution",
            Choice::Upscaler => "anti_aliasing",
            Choice::Quality => "upscale_preset",
            Choice::FrameGeneration => "frame_generation",
        }
    }
}

fn upscaler_label(value: &str) -> String {
    match value {
        "off" => "Off".into(),
        "taa" => "TAA (anti-aliasing only)".into(),
        "fsr1" => "FSR 1".into(),
        "fsr3" => "FSR 3.1".into(),
        "fsr4" => "FSR 4".into(),
        "fsr411" => "FSR 4.1.1".into(),
        "xess" => "XeSS".into(),
        "dlss" => "DLSS".into(),
        other => other.into(),
    }
}

fn frame_generation_label(value: &str) -> String {
    match value {
        "off" => "Off".into(),
        "fsr3" => "FSR 3 Frame Generation".into(),
        "dlss" => "DLSS Frame Generation".into(),
        other => other.into(),
    }
}

fn preset_label(value: &str) -> String {
    PRESETS
        .iter()
        .find(|(v, _)| *v == value)
        .map_or_else(|| value.to_owned(), |(_, label)| (*label).to_owned())
}

/// The cards' names, in their order on the page.
pub const CARD_NAMES: [&str; 3] = ["Handheld 30", "Balanced 60", "Max quality"];

/// The first row of choices; rows 0 and 1 are the cards and "Tune this device".
const FIRST_CHOICE: usize = 2;

/// What the app must do after an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsCommand {
    None,
    /// Back to the Continue screen.
    Back,
    /// Write these settings (`settings_store::save`).
    Save(Stored),
    /// Run the hardware probe and read the benchmark reports, then call [`SettingsVm::tuned`].
    Tune,
}

/// The Settings page.
#[derive(Debug)]
pub struct SettingsVm {
    stored: Stored,
    /// The profile `core::profile::detect` picked for this device ("auto").
    detected: &'static DeviceProfile,
    /// The probe's report; `None` until the device is tuned.
    report: Option<HardwareReport>,
    bench: Bench,
    focus: Grid,
    tuning: bool,
    status: String,
}

impl SettingsVm {
    /// The page with the saved values (empty ones become the device's defaults) and the last
    /// tune, if any; focus on the card in use, else the first card.
    pub fn new(
        stored: Stored,
        detected: &'static DeviceProfile,
        tune: Option<(HardwareReport, Bench)>,
    ) -> SettingsVm {
        let (report, bench) = match tune {
            Some((report, bench)) => (Some(report), bench),
            None => (None, Bench::default()),
        };
        let mut vm = SettingsVm {
            stored,
            detected,
            report,
            bench,
            focus: Grid::default(),
            tuning: false,
            status: String::new(),
        };
        vm.fit();
        let cards = vm.cards();
        let col = [&cards.handheld30, &cards.balanced60, &cards.max]
            .iter()
            .position(|card| vm.stored.matches(card))
            .unwrap_or(0);
        vm.focus = Grid::at(0, col);
        vm
    }

    #[cfg(test)]
    pub fn stored(&self) -> &Stored {
        &self.stored
    }

    #[cfg(test)]
    pub fn focus(&self) -> Grid {
        self.focus
    }

    pub fn set_status(&mut self, status: &str) {
        status.clone_into(&mut self.status);
    }

    /// The profile in use: the stored one, or the detected one for "auto".
    pub fn profile(&self) -> &'static DeviceProfile {
        profile::by_id(&self.stored.device_profile).unwrap_or(self.detected)
    }

    fn report(&self) -> HardwareReport {
        self.report.clone().unwrap_or_else(unknown_gpu)
    }

    /// The three cards for this GPU and its benchmarks, kept to what the profile allows.
    pub fn cards(&self) -> Cards {
        let mut cards = tune::pick_for(&self.report(), &self.bench);
        let profile = self.profile();
        for card in [&mut cards.handheld30, &mut cards.balanced60, &mut cards.max] {
            if !profile_allows(profile, "frame_rate", &card.frame_rate) {
                profile_rate(profile).clone_into(&mut card.frame_rate);
            }
            if !profile_allows(profile, "anti_aliasing", &card.anti_aliasing) {
                card.anti_aliasing = "off".into();
            }
            if !profile_allows(profile, "frame_generation", &card.frame_generation) {
                card.frame_generation = "off".into();
            }
        }
        cards
    }

    /// The values a choice row offers, with their labels, in order.
    pub fn options(&self, choice: Choice) -> Vec<(String, String)> {
        let profile = self.profile();
        let rules = tune::upscalers(&self.report());
        let allowed = |value: &&str| profile_allows(profile, choice.key(), value);
        let pairs = |values: Vec<&str>, label: fn(&str) -> String| {
            values
                .into_iter()
                .map(|v| (v.to_owned(), label(v)))
                .collect::<Vec<_>>()
        };
        match choice {
            Choice::Profile => {
                std::iter::once(("auto".to_owned(), format!("Auto ({})", self.detected.name)))
                    .chain(
                        profile::builtin()
                            .iter()
                            .map(|p| (p.id.to_owned(), p.name.to_owned())),
                    )
                    .collect()
            }
            Choice::FrameRate => profile
                .frame_rates
                .iter()
                .map(|rate| (rate.to_string(), format!("{rate} FPS")))
                .collect(),
            Choice::Resolution => {
                let above_1080p = self
                    .report
                    .as_ref()
                    .is_none_or(|report| tune::choices(report).above_1080p);
                pairs(
                    RESOLUTIONS
                        .into_iter()
                        .filter(|r| above_1080p || resolution_height(r) <= 1080)
                        .collect(),
                    |r| r.replace('x', " x "),
                )
            }
            Choice::Upscaler => pairs(
                rules.offered.iter().copied().filter(allowed).collect(),
                upscaler_label,
            ),
            Choice::Quality => pairs(PRESETS.iter().map(|(v, _)| *v).collect(), preset_label),
            Choice::FrameGeneration => pairs(
                rules
                    .frame_generation
                    .iter()
                    .copied()
                    .filter(allowed)
                    .collect(),
                frame_generation_label,
            ),
        }
    }

    /// Fill empty values with the device's defaults and move every value the device does not
    /// offer to its default.
    fn fit(&mut self) {
        if profile::by_id(&self.stored.device_profile).is_none() {
            self.stored.device_profile = "auto".into();
        }
        let profile = self.profile();
        let default_upscaler = tune::upscalers(&self.report()).default;
        for choice in &Choice::ALL[1..] {
            let options = self.options(*choice);
            let value = self.stored.get(*choice);
            if options.iter().any(|(v, _)| v == value) {
                continue;
            }
            let fallback = match choice {
                Choice::FrameRate => profile_rate(profile),
                Choice::Resolution => "1920x1080",
                Choice::Upscaler if value.is_empty() => default_upscaler,
                Choice::Quality if value.is_empty() => "quality",
                _ => "off",
            };
            let fallback = if options.iter().any(|(v, _)| v == fallback) {
                fallback.to_owned()
            } else {
                // A resolution above what the GPU is offered: its largest.
                options.last().map(|(v, _)| v.clone()).unwrap_or_default()
            };
            self.stored.set(*choice, fallback);
        }
    }

    /// The probe and benchmarks came back: `Ok` with the report and the benchmark results, or
    /// `Err` with why not.
    pub fn tuned(&mut self, result: Result<(HardwareReport, Bench), String>) {
        self.tuning = false;
        match result {
            Ok((report, bench)) => {
                self.status = format!("Tuned for {}.", report.name);
                self.report = Some(report);
                self.bench = bench;
                self.fit();
            }
            Err(err) => self.status = format!("Could not tune this device: {err}"),
        }
    }

    #[cfg(test)]
    pub fn is_tuning(&self) -> bool {
        self.tuning
    }

    fn lens(&self) -> Vec<usize> {
        let mut lens = vec![3, 1];
        lens.extend(Choice::ALL.iter().map(|_| 1));
        lens
    }

    fn choice_at(row: usize) -> Option<Choice> {
        row.checked_sub(FIRST_CHOICE)
            .and_then(|i| Choice::ALL.get(i).copied())
    }

    /// Step a choice by `by` (-1 or 1); `wrap` goes round from the last value to the first.
    fn change(&mut self, choice: Choice, by: isize, wrap: bool) -> SettingsCommand {
        let options = self.options(choice);
        if options.is_empty() {
            return SettingsCommand::None;
        }
        let here = options
            .iter()
            .position(|(v, _)| v == self.stored.get(choice))
            .unwrap_or(0) as isize;
        let len = options.len() as isize;
        let next = if wrap {
            (here + by).rem_euclid(len)
        } else {
            (here + by).clamp(0, len - 1)
        };
        if next == here {
            return SettingsCommand::None;
        }
        self.stored.set(choice, options[next as usize].0.clone());
        self.fit();
        self.status = String::new();
        SettingsCommand::Save(self.stored.clone())
    }

    /// Carry out one input.
    pub fn handle(&mut self, input: Input) -> SettingsCommand {
        let choice = Self::choice_at(self.focus.row);
        match input {
            Input::Move(nav @ (Nav::Left | Nav::Right)) if choice.is_some() => {
                let by = if nav == Nav::Left { -1 } else { 1 };
                return self.change(choice.unwrap_or(Choice::Profile), by, false);
            }
            Input::Move(nav) => self.focus = self.focus.step(nav, &self.lens()),
            Input::Tab { back } => self.focus = self.focus.tab(back, &self.lens()),
            Input::Back => return SettingsCommand::Back,
            Input::PrevGame | Input::NextGame | Input::OtherSaves => {}
            Input::Activate => return self.activate(),
        }
        SettingsCommand::None
    }

    fn activate(&mut self) -> SettingsCommand {
        match (self.focus.row, Self::choice_at(self.focus.row)) {
            (0, _) => {
                let cards = self.cards();
                let card =
                    [&cards.handheld30, &cards.balanced60, &cards.max][self.focus.col.min(2)];
                self.stored = self.stored.with_card(card);
                self.status = format!("{} is in use.", CARD_NAMES[self.focus.col.min(2)]);
                SettingsCommand::Save(self.stored.clone())
            }
            (1, _) if self.tuning => SettingsCommand::None,
            (1, _) => {
                self.tuning = true;
                self.status = "Tuning: reading this device's GPU and benchmarks...".into();
                SettingsCommand::Tune
            }
            (_, Some(choice)) => self.change(choice, 1, true),
            _ => SettingsCommand::None,
        }
    }

    /// A click or a tap: on a card or "Tune" (`col` 0) it acts as A there; on a choice row,
    /// `col` 0 is the previous value and 1 the next.
    pub fn click(&mut self, row: usize, col: usize) -> SettingsCommand {
        match (row, Self::choice_at(row)) {
            (0, _) if col < 3 => {
                self.focus = Grid::at(0, col);
                self.activate()
            }
            (1, _) => {
                self.focus = Grid::at(1, 0);
                self.activate()
            }
            (_, Some(choice)) => {
                self.focus = Grid::at(row, 0);
                self.change(choice, if col == 0 { -1 } else { 1 }, false)
            }
            _ => SettingsCommand::None,
        }
    }

    /// What to draw now.
    pub fn view(&self) -> SettingsView {
        let cards = self.cards();
        let bench = [self.bench.handheld30, self.bench.balanced60, self.bench.max];
        let targets = ["30", "60", "60"];
        let card_views = [&cards.handheld30, &cards.balanced60, &cards.max]
            .iter()
            .enumerate()
            .map(|(i, card)| {
                let mut lines = vec![
                    format!(
                        "{} FPS · {}",
                        card.frame_rate,
                        card.render_resolution.replace('x', " x ")
                    ),
                    if card.anti_aliasing == "off" {
                        "Upscaler off".to_owned()
                    } else {
                        format!(
                            "{} · {}",
                            upscaler_label(&card.anti_aliasing),
                            preset_label(&card.upscale_preset)
                        )
                    },
                    if card.frame_generation == "off" {
                        "Frame generation off".to_owned()
                    } else {
                        frame_generation_label(&card.frame_generation)
                    },
                    match bench[i] {
                        Some(fps) => format!("Last benchmark {fps:.0} FPS"),
                        None => "Not benchmarked yet".to_owned(),
                    },
                ];
                if card.stepped_down {
                    lines.push(format!(
                        "Quality stepped down: the benchmark missed {} FPS",
                        targets[i]
                    ));
                }
                TuneCard {
                    title: CARD_NAMES[i].to_owned(),
                    lines,
                    in_use: self.stored.matches(card),
                }
            })
            .collect();
        let choices = Choice::ALL
            .iter()
            .map(|choice| {
                let options = self.options(*choice);
                let value = self.stored.get(*choice);
                let at = options.iter().position(|(v, _)| v == value);
                ChoiceView {
                    label: choice.label().to_owned(),
                    value: at.map_or_else(|| value.to_owned(), |i| options[i].1.clone()),
                    can_prev: at.is_some_and(|i| i > 0),
                    can_next: at.is_some_and(|i| i + 1 < options.len()),
                }
            })
            .collect();
        SettingsView {
            cards: card_views,
            choices,
            notes: self.notes(),
            focus: self.focus,
            tuning: self.tuning,
            status: self.status.clone(),
        }
    }

    /// The lines under the rows: the GPU, its warning, every value left out and why, and the
    /// profile's power advice.
    fn notes(&self) -> Vec<String> {
        let profile = self.profile();
        let mut notes = Vec::new();
        match &self.report {
            Some(report) => {
                notes.push(format!("GPU: {}", report.name));
                if let Some(warning) = tune::choices(report).integrated_gpu_warning {
                    notes.push(warning.to_owned());
                }
                let rules = tune::upscalers(report);
                for hidden in &rules.hidden {
                    let label = if hidden.setting == "frame_generation" {
                        frame_generation_label(hidden.value)
                    } else {
                        upscaler_label(hidden.value)
                    };
                    notes.push(format!("{label}: {}.", hidden.reason));
                }
                if !tune::choices(report).above_1080p {
                    notes.push("Resolutions above 1920 x 1080: need 6 GB of video memory.".into());
                }
                let refused = |setting: &str, values: &[&'static str]| {
                    values
                        .iter()
                        .filter(|v| !profile_allows(profile, setting, v))
                        .copied()
                        .collect::<Vec<_>>()
                };
                for value in refused("anti_aliasing", &rules.offered) {
                    notes.push(format!(
                        "{}: not with the {} profile.",
                        upscaler_label(value),
                        profile.name
                    ));
                }
                for value in refused("frame_generation", &rules.frame_generation) {
                    notes.push(format!(
                        "{}: not with the {} profile.",
                        frame_generation_label(value),
                        profile.name
                    ));
                }
            }
            None => notes.push(
                "Not tuned yet: Tune this device reads the GPU, then the cards and choices fit it."
                    .into(),
            ),
        }
        if self.stored.anti_aliasing == "off" {
            notes.push(
                "With the upscaler off the game draws at the resolution: Quality does nothing."
                    .into(),
            );
        }
        if !profile.power.is_empty() {
            notes.push(format!(
                "Power: {} in Steam's performance menu is a good limit for this device.",
                profile.power
            ));
        }
        notes
    }
}

/// The height of a "WxH" resolution; 0 when it is not one.
fn resolution_height(resolution: &str) -> u32 {
    resolution
        .split_once('x')
        .and_then(|(_, h)| h.parse().ok())
        .unwrap_or(0)
}

/// Everything the window draws on the Settings page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsView {
    pub cards: Vec<TuneCard>,
    pub choices: Vec<ChoiceView>,
    pub notes: Vec<String>,
    /// Row 0 the cards, row 1 "Tune this device", rows 2.. the choices.
    pub focus: Grid,
    pub tuning: bool,
    pub status: String,
}

/// One display card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuneCard {
    pub title: String,
    pub lines: Vec<String>,
    /// The saved values are this card's.
    pub in_use: bool,
}

/// One choice row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceView {
    pub label: String,
    pub value: String,
    pub can_prev: bool,
    pub can_next: bool,
}

#[cfg(test)]
mod tests;
