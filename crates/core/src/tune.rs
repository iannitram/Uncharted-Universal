//! The settings picker for the three display cards (SPEC.md M5, issue #174): Handheld 30,
//! Balanced 60 and Max quality. It turns the hardware probe (#171) and the benchmark results
//! (#173) into the settings the Settings page offers (`app/src/vm/settings`, ROADMAP 2.06).
//! A pure function: no files, no clock. Values are the strings the settings store keeps
//! (`frame_rate` "30" or "60", `render_resolution` "WxH", `upscale_preset` one of `off`,
//! `quality`, `balanced`, `performance`, `ultra`, `anti_aliasing` and `frame_generation` an
//! upscaler name or `off`).
//!
//! # Hardware report (#171)
//! `parse_probe` reads one JSON object describing the GPU selected for the game, rather
//! than the GPU driving the launcher window. The future runtime probe owns Vulkan device
//! enumeration, generation detection, `BB_GPU_ID`, refresh-rate and CPU feature queries.
//! No game files are needed. `choices` supplies the memory, RTX and laptop gates;
//! `upscalers` adds the DLSS and FSR rules and the default upscaler on top of them (#147), and
//! `pick_for` keeps the three cards to them.
//!
//! # Benchmark reports (#173)
//! `parse_bench` reads one `benchmark.json` from `BB_BENCHMARK`, `bench_from` turns one report
//! per card into the [`Bench`] the cards step down from, and `cards_from` runs probe, bench
//! and `pick_for` in one call. The per-chapter fixes are `recipe::CHAPTER_FIXES` (#178).

use crate::error::{Error, Result};
use serde_json::Value;

/// The GPU vendor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Amd,
    Nvidia,
    Intel,
    Other,
}

/// The generation identified by the runtime; unknown hardware never implies RTX support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Generation {
    Rdna2,
    Rdna3,
    Rdna4,
    Rtx20,
    Rtx30,
    Rtx40,
    Rtx50,
    IntelArc,
    Older,
    Unknown,
}

/// What the hardware probe found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub vendor: Vendor,
    /// An RTX card (DLSS is available).
    pub rtx: bool,
    /// Video memory in MiB.
    pub vram_mib: u32,
    /// The GPU shares system memory (an APU).
    pub shared_memory: bool,
    /// The display's refresh rate, when known.
    pub refresh_hz: Option<u32>,
}

/// Local hardware data from `uncharted-run --probe` (the runtime loader is still planned).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareReport {
    pub device: Device,
    pub name: String,
    pub generation: Generation,
    /// The selected GPU is integrated. Shared memory alone does not establish this.
    pub integrated_gpu: bool,
    /// Both an integrated and a discrete GPU were enumerated, regardless of selection.
    pub hybrid_graphics: bool,
    pub cpu_cores: u32,
    /// The runtime's CPU instruction check, including OS support for AVX state.
    /// This is independent of any instructions the runtime can rewrite or emulate.
    pub cpu_supported: bool,
}

/// Parse the flat report object: `vendor` (`amd`, `nvidia`, `intel`, `other`), `name`,
/// `generation` (`rdna2`/`rdna3`/`rdna4`, `rtx20`/`rtx30`/`rtx40`/`rtx50`, `intel_arc`,
/// `older`, `unknown`), `vram_mib` (dedicated or shared GPU heap, never total system RAM),
/// `shared_memory`, `integrated_gpu`, `hybrid_graphics`, `refresh_hz` (positive integer or
/// null), `cpu_cores` (positive integer) and `cpu_supported` (boolean). All keys are required;
/// extra keys are ignored for future probe versions. Invalid reports give a fixed error
/// without echoing their contents. RTX support is derived, never trusted as a separate flag.
pub fn parse_probe(json: &str) -> Result<HardwareReport> {
    let damaged = || Error::Launch("The hardware probe report is damaged".into());
    let report: Value = serde_json::from_str(json).map_err(|_| damaged())?;
    let text = |key: &str| report[key].as_str().ok_or_else(damaged);
    let boolean = |key: &str| report[key].as_bool().ok_or_else(damaged);
    let number = |key: &str| {
        report[key]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(damaged)
    };
    let vendor = match text("vendor")? {
        "amd" => Vendor::Amd,
        "nvidia" => Vendor::Nvidia,
        "intel" => Vendor::Intel,
        "other" => Vendor::Other,
        _ => return Err(damaged()),
    };
    let generation = match text("generation")? {
        "rdna2" => Generation::Rdna2,
        "rdna3" => Generation::Rdna3,
        "rdna4" => Generation::Rdna4,
        "rtx20" => Generation::Rtx20,
        "rtx30" => Generation::Rtx30,
        "rtx40" => Generation::Rtx40,
        "rtx50" => Generation::Rtx50,
        "intel_arc" => Generation::IntelArc,
        "older" => Generation::Older,
        "unknown" => Generation::Unknown,
        _ => return Err(damaged()),
    };
    let generation_vendor = match generation {
        Generation::Rdna2 | Generation::Rdna3 | Generation::Rdna4 => Vendor::Amd,
        Generation::Rtx20 | Generation::Rtx30 | Generation::Rtx40 | Generation::Rtx50 => {
            Vendor::Nvidia
        }
        Generation::IntelArc => Vendor::Intel,
        Generation::Older | Generation::Unknown => vendor,
    };
    let name = text("name")?.trim().to_string();
    let cpu_cores = number("cpu_cores")?;
    if vendor != generation_vendor || name.is_empty() || cpu_cores == 0 {
        return Err(damaged());
    }
    let refresh_hz = match report.get("refresh_hz") {
        Some(Value::Null) => None,
        Some(_) => Some(number("refresh_hz")?).filter(|n| *n > 0),
        None => return Err(damaged()),
    };
    if report["refresh_hz"] != Value::Null && refresh_hz.is_none() {
        return Err(damaged());
    }
    Ok(HardwareReport {
        device: Device {
            vendor,
            rtx: matches!(
                generation,
                Generation::Rtx20 | Generation::Rtx30 | Generation::Rtx40 | Generation::Rtx50
            ),
            vram_mib: number("vram_mib")?,
            shared_memory: boolean("shared_memory")?,
            refresh_hz,
        },
        name,
        generation,
        integrated_gpu: boolean("integrated_gpu")?,
        hybrid_graphics: boolean("hybrid_graphics")?,
        cpu_cores,
        cpu_supported: boolean("cpu_supported")?,
    })
}

/// Necessary hardware gates for the Display page, before #147's upscaler feature rules.
/// A true gate permits further checks; it does not promise driver or asset availability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choices {
    pub frame_generation: bool,
    pub above_1080p: bool,
    pub dlss: bool,
    pub integrated_gpu_warning: Option<&'static str>,
}

/// Gate against the selected GPU's memory, never a different adapter's larger heap.
pub fn choices(report: &HardwareReport) -> Choices {
    Choices {
        frame_generation: report.device.vram_mib >= 6144,
        above_1080p: report.device.vram_mib >= 6144,
        dlss: report.device.vendor == Vendor::Nvidia && report.device.rtx,
        integrated_gpu_warning: (report.integrated_gpu && report.hybrid_graphics).then_some(
            "The game would use the integrated GPU. Select the discrete GPU for better performance.",
        ),
    }
}

/// Device names (Vulkan `deviceName`, as the driver reports it) that FSR 3.1 was measured
/// clean on. The same list as `FsrDeviceValidated` in `gpu/shim/bbport_settings.cpp`, which
/// a test keeps in step: RADV gives every AMD GPU one driver id, so the name tells them apart.
pub const FSR3_VALIDATED_DEVICES: &[&str] = &["(RADV PHOENIX)", "(RADV STRIX_HALO)"];

/// A setting value the Display page leaves out on this GPU, with the line it shows instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hidden {
    /// `anti_aliasing` or `frame_generation`.
    pub setting: &'static str,
    pub value: &'static str,
    pub reason: &'static str,
}

/// The upscaler and frame-generation rules for the selected GPU (#147).
///
/// The game keeps the final word: its own probe falls back to FSR 3.1 when DLSS cannot load
/// and when the FSR 4 shaders cannot run, and says why in the overlay. FSR 4.1.1 and XeSS are
/// offered by whoever knows their files are installed; they are not GPU rules. The cards of
/// [`pick_for`] only take values offered here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upscalers {
    /// `anti_aliasing` values offered, in the page's order: `off`, `taa`, `fsr1` and `fsr3`
    /// everywhere, `fsr4` on RDNA 3 and 4 and on RTX 40 and 50 (where the launcher says FSR 4
    /// runs best), `dlss` on any RTX.
    pub offered: Vec<&'static str>,
    /// `frame_generation` values offered, in the page's order: `off`, `fsr3` with 6 GB of video
    /// memory (#171's gate), `dlss` with that and an RTX 40 or 50 (an RTX 30 such as the
    /// 3090 Ti has no DLSS Frame Generation; it gets FSR 3's).
    pub frame_generation: Vec<&'static str>,
    /// `frame_gen_multiplier` values for DLSS Frame Generation: 2 on an RTX 40, 2, 3 and 4 on
    /// an RTX 50, empty when it is hidden. FSR 3 Frame Generation is always 2.
    pub dlss_multipliers: &'static [u32],
    /// Every value left out of either list, `anti_aliasing` first, each in the page's order.
    pub hidden: Vec<Hidden>,
    /// The profile's default `anti_aliasing`: `dlss` on an RTX; `fsr3` on a GPU whose name
    /// holds one of [`FSR3_VALIDATED_DEVICES`]; else `off`, as the game itself starts an
    /// unmeasured FSR 3.1 (`DriverDefaultUpscaler`).
    pub default: &'static str,
}

/// What the Display page offers on this GPU. `report.name` is the driver's device name.
pub fn upscalers(report: &HardwareReport) -> Upscalers {
    use Generation::*;
    let gates = choices(report);
    let fsr4 = matches!(report.generation, Rdna3 | Rdna4 | Rtx40 | Rtx50);
    let dlss_fg = matches!(report.generation, Rtx40 | Rtx50);

    let mut rules = Upscalers {
        offered: Vec::new(),
        frame_generation: Vec::new(),
        dlss_multipliers: &[],
        hidden: Vec::new(),
        default: "off",
    };
    let mut offer =
        |setting: &'static str, value: &'static str, refused: Option<&'static str>| match refused {
            None if setting == "anti_aliasing" => rules.offered.push(value),
            None => rules.frame_generation.push(value),
            Some(reason) => rules.hidden.push(Hidden {
                setting,
                value,
                reason,
            }),
        };
    // `None` offers the value; `Some(reason)` hides it, with that line on the page.
    let unless = |ok: bool, reason: &'static str| (!ok).then_some(reason);
    const NEEDS_6_GB: &str = "needs 6 GB of video memory";

    for value in ["off", "taa", "fsr1", "fsr3"] {
        offer("anti_aliasing", value, None);
    }
    offer(
        "anti_aliasing",
        "fsr4",
        unless(
            fsr4,
            "needs a Radeon RX 7000 or 9000, or a GeForce RTX 40 or 50",
        ),
    );
    offer(
        "anti_aliasing",
        "dlss",
        unless(gates.dlss, "needs a GeForce RTX"),
    );
    offer("frame_generation", "off", None);
    offer(
        "frame_generation",
        "fsr3",
        unless(gates.frame_generation, NEEDS_6_GB),
    );
    offer(
        "frame_generation",
        "dlss",
        unless(dlss_fg, "needs a GeForce RTX 40 or 50")
            .or(unless(gates.frame_generation, NEEDS_6_GB)),
    );

    if rules.frame_generation.contains(&"dlss") {
        rules.dlss_multipliers = if report.generation == Rtx50 {
            &[2, 3, 4]
        } else {
            &[2]
        };
    }
    rules.default = if gates.dlss {
        "dlss"
    } else if FSR3_VALIDATED_DEVICES
        .iter()
        .any(|device| report.name.contains(device))
    {
        "fsr3"
    } else {
        "off"
    };
    rules
}

/// Average frames per second the benchmark measured on each card; `None` for a card not
/// benchmarked.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bench {
    pub handheld30: Option<f64>,
    pub balanced60: Option<f64>,
    pub max: Option<f64>,
}

/// One `benchmark.json` from the runtime's benchmark mode (#173, `BB_BENCHMARK`): the fields
/// the picker reads. The runtime writes it as `{"game", "seconds", "settings", "frame_ms":
/// {"p50", "p95", "p99"}, "avg_fps", "vram_peak_mib", "stalls_over_40ms", "ended"}`.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchReport {
    /// `u1`, `u2` or `u3`.
    pub game: String,
    pub seconds: u32,
    pub avg_fps: f64,
    /// The run reached the end of its path (`ended` is `ok`); a `crash` did not.
    pub finished: bool,
}

/// Parse one benchmark report. Every key the runtime writes is required and checked for its
/// type; extra keys are ignored for future runtime versions. A damaged report gives a fixed
/// error without echoing its contents.
pub fn parse_bench(json: &str) -> Result<BenchReport> {
    let damaged = || Error::Launch("The benchmark report is damaged".into());
    let report: Value = serde_json::from_str(json).map_err(|_| damaged())?;
    let positive = |value: &Value| value.as_f64().filter(|n| n.is_finite() && *n > 0.0);
    let game = report["game"].as_str().ok_or_else(damaged)?;
    if !matches!(game, "u1" | "u2" | "u3") {
        return Err(damaged());
    }
    let seconds = report["seconds"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(damaged)?;
    let avg_fps = positive(&report["avg_fps"]).ok_or_else(damaged)?;
    let finished = match report["ended"].as_str() {
        Some("ok") => true,
        Some("crash") => false,
        _ => return Err(damaged()),
    };
    let frame_ms_ok = ["p50", "p95", "p99"]
        .iter()
        .all(|key| positive(&report["frame_ms"][*key]).is_some());
    if !frame_ms_ok
        || !report["settings"].is_object()
        || report["vram_peak_mib"].as_u64().is_none()
        || report["stalls_over_40ms"].as_u64().is_none()
    {
        return Err(damaged());
    }
    Ok(BenchReport {
        game: game.to_owned(),
        seconds,
        avg_fps,
        finished,
    })
}

/// The [`Bench`] for [`pick_for`] from up to one report per card, each run with that card's
/// settings. A card without a report, or whose run crashed before the end of its path, is not
/// benchmarked (`None`): a partial run's rate is not compared with the target. The reports
/// must all be of one game.
pub fn bench_from(
    handheld30: Option<&str>,
    balanced60: Option<&str>,
    max: Option<&str>,
) -> Result<Bench> {
    let parse = |json: Option<&str>| json.map(parse_bench).transpose();
    let (handheld30, balanced60, max) = (parse(handheld30)?, parse(balanced60)?, parse(max)?);
    let games: Vec<&str> = [&handheld30, &balanced60, &max]
        .into_iter()
        .flatten()
        .map(|report| report.game.as_str())
        .collect();
    if games.windows(2).any(|pair| pair[0] != pair[1]) {
        return Err(Error::Launch(
            "The benchmark reports are of different games".into(),
        ));
    }
    let rate = |report: Option<BenchReport>| report.filter(|r| r.finished).map(|r| r.avg_fps);
    Ok(Bench {
        handheld30: rate(handheld30),
        balanced60: rate(balanced60),
        max: rate(max),
    })
}

/// The Settings page's whole chain (2.06): the probe report of `uncharted-run --probe`, the
/// benchmark reports of the cards that were run, then [`pick_for`].
pub fn cards_from(
    probe_json: &str,
    handheld30: Option<&str>,
    balanced60: Option<&str>,
    max: Option<&str>,
) -> Result<Cards> {
    let report = parse_probe(probe_json)?;
    let bench = bench_from(handheld30, balanced60, max)?;
    Ok(pick_for(&report, &bench))
}

/// The settings of one card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub frame_rate: String,
    pub render_resolution: String,
    /// The upscaler's name: from [`pick`], `dlss` on an RTX card, else `fsr3`; from
    /// [`pick_for`], the GPU's default upscaler.
    pub anti_aliasing: String,
    pub upscale_preset: String,
    pub frame_generation: String,
    /// The benchmark was below target and the preset was moved one step down.
    pub stepped_down: bool,
}

/// The three cards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cards {
    pub handheld30: Card,
    pub balanced60: Card,
    pub max: Card,
}

/// Pick the settings of the three cards for `device`, stepping a card's preset down when
/// `bench` shows it missed its frame-rate target (30 for `handheld30`, 60 for the others).
/// A `Device` has no generation, so this cannot apply #147's rules: the Settings page uses
/// [`pick_for`], which starts from it.
pub fn pick(device: &Device, bench: &Bench) -> Cards {
    let anti_aliasing = if device.rtx { "dlss" } else { "fsr3" };

    // Create base cards with default values
    let mut handheld30 = Card {
        frame_rate: "30".to_string(),
        render_resolution: "1280x720".to_string(),
        anti_aliasing: anti_aliasing.to_string(),
        upscale_preset: "quality".to_string(),
        frame_generation: "off".to_string(),
        stepped_down: false,
    };

    let mut balanced60 = Card {
        frame_rate: "60".to_string(),
        render_resolution: "1920x1080".to_string(),
        anti_aliasing: anti_aliasing.to_string(),
        upscale_preset: if device.shared_memory {
            "balanced"
        } else {
            "quality"
        }
        .to_string(),
        frame_generation: "off".to_string(),
        stepped_down: false,
    };

    // Determine max resolution based on VRAM
    let max_resolution = if !device.shared_memory {
        if device.vram_mib >= 10240 {
            "3840x2160"
        } else if device.vram_mib >= 6144 {
            "2560x1440"
        } else {
            "1920x1080"
        }
    } else {
        "1920x1080"
    };

    let mut max = Card {
        frame_rate: "60".to_string(),
        render_resolution: max_resolution.to_string(),
        anti_aliasing: anti_aliasing.to_string(),
        upscale_preset: "quality".to_string(),
        frame_generation: if device.rtx {
            "dlss"
        } else if device.vram_mib >= 6144 && device.refresh_hz.unwrap_or(0) >= 120 {
            "fsr3"
        } else {
            "off"
        }
        .to_string(),
        stepped_down: false,
    };

    // Handle low memory case
    let low_memory = device.vram_mib < 6144;
    if low_memory {
        handheld30.frame_generation = "off".to_string();
        balanced60.frame_generation = "off".to_string();
        max.frame_generation = "off".to_string();

        // Cap resolutions to 1920x1080 for balanced60 and max cards only
        balanced60.render_resolution = "1920x1080".to_string();
        max.render_resolution = "1920x1080".to_string();
    }

    // Process benchmark results and step presets down if needed
    let step_down_preset = |preset: &str| -> String {
        const STEPS: [&str; 5] = ["off", "quality", "balanced", "performance", "ultra"];
        match STEPS.iter().position(|step| *step == preset) {
            Some(at) => STEPS[(at + 1).min(STEPS.len() - 1)].to_string(),
            None => preset.to_string(),
        }
    };

    // Check handheld30 benchmark
    if let Some(fps) = bench.handheld30 {
        if fps < 0.95 * 30.0 {
            handheld30.upscale_preset = step_down_preset(&handheld30.upscale_preset);
            handheld30.stepped_down = true;
        }
    }

    // Check balanced60 benchmark
    if let Some(fps) = bench.balanced60 {
        if fps < 0.95 * 60.0 {
            balanced60.upscale_preset = step_down_preset(&balanced60.upscale_preset);
            balanced60.stepped_down = true;
        }
    }

    // Check max benchmark
    if let Some(fps) = bench.max {
        if fps < 0.95 * 60.0 {
            max.upscale_preset = step_down_preset(&max.upscale_preset);
            max.stepped_down = true;
        }
    }

    Cards {
        handheld30,
        balanced60,
        max,
    }
}

/// The three cards for the selected GPU: the entry point the Settings page uses. [`pick`]
/// knows the [`Device`] alone and gives DLSS Frame Generation to every RTX with 6 GB; this keeps
/// each card to what [`upscalers`] offers (#147):
/// - `anti_aliasing` is the GPU's default upscaler ([`Upscalers::default`]). It differs from
///   `pick` only on a non-RTX GPU the game has not measured FSR 3.1 on, which starts with `off`:
///   the launcher always sends the card's value (`BB_UPSCALER`), and the game keeps a chosen
///   FSR 3.1 even there. With `off` the launcher draws at the resolution and ignores the preset.
/// - A hidden `frame_generation` (DLSS's on an RTX 20 or 30) becomes FSR 3's under `pick`'s
///   own rule for it (offered, which needs 6 GB, and a display of 120 Hz or more), else `off`.
///
/// Every other value is `pick`'s.
pub fn pick_for(report: &HardwareReport, bench: &Bench) -> Cards {
    let rules = upscalers(report);
    let offered = &rules.frame_generation;
    let fsr3 = offered.contains(&"fsr3") && report.device.refresh_hz.unwrap_or(0) >= 120;
    let mut cards = pick(&report.device, bench);
    for card in [&mut cards.handheld30, &mut cards.balanced60, &mut cards.max] {
        card.anti_aliasing = rules.default.to_string();
        if !offered.contains(&card.frame_generation.as_str()) {
            card.frame_generation = if fsr3 { "fsr3" } else { "off" }.to_string();
        }
    }
    cards
}
